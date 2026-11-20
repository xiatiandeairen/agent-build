//! Steps 1–4 of a run: parse the arguments, find the worktree and its slot, take the slot's lock,
//! and sync the slot to the worktree. Step 5 and 6 (build, report) are in `build`.
use crate::build;
use agent_build_core::config::{self, Config, Dirs};
use agent_build_core::report::{self, Outcome};
use agent_build_core::{args, project, slot};
use agent_build_sys::lock::{LockError, SlotLock};
use agent_build_sys::{git, ignored, signals, sync};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, fs, io};

pub struct Context {
    pub inv: args::Invocation,
    pub worktree: PathBuf,
    /// The caller's directory relative to the worktree root, so xcodebuild runs in the same place in the slot.
    pub prefix: String,
    pub dirs: Dirs,
    pub config: Config,
    pub slot_name: String,
    pub paths: slot::SlotPaths,
    /// Git-ignored paths the project declares its build needs (`.agent-build.yaml`, precondition P8).
    pub ignored_inputs: Vec<String>,
}

pub fn run(argv: &[String]) -> Outcome {
    let ctx = match prepare(argv) {
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
    let synced = sync::sync(&ctx.worktree, &ctx.paths.src)
        .and_then(|_| ignored::sync(&ctx.worktree, &ctx.paths.src, &ctx.ignored_inputs, &ctx.paths.ignored_record));
    if let Err(e) = synced {
        eprintln!("agb: sync failed, nothing was built: {e}");
        return Outcome::SyncFailed;
    }
    if let Some(sig) = signals::pending() {
        return Outcome::Signaled(sig);
    }
    build::run(&ctx, &lock, started.elapsed().as_secs_f64())
}

fn usage(message: &str) -> Outcome {
    eprintln!("agb: {message}");
    Outcome::Usage
}

fn prepare(argv: &[String]) -> Result<Context, Outcome> {
    let inv = args::parse(argv).map_err(|e| usage(&e.message()))?;
    for d in &inv.dropped {
        eprintln!("{}", report::dropped_line(d));
    }
    let cwd = env::current_dir().map_err(|e| usage(&format!("cannot read the current directory: {e}")))?;
    let worktree = git::toplevel(&cwd).map_err(|_| usage("run agb inside a git repository (precondition P3)"))?;
    git::head(&worktree).map_err(|_| usage("the repository has no commit yet"))?;
    let prefix = git::prefix(&cwd).unwrap_or_default();
    let dirs = config::dirs(|k| env::var(k).ok()).ok_or_else(|| usage("HOME is not set"))?;
    let config = load_yaml(&dirs.config.join("config.yaml"), config::parse)?.unwrap_or_default();
    let ignored_inputs = load_yaml(&worktree.join(project::FILE), project::parse)?.unwrap_or_default();
    let common = git::common_dir(&worktree).map_err(|e| usage(&e.to_string()))?;
    let slot_name = slot::slot_name(git::origin(&worktree).as_deref(), &common.to_string_lossy());
    let paths = slot::slot_paths(&dirs.cache, &slot_name);
    Ok(Context { inv, worktree, prefix, dirs, config, slot_name, paths, ignored_inputs })
}

/// An optional YAML file: `None` when it does not exist; a parse error names the file and line.
fn load_yaml<T>(path: &Path, parse: impl Fn(&str) -> Result<T, config::ConfigError>) -> Result<Option<T>, Outcome> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text).map(Some).map_err(|e| usage(&format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(usage(&format!("cannot read {}: {e}", path.display()))),
    }
}
