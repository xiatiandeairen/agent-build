//! Steps 5 and 6 of `agb cargo`: run cargo in the slot with the slot's target directory and no
//! rustc wrapper, keep its full output in the worktree's log, then report from cargo's JSON
//! messages: errors, failed tests, what was rebuilt, preconditions, products. `run` and `bench`
//! show their program's output as is; `clean` shows cargo's.
use crate::app::Context;
use crate::build::text;
use agent_build_core::args::Action;
use agent_build_core::cargo::{self as args, Invocation, Kind};
use agent_build_core::cargo_out::{self as msg, Diagnostic, Message, Stats, Unit};
use agent_build_core::report::{self, Outcome};
use agent_build_core::xcresult::Severity;
use agent_build_core::{output, project, slot};
use agent_build_sys::lock::SlotLock;
use agent_build_sys::run::{self, Ended, Stream};
use agent_build_sys::{deliver, ignored, sync};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const TAIL: usize = 30;

/// What cargo said during the run.
#[derive(Default)]
struct Heard {
    diagnostics: Vec<Diagnostic>,
    units: Vec<Unit>,
    /// Output of programs and test harnesses cargo ran (not its JSON messages).
    plain: Vec<String>,
    /// The end of cargo's own output (stderr), for a failure nothing else explains.
    tail: VecDeque<String>,
}

pub fn run(ctx: &Context, inv: &Invocation, lock: &SlotLock, sync_secs: f64) -> Outcome {
    let out = &ctx.output;
    let replaced = if inv.kind == Kind::Build { vec![out.log.clone(), out.target.clone()] } else { vec![out.log.clone()] };
    if let Err(e) = deliver::prepare(out, &replaced) {
        eprintln!("agb: cannot ready the output directory {}: {e}", out.dir.display());
        return Outcome::SyncFailed;
    }
    let program = std::env::var("AGENT_BUILD_CARGO").unwrap_or_else(|_| "cargo".to_string());
    let env = args::env(&text(&ctx.paths.target));
    let set: Vec<String> =
        args::WRAPPER_VARS.iter().filter(|k| std::env::var(k).is_ok_and(|v| !v.is_empty())).map(|k| k.to_string()).collect();
    if let Some(line) = msg::wrappers_off_line(&set) {
        eprintln!("{line}");
    }
    let cwd = ctx.paths.source.join(&ctx.prefix);
    let root = workspace_root(&program, &env, &cwd, &inv.args).unwrap_or_else(|| cwd.clone());
    eprintln!("{}", msg::started_line(&ctx.slot_name, &text(&out.log)));
    let argv = args::command_line(inv);
    let started = Instant::now();
    let (ended, heard) = listen(ctx, inv, &program, &argv, &env, &cwd, &text(&root), lock);
    let build_secs = started.elapsed().as_secs_f64();
    let outcome = match ended {
        Ok(Ended::Exited(code)) => Outcome::Built(code),
        Ok(Ended::Stopped(sig)) => Outcome::Signaled(sig),
        Err(e) => {
            eprintln!("agb: cannot run {program}: {e}");
            Outcome::Built(127)
        }
    };
    finish(ctx, inv, &heard, &root, outcome == Outcome::Built(0), (sync_secs, build_secs));
    outcome
}

/// Runs cargo, sorting each line into what the report needs and writing the log: cargo's own
/// output, rustc's rendered diagnostics in place of their JSON, and programs' output.
#[allow(clippy::too_many_arguments)] // one call site; a struct would only rename these
fn listen(
    ctx: &Context,
    inv: &Invocation,
    program: &str,
    argv: &[String],
    env: &[(String, String)],
    cwd: &Path,
    root: &str,
    lock: &SlotLock,
) -> (std::io::Result<Ended>, Heard) {
    let mut log = File::create(&ctx.output.log).ok().map(BufWriter::new);
    let mut write = |t: &str| {
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{t}"); // the log is best effort
        }
    };
    write(&format!("$ {program} {}", argv.join(" ")));
    let (src, wt) = (text(&ctx.paths.source), text(&ctx.worktree));
    let mut heard = Heard::default();
    let ended = run::run(program, argv, env, cwd, lock, |stream, line| match (stream, msg::message(line, root)) {
        (Stream::Out, Some(Message::Diagnostic(d))) => {
            write(d.rendered.as_deref().unwrap_or(&d.message));
            heard.diagnostics.push(d);
        }
        (Stream::Out, Some(Message::Unit(u))) => heard.units.push(u),
        (Stream::Out, Some(Message::Other)) => {}
        (Stream::Out, None) => {
            write(line);
            if inv.kind == Kind::Run {
                println!("{}", output::to_caller(line, &src, &wt));
            }
            heard.plain.push(line.to_string());
        }
        (Stream::Err, _) => {
            write(line);
            if inv.kind == Kind::Clean {
                eprintln!("{}", output::to_caller(line, &src, &wt));
            }
            heard.tail.push_back(line.to_string());
            if heard.tail.len() > TAIL {
                heard.tail.pop_front();
            }
        }
    });
    (ended, heard)
}

