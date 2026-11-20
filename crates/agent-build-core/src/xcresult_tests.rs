//! Against JSON recorded from the real xcresulttool (Xcode 27.2) on tests/fixtures/SampleApp:
//! `hits` rebuilt from a warm cache, `error` with a compile error and a warning, `p4` with
//! explicit modules off, `testfail` with a failing unit test. Paths are replaced by `@SRC@`.
use super::*;
use crate::json::parse;

fn fixture(name: &str, kind: &str) -> Json {
    let text = match (name, kind) {
        ("hits", "build") => include_str!("../../../tests/fixtures/xcresult/hits/build.json"),
        ("hits", "log") => include_str!("../../../tests/fixtures/xcresult/hits/log.json"),
        ("error", "build") => include_str!("../../../tests/fixtures/xcresult/error/build.json"),
        ("p4", "build") => include_str!("../../../tests/fixtures/xcresult/p4/build.json"),
        ("p4", "log") => include_str!("../../../tests/fixtures/xcresult/p4/log.json"),
        ("testfail", "tests") => include_str!("../../../tests/fixtures/xcresult/testfail/tests.json"),
        _ => "",
    };
    parse(text).unwrap()
}

// A7.2
#[test]
fn takes_errors_and_warnings_with_one_based_locations() {
    let got = issues(&fixture("error", "build"));
    assert_eq!(got.len(), 2, "{got:#?}");
    assert_eq!(got[0].severity, Severity::Error);
    assert_eq!(got[0].location.as_deref(), Some("@SRC@/App/App.swift:14:31"), "xcodebuild printed 14:31 for this error");
    assert!(got[0].message.starts_with("Cannot convert return expression"));
    assert_eq!((got[1].severity, got[1].location.as_deref()), (Severity::Warning, Some("@SRC@/App/App.swift:13:20")));
    assert_eq!(location("file:///a%20b/%E4%B8%AD.swift#StartingLineNumber=0"), Some("/a b/中.swift:1".to_string()));
    assert_eq!(location("file:///x/Info.plist"), Some("/x/Info.plist".to_string()));
    assert_eq!(location("https://x"), None);
    assert!(issues(&fixture("hits", "build")).is_empty());
}

// A7.2
#[test]
fn names_each_failed_test_with_its_reason() {
    assert_eq!(
        test_failures(&fixture("testfail", "tests")),
        ["CoreTests/testAddition(): XCTAssertEqual failed: (\"1\") is not equal to (\"2\") - deliberate"]
    );
    assert!(test_failures(&Json::Null).is_empty());
}

// A7.2
#[test]
fn counts_hits_and_misses_from_xcode_metrics_and_groups_missed_tasks_by_target() {
    let c = cache(&fixture("hits", "log"));
    assert_eq!((c.hits, c.misses), (126, 4));
    assert_eq!(c.misses_by_target, [("App".to_string(), 4)]);
    let off = cache(&fixture("p4", "log"));
    assert_eq!((off.hits, off.misses, off.misses_by_target.len()), (0, 0, 0));
}

// A7.4
#[test]
fn names_the_targets_left_uncached_only_when_xcode_warns_about_explicit_modules() {
    assert_eq!(p4_targets(&fixture("p4", "build"), &fixture("p4", "log")), Some(vec!["App".to_string()]));
    assert_eq!(p4_targets(&fixture("hits", "build"), &fixture("hits", "log")), None);
}

// A7.3
#[test]
fn splits_the_build_into_phases_from_the_log_sections() {
    let p = phases(&fixture("hits", "log"));
    let names: Vec<&str> = p.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["resolve", "graph", "plan", "execute"]);
    let total = fixture("hits", "log").get("duration").num().unwrap();
    assert!((p.iter().map(|(_, d)| d).sum::<f64>() - total).abs() < 1e-6, "phases add up to the build's duration");
    assert!(phases(&Json::Null).is_empty());
}
