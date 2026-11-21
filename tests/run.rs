//! Locking, failures, output and configuration through the real binary, with a fake xcodebuild.
#![allow(clippy::expect_used)] // test helpers: a failed setup step should stop the test with its message
mod support;

use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};
use support::*;

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out waiting for {}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn finish(child: Child) -> Run {
    Run(child.wait_with_output().expect("wait for agb"))
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).and_then(|m| m.modified()).expect("mtime")
}

fn signal(pid: u32, sig: &str) {
    assert!(Command::new("kill").args([sig, &pid.to_string()]).status().expect("kill").success());
}

fn fake(w: &World, name: &str, sleep: &str) -> Vec<(&'static str, String)> {
    let p = |s: &str| w.path(&format!("{name}-{s}")).to_string_lossy().into_owned();
    vec![("FAKE_SLEEP", sleep.to_string()), ("FAKE_STARTED", p("started")), ("FAKE_DONE", p("done"))]
}

// A5
#[test]
fn builds_of_one_project_take_turns_and_say_whom_they_wait_for() {
    let w = World::new("turns");
    let a = repo(&w.path("app"));
    let first = w.spawn(&a, &fake(&w, "a", "2"));
    wait_for(&w.path("a-started"));
    let second = finish(w.spawn(&a, &fake(&w, "b", "0")));
    let first_pid = first.id();
    let first = finish(first);
    assert_eq!((first.code(), second.code()), (0, 0), "{}\n{}", first.all(), second.all());
    let waiting = format!("waiting for the slot: pid {first_pid}, worktree {}", a.display());
    assert!(second.stderr().contains(&waiting), "{}", second.all());
    assert!(mtime(&w.path("b-started")) >= mtime(&w.path("a-done")), "the second build started before the first ended");
}

// A5
#[test]
fn different_projects_do_not_wait_for_each_other() {
    let w = World::new("parallel");
    let (one, two) = (repo(&w.path("one")), repo(&w.path("two")));
    git(&two, &["remote", "add", "origin", "https://example.invalid/two.git"]);
    let first = w.spawn(&one, &fake(&w, "a", "3"));
    wait_for(&w.path("a-started"));
    let second = w.build(&two, &fake(&w, "b", "0"));
    assert!(!w.path("a-done").exists(), "the other project's build ran while the first was still running");
    assert!(!second.stderr().contains("waiting"), "{}", second.all());
    assert_eq!(finish(first).code(), 0);
}

// A5
#[test]
fn on_sigterm_it_stops_xcodebuild_before_releasing_the_slot() {
    let w = World::new("sigterm");
    let a = repo(&w.path("app"));
    let child = w.spawn(&a, &fake(&w, "a", "30"));
    wait_for(&w.path("a-started"));
    signal(child.id(), "-TERM");
    let started = Instant::now();
    let r = finish(child);
    assert_eq!(r.code(), 143, "{}", r.all());
    assert!(started.elapsed() < Duration::from_secs(10));
    let next = w.build(&a, &fake(&w, "b", "0"));
    assert!(!next.stderr().contains("waiting"), "the slot was still held: {}", next.all());
    assert!(!w.path("a-done").exists(), "xcodebuild kept running after agent-build stopped");
}

// A5
#[test]
fn after_kill_9_the_slot_stays_locked_until_the_leftover_xcodebuild_exits() {
    let w = World::new("kill9");
    let a = repo(&w.path("app"));
    let child = w.spawn(&a, &fake(&w, "a", "3"));
    wait_for(&w.path("a-started"));
    let pid = child.id();
    signal(pid, "-KILL");
    let _ = finish(child);
    assert!(!w.path("a-done").exists(), "the fake xcodebuild should still be running");
    let next = w.build(&a, &fake(&w, "b", "0"));
    assert_eq!(next.code(), 0, "{}", next.all());
    assert!(next.stderr().contains(&format!("waiting for the slot: pid {pid}")), "{}", next.all());
    assert!(mtime(&w.path("b-started")) >= mtime(&w.path("a-done")), "took over while the leftover xcodebuild still ran");
}

