//! Steps 5 and 6 of a run: run xcodebuild in the slot with its output locations and the cache,
//! keep its full output in the log, then report from the result bundle: errors, failed tests,
//! preconditions, cache, where the outputs are. Information commands (`-showBuildSettings`, …)
//! print xcodebuild's output as is instead.
use crate::app::Context;
use agent_build_core::args::{self, Action, Outputs};
use agent_build_core::json::{self, Json};
use agent_build_core::report::{self, Outcome, Summary};
use agent_build_core::xcresult::{self, Severity};
use agent_build_core::{config, output, project};
use agent_build_sys::lock::SlotLock;
use agent_build_sys::run::{self, Ended, Stream};
use agent_build_sys::{cache, ignored, sync, xcresult as tool};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const KEEP_LOGS: usize = 50;
/// Lines of raw output shown when a failed build left no result bundle to read.
const TAIL: usize = 30;

pub fn run(ctx: &Context, lock: &SlotLock, sync_secs: f64) -> Outcome {
    let archive = ctx.paths.archives.join(args::archive_name(ctx.inv.scheme.as_deref()));
    let _ = fs::remove_dir_all(&ctx.paths.result_bundle); // xcodebuild refuses an existing bundle; a missing one is fine
    let (dd, archive_s, bundle_s) = (text(&ctx.paths.derived_data), text(&archive), text(&ctx.paths.result_bundle));
    let outputs = Outputs { derived_data: &dd, archive: &archive_s, result_bundle: &bundle_s };
    let mut argv = args::injected(ctx.inv.action, &outputs, &text(&ctx.paths.cas), &ctx.config.cache_limit);
    argv.extend(ctx.inv.args.iter().cloned());
    let program = std::env::var("AGENT_BUILD_XCODEBUILD").unwrap_or_else(|_| "xcodebuild".to_string());
    let log_path = log_path(ctx);
    let log_shown = log_path.as_deref().map(text).unwrap_or_else(|| "(not written)".to_string());
    let quiet = ctx.inv.action != Action::Info;
    if quiet {
        eprintln!("{}", report::started_line(&ctx.slot_name, &log_shown));
    }
    let mut log_file = log_path.as_ref().and_then(|p| File::create(p).ok()).map(BufWriter::new);
    if let Some(f) = log_file.as_mut() {
        let _ = writeln!(f, "$ {program} {}", argv.join(" ")); // the log is best effort
    }
    let (slot_src, worktree) = (text(&ctx.paths.src), text(&ctx.worktree));
    let started = Instant::now();
    let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL + 1);
    let ended = run::run(&program, &argv, &ctx.paths.src.join(&ctx.prefix), lock, |stream, line| {
        if let Some(f) = log_file.as_mut() {
            let _ = writeln!(f, "{line}"); // best effort
        }
        if quiet {
            tail.push_back(line.to_string());
            if tail.len() > TAIL {
                tail.pop_front();
            }
            return;
        }
        let shown = output::to_caller(line, &slot_src, &worktree);
        match stream {
            Stream::Out => println!("{shown}"),
            Stream::Err => eprintln!("{shown}"),
        }
    });
    drop(log_file);
    let build_secs = started.elapsed().as_secs_f64();
    let outcome = match ended {
        Ok(Ended::Exited(code)) => Outcome::Built(code),
        Ok(Ended::Stopped(sig)) => Outcome::Signaled(sig),
        Err(e) => {
            eprintln!("agb: cannot run {program}: {e}");
            Outcome::Built(127)
        }
    };
    let ok = outcome == Outcome::Built(0);
    finish(ctx, &Run { tail: &tail, archive: &archive, ok, sync_secs, build_secs, log: &log_shown });
    outcome
}

/// What the build left for the report.
struct Run<'a> {
    tail: &'a VecDeque<String>,
    archive: &'a Path,
    ok: bool,
    sync_secs: f64,
    build_secs: f64,
    log: &'a str,
}

