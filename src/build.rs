//! Steps 5 and 6 of a run: run xcodebuild in the slot with its output locations and the cache,
//! show its output with worktree paths, keep the full log, then report what happened.
use crate::app::Context;
use agent_build_core::report::{self, Outcome, Summary};
use agent_build_core::{args, log, output, slot};
use agent_build_sys::lock::SlotLock;
use agent_build_sys::run::{self, Ended, Stream};
use agent_build_sys::sync;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const KEEP_LOGS: usize = 50;

pub fn run(ctx: &Context, lock: &SlotLock, sync_secs: f64) -> Outcome {
    let archive = ctx.paths.archives.join(args::archive_name(ctx.inv.scheme.as_deref()));
    let mut argv = args::injected(
        ctx.inv.action,
        &text(&ctx.paths.derived_data),
        &text(&archive),
        &text(&slot::cas_path(&ctx.dirs.cache)),
        &ctx.config.cache_limit,
    );
    argv.extend(ctx.inv.args.iter().cloned());
    let program = std::env::var("AGENT_BUILD_XCODEBUILD").unwrap_or_else(|_| "xcodebuild".to_string());
    let log_path = log_path(ctx);
    let mut log_file = log_path.as_ref().and_then(|p| File::create(p).ok()).map(BufWriter::new);
    if let Some(f) = log_file.as_mut() {
        let _ = writeln!(f, "$ {program} {}", argv.join(" ")); // the log is best effort
    }
    let (slot_src, worktree) = (text(&ctx.paths.src), text(&ctx.worktree));
    let started = Instant::now();
    let mut lines: Vec<(f64, String)> = Vec::new();
    let ended = run::run(&program, &argv, &ctx.paths.src.join(&ctx.prefix), lock, |stream, line| {
        if let Some(f) = log_file.as_mut() {
            let _ = writeln!(f, "{line}"); // best effort
        }
        lines.push((started.elapsed().as_secs_f64(), line.to_string()));
        if !output::is_cache_remark(line) {
            let shown = output::to_caller(line, &slot_src, &worktree);
            match stream {
                Stream::Out => println!("{shown}"),
                Stream::Err => eprintln!("{shown}"),
            }
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
    let log_shown = log_path.as_deref().map(text).unwrap_or_else(|| "(not written)".to_string());
    finish(
        ctx,
        &lines,
        &archive,
        Summary {
            ok: outcome == Outcome::Built(0),
            slot: &ctx.slot_name,
            sync_secs,
            build_secs,
            cache: &log::CacheStats::default(),
            log: &log_shown,
        },
    );
    outcome
}

/// Report lines, in order: phases, P4, top misses, P5, output locations, summary.
fn finish(ctx: &Context, lines: &[(f64, String)], archive: &Path, summary: Summary) {
    let plain = || lines.iter().map(|(_, l)| l.as_str());
    let stamped: Vec<(f64, &str)> = lines.iter().map(|(t, l)| (*t, l.as_str())).collect();
    let cache = log::cache_stats(plain());
    let rewritten = sync::rewritten_by_build(&ctx.worktree, &ctx.paths.src).unwrap_or_default();
    let products = ctx.paths.derived_data.join("Build").join("Products");
    let xcresult = log::xcresult_path(plain()).map(|p| output::to_caller(&p, &text(&ctx.paths.src), &text(&ctx.worktree)).into_owned());
    let report = [
        report::phases_line(&log::phases(&stamped)),
        report::p4_line(&log::p4_targets(plain())),
        report::top_misses_line(&cache),
        report::p5_line(&rewritten),
    ];
    for line in report.into_iter().flatten() {
        eprintln!("{line}");
    }
    for line in report::output_lines(ctx.inv.action, &text(&products), &text(archive), xcresult.as_deref()) {
        eprintln!("{line}");
    }
    eprintln!("{}", report::summary_line(&Summary { cache: &cache, ..summary }));
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
