//! Steps 1–4 of a run: parse the arguments, find the worktree and its slot, take the slot's lock,
//! and sync the slot to the worktree. Step 5 and 6 (build, report) are in `build`.
use crate::{build, cargo};
use agent_build_core::args::Request;
use agent_build_core::config::{self, Config};
use agent_build_core::report::{self, Outcome};
use agent_build_core::{args, cargo as cargo_args, project, slot};
use agent_build_sys::lock::{LockError, SlotLock};
use agent_build_sys::{git, ignored, signals, sync};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs, io};

pub struct Context {
    pub worktree: PathBuf,
    /// The caller's directory relative to the worktree root, so xcodebuild runs in the same place in the slot.
    pub prefix: String,
    pub config: Config,
    pub slot_name: String,
    pub paths: slot::SlotPaths,
    /// What this run hands to its caller, in the worktree.
    pub output: project::Output,
    /// Git-ignored paths the project declares its build needs (`.agent-build/config.yaml`, precondition P8).
    pub ignored_inputs: Vec<String>,
}

pub fn run(argv: &[String]) -> Outcome {
    let (ctx, request) = match prepare(argv) {
        Ok(c) => c,
        Err(o) => return o,
    };
    signals::install();
    if let Err(e) = fs::create_dir_all(&ctx.paths.root) {
        eprintln!("agb: cannot create the slot {}: {e}", ctx.paths.root.display());
        return Outcome::SyncFailed;
    }
    let me = format!("pid {}, worktree {}", std::process::id(), ctx.worktree.display());
    let lock = match SlotLock::acquire(&ctx.paths.lock, &ctx.paths.owner, &me, |holder| eprintln!("{}", report::waiting_line(holder))) {
        Ok(l) => l,
        Err(LockError::Interrupted(sig)) => return Outcome::Signaled(sig),
        Err(LockError::Io(e)) => {
            eprintln!("agb: cannot lock the slot: {e}");
            return Outcome::SyncFailed;
        }
    };
    let started = Instant::now();
    let synced = sync::sync(&ctx.worktree, &ctx.paths.source)
        .and_then(|_| ignored::sync(&ctx.worktree, &ctx.paths.source, &ctx.ignored_inputs, &ctx.paths.ignored_record));
    if let Err(e) = synced {
        eprintln!("agb: sync failed, nothing was built: {e}");
        return Outcome::SyncFailed;
    }
    if let Some(sig) = signals::pending() {
        return Outcome::Signaled(sig);
    }
    let sync_secs = started.elapsed().as_secs_f64();
    match &request {
        Request::Xcodebuild(inv) => build::run(&ctx, inv, &lock, sync_secs),
        Request::Cargo(inv) => cargo::run(&ctx, inv, &lock, sync_secs),
    }
}

fn usage(message: &str) -> Outcome {
    eprintln!("agb: {message}");
    Outcome::Usage
}

fn prepare(argv: &[String]) -> Result<(Context, Request), Outcome> {
    let mut request = args::request(argv).map_err(|m| usage(&m))?;
    let dropped = match &request {
        Request::Xcodebuild(inv) => &inv.dropped,
        Request::Cargo(inv) => &inv.dropped,
    };
    for d in dropped {
        eprintln!("{}", report::dropped_line(d));
    }
    let cwd = env::current_dir().map_err(|e| usage(&format!("cannot read the current directory: {e}")))?;
    let worktree = git::toplevel(&cwd).map_err(|_| usage("run agb inside a git repository (precondition P3)"))?;
    git::head(&worktree).map_err(|_| usage("the repository has no commit yet"))?;
    let prefix = git::prefix(&cwd).unwrap_or_default();
    let mut dirs = config::dirs(|k| env::var(k).ok()).ok_or_else(|| usage("HOME is not set"))?;
    dirs.cache = slot::as_xcode_prints(&dirs.cache);
    let config = load_yaml(&dirs.config.join("config.yaml"), config::parse)?.unwrap_or_default();
    let ignored_inputs = load_yaml(&worktree.join(project::FILE), project::parse)?.unwrap_or_default();
    let common = git::common_dir(&worktree).map_err(|e| usage(&e.to_string()))?;
    let slot_name = slot::slot_name(git::origin(&worktree).as_deref(), &common.to_string_lossy());
    let paths = slot::slot_paths(&dirs.cache, &slot_name);
    let output = project::output(&worktree);
    let to_slot = |p: &str| in_slot(p, &cwd, &worktree, &output.dir, &paths.source);
    let moved = match &mut request {
        Request::Xcodebuild(inv) => args::relocate(&mut inv.args, to_slot),
        Request::Cargo(inv) => cargo_args::relocate(&mut inv.args, to_slot),
    };
    for (flag, from, to) in moved {
        if Path::new(&to).starts_with(&paths.source) {
            eprintln!("{}", report::relocated_line(&flag, &from, &to));
        }
    }
    Ok((Context { worktree, prefix, config, slot_name, paths, output, ignored_inputs }, request))
}

/// Where xcodebuild, running in the slot, finds `value`. A path inside the worktree, however it is
/// spelled (through a symbolic link, or not created yet), becomes the slot's copy. A path into the
/// worktree's output stays there, as an absolute path: the slot has no copy of it (an `.xctestrun`
/// from an earlier `build-for-testing`, say). Other relative paths need nothing: xcodebuild runs in
/// the same directory of the slot.
fn in_slot(value: &str, cwd: &Path, worktree: &Path, output: &Path, slot_src: &Path) -> Option<String> {
    let path = cwd.join(value);
    let real = path.canonicalize().ok().or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))?;
    if real.starts_with(output) {
        return Path::new(value).is_relative().then(|| real.to_string_lossy().into_owned());
    }
    if Path::new(value).is_relative() {
        return None;
    }
    let rest = real.strip_prefix(worktree).ok()?;
    Some(slot_src.join(rest).to_string_lossy().into_owned())
}

/// An optional YAML file: `None` when it does not exist; a parse error names the file and line.
fn load_yaml<T>(path: &Path, parse: impl Fn(&str) -> Result<T, config::ConfigError>) -> Result<Option<T>, Outcome> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text).map(Some).map_err(|e| usage(&format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(usage(&format!("cannot read {}: {e}", path.display()))),
    }
}