/// What the result bundle says about this run.
struct Results {
    issues: Vec<xcresult::Issue>,
    failures: Vec<String>,
    cache: xcresult::CacheStats,
    p4: Option<Vec<String>>,
    phases: Vec<(&'static str, f64)>,
}

fn read_results(bundle: &Path, action: Action) -> Results {
    let read = |what: &[&str]| tool::get(bundle, what).and_then(|t| json::parse(&t)).unwrap_or(Json::Null);
    let build = read(&["build-results"]);
    let log = read(&["log", "--type", "build"]);
    let tests = if action == Action::Test { read(&["test-results", "summary"]) } else { Json::Null };
    Results {
        issues: xcresult::issues(&build),
        failures: xcresult::test_failures(&tests),
        cache: xcresult::cache(&log),
        p4: xcresult::p4_targets(&build, &log),
        phases: xcresult::phases(&log),
    }
}

/// Report lines, in order: errors, failed tests (or the end of the raw output when a failed
/// build left nothing to read), phases, P4, top misses, P5, P8, output locations, summary.
fn finish(ctx: &Context, run: &Run) {
    let (src, wt) = (text(&ctx.paths.src), text(&ctx.worktree));
    let show = |s: &str| output::to_caller(s, &src, &wt).into_owned();
    let r = read_results(&ctx.paths.result_bundle, ctx.inv.action);
    let errors: Vec<&xcresult::Issue> = r.issues.iter().filter(|i| i.severity == Severity::Error).collect();
    for e in &errors {
        eprintln!("{}", report::issue_line(e.location.as_deref().map(show).as_deref(), e.severity, &show(&e.message)));
    }
    for f in &r.failures {
        eprintln!("{}", report::test_failure_line(&show(f)));
    }
    let explained = !errors.is_empty() || !r.failures.is_empty();
    if !run.ok && !explained && !run.tail.is_empty() {
        eprintln!("{}", report::tail_header(run.tail.len()));
        run.tail.iter().for_each(|l| eprintln!("{}", show(l)));
    }
    let lines = [
        report::phases_line(&r.phases),
        report::p4_line(r.p4.as_deref()),
        report::top_misses_line(&r.cache),
        report::p5_line(&sync::rewritten_by_build(&ctx.worktree, &ctx.paths.src).unwrap_or_default()),
        report::p8_line(&missing_ignored(ctx, run, &errors)),
    ];
    lines.into_iter().flatten().for_each(|l| eprintln!("{l}"));
    let products = text(&ctx.paths.derived_data.join("Build").join("Products"));
    let bundle = ctx.paths.result_bundle.exists().then(|| text(&ctx.paths.result_bundle));
    for line in report::output_lines(ctx.inv.action, run.ok, &products, &text(run.archive), bundle.as_deref()) {
        eprintln!("{line}");
    }
    if let Some(line) = clear_cache_over_limit(ctx) {
        eprintln!("{line}");
    }
    let (n_errors, warnings) = (errors.len(), r.issues.len() - errors.len());
    let summary = Summary {
        ok: run.ok,
        slot: &ctx.slot_name,
        sync_secs: run.sync_secs,
        build_secs: run.build_secs,
        cache: &r.cache,
        errors: n_errors,
        warnings,
        log: run.log,
    };
    eprintln!("{}", report::summary_line(&summary));
}

/// Xcode does not enforce COMPILATION_CACHE_LIMIT_SIZE, so the project's cache is cleared once a
/// build leaves it over the limit. This runs under the slot lock, and the cache is this project's
/// own, so no other build is using it.
fn clear_cache_over_limit(ctx: &Context) -> Option<String> {
    let limit = config::size_bytes(&ctx.config.cache_limit)?;
    let used = cache::disk_usage(&ctx.paths.cas);
    if used <= limit {
        return None;
    }
    let cleared = fs::remove_dir_all(&ctx.paths.cas).is_ok();
    Some(report::cache_cleared_line(used, &ctx.config.cache_limit, cleared))
}

/// P8: git-ignored files a failed build looked for, from its errors or, without them, its last lines.
fn missing_ignored(ctx: &Context, run: &Run, errors: &[&xcresult::Issue]) -> Vec<String> {
    if run.ok {
        return Vec::new();
    }
    let texts: Vec<String> = if errors.is_empty() {
        run.tail.iter().cloned().collect()
    } else {
        errors.iter().map(|e| format!("{} {}", e.location.as_deref().unwrap_or_default(), e.message)).collect()
    };
    let mentioned = project::mentioned_paths(texts.iter().map(String::as_str), &text(&ctx.paths.src));
    ignored::missing_in_slot(&ctx.worktree, &ctx.paths.src, &mentioned)
}

fn log_path(ctx: &Context) -> Option<PathBuf> {
    let dir = ctx.dirs.state.join("logs");
    fs::create_dir_all(&dir).ok()?;
    let names: Vec<String> = fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    for old in report::logs_to_delete(names, KEEP_LOGS - 1) {
        let _ = fs::remove_file(dir.join(old)); // another run may have removed it already
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Some(dir.join(report::log_file_name(now, std::process::id(), &ctx.slot_name)))
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
