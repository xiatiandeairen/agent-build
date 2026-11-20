//! What agb prints, read from result bundles recorded with the real Xcode (tests/fixtures/xcresult)
//! and served by tests/fixtures/fake-xcresulttool.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::fs;
use support::*;

fn bundle(name: &str) -> (&'static str, String) {
    ("FAKE_XCRESULT", fixture(&format!("xcresult/{name}")).to_string_lossy().into_owned())
}

/// The lines agb printed itself, by kind: the first word after `agb:` up to `=`.
fn kinds(r: &Run) -> Vec<String> {
    r.stderr()
        .lines()
        .filter_map(|l| l.strip_prefix("agb: "))
        .map(|l| l.split_whitespace().next().unwrap_or("").split('=').next().unwrap_or("").to_string())
        .collect()
}

// A7
#[test]
fn a_failed_build_shows_its_errors_from_the_result_bundle_and_not_the_raw_output() -> TestResult {
    let w = World::new("report-error");
    let a = repo(&w.path("app"));
    let raw = w.path("raw.log");
    write(&raw, "CompileSwift normal arm64\n** BUILD FAILED **\n");
    let env = [
        bundle("error"),
        ("FAKE_EXIT", "65".to_string()),
        ("FAKE_OUTPUT", raw.to_string_lossy().into_owned()),
        ("FAKE_TOUCH", "App/B.swift".to_string()),
    ];
    let r = w.build(&a, &env);
    assert_eq!(r.code(), 65, "{}", r.all());
    let err = r.stderr();
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(
        lines[1],
        format!("{}/App/App.swift:14:31: error: Cannot convert return expression of type 'String' to return type 'Int'", a.display())
    );
    assert_eq!(kinds(&r), ["building", "phases", "top", "precondition", "results", "failed"], "{err}");
    assert!(err.contains("P5 not met") && err.contains("App/B.swift"), "{err}");
    assert!(err.contains("errors=1 warnings=1"), "{err}");
    assert!(!r.all().contains("BUILD FAILED"), "raw xcodebuild output reached the terminal: {}", r.all());
    let log = fs::read_to_string(err.lines().last().and_then(|l| l.split("log=").nth(1)).expect("log path"))?;
    assert!(log.contains("** BUILD FAILED **"), "the log keeps the raw output");

    // The next sync puts the file the build rewrote back to the worktree's content.
    let again = w.build(&a, &[]);
    assert!(!again.stderr().contains("P5 not met"), "{}", again.all());
    assert_eq!(
        fs::read_to_string(w.slot_src().join("App/B.swift"))?,
        fs::read_to_string(a.join("App/B.swift"))?,
        "the next run restores it"
    );
    Ok(())
}

// A7
#[test]
fn a_successful_build_reports_cache_counters_misses_and_phases() {
    let w = World::new("report-hits");
    let a = repo(&w.path("app"));
    let r = w.build(&a, &[bundle("hits"), ("FAKE_PRODUCT", "app".to_string())]);
    assert_eq!(r.code(), 0, "{}", r.all());
    assert_eq!(kinds(&r), ["building", "phases", "top", "results", "products", "ok"], "{}", r.all());
    assert!(
        r.stderr().contains("agb: top misses: App 4") && r.stderr().contains("cache hit=126 miss=4 errors=0 warnings=0"),
        "{}",
        r.all()
    );

    let p4 = w.build(&a, &[bundle("p4")]);
    assert!(p4.stderr().contains("agb: precondition P4 not met: App (explicit modules off)"), "{}", p4.all());
}

// A7
#[test]
fn failed_tests_are_named_with_their_reasons() {
    let w = World::new("report-tests");
    let a = repo(&w.path("app"));
    let r = w.run(&a, &["xcodebuild", "test", "-scheme", "CoreTests"], &[bundle("testfail"), ("FAKE_EXIT", "65".to_string())]);
    assert_eq!(r.code(), 65, "{}", r.all());
    assert!(
        r.stderr()
            .contains("agb: test failed: CoreTests/testAddition(): XCTAssertEqual failed: (\"1\") is not equal to (\"2\") - deliberate"),
        "{}",
        r.all()
    );
    assert!(!r.stderr().contains("no result bundle to read"), "{}", r.all());
}

// A7.6
#[test]
fn information_commands_print_xcodebuild_output_as_is() {
    let w = World::new("report-info");
    let a = repo(&w.path("app"));
    let out = w.path("settings.txt");
    write(&out, "Build settings for action build and target App:\n    SRCROOT = x\n");
    let args = w.path("args");
    let env = [("FAKE_OUTPUT", out.to_string_lossy().into_owned()), ("FAKE_ARGS", args.to_string_lossy().into_owned())];
    let r = w.run(&a, &["xcodebuild", "-showBuildSettings", "-scheme", "App"], &env);
    assert_eq!(r.code(), 0, "{}", r.all());
    assert!(r.stdout().contains("Build settings for action build and target App:"), "{}", r.all());
    assert!(!kinds(&r).contains(&"building".to_string()), "{}", r.all());
    assert!(!fs::read_to_string(&args).unwrap_or_default().contains("-resultBundlePath"), "an information command got a result bundle");
}
