//! Messages recorded from a real `cargo build --message-format=json` and `cargo test` of
//! tests/fixtures/CargoSample, with the build directory written as `/slot`.
use super::*;

const OK: &str = include_str!("../../../tests/fixtures/cargo-ok.json");
const ERROR: &str = include_str!("../../../tests/fixtures/cargo-error.json");
const TEST_FAILED: &str = include_str!("../../../tests/fixtures/cargo-test-failed.txt");

fn messages(text: &str) -> Vec<Message> {
    text.lines().filter_map(|l| message(l, "/slot")).collect()
}

fn units(text: &str) -> Vec<Unit> {
    messages(text).into_iter().filter_map(|m| if let Message::Unit(u) = m { Some(u) } else { None }).collect()
}

// A21.1
#[test]
fn renders_errors_and_warnings_at_absolute_paths_and_skips_notes() {
    let d: Vec<Diagnostic> =
        messages(ERROR).into_iter().filter_map(|m| if let Message::Diagnostic(d) = m { Some(d) } else { None }).collect();
    assert_eq!(d.len(), 2, "the failure note is not a diagnostic: {d:?}");
    assert!(!d[0].error && d[0].location.as_deref() == Some("/slot/core/src/extra.rs:1:23") && d[0].message.contains("unused variable"));
    assert!(d[1].error && d[1].location.as_deref() == Some("/slot/ffi/src/lib.rs:6:25"), "primary span, not the label: {:?}", d[1]);
    assert!(d[1].rendered.as_deref().is_some_and(|r| r.contains("expected `u8`")));
    let abs = r#"{"reason":"compiler-message","message":{"level":"warning","message":"w","spans":[{"is_primary":true,"file_name":"/reg/x.rs","line_start":2,"column_start":3}]}}"#;
    assert!(matches!(message(abs, "/slot"), Some(Message::Diagnostic(Diagnostic { location: Some(l), .. })) if l == "/reg/x.rs:2:3"));
    let linker = r#"{"reason":"compiler-message","message":{"level":"error","message":"linking with `cc` failed","spans":[]}}"#;
    assert!(matches!(message(linker, "/slot"), Some(Message::Diagnostic(Diagnostic { error: true, location: None, .. }))));
    assert_eq!(message("test a ... ok", "/slot"), None, "plain output is not a message");
    assert_eq!(message("{not json", "/slot"), None);
}

// A21.2
#[test]
fn counts_reuse_from_cargos_fresh_flags() {
    let mut s = Stats::default();
    units(ERROR).iter().for_each(|u| s.add(u));
    assert_eq!((s.fresh, s.rebuilt), (1, 1));
    assert_eq!(s.rebuilt_local, ["sample-core"]);
    let mut all = Stats::default();
    units(OK).iter().for_each(|u| all.add(u));
    assert_eq!(all.rebuilt, 4);
    assert_eq!(all.rebuilt_local, ["sample-core", "sample-ffi"], "each package once");
    assert_eq!(rebuilt_line(&all).as_deref(), Some("agb: rebuilt: sample-core, sample-ffi"));
    assert_eq!(rebuilt_line(&Stats::default()), None);
    let short = r#"{"reason":"compiler-artifact","package_id":"path+file:///w/crates/core-ffi#0.0.0","target":{"name":"core_ffi","kind":["lib"]},"fresh":false,"filenames":[]}"#;
    assert!(
        matches!(message(short, "/slot"), Some(Message::Unit(Unit { package, .. })) if package == "core-ffi"),
        "name left out of the id"
    );
    let line =
        summary_line(&Summary { ok: true, slot: "s-1", sync_secs: 0.25, build_secs: 2.0, stats: &all, errors: 0, warnings: 1, log: "/l" });
    assert_eq!(line, "agb: ok slot=s-1 sync=0.2s build=2.0s units fresh=0 rebuilt=4 errors=0 warnings=1 log=/l");
}

// A26.1
#[test]
fn picks_the_projects_libraries_and_programs_as_products() {
    assert_eq!(products(&units(OK), "/slot/target"), ["debug/libsample_ffi.a", "debug/sample-tool"]);
    let mut from_registry = units(OK);
    from_registry.iter_mut().for_each(|u| u.local = false);
    assert!(products(&from_registry, "/slot/target").is_empty(), "dependencies are not products");
    let mut harness = units(OK);
    harness.iter_mut().for_each(|u| u.test = true);
    assert!(products(&harness, "/slot/target").is_empty(), "test harnesses are not products");
}

// A21.1
#[test]
fn names_failed_tests_with_where_they_panicked() {
    let lines: Vec<String> = TEST_FAILED.lines().map(str::to_string).collect();
    assert_eq!(test_failures(&lines, "/w"), ["tests::version_is_set: /w/core/src/lib.rs:11:9: SAMPLE_FAIL is set"]);
    assert_eq!(test_failures(&["test a::b ... FAILED".to_string()], "/w"), ["a::b"]);
    assert!(test_failures(&["test a ... ok".to_string()], "/w").is_empty());
}