// A6, A7.5 (the test directory is under /private/var; the fake prints it as /var, like xcodebuild)
#[test]
fn a_failed_build_is_reported_as_is_once_with_its_exit_code_and_log() -> TestResult {
    let w = World::new("fail");
    let a = repo(&w.path("app"));
    let count = w.path("count");
    let env = [("FAKE_EXIT", "65".to_string()), ("FAKE_ERROR", "App/A.swift".to_string()), ("FAKE_COUNT", count.to_string_lossy().into())];
    let r = w.build(&a, &env);
    assert_eq!(r.code(), 65, "{}", r.all());
    assert!(r.stderr().contains("agb: no result bundle to read; the last"), "{}", r.all());
    assert!(r.stderr().contains(&format!("{}/App/A.swift:1:1: error: boom", a.display())), "{}", r.all());
    assert!(!r.stderr().lines().any(|l| l.contains("error: boom") && l.contains("/slots/")), "the error points into the slot: {}", r.all());
    assert!(r.stderr().contains("agb: failed slot="), "{}", r.all());
    assert_eq!(fs::read_to_string(&count)?.lines().count(), 1, "xcodebuild must run exactly once");
    let log = a.join(".agents/build/output/build.log");
    assert!(r.stderr().contains(&format!("log={}", log.display())), "{}", r.all());
    assert!(fs::read_to_string(&log)?.contains("/slots/"), "the log keeps the raw output");
    Ok(())
}

// A6
#[test]
fn a_failed_sync_exits_125_without_building_and_the_next_run_recovers() -> TestResult {
    let w = World::new("syncfail");
    let a = repo(&w.path("app"));
    let count = w.path("count");
    let env = [("FAKE_COUNT", count.to_string_lossy().into_owned())];
    assert_eq!(w.build(&a, &env).code(), 0);
    let slot_app = w.slot_src().join("App");
    let chmod = |mode: &str| Command::new("chmod").args(["-R", mode]).arg(&slot_app).status().map(|s| s.success());
    assert!(chmod("a-w")?);
    write(&a.join("App/A.swift"), "let a = 2\n");
    let r = w.build(&a, &env);
    assert_eq!(r.code(), 125, "{}", r.all());
    assert!(r.stderr().contains("sync failed, nothing was built"), "{}", r.all());
    assert_eq!(fs::read_to_string(&count)?.lines().count(), 1, "xcodebuild ran although sync failed");
    assert!(chmod("u+w")?);
    assert_eq!(w.build(&a, &env).code(), 0);
    assert_eq!(fs::read_to_string(w.slot_src().join("App/A.swift"))?, "let a = 2\n");
    Ok(())
}

fn args_seen(w: &World, cwd: &Path) -> (Run, Vec<String>) {
    let file = w.path("args");
    let _ = fs::remove_file(&file); // from an earlier run in this test
    let r = w.build(cwd, &[("FAKE_ARGS", file.to_string_lossy().into_owned())]);
    let seen = fs::read_to_string(&file).unwrap_or_default().lines().map(String::from).collect();
    (r, seen)
}

