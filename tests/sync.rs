//! Slot and sync behaviour through the real binary, with a fake xcodebuild.
mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use support::*;

// A1.4
#[test]
fn outside_a_git_repository_it_refuses_and_creates_nothing() {
    let w = World::new("nogit");
    let dir = w.path("plain");
    fs::create_dir_all(&dir).expect("mkdir");
    let r = w.build(&dir, &[]);
    assert_eq!(r.code(), 64, "{}", r.all());
    assert!(r.stderr().contains("inside a git repository"), "{}", r.all());
    assert!(!w.cache().join("slots").exists());
}

// A2
#[test]
fn only_files_that_changed_are_rewritten_and_worktrees_share_the_slot() -> TestResult {
    let w = World::new("minimal");
    let a = repo(&w.path("a"));
    write(&a.join("App/C.swift"), "let c = 3\n");
    commit_all(&a, "c");
    let files = ["App/A.swift", "App/B.swift", "App/C.swift"];
    assert_eq!(w.build(&a, &[]).code(), 0);
    let slot = w.slot_src();
    let first = mtimes(&slot, &files);

    std::thread::sleep(std::time::Duration::from_millis(1100)); // mtime granularity
    assert_eq!(w.build(&a, &[]).code(), 0);
    assert_eq!(mtimes(&slot, &files), first, "a rebuild with no change must not touch any file");

    write(&a.join("App/B.swift"), "let b = 20\n");
    assert_eq!(w.build(&a, &[]).code(), 0);
    let after = mtimes(&slot, &files);
    assert_eq!((after[0] == first[0], after[1] == first[1], after[2] == first[2]), (true, false, true), "only B changes");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(&a.join("untracked.swift"), "let u = 1\n");
    assert_eq!(w.build(&a, &[]).code(), 0);
    let untracked = mtimes(&slot, &["untracked.swift"]);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(w.build(&a, &[]).code(), 0);
    assert_eq!(mtimes(&slot, &files), after, "uncommitted changes that did not change again are not rewritten");
    assert_eq!(mtimes(&slot, &["untracked.swift"]), untracked);
    fs::remove_file(a.join("untracked.swift"))?;

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let before_b = mtimes(&slot, &files);
    git(&a, &["stash", "-q"]);
    git(&a, &["branch", "feature"]);
    let b = w.path("b");
    git(&a, &["worktree", "add", "-q", &b.to_string_lossy(), "feature"]);
    write(&b.join("App/C.swift"), "let c = 30\n");
    commit_all(&b, "c30");
    assert_eq!(w.build(&b, &[]).code(), 0);
    assert_eq!(w.slot_src(), slot, "the second worktree uses the same slot");
    let now = mtimes(&slot, &files);
    assert_eq!((now[0] == before_b[0], now[2] == before_b[2]), (true, false), "only the file that differs between worktrees changes");
    assert_eq!(fs::read_to_string(slot.join("App/C.swift"))?, "let c = 30\n");
    Ok(())
}

fn step(w: &World, wt: &std::path::Path, label: &str) {
    let r = w.build(wt, &[]);
    assert_eq!(r.code(), 0, "{label}: {}", r.all());
    let (want, got) = (snapshot(wt), snapshot(&w.slot_src()));
    let diff: Vec<_> = want.keys().chain(got.keys()).filter(|k| want.get(*k) != got.get(*k)).collect();
    assert!(diff.is_empty(), "{label}: slot differs from worktree at {diff:?}");
}

