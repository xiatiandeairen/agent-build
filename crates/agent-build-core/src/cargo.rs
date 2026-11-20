//! What the caller asked for: `agb cargo <args...>`.
//! Only subcommands that build (and `clean`) run in the slot; the target directory and the message
//! format belong to agb, so the flags that set them are dropped here. Everything after `--` belongs
//! to the program or test harness and is passed on untouched.

/// What the subcommand does, which decides what agb reads and hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `build`, `rustc`: products are copied to the worktree's output.
    Build,
    /// `run`, `bench`: the program's own output is shown as is; nothing is copied.
    Run,
    /// `test`: failed tests are listed; nothing is copied.
    Test,
    /// `check`, `clippy`, `doc`: diagnostics only.
    Check,
    /// `clean`: removes the slot's target directory; its output is shown as is.
    Clean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Arguments for cargo, in the caller's order, minus the dropped flags.
    pub args: Vec<String>,
    /// Each dropped flag with its value, e.g. `--target-dir /tmp/t`.
    pub dropped: Vec<String>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoError {
    NoSubcommand,
    /// The subcommand does not build, or changes files that the next sync would put back.
    NotBuilding(String),
    MissingValue(String),
}

pub const USAGE: &str = "usage: agb cargo <build|check|test|bench|run|doc|clippy|rustc|clean> [arguments...]";

impl CargoError {
    pub fn message(&self) -> String {
        match self {
            CargoError::NoSubcommand => USAGE.to_string(),
            CargoError::NotBuilding(s) => {
                format!("`cargo {s}` is not run in the build slot: agb only builds; run cargo directly for it\n{USAGE}")
            }
            CargoError::MissingValue(f) => format!("{f} needs a value"),
        }
    }
}

/// Flags agb owns: the target directory is the slot's, and agb reads cargo's JSON messages.
const DROPPED: [&str; 2] = ["--target-dir", "--message-format"];
/// Options before the subcommand that take a value (`cargo --config k=v build`).
const GLOBAL_VALUE_FLAGS: [&str; 5] = ["--config", "-Z", "--color", "-C", "--explain"];

fn kind_of(sub: &str) -> Option<Kind> {
    Some(match sub {
        "build" | "b" | "rustc" => Kind::Build,
        "run" | "r" | "bench" => Kind::Run,
        "test" | "t" => Kind::Test,
        "check" | "c" | "clippy" | "doc" | "d" => Kind::Check,
        "clean" => Kind::Clean,
        _ => return None,
    })
}

/// `argv` is everything after `cargo`.
pub fn parse(argv: &[String]) -> Result<Invocation, CargoError> {
    let end = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    let (ours, theirs) = argv.split_at(end);
    let sub_at = subcommand_at(ours).ok_or(CargoError::NoSubcommand)?;
    let sub = &ours[sub_at];
    let kind = kind_of(sub).ok_or_else(|| CargoError::NotBuilding(sub.clone()))?;
    if sub == "clippy" && ours.iter().any(|a| a == "--fix") {
        return Err(CargoError::NotBuilding("clippy --fix".to_string()));
    }
    let mut args = Vec::new();
    let mut dropped = Vec::new();
    let mut i = 0;
    while i < ours.len() {
        let a = &ours[i];
        if let Some(flag) = DROPPED.iter().find(|f| a.strip_prefix(**f).is_some_and(|r| r.starts_with('='))) {
            dropped.push(format!("{flag} {}", &a[flag.len() + 1..]));
        } else if DROPPED.contains(&a.as_str()) {
            let v = ours.get(i + 1).ok_or_else(|| CargoError::MissingValue(a.clone()))?;
            dropped.push(format!("{a} {v}"));
            i += 1;
        } else {
            args.push(a.clone());
        }
        i += 1;
    }
    args.extend(theirs.iter().cloned());
    Ok(Invocation { args, dropped, kind })
}

