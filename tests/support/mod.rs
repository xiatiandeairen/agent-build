//! Shared by the integration tests: throwaway git repositories, an isolated HOME/XDG,
//! and running the real agb binary against tests/fixtures/fake-xcodebuild.
#![allow(dead_code)] // each test file uses a different part of this module
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A temporary directory holding repositories, HOME and the XDG directories for one test.
pub struct World {
    pub root: PathBuf,
}

impl World {
    pub fn new(name: &str) -> World {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("agent-build-it-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root); // leftovers from an aborted run
        fs::create_dir_all(root.join("home")).expect("create test world");
        World { root: root.canonicalize().expect("canonical test root") }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("xdg-cache/agent-build")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("xdg-config/agent-build/config.yaml")
    }

    /// The single slot's `source` directory (fails if there is not exactly one slot).
    pub fn slot_src(&self) -> PathBuf {
        let slots: Vec<PathBuf> = fs::read_dir(self.cache().join("slots")).expect("slots dir").map(|e| e.expect("entry").path()).collect();
        assert_eq!(slots.len(), 1, "expected one slot, found {slots:?}");
        slots[0].join("source")
    }

    pub fn command(&self, cwd: &Path, env: &[(&str, String)]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_agb"));
        c.current_dir(cwd)
            .env("HOME", self.root.join("home"))
            .env("XDG_CACHE_HOME", self.root.join("xdg-cache"))
            .env("XDG_STATE_HOME", self.root.join("xdg-state"))
            .env("XDG_CONFIG_HOME", self.root.join("xdg-config"))
            .env("AGENT_BUILD_XCODEBUILD", fixture("fake-xcodebuild"))
            .env("AGENT_BUILD_XCRESULTTOOL", fixture("fake-xcresulttool"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in env {
            c.env(k, v);
        }
        c
    }

    /// `agb xcodebuild build` from `cwd`, waiting for it.
    pub fn build(&self, cwd: &Path, env: &[(&str, String)]) -> Run {
        self.run(cwd, &["xcodebuild", "build", "-scheme", "App"], env)
    }

    pub fn run(&self, cwd: &Path, args: &[&str], env: &[(&str, String)]) -> Run {
        Run(self.command(cwd, env).args(args).output().expect("run agent-build"))
    }

    pub fn spawn(&self, cwd: &Path, env: &[(&str, String)]) -> Child {
        self.command(cwd, env)
            .args(["xcodebuild", "build", "-scheme", "App"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn agent-build")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = Command::new("chmod").args(["-R", "u+w"]).arg(&self.root).status(); // tests may make dirs read-only
        let _ = fs::remove_dir_all(&self.root); // best effort
    }
}

pub struct Run(pub Output);

impl Run {
    pub fn code(&self) -> i32 {
        self.0.status.code().unwrap_or(-1)
    }
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.0.stdout).into_owned()
    }
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.0.stderr).into_owned()
    }
    pub fn all(&self) -> String {
        format!("--- stdout\n{}--- stderr\n{}", self.stdout(), self.stderr())
    }
}

/// A file under tests/fixtures.
pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rel)
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "protocol.file.allow=always", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

pub fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write file");
}

/// A repository with a couple of files and one commit.
pub fn repo(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).expect("mkdir repo");
    git(dir, &["init", "-q"]);
    write(&dir.join("App/A.swift"), "let a = 1\n");
    write(&dir.join("App/B.swift"), "let b = 2\n");
    write(&dir.join(".gitignore"), "*.generated\n");
    commit_all(dir, "initial");
    dir.to_path_buf()
}

pub fn commit_all(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Every tracked and non-ignored file of a checkout and its initialised submodules, by relative path:
/// (content or link target, executable bit).
pub fn snapshot(dir: &Path) -> BTreeMap<String, (Vec<u8>, bool)> {
    let mut out = BTreeMap::new();
    collect(dir, dir, &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, (Vec<u8>, bool)>) {
    let links: Vec<String> = git(dir, &["ls-files", "-s"])
        .lines()
        .filter(|l| l.starts_with("160000 "))
        .filter_map(|l| l.split('\t').nth(1).map(String::from))
        .collect();
    for f in
        git(dir, &["ls-files", "-z", "-co", "--exclude-standard"]).split('\0').filter(|f| !f.is_empty() && !links.contains(&f.to_string()))
    {
        let p = dir.join(f);
        let Ok(meta) = fs::symlink_metadata(&p) else { continue }; // deleted but still in the index
        if meta.is_dir() {
            continue; // an untracked nested repository: a separate repository, not mirrored
        }
        let content = if meta.file_type().is_symlink() {
            fs::read_link(&p).expect("link").to_string_lossy().as_bytes().to_vec()
        } else {
            fs::read(&p).expect("read")
        };
        let rel = p.strip_prefix(root).expect("under root").to_string_lossy().into_owned();
        out.insert(rel, (content, meta.permissions().mode() & 0o111 != 0));
    }
    for l in links {
        let sub = dir.join(&l);
        if sub.join(".git").exists() {
            collect(root, &sub, out);
        }
    }
}

pub fn mtimes(dir: &Path, files: &[&str]) -> Vec<std::time::SystemTime> {
    files.iter().map(|f| fs::metadata(dir.join(f)).and_then(|m| m.modified()).expect("mtime")).collect()
}