// A3
#[test]
fn the_slot_matches_the_worktree_byte_for_byte_in_every_state() {
    let w = World::new("fidelity");
    let lib = repo(&w.path("lib"));
    let inner = repo(&w.path("inner"));
    git(&lib, &["submodule", "add", "-q", &inner.to_string_lossy(), "Inner"]);
    commit_all(&lib, "nested");
    let wt = repo(&w.path("app"));
    git(&wt, &["submodule", "add", "-q", &lib.to_string_lossy(), "Lib"]);
    git(&wt, &["submodule", "update", "-q", "--init", "--recursive"]);
    commit_all(&wt, "submodule");
    step(&w, &wt, "first build with a nested submodule");

    write(&wt.join("App/A.swift"), "let a = 10\n");
    step(&w, &wt, "modified file");
    write(&wt.join("New Dir/文件 b.swift"), "let n = 1\n");
    step(&w, &wt, "untracked file with spaces and Chinese in its name");
    fs::remove_file(wt.join("App/B.swift")).expect("rm");
    step(&w, &wt, "deleted file");
    git(&wt, &["rm", "-q", "--cached", ".gitignore"]);
    fs::remove_file(wt.join(".gitignore")).expect("rm");
    step(&w, &wt, "staged deletion");
    git(&wt, &["checkout", "-q", "--", "."]);
    git(&wt, &["reset", "-q", "--hard"]);
    step(&w, &wt, "earlier uncommitted changes undone (untracked file still there)");
    fs::remove_dir_all(wt.join("New Dir")).expect("rm");
    step(&w, &wt, "untracked file copied last round is gone");
    git(&wt, &["mv", "App/A.swift", "App/Renamed.swift"]);
    step(&w, &wt, "rename");
    symlink("Renamed.swift", wt.join("App/Link.swift")).expect("symlink");
    fs::set_permissions(wt.join("App/B.swift"), fs::Permissions::from_mode(0o755)).expect("chmod");
    step(&w, &wt, "symlink and executable bit");
    write(&wt.join("local.generated"), "ignored\n");
    step(&w, &wt, "ignored file");
    assert!(!w.slot_src().join("local.generated").exists(), "ignored files are not copied");
    commit_all(&wt, "rename, link, mode");
    step(&w, &wt, "committed");
    write(&wt.join("Lib/Lib.swift"), "let l = 1\n");
    commit_all(&wt.join("Lib"), "lib change");
    write(&wt.join("Lib/Inner/In.swift"), "let i = 1\n");
    step(&w, &wt, "submodule moved to a new commit, nested submodule has an untracked file");
    git(&wt, &["checkout", "-q", "HEAD~2"]);
    git(&wt, &["submodule", "update", "-q", "--recursive"]);
    step(&w, &wt, "switched to an older commit");
}

// A4
#[test]
fn the_worktree_is_left_untouched_and_nothing_goes_to_the_network() -> TestResult {
    let w = World::new("boundary");
    let lib = repo(&w.path("lib"));
    let wt = repo(&w.path("app"));
    git(&wt, &["submodule", "add", "-q", &lib.to_string_lossy(), "Lib"]);
    commit_all(&wt, "submodule");
    git(&wt, &["remote", "add", "origin", "https://unreachable.invalid/team/app.git"]);
    let clone = w.path("clone");
    git(&w.root, &["clone", "-q", &wt.to_string_lossy(), &clone.to_string_lossy()]);
    git(&clone, &["remote", "set-url", "origin", "https://unreachable.invalid/team/app.git"]);
    write(&clone.join("App/A.swift"), "let a = 5\n");

    let index = || fs::read(clone.join(".git/index")).expect("index");
    let before = (
        git(&clone, &["status", "--porcelain"]),
        git(&clone, &["rev-parse", "HEAD"]),
        index(),
        fs::read(clone.join(".git/config"))?,
        snapshot(&clone),
    );
    let r = w.build(&clone, &[]);
    assert_eq!(r.code(), 0, "{}", r.all());
    let after = (
        git(&clone, &["status", "--porcelain"]),
        git(&clone, &["rev-parse", "HEAD"]),
        index(),
        fs::read(clone.join(".git/config"))?,
        snapshot(&clone),
    );
    assert!(before == after, "the worktree changed");
    let slot = w.slot_src();
    let lib_entries = fs::read_dir(slot.join("Lib")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(lib_entries, 0, "an uninitialised submodule stays empty in the slot");
    assert_eq!(fs::read_to_string(slot.join("App/A.swift"))?, "let a = 5\n");
    Ok(())
}