/// Index of the subcommand: the first word that is neither a `+toolchain`, an option, nor an
/// option's value.
fn subcommand_at(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if GLOBAL_VALUE_FLAGS.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if !a.starts_with('-') && !a.starts_with('+') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The arguments agb adds before the caller's: cargo's JSON messages, which the report is read
/// from. `clean` takes none. The target directory goes in the environment (`env`).
pub fn injected(kind: Kind) -> Vec<String> {
    match kind {
        Kind::Clean => Vec::new(),
        _ => vec!["--message-format=json".to_string()],
    }
}

/// Inserts `injected` right after the subcommand, so options of a subcommand stay with it and
/// anything after `--` stays last.
pub fn command_line(inv: &Invocation) -> Vec<String> {
    let mut v = inv.args.clone();
    if let Some(at) = subcommand_at(&v[..v.iter().position(|a| a == "--").unwrap_or(v.len())]) {
        let extra = injected(inv.kind);
        v.splice(at + 1..at + 1, extra);
    }
    v
}

/// Environment variables that put a program in front of rustc (kache, sccache). Set empty, they
/// override `build.rustc-wrapper` in Cargo's config files too. A wrapper's cache decides reuse by
/// its own key, which can miss inputs that change the result (precondition: correctness rests on
/// Cargo's own tracking alone); the slot's fixed target directory already reuses what Cargo can.
pub const WRAPPER_VARS: [&str; 4] =
    ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"];

/// The environment cargo runs with in the slot: its target directory, and no rustc wrapper.
pub fn env(target_dir: &str) -> Vec<(String, String)> {
    let mut v = vec![("CARGO_TARGET_DIR".to_string(), target_dir.to_string())];
    v.extend(WRAPPER_VARS.iter().map(|k| (k.to_string(), String::new())));
    v
}

/// Values of `--manifest-path`, the only cargo flag that names a path to build from.
pub fn relocate(args: &mut [String], to_slot: impl Fn(&str) -> Option<String>) -> Vec<(String, String, String)> {
    const FLAG: &str = "--manifest-path";
    let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let mut moved = Vec::new();
    for i in 0..end {
        let (value, joined) = match args[i].strip_prefix(FLAG).and_then(|r| r.strip_prefix('=')) {
            Some(v) => (v.to_string(), true),
            None if i > 0 && args[i - 1] == FLAG => (args[i].clone(), false),
            None => continue,
        };
        if let Some(to) = to_slot(&value) {
            args[i] = if joined { format!("{FLAG}={to}") } else { to.clone() };
            moved.push((FLAG.to_string(), value, to));
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    // A25.1
    #[test]
    fn finds_the_subcommand_and_runs_only_those_that_build() {
        let kind = |s: &[&str]| parse(&v(s)).map(|i| i.kind);
        assert_eq!(kind(&["build", "-p", "x"]), Ok(Kind::Build));
        assert_eq!(kind(&["+1.83.0", "--config", "build.jobs=2", "-v", "test"]), Ok(Kind::Test));
        assert_eq!(kind(&["--color", "never", "b"]), Ok(Kind::Build));
        assert_eq!(kind(&["rustc", "--crate-type", "staticlib"]), Ok(Kind::Build));
        for (sub, k) in [("run", Kind::Run), ("bench", Kind::Run), ("check", Kind::Check), ("clippy", Kind::Check)] {
            assert_eq!(kind(&[sub]), Ok(k), "{sub}");
        }
        assert_eq!(kind(&["doc"]), Ok(Kind::Check));
        assert_eq!(kind(&["clean"]), Ok(Kind::Clean));
        for sub in
            ["fmt", "fix", "add", "remove", "update", "generate-lockfile", "vendor", "install", "publish", "package", "metadata", "tree"]
        {
            assert_eq!(kind(&[sub]), Err(CargoError::NotBuilding(sub.into())), "{sub}");
        }
        assert_eq!(kind(&["clippy", "--fix"]), Err(CargoError::NotBuilding("clippy --fix".into())));
        assert_eq!(kind(&["run", "--", "--fix"]), Ok(Kind::Run), "after -- belongs to the program");
        assert_eq!(kind(&[]), Err(CargoError::NoSubcommand));
        assert_eq!(kind(&["--version"]), Err(CargoError::NoSubcommand));
        assert!(CargoError::NotBuilding("fmt".into()).message().contains("run cargo directly"));
    }

    // A25.1
    #[test]
    fn drops_the_target_dir_and_message_format_with_their_values_but_not_after_dashes() {
        let inv = parse(&v(&["build", "--target-dir", "/t", "-p", "x", "--message-format=short", "--release", "--", "--target-dir", "y"]))
            .unwrap();
        assert_eq!(inv.args, v(&["build", "-p", "x", "--release", "--", "--target-dir", "y"]));
        assert_eq!(inv.dropped, v(&["--target-dir /t", "--message-format short"]));
        assert_eq!(parse(&v(&["build", "--target-dir"])), Err(CargoError::MissingValue("--target-dir".into())));
    }

    // A25.1
    #[test]
    fn adds_json_messages_after_the_subcommand_and_clears_rustc_wrappers() {
        let line = |s: &[&str]| command_line(&parse(&v(s)).unwrap());
        assert_eq!(
            line(&["+1.83.0", "test", "-p", "x", "--", "--nocapture"]),
            v(&["+1.83.0", "test", "--message-format=json", "-p", "x", "--", "--nocapture"])
        );
        assert_eq!(line(&["clean"]), v(&["clean"]));
        let e = env("/s/target");
        assert_eq!(e[0], ("CARGO_TARGET_DIR".to_string(), "/s/target".to_string()));
        assert_eq!(e.len(), 1 + WRAPPER_VARS.len());
        assert!(e[1..].iter().all(|(k, val)| WRAPPER_VARS.contains(&k.as_str()) && val.is_empty()));
    }

    // A25.1
    #[test]
    fn points_a_worktree_manifest_path_at_the_slot() {
        let to = |p: &str| p.strip_prefix("/w").map(|r| format!("/s{r}"));
        let mut a =
            v(&["build", "--manifest-path", "/w/a/Cargo.toml", "--manifest-path=/w/b/Cargo.toml", "--manifest-path", "/o/Cargo.toml"]);
        let moved = relocate(&mut a, to);
        assert_eq!(
            a,
            v(&["build", "--manifest-path", "/s/a/Cargo.toml", "--manifest-path=/s/b/Cargo.toml", "--manifest-path", "/o/Cargo.toml"])
        );
        assert_eq!(moved.len(), 2);
    }
}
