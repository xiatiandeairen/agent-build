//! The real xcodebuild on tests/fixtures/SampleApp. Slow (a minute or two), so it is ignored by
//! `cargo test` and run by `just smoke` before every delivery.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use support::*;

fn line_value(out: &str, key: &str) -> Option<PathBuf> {
    let line = out.lines().find(|l| l.starts_with(&format!("agent-build: {key}=")))?;
    let value = line.split_once('=')?.1;
    Some(PathBuf::from(value.split(" (").next().unwrap_or(value)))
}

fn real(w: &World, cwd: &Path, args: &[&str]) -> Run {
    Run(w.command(cwd, &[]).env_remove("AGENT_BUILD_XCODEBUILD").args(args).output().expect("run agent-build"))
}

// A1
#[test]
#[ignore = "runs the real xcodebuild; use `just smoke`"]
fn a_standard_project_builds_and_tests_without_any_configuration() {
    let w = World::new("smoke");
    let app = w.path("SampleApp");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/SampleApp");
    assert!(Command::new("cp").arg("-R").arg(&fixture).arg(&app).status().expect("cp").success());
    git(&app, &["init", "-q"]);
    commit_all(&app, "sample");

    let build = real(
        &w,
        &app,
        &["xcodebuild", "build", "-project", "SampleApp.xcodeproj", "-scheme", "App", "-destination", "generic/platform=iOS Simulator"],
    );
    assert_eq!(build.code(), 0, "{}", build.all());
    assert!(build.stderr().contains("agent-build: ok slot=SampleApp-"), "{}", build.all());
    let products = line_value(&build.stderr(), "products").expect("products line");
    assert!(products.join("Debug-iphonesimulator/App.app").is_dir(), "no App.app under {}", products.display());

    let test = real(
        &w,
        &app,
        &["xcodebuild", "test", "-project", "SampleApp.xcodeproj", "-scheme", "CoreTests", "-destination", "platform=macOS"],
    );
    assert_eq!(test.code(), 0, "{}", test.all());
    let results = line_value(&test.stderr(), "test results").expect("test results line");
    assert!(results.exists(), "{} does not exist", results.display());
}
