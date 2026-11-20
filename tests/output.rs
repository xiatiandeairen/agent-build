//! The worktree's output: what a run hands its caller stays there until this worktree's next run
//! that makes the same kind of output, whatever other worktrees of the project build meanwhile.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use support::*;

fn out(worktree: &Path) -> PathBuf {
    worktree.join(".agent-build/output")
}

fn product(worktree: &Path) -> Option<String> {
    fs::read_to_string(out(worktree).join("Products/Debug/App.app/App")).ok()
}

/// Every file under `dir` with its content and modification time.
fn tree(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut found = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).expect("read dir").map(|e| e.expect("entry").path()) {
            if e.is_dir() {
                todo.push(e);
            } else {
                let meta = fs::metadata(&e).and_then(|m| m.modified()).expect("mtime");
                found.insert(e.clone(), (fs::read(&e).expect("read"), meta));
            }
        }
    }
    found
}

/// A fake build that leaves a product and a log saying `who` made them.
fn made_by(w: &World, who: &str) -> Vec<(&'static str, String)> {
    let says = w.path(&format!("{who}-says"));
    write(&says, &format!("built for {who}\n"));
    vec![("FAKE_PRODUCT", who.to_string()), ("FAKE_XCRESULT", "1".to_string()), ("FAKE_OUTPUT", says.to_string_lossy().into_owned())]
}

fn line_value(stderr: &str, key: &str) -> Option<String> {
    stderr.lines().find_map(|l| l.strip_prefix(&format!("agb: {key}="))).map(|v| v.split(" (").next().unwrap_or(v).to_string())
}

// A19
#[test]
fn another_worktrees_build_leaves_this_worktrees_output_alone() -> TestResult {
    let w = World::new("isolation");
    let a = repo(&w.path("a"));
    let b = w.path("b");
    git(&a, &["worktree", "add", "-q", "-b", "other", &b.to_string_lossy()]);

    let first = w.build(&a, &made_by(&w, "a"));
    assert_eq!(first.code(), 0, "{}", first.all());
    let o = out(&a);
    assert_eq!(line_value(&first.stderr(), "products"), Some(o.join("Products").to_string_lossy().into_owned()), "{}", first.all());
    assert_eq!(line_value(&first.stderr(), "results"), Some(o.join("result.xcresult").to_string_lossy().into_owned()));
    assert!(first.stderr().contains(&format!("log={}", o.join("build.log").display())), "{}", first.all());
    assert!(first.stderr().contains("replaced by this worktree's next agb build"), "{}", first.all());
    assert_eq!(product(&a).as_deref(), Some("a\n"));
    let before = tree(&o);

    let other = w.build(&b, &made_by(&w, "b"));
    assert_eq!(other.code(), 0, "{}", other.all());
    w.slot_src(); // one slot for both worktrees
    assert_eq!(product(&b).as_deref(), Some("b\n"));
    assert_eq!(tree(&o), before, "another worktree's build changed this worktree's output");
    assert!(fs::read_to_string(o.join("build.log"))?.contains("built for a"));

    let failed = w.build(&a, &[made_by(&w, "a2"), vec![("FAKE_EXIT", "65".to_string())]].concat());
    assert_eq!(failed.code(), 65, "{}", failed.all());
    assert_eq!(product(&a), None, "a failed build left the last build's products beside its own results");
    assert!(line_value(&failed.stderr(), "products").is_none(), "{}", failed.all());
    assert!(o.join("result.xcresult").is_dir() && fs::read_to_string(o.join("build.log"))?.contains("built for a2"));
    assert_eq!(product(&b).as_deref(), Some("b\n"), "this worktree's build changed another worktree's output");
    assert!(!w.path("xdg-state").exists(), "agb still writes to the XDG state directory");
    Ok(())
}

// A19.1, A1.7
#[test]
fn only_runs_that_make_an_output_replace_it() -> TestResult {
    let w = World::new("replace");
    let a = repo(&w.path("app"));
    let o = out(&a);
    write(&o.join(".staging/Debug/half"), "cut off\n");
    let r = w.build(&a.join("App"), &made_by(&w, "a"));
    assert_eq!(r.code(), 0, "{}", r.all());
    assert!(!a.join("App/.agent-build").exists(), "the output followed the caller into a subdirectory");
    assert_eq!(product(&a).as_deref(), Some("a\n"));
    assert!(!o.join(".staging").exists(), "a copy cut off by an earlier run is still there");

    let before = tree(&o);
    let info = w.run(&a, &["xcodebuild", "-showBuildSettings", "-scheme", "App"], &made_by(&w, "info"));
    assert_eq!(info.code(), 0, "{}", info.all());
    assert_eq!(tree(&o), before, "an information command changed the output");

    let args = w.path("args");
    let env = [made_by(&w, "x"), vec![("FAKE_ARGS", args.to_string_lossy().into_owned())]].concat();
    let test = w.run(&a, &["xcodebuild", "test-without-building", "-xctestrun", ".agent-build/output/Products/Debug/App.app/App"], &env);
    assert_eq!(test.code(), 0, "{}", test.all());
    let given = fs::read_to_string(&args)?.lines().skip_while(|l| *l != "-xctestrun").nth(1).unwrap_or_default().to_string();
    assert_eq!(Path::new(&given), o.join("Products/Debug/App.app/App"), "the slot has no copy of the output");
    assert!(!test.stderr().contains("building the slot's copy"), "{}", test.all());
    assert_eq!(product(&a).as_deref(), Some("a\n"), "testing without building replaced the products it tested");
    assert!(fs::read_to_string(o.join("build.log"))?.contains("built for x"), "the test run's log is not there");

    fs::create_dir_all(o.join("Old.xcarchive"))?;
    let archive = w.run(&a, &["xcodebuild", "archive", "-scheme", "App"], &made_by(&w, "y"));
    assert_eq!(archive.code(), 0, "{}", archive.all());
    assert!(!o.join("Old.xcarchive").exists(), "an archive run kept an older archive");
    assert_eq!(line_value(&archive.stderr(), "archive"), Some(o.join("App.xcarchive").to_string_lossy().into_owned()));
    assert_eq!(product(&a).as_deref(), Some("a\n"), "an archive run replaced the products");
    Ok(())
}

// A20
#[test]
fn the_output_stays_out_of_git_and_out_of_the_slot() -> TestResult {
    let w = World::new("gitout");
    let a = repo(&w.path("app"));
    write(&a.join(".agent-build/config.yaml"), "ignored_inputs:\n");
    assert_eq!(w.build(&a, &made_by(&w, "a")).code(), 0);
    let ignore = fs::read_to_string(out(&a).join(".gitignore"))?;
    assert_eq!(ignore.lines().next(), Some("# Automatically created by agent-build."));
    git(&a, &["add", "-A"]);
    assert_eq!(git(&a, &["diff", "--cached", "--name-only"]), ".agent-build/config.yaml", "only the project's config is committed");

    assert_eq!(w.build(&a, &made_by(&w, "b")).code(), 0);
    let slot = w.slot_src();
    assert!(slot.join(".agent-build/config.yaml").is_file(), "the project's config did not reach the slot");
    assert!(!slot.join(".agent-build/output").exists(), "the last run's output was synced into the slot");
    Ok(())
}