// A8, A1.3
#[test]
fn the_cache_limit_comes_from_the_config_and_a_bad_config_is_refused() {
    let w = World::new("config");
    let a = repo(&w.path("app"));
    let (r, seen) = args_seen(&w, &a);
    assert_eq!(r.code(), 0, "{}", r.all());
    assert!(seen.contains(&"COMPILATION_CACHE_LIMIT_SIZE=20G".to_string()), "{seen:?}");
    let cas = seen.iter().find_map(|a| a.strip_prefix("COMPILATION_CACHE_CAS_PATH=")).unwrap_or_default();
    assert!(
        cas.contains("/xdg-cache/agent-build/slots/") && cas.ends_with("/CompilationCache"),
        "the cache is not the project's own: {seen:?}"
    );
    let dd = seen.iter().position(|a| a == "-derivedDataPath").and_then(|i| seen.get(i + 1)).cloned().unwrap_or_default();
    assert!(dd.contains("/xdg-cache/agent-build/slots/") && dd.ends_with("/DerivedData"), "{seen:?}");
    assert_eq!(&seen[seen.len() - 3..], ["build", "-scheme", "App"], "the caller's arguments come last, unchanged");

    write(&w.config_file(), "# smaller cache\ncache_limit: 10G\n");
    let (_, seen) = args_seen(&w, &a);
    assert!(seen.contains(&"COMPILATION_CACHE_LIMIT_SIZE=10G".to_string()), "{seen:?}");

    write(&w.config_file(), "cache_limit: 20G\nslots: 3\n");
    let (r, seen) = args_seen(&w, &a);
    assert_eq!(r.code(), 64, "{}", r.all());
    assert!(r.stderr().contains("config.yaml: line 2: unknown key `slots`"), "{}", r.all());
    assert!(seen.is_empty(), "xcodebuild ran with a bad config");
}

// A1.7
#[test]
fn an_absolute_worktree_path_to_the_project_is_built_from_the_slot() -> TestResult {
    let w = World::new("abspath");
    let a = repo(&w.path("app"));
    // Spelled through the /var -> /private/var link, as `$PWD` often is.
    let given = a.to_string_lossy().replacen("/private/var/", "/var/", 1) + "/App.xcworkspace";
    let args = w.path("args");
    let r =
        w.run(&a, &["xcodebuild", "build", "-workspace", &given, "-scheme", "App"], &[("FAKE_ARGS", args.to_string_lossy().into_owned())]);
    assert_eq!(r.code(), 0, "{}", r.all());
    let seen = fs::read_to_string(&args)?;
    let value = seen.lines().skip_while(|l| *l != "-workspace").nth(1).unwrap_or_default().to_string();
    assert!(value.contains("/slots/") && value.ends_with("/source/App.xcworkspace"), "xcodebuild got {value}");
    assert!(r.stderr().contains(&format!("agb: -workspace {given} is in the worktree; building the slot's copy")), "{}", r.all());
    Ok(())
}

// A16.1
#[test]
fn a_project_cache_over_the_limit_is_cleared_after_the_build_without_touching_other_projects() -> TestResult {
    let w = World::new("cachelimit");
    let (a, b) = (repo(&w.path("a")), repo(&w.path("b")));
    write(&w.config_file(), "cache_limit: 1M\n");
    let fill = |bytes: &str| [("FAKE_CAS_BYTES", bytes.to_string())];
    let cas_of = |r: &Run| -> std::path::PathBuf {
        let log = r.stderr().lines().last().and_then(|l| l.split("log=").nth(1)).map(String::from).unwrap_or_default();
        let raw = fs::read_to_string(&log).unwrap_or_default();
        let arg = raw.split_whitespace().find_map(|t| t.strip_prefix("COMPILATION_CACHE_CAS_PATH=").map(String::from)).unwrap_or_default();
        std::path::PathBuf::from(arg)
    };

    let small_b = w.build(&b, &fill("100000"));
    assert_eq!(small_b.code(), 0, "{}", small_b.all());
    assert!(!small_b.stderr().contains("cache_limit"), "{}", small_b.all());
    let cas_b = cas_of(&small_b);
    assert!(cas_b.is_dir(), "a cache under the limit was removed: {}", cas_b.display());

    let big_a = w.build(&a, &fill("3000000"));
    assert_eq!(big_a.code(), 0, "{}", big_a.all());
    assert!(big_a.stderr().contains("exceeded cache_limit 1M and was cleared"), "{}", big_a.all());
    let summary = big_a.stderr().lines().last().unwrap_or_default().to_string();
    assert!(summary.starts_with("agb: ok "), "the cache line comes before the summary: {}", big_a.all());
    assert!(!cas_of(&big_a).exists(), "the cache over the limit is still there");
    assert!(cas_b.is_dir(), "another project's cache was removed");
    Ok(())
}
