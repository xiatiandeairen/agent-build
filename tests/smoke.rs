//! The real xcodebuild on the sample projects in tests/fixtures. Slow (minutes), so ignored by
//! `cargo test` and run by `just smoke` before every delivery; `just smoke` borrows the iOS
//! simulator A13 needs and passes its id in AGENT_BUILD_SMOKE_SIM.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::*;

fn line_value(out: &str, key: &str) -> Option<PathBuf> {
    let line = out.lines().find(|l| l.starts_with(&format!("agb: {key}=")))?;
    let value = line.split_once('=')?.1;
    Some(PathBuf::from(value.split(" (").next().unwrap_or(value)))
}

fn real(w: &World, cwd: &Path, args: &[&str]) -> Run {
    Run(w
        .command(cwd, &[])
        .env_remove("AGENT_BUILD_XCODEBUILD")
        .env_remove("AGENT_BUILD_XCRESULTTOOL")
        .args(args)
        .output()
        .expect("run agb"))
}

/// A fixture copied to `dest` and committed as a repository of its own.
fn checkout(fixture: &str, dest: &Path) -> PathBuf {
    fs::create_dir_all(dest.parent().expect("parent")).expect("mkdir");
    assert!(Command::new("cp").arg("-R").arg(fixture_path(fixture)).arg(dest).status().expect("cp").success());
    git(dest, &["init", "-q"]);
    commit_all(dest, "sample");
    dest.to_path_buf()
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn append(path: &Path, text: &str) -> String {
    let before = fs::read_to_string(path).expect("read");
    fs::write(path, format!("{before}{text}")).expect("write");
    before
}

const SIM_APP: [&str; 4] = ["-scheme", "App", "-destination", "generic/platform=iOS Simulator"];

// A1
#[test]
#[ignore = "runs the real xcodebuild; use `just smoke`"]
fn a_standard_project_builds_and_tests_without_any_configuration() {
    let w = World::new("smoke");
    let app = checkout("SampleApp", &w.path("SampleApp"));
    let project = ["xcodebuild", "build", "-project", "SampleApp.xcodeproj"];
    let build = real(&w, &app, &[&project[..], &SIM_APP[..]].concat());
    assert_eq!(build.code(), 0, "{}", build.all());
    assert!(build.stderr().contains("agb: ok slot=SampleApp-"), "{}", build.all());
    let products = line_value(&build.stderr(), "products").expect("products line");
    assert!(products.join("Debug-iphonesimulator/App.app").is_dir(), "no App.app under {}", products.display());

    let test = real(
        &w,
        &app,
        &["xcodebuild", "test", "-project", "SampleApp.xcodeproj", "-scheme", "CoreTests", "-destination", "platform=macOS"],
    );
    assert_eq!(test.code(), 0, "{}", test.all());
    let results = line_value(&test.stderr(), "results").expect("results line");
    assert!(results.exists(), "{} does not exist", results.display());

    // A compile error comes back from the real xcresulttool, located in the worktree.
    let source = app.join("App/App.swift");
    let line = append(&source, "func broken() -> Int { return \"not an int\" }\n").lines().count() + 1;
    let broken = real(&w, &app, &[&project[..], &SIM_APP[..]].concat());
    assert_eq!(broken.code(), 65, "{}", broken.all());
    let expected = format!("{}:{line}:", source.display());
    assert!(
        broken.stderr().lines().any(|l| l.starts_with(&expected) && l.contains(": error: ")),
        "no error at {expected}: {}",
        broken.all()
    );
    assert!(broken.stderr().contains("errors=1"), "{}", broken.all());
}

// A12
#[test]
#[ignore = "runs the real xcodebuild; use `just smoke`"]
fn a_workspace_with_a_local_mixed_package_in_a_subdirectory_builds_incrementally() {
    let w = World::new("smoke-ws");
    let repo = checkout("SampleWorkspace", &w.path("my 应用/SampleWorkspace"));
    let ios = repo.join("ios");
    let build = || real(&w, &ios, &[&["xcodebuild", "build", "-workspace", "SampleWorkspace.xcworkspace"][..], &SIM_APP[..]].concat());

    let first = build();
    assert_eq!(first.code(), 0, "{}", first.all());
    let products = line_value(&first.stderr(), "products").expect("products line");
    assert!(products.join("Debug-iphonesimulator/App.app").is_dir(), "no App.app under {}", products.display());
    let again = build();
    assert!(again.stderr().contains(" miss=0 "), "an unchanged rebuild redid cached work: {}", again.all());

    let header = ios.join("Packages/Core/Sources/CoreObjC/include/CoreObjC.h");
    let original = fs::read_to_string(&header).expect("read header");
    fs::write(&header, original.replace("+ (NSString *)greeting;", "+ (NSString *)greeting;\n+ (NSInteger)answer;")).expect("write header");
    let after_header = build();
    assert_eq!(after_header.code(), 0, "a public ObjC header change broke the incremental build: {}", after_header.all());

    let source = ios.join("App/AppMain.swift");
    let line = append(&source, "func broken() -> Int { return \"not an int\" }\n").lines().count() + 1;
    let broken = build();
    assert_eq!(broken.code(), 65, "{}", broken.all());
    let expected = format!("{}:{line}:", source.display());
    assert!(
        broken.stderr().lines().any(|l| l.starts_with(&expected) && l.contains(": error: ")),
        "no error at {expected}: {}",
        broken.all()
    );

    let resolve =
        real(&w, &ios, &["xcodebuild", "-resolvePackageDependencies", "-workspace", "SampleWorkspace.xcworkspace", "-scheme", "App"]);
    assert_eq!(resolve.code(), 0, "{}", resolve.all());
    assert!(!resolve.stderr().contains("agb: products="), "resolving packages pointed at products: {}", resolve.all());
}

// A13
#[test]
#[ignore = "runs the real xcodebuild on a borrowed simulator; use `just smoke`"]
fn package_tests_in_the_package_directory_name_failures_and_leave_the_app_build_warm() {
    let sim = std::env::var("AGENT_BUILD_SMOKE_SIM").expect("AGENT_BUILD_SMOKE_SIM: run through `just smoke`, which borrows a simulator");
    let w = World::new("smoke-pkg");
    let repo = checkout("SampleWorkspace", &w.path("SampleWorkspace"));
    let (ios, package) = (repo.join("ios"), repo.join("ios/Packages/Core"));
    let app = || real(&w, &ios, &[&["xcodebuild", "build", "-workspace", "SampleWorkspace.xcworkspace"][..], &SIM_APP[..]].concat());
    let destination = format!("id={sim}");
    let test = || {
        real(
            &w,
            &package,
            &["xcodebuild", "test", "-scheme", "Core", "-destination", &destination, "-only-testing:CoreTests/CoreTests/testAdd"],
        )
    };
    assert_eq!(app().code(), 0);

    let tests = package.join("Tests/CoreTests/CoreTests.swift");
    let original = fs::read_to_string(&tests).expect("read tests");
    fs::write(&tests, original.replace("XCTAssertEqual(Core.add(1, 1), 2)", "XCTAssertEqual(Core.add(1, 1), 3)")).expect("write tests");
    let failing = test();
    assert_eq!(failing.code(), 65, "{}", failing.all());
    assert!(failing.stderr().lines().any(|l| l.starts_with("agb: test failed: ") && l.contains("testAdd")), "{}", failing.all());
    fs::write(&tests, original).expect("restore tests");
    let passing = test();
    assert_eq!(passing.code(), 0, "{}", passing.all());

    let warm = app();
    assert_eq!(warm.code(), 0, "{}", warm.all());
    // Switching containers re-runs some tasks; they must come from the compilation cache, not recompile.
    assert!(warm.stderr().contains(" miss=0 "), "testing the package made the app build recompile: {}", warm.all());
}

// A14
#[test]
#[ignore = "runs the real xcodebuild; use `just smoke`"]
fn tests_built_once_run_later_from_their_xctestrun() {
    let w = World::new("smoke-xctestrun");
    let app = checkout("SampleApp", &w.path("SampleApp"));
    let build_for_testing = || {
        let r = real(
            &w,
            &app,
            &[
                "xcodebuild",
                "build-for-testing",
                "-project",
                "SampleApp.xcodeproj",
                "-scheme",
                "CoreTests",
                "-destination",
                "platform=macOS",
            ],
        );
        assert_eq!(r.code(), 0, "{}", r.all());
        let products = line_value(&r.stderr(), "products").expect("products line");
        let found = fs::read_dir(&products)
            .expect("products dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "xctestrun"));
        found.unwrap_or_else(|| panic!("no .xctestrun in {}", products.display()))
    };
    let run = |xctestrun: &Path| {
        real(
            &w,
            &app,
            &["xcodebuild", "test-without-building", "-xctestrun", &xctestrun.to_string_lossy(), "-destination", "platform=macOS"],
        )
    };

    let passing = run(&build_for_testing());
    assert_eq!(passing.code(), 0, "{}", passing.all());
    assert!(line_value(&passing.stderr(), "results").is_some_and(|p| p.exists()), "{}", passing.all());

    let tests = app.join("Tests/CoreTests.swift");
    let original = fs::read_to_string(&tests).expect("read tests");
    fs::write(&tests, original.replace("XCTAssertEqual(1 + 1, 2)", "XCTAssertEqual(1 + 1, 3)")).expect("write tests");
    let failing = run(&build_for_testing());
    assert_eq!(failing.code(), 65, "{}", failing.all());
    assert!(failing.stderr().lines().any(|l| l.starts_with("agb: test failed: ") && l.contains("testAddition")), "{}", failing.all());
}

// A17
#[test]
#[ignore = "runs the real xcodebuild; use `just smoke`"]
fn an_unsigned_archive_lands_where_agb_says() {
    let w = World::new("smoke-archive");
    let app = checkout("SampleApp", &w.path("SampleApp"));
    let r = real(
        &w,
        &app,
        &[
            "xcodebuild",
            "archive",
            "-project",
            "SampleApp.xcodeproj",
            "-scheme",
            "App",
            "-destination",
            "generic/platform=iOS",
            "CODE_SIGNING_ALLOWED=NO",
        ],
    );
    assert_eq!(r.code(), 0, "{}", r.all());
    let archive = line_value(&r.stderr(), "archive").expect("archive line");
    assert!(archive.join("Products/Applications/App.app").is_dir(), "no App.app in {}", archive.display());
}
