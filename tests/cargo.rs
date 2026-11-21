//! `agb cargo` through the real binary and the real cargo, on tests/fixtures/CargoSample: a
//! two-crate workspace with no dependencies (no network), a build script, a static library and a
//! program. Each build takes a second or two.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use support::*;

/// Cargo and rustup find their toolchains under the real HOME, which the tests replace.
fn toolchain_env() -> Vec<(&'static str, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| format!("{home}/{d}"));
    vec![("CARGO_HOME", or("CARGO_HOME", ".cargo")), ("RUSTUP_HOME", or("RUSTUP_HOME", ".rustup")), ("CARGO_NET_OFFLINE", "true".into())]
}

fn with(extra: &[(&'static str, String)]) -> Vec<(&'static str, String)> {
    let mut v = toolchain_env();
    v.extend(extra.iter().cloned());
    v
}

/// A repository holding the sample workspace, committed.
fn sample(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir.parent().expect("parent")).expect("mkdir");
    assert!(Command::new("cp").arg("-R").arg(fixture("CargoSample")).arg(dir).status().expect("cp").success());
    write(&dir.join(".gitignore"), "target/\n");
    git(dir, &["init", "-q"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "sample"]);
    dir.to_path_buf()
}

fn agb(w: &World, cwd: &Path, args: &[&str], extra: &[(&'static str, String)]) -> Run {
    let mut full = vec!["cargo"];
    full.extend_from_slice(args);
    w.run(cwd, &full, &with(extra))
}

fn out(worktree: &Path, rel: &str) -> PathBuf {
    worktree.join(".agents/build/output").join(rel)
}

fn set_version(repo: &Path, v: &str) {
    write(&repo.join("core/src/version.txt"), v);
    let lib = fs::read_to_string(repo.join("core/src/lib.rs")).expect("read lib");
    let lib = lib.replacen("\"v1\"", "include_str!(\"version.txt\")", 1);
    write(&repo.join("core/src/lib.rs"), &lib);
}

// A21
#[test]
fn builds_tests_and_runs_in_the_slot_and_reports_like_a_compiler() {
    let w = World::new("cargo-basic");
    let r = sample(&w.path("repo"));
    let first = agb(&w, &r, &["build"], &[]);
    assert_eq!(first.code(), 0, "{}", first.all());
    assert!(out(&r, "target/debug/libsample_ffi.a").is_file(), "{}", first.all());
    assert!(!r.join("target").exists(), "nothing is built in the worktree");
    let again = agb(&w, &r, &["build"], &[]);
    assert!(again.stderr().contains("units fresh=4 rebuilt=0"), "{}", again.all());
    let ran = agb(&w, &r, &["run", "-q", "--bin", "sample-tool"], &[]);
    assert_eq!((ran.code(), ran.stdout().trim()), (0, "sample-tool v1"), "{}", ran.all());
    let failed = agb(&w, &r, &["test"], &[("SAMPLE_FAIL", "1".into())]);
    assert_eq!(failed.code(), 101, "{}", failed.all());
    let at = format!("agb: test failed: tests::version_is_set: {}/core/src/lib.rs:11:9: SAMPLE_FAIL is set", r.display());
    assert!(failed.stderr().contains(&at), "{}", failed.all());
    fs::write(r.join("ffi/src/lib.rs"), "pub fn broken() -> u8 { \"x\" }\n").expect("write");
    let broken = agb(&w, &r, &["build"], &[]);
    assert_eq!(broken.code(), 101, "{}", broken.all());
    let error = format!("{}/ffi/src/lib.rs:1:25: error: mismatched types", r.display());
    assert!(broken.stderr().contains(&error), "{}", broken.all());
    assert!(fs::read_to_string(out(&r, "build.log")).expect("log").contains("expected `u8`"), "the log has rustc's full text");
}

// A22
#[test]
fn a_worktree_on_an_older_commit_never_gets_another_worktrees_build() {
    let w = World::new("cargo-older");
    let x = sample(&w.path("x"));
    let y = w.path("y");
    git(&x, &["worktree", "add", "-q", "--detach", &y.to_string_lossy()]);
    // y's files are older than any build of x, which is when Cargo, judging by modification time
    // alone, would take x's build for y's (measured with a shared build directory).
    assert!(Command::new("touch").args(["-t", "200001010000"]).arg(y.join("core/src/lib.rs")).status().expect("touch").success());
    set_version(&x, "v2");
    git(&x, &["add", "-A"]);
    git(&x, &["commit", "-qm", "v2"]);
    for (tree, want) in [(&x, "v2"), (&y, "v1"), (&x, "v2"), (&y, "v1")] {
        let ran = agb(&w, tree, &["run", "-q", "--bin", "sample-tool"], &[]);
        assert_eq!((ran.code(), ran.stdout().trim()), (0, format!("sample-tool {want}").as_str()), "{}", ran.all());
    }
}

fn rebuilt(r: &Run) -> String {
    r.stderr().lines().find_map(|l| l.strip_prefix("agb: rebuilt: ")).unwrap_or_default().to_string()
}

// A23
#[test]
fn another_worktree_reuses_the_slot_and_rebuilds_only_what_differs() {
    let w = World::new("cargo-reuse");
    let x = sample(&w.path("x"));
    let y = w.path("y");
    git(&x, &["worktree", "add", "-q", "--detach", &y.to_string_lossy()]);
    assert_eq!(agb(&w, &x, &["build"], &[]).code(), 0);
    let same = agb(&w, &y, &["build"], &[]);
    assert!(same.stderr().contains("rebuilt=0"), "same commit, nothing to build: {}", same.all());
    fs::write(y.join("ffi/src/bin/sample-tool.rs"), "fn main() { println!(\"changed\"); }\n").expect("write");
    let one = agb(&w, &y, &["build"], &[]);
    assert_eq!(rebuilt(&one), "sample-ffi", "only the changed crate: {}", one.all());
}

// A24
#[test]
fn rustc_runs_without_the_callers_wrapper() {
    let w = World::new("cargo-wrapper");
    let r = sample(&w.path("repo"));
    let wrapper = w.path("bad-wrapper");
    write(&wrapper, "#!/bin/sh\necho wrapper ran >&2\nexit 1\n");
    assert!(Command::new("chmod").arg("+x").arg(&wrapper).status().expect("chmod").success());
    let built = agb(&w, &r, &["build"], &[("RUSTC_WRAPPER", wrapper.to_string_lossy().into_owned())]);
    assert_eq!(built.code(), 0, "{}", built.all());
    assert!(built.stderr().contains("agb: RUSTC_WRAPPER cleared"), "{}", built.all());
}

// A25
#[test]
fn owns_the_target_directory_points_manifests_at_the_slot_and_refuses_what_does_not_build() {
    let w = World::new("cargo-args");
    let r = sample(&w.path("repo"));
    let elsewhere = w.path("elsewhere");
    let manifest = r.join("Cargo.toml").to_string_lossy().into_owned();
    let dir = elsewhere.to_string_lossy().into_owned();
    let built = agb(&w, &r, &["build", "--target-dir", &dir, "--manifest-path", &manifest], &[("CARGO_TARGET_DIR", dir.clone())]);
    assert_eq!(built.code(), 0, "{}", built.all());
    assert!(built.stderr().contains(&format!("agb: ignored --target-dir {dir}")), "{}", built.all());
    assert!(built.stderr().contains("--manifest-path") && built.stderr().contains("building the slot's copy"), "{}", built.all());
    assert!(!elsewhere.exists(), "neither the flag nor the environment moved the target directory");
    for sub in [&["fmt"][..], &["add", "x"], &["update"], &["clippy", "--fix"], &["install", "--path", "."]] {
        let refused = agb(&w, &r, sub, &[]);
        assert_eq!(refused.code(), 64, "{sub:?}: {}", refused.all());
    }
    let target = w.slot_src().with_file_name("target");
    assert!(target.is_dir());
    let cleaned = agb(&w, &r, &["clean"], &[]);
    assert_eq!(cleaned.code(), 0, "{}", cleaned.all());
    assert!(!target.join("debug").exists(), "clean empties the slot's target directory");
}

// A26
#[test]
fn hands_back_only_this_builds_products_and_keeps_them_through_checks() {
    let w = World::new("cargo-products");
    let r = sample(&w.path("repo"));
    assert_eq!(agb(&w, &r, &["build"], &[]).code(), 0);
    let mut found: Vec<String> = Vec::new();
    let mut todo = vec![out(&r, "target")];
    while let Some(d) = todo.pop() {
        for p in fs::read_dir(&d).expect("read").map(|e| e.expect("entry").path()) {
            if p.is_dir() {
                todo.push(p)
            } else {
                found.push(p.strip_prefix(out(&r, "target")).expect("under").to_string_lossy().into_owned())
            }
        }
    }
    found.sort();
    assert_eq!(found, ["debug/libsample_ffi.a", "debug/sample-tool"]);
    assert_eq!(agb(&w, &r, &["check"], &[]).code(), 0);
    assert_eq!(agb(&w, &r, &["test"], &[]).code(), 0);
    assert!(out(&r, "target/debug/libsample_ffi.a").is_file(), "check and test leave the products alone");
    fs::write(r.join("ffi/src/lib.rs"), "pub fn broken() -> u8 { \"x\" }\n").expect("write");
    let broken = agb(&w, &r, &["build"], &[]);
    assert_eq!(broken.code(), 101);
    assert!(!out(&r, "target").exists() && !broken.stderr().contains("products="), "a failed build hands back nothing old");
}

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out waiting for {}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

// A27
#[test]
fn xcodebuild_and_cargo_share_one_slot_and_keep_their_own_outputs() {
    let w = World::new("cargo-mixed");
    let r = sample(&w.path("repo"));
    let fake = vec![
        ("FAKE_SLEEP", "2".to_string()),
        ("FAKE_STARTED", w.path("x-started").to_string_lossy().into_owned()),
        ("FAKE_PRODUCT", "app".into()),
    ];
    let xcode: Child =
        w.command(&r, &fake).args(["xcodebuild", "build", "-scheme", "App"]).stderr(std::process::Stdio::piped()).spawn().expect("spawn");
    wait_for(&w.path("x-started"));
    let pid = xcode.id();
    let cargo = agb(&w, &r, &["build"], &[]);
    let xcode = Run(xcode.wait_with_output().expect("wait"));
    assert_eq!((xcode.code(), cargo.code()), (0, 0), "{}\n{}", xcode.all(), cargo.all());
    assert!(cargo.stderr().contains(&format!("waiting for the slot: pid {pid}")), "{}", cargo.all());
    assert!(out(&r, "Products/Debug/App.app/App").is_file() && out(&r, "target/debug/libsample_ffi.a").is_file());
}