/// Report lines, in order: errors, failed tests (or the end of cargo's output when a failure has
/// no other explanation), what was rebuilt, P5, P8, products, summary.
fn finish(ctx: &Context, inv: &Invocation, heard: &Heard, root: &Path, ok: bool, (sync_secs, build_secs): (f64, f64)) {
    let (src, wt) = (text(&ctx.paths.source), text(&ctx.worktree));
    let show = |s: &str| output::to_caller(s, &src, &wt).into_owned();
    let errors: Vec<&Diagnostic> = heard.diagnostics.iter().filter(|d| d.error).collect();
    for e in &errors {
        eprintln!("{}", report::issue_line(e.location.as_deref().map(show).as_deref(), Severity::Error, &show(&e.message)));
    }
    let failures = if inv.kind == Kind::Test { msg::test_failures(&heard.plain, &text(root)) } else { Vec::new() };
    failures.iter().for_each(|f| eprintln!("{}", report::test_failure_line(&show(f))));
    if !ok && errors.is_empty() && failures.is_empty() && inv.kind != Kind::Clean {
        eprintln!("agb: the last {} lines of cargo's output:", heard.tail.len());
        heard.tail.iter().for_each(|l| eprintln!("{}", show(l)));
    }
    let mut stats = Stats::default();
    heard.units.iter().for_each(|u| stats.add(u));
    let texts: Vec<String> = if errors.is_empty() {
        heard.tail.iter().cloned().collect()
    } else {
        errors.iter().map(|e| format!("{} {}", e.location.as_deref().unwrap_or_default(), e.message)).collect()
    };
    let missing = if ok { Vec::new() } else { missing_ignored(ctx, &texts) };
    let lines = [
        msg::rebuilt_line(&stats),
        report::p5_line(&sync::rewritten_by_build(&ctx.worktree, &ctx.paths.source).unwrap_or_default()),
        report::p8_line(&missing),
    ];
    lines.into_iter().flatten().for_each(|l| eprintln!("{l}"));
    let products = if ok && inv.kind == Kind::Build { publish(ctx, &heard.units) } else { None };
    report::output_lines(Action::Build, ok, products.as_deref(), "", None).iter().for_each(|l| eprintln!("{l}"));
    let log = text(&ctx.output.log);
    let warnings = heard.diagnostics.len() - errors.len();
    let s = msg::Summary { ok, slot: &ctx.slot_name, sync_secs, build_secs, stats: &stats, errors: errors.len(), warnings, log: &log };
    eprintln!("{}", msg::summary_line(&s));
}

fn missing_ignored(ctx: &Context, texts: &[String]) -> Vec<String> {
    let mentioned = project::mentioned_paths(texts.iter().map(String::as_str), &text(&ctx.paths.source));
    ignored::missing_in_slot(&ctx.worktree, &ctx.paths.source, &mentioned)
}

/// The worktree's copy of this build's products, made under the slot lock.
fn publish(ctx: &Context, units: &[Unit]) -> Option<String> {
    let files = msg::products(units, &text(&ctx.paths.target));
    match deliver::publish_files(&ctx.paths.target, &files, &ctx.output) {
        Ok(true) => Some(text(&ctx.output.target)),
        Ok(false) => None,
        Err(e) => {
            eprintln!("agb: cannot copy the products to {}: {e}", ctx.output.target.display());
            None
        }
    }
}

/// The workspace root, which rustc's relative paths in messages are relative to.
fn workspace_root(program: &str, env: &[(String, String)], cwd: &Path, args: &[String]) -> Option<PathBuf> {
    let mut c = Command::new(program);
    c.args(["locate-project", "--workspace", "--message-format", "plain"]).envs(env.iter().map(|(k, v)| (k, v))).current_dir(cwd);
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    for (i, a) in args[..end].iter().enumerate() {
        if a == "--manifest-path" {
            c.arg(a).args(args.get(i + 1));
        } else if a.starts_with("--manifest-path=") {
            c.arg(a);
        }
    }
    let out = c.output().ok().filter(|o| o.status.success())?;
    let manifest = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    // Spelled like the slot path agb rewrites (`/private/tmp/…` as `/tmp/…`), so paths made from it are rewritten too.
    manifest.parent().map(slot::as_xcode_prints)
}
