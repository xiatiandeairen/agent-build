//! Git-ignored inputs (precondition P8): named when a failed build looks for one, mirrored when
//! the project declares it in `.agent-build.yaml`.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use support::*;

const P8: &str = "agb: precondition P8 not met";

fn tree(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read dir") {
        let p = entry.expect("entry").path();
        let name = p.file_name().expect("name").to_string_lossy().into_owned();
        let meta = fs::symlink_metadata(&p).expect("meta");
        if meta.is_dir() {
            out.extend(tree(&p).into_iter().map(|(n, c)| (format!("{name}/{n}"), c)));
        } else if meta.file_type().is_symlink() {
            out.push((name, fs::read_link(&p).expect("link").to_string_lossy().as_bytes().to_vec()));
        } else {
            out.push((format!("{name} mode={:o}", meta.permissions().mode() & 0o777), fs::read(&p).expect("read")));
        }
    }
    out.sort();
    out
}

// A11
#[test]
fn ignored_inputs_are_named_when_missing_and_mirrored_once_declared() -> TestResult {
    let w = World::new("ignored");
    let a = repo(&w.path("app"));
    write(&a.join("App/Gen.generated"), "enum Greeting {}\n");
    write(&a.join(".gitignore"), "*.generated\ngen/\n");
    commit_all(&a, "ignore gen");

    // Not declared: the failed build is told which ignored file it was missing; tracked files are not named.
    let fail = |file: &str| [("FAKE_ERROR", file.to_string()), ("FAKE_EXIT", "65".to_string())];
    let r = w.build(&a, &fail("App/Gen.generated"));
    assert_eq!(r.code(), 65, "{}", r.all());
    assert!(
        r.stderr().contains(&format!("{P8}: the build needs git-ignored files, which are not in the slot: App/Gen.generated")),
        "{}",
        r.all()
    );
    let r = w.build(&a, &fail("App/A.swift"));
    assert!(!r.stderr().contains(P8), "{}", r.all());

    // Declared: files, directories, executables and symlinks arrive as they are.
    write(&a.join("gen/lib/core.h"), "int core(void);\n");
    write(&a.join("gen/tool"), "#!/bin/sh\n");
    fs::set_permissions(a.join("gen/tool"), fs::Permissions::from_mode(0o755))?;
    symlink("lib/core.h", a.join("gen/link.h"))?;
    write(&a.join(".agent-build.yaml"), "ignored_inputs:\n  - App/Gen.generated\n  - gen/\n");
    let r = w.build(&a, &[]);
    assert_eq!(r.code(), 0, "{}", r.all());
    let slot = w.slot_src();
    assert_eq!(tree(&slot.join("gen")), tree(&a.join("gen")));
    assert_eq!(fs::read(slot.join("App/Gen.generated"))?, fs::read(a.join("App/Gen.generated"))?);
    assert!(!w.build(&a, &fail("App/Gen.generated")).stderr().contains(P8));

    // Changes: only the changed file is rewritten; additions and deletions follow the worktree.
    let before = mtimes(&slot, &["gen/tool", "App/Gen.generated"]);
    write(&a.join("gen/lib/core.h"), "int core(int);\n");
    write(&a.join("gen/lib/new.h"), "\n");
    fs::remove_file(a.join("gen/link.h"))?;
    assert_eq!(w.build(&a, &[]).code(), 0);
    assert_eq!(tree(&slot.join("gen")), tree(&a.join("gen")));
    assert_eq!(mtimes(&slot, &["gen/tool", "App/Gen.generated"]), before, "unchanged ignored files were rewritten");

    // No longer declared: removed from the slot, so the build cannot use a stale copy.
    write(&a.join(".agent-build.yaml"), "ignored_inputs:\n  - App/Gen.generated\n");
    assert_eq!(w.build(&a, &[]).code(), 0);
    assert!(!slot.join("gen").exists());
    fs::remove_file(a.join("App/Gen.generated"))?;
    assert_eq!(w.build(&a, &[]).code(), 0);
    assert!(!slot.join("App/Gen.generated").exists());

    // A broken project file is a usage error with its line; nothing is built.
    let count = w.path("count");
    write(&a.join(".agent-build.yaml"), "ignored_inputs:\n  - ../outside\n");
    let r = w.build(&a, &[("FAKE_COUNT", count.to_string_lossy().into_owned())]);
    assert_eq!(r.code(), 64, "{}", r.all());
    assert!(r.stderr().contains(".agent-build.yaml: line 2:"), "{}", r.all());
    assert!(!count.exists(), "xcodebuild ran with a bad project file");
    Ok(())
}
