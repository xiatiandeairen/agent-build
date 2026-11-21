//! The lines agb prints itself and the exit code.
//! Every line starts with `agb:` so agents can find them in a long build log.
use crate::args::Action;
use crate::xcresult::{CacheStats, Severity};

pub const PREFIX: &str = "agb:";

pub struct Summary<'a> {
    pub ok: bool,
    pub slot: &'a str,
    pub sync_secs: f64,
    pub build_secs: f64,
    pub cache: &'a CacheStats,
    pub errors: usize,
    pub warnings: usize,
    pub log: &'a str,
}

pub fn summary_line(s: &Summary) -> String {
    format!(
        "{PREFIX} {} slot={} sync={:.1}s build={:.1}s cache hit={} miss={} errors={} warnings={} log={}",
        if s.ok { "ok" } else { "failed" },
        s.slot,
        s.sync_secs,
        s.build_secs,
        s.cache.hits,
        s.cache.misses,
        s.errors,
        s.warnings,
        s.log
    )
}

pub fn phases_line(phases: &[(&str, f64)]) -> Option<String> {
    if phases.is_empty() {
        return None;
    }
    let parts: Vec<String> = phases.iter().map(|(n, d)| format!("{n}={d:.1}s")).collect();
    Some(format!("{PREFIX} phases {}", parts.join(" ")))
}

/// `None`: Xcode did not warn. An empty list: it warned but no target could be named.
pub fn p4_line(targets: Option<&[String]>) -> Option<String> {
    let names = match targets? {
        [] => "some Swift targets".to_string(),
        t => t.join(", "),
    };
    Some(format!("{PREFIX} precondition P4 not met: {names} (explicit modules off) — their Swift compiles are not cached"))
}

/// An error or warning as compilers print it, so editors and agents can jump to it.
pub fn issue_line(location: Option<&str>, severity: Severity, message: &str) -> String {
    let kind = match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    match location {
        Some(at) => format!("{at}: {kind}: {message}"),
        None => format!("{kind}: {message}"),
    }
}

/// An error Xcode recorded in a build that still succeeded, typically a build script that printed
/// `error:` and exited 0. It did not fail the build, so it is not printed the way compilers print
/// errors (editors and agents treat those as something to fix) and is not counted in `errors=`.
pub fn nonfatal_error_line(location: Option<&str>, message: &str) -> String {
    match location {
        Some(at) => format!("{PREFIX} error that did not fail the build: {at}: {message}"),
        None => format!("{PREFIX} error that did not fail the build: {message}"),
    }
}

pub fn test_failure_line(failure: &str) -> String {
    format!("{PREFIX} test failed: {failure}")
}

/// Before the end of xcodebuild's output, shown when a failed build left no result to read.
pub fn tail_header(lines: usize) -> String {
    format!("{PREFIX} no result bundle to read; the last {lines} lines of xcodebuild's output:")
}

/// Before xcodebuild starts: its output goes to the log, not the terminal.
pub fn started_line(slot: &str, log: &str) -> String {
    format!("{PREFIX} building in slot {slot}; xcodebuild's full output: {log}")
}

pub fn p5_line(files: &[String]) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "{PREFIX} precondition P5 not met: the build rewrote tracked files: {} — they are restored on the next run",
            files.join(", ")
        )
    })
}

/// Git-ignored files a failed build looked for: sync does not carry them unless the project declares them.
pub fn p8_line(files: &[String]) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "{PREFIX} precondition P8 not met: the build needs git-ignored files, which are not in the slot: {} — list them under ignored_inputs in {}",
            files.join(", "),
            crate::project::FILE
        )
    })
}

pub fn top_misses_line(cache: &CacheStats) -> Option<String> {
    if cache.misses == 0 {
        return None;
    }
    let top: Vec<String> = cache.misses_by_target.iter().take(5).map(|(t, n)| format!("{t} {n}")).collect();
    Some(format!("{PREFIX} top misses: {}", top.join(", ")))
}

/// Where the outputs of this run are: the worktree's output, replaced only by this worktree's
/// next run that makes the same kind of output. `products` is given only when this run copied them;
/// a failed build points only at its results.
pub fn output_lines(action: Action, ok: bool, products: Option<&str>, archive: &str, results: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(r) = results {
        v.push(format!("{PREFIX} results={r}"));
    }
    if !ok {
        return v;
    }
    if action == Action::Archive {
        v.push(format!("{PREFIX} archive={archive}"));
    }
    if let Some(p) = products {
        v.push(format!("{PREFIX} products={p} (replaced by this worktree's next agb build; copy what you keep)"));
    }
    v
}

pub fn dropped_line(flag_and_value: &str) -> String {
    format!("{PREFIX} ignored {flag_and_value}: output locations are managed by agb")
}

/// After a build that left the project's cache over the limit: it was cleared, or could not be.
pub fn cache_cleared_line(used_bytes: u64, limit: &str, cleared: bool) -> String {
    let used = format!("{:.1}G", used_bytes as f64 / f64::from(1u32 << 30));
    if cleared {
        format!("{PREFIX} cache {used} exceeded cache_limit {limit} and was cleared; the next build refills it")
    } else {
        format!("{PREFIX} cache {used} exceeds cache_limit {limit}, but clearing it failed; delete it by hand")
    }
}

pub fn relocated_line(flag: &str, from: &str, to: &str) -> String {
    format!("{PREFIX} {flag} {from} is in the worktree; building the slot's copy {to}")
}

pub fn waiting_line(owner: &str) -> String {
    format!("{PREFIX} waiting for the slot: {owner}")
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Usage,
    SyncFailed,
    Built(i32),
    Signaled(i32),
}

pub fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::Usage => 64,       // EX_USAGE, what xcodebuild itself exits with for bad arguments
        Outcome::SyncFailed => 125, // the wrapper itself failed, as with env and timeout
        Outcome::Built(code) => u8::try_from(code.clamp(0, 255)).unwrap_or(1),
        Outcome::Signaled(sig) => u8::try_from((128 + sig).clamp(0, 255)).unwrap_or(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A7.1
    #[test]
    fn summary_carries_result_slot_timings_cache_and_log() {
        let cache = CacheStats { hits: 46, misses: 23, misses_by_target: vec![] };
        let s = Summary {
            ok: true,
            slot: "app-1a2b3c4d",
            sync_secs: 1.06,
            build_secs: 35.94,
            cache: &cache,
            errors: 0,
            warnings: 2,
            log: "/l/x.log",
        };
        assert_eq!(
            summary_line(&s),
            "agb: ok slot=app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 errors=0 warnings=2 log=/l/x.log"
        );
        assert!(summary_line(&Summary { ok: false, ..s }).starts_with("agb: failed "));
        assert_eq!(phases_line(&[("resolve", 7.7), ("execute", 18.6)]).as_deref(), Some("agb: phases resolve=7.7s execute=18.6s"));
        assert_eq!(phases_line(&[]), None);
    }

    // A7.2
    #[test]
    fn top_misses_lists_at_most_five_targets_and_nothing_without_misses() {
        let by: Vec<(String, usize)> = (0..7).map(|i| (format!("T{i}"), 10 - i)).collect();
        let c = CacheStats { hits: 0, misses: 49, misses_by_target: by };
        assert_eq!(top_misses_line(&c).as_deref(), Some("agb: top misses: T0 10, T1 9, T2 8, T3 7, T4 6"));
        assert_eq!(top_misses_line(&CacheStats::default()), None);
        assert!(p4_line(Some(&["App".into()])).is_some_and(|l| l.contains("P4 not met: App")));
        assert!(p4_line(Some(&[])).is_some_and(|l| l.contains("P4 not met: some Swift targets")));
        assert_eq!(p4_line(None), None);
        assert!(p5_line(&["App/Gen.swift".into()]).is_some_and(|l| l.contains("P5 not met") && l.contains("App/Gen.swift")));
    }

    // A7.7
    #[test]
    fn tells_where_each_action_left_its_output() {
        let (p, a, r) =
            ("/w/.agent-build/output/Products", "/w/.agent-build/output/App.xcarchive", "/w/.agent-build/output/result.xcresult");
        let build = output_lines(Action::Build, true, Some(p), a, Some(r));
        assert_eq!(build[0], format!("agb: results={r}"));
        assert!(build[1].contains(&format!("products={p}")) && build[1].contains("this worktree's next agb build"));
        assert_eq!(output_lines(Action::Test, true, Some(p), a, None).len(), 1, "no results line without a bundle");
        assert_eq!(output_lines(Action::Test, true, None, a, Some(r)), [format!("agb: results={r}")], "nothing built, no products");
        assert_eq!(output_lines(Action::Archive, true, None, a, None), [format!("agb: archive={a}")]);
        assert!(output_lines(Action::Info, true, None, a, None).is_empty());
        assert_eq!(
            output_lines(Action::Build, false, Some(p), a, Some(r)),
            [format!("agb: results={r}")],
            "a failed build has no products"
        );
    }

    // A7.10
    #[test]
    fn errors_that_did_not_fail_the_build_are_marked_as_such() {
        assert_eq!(
            nonfatal_error_line(None, "SwiftFormat not installed"),
            "agb: error that did not fail the build: SwiftFormat not installed"
        );
        assert_eq!(nonfatal_error_line(Some("/w/x.sh:3"), "boom"), "agb: error that did not fail the build: /w/x.sh:3: boom");
    }

    // A7.6
    #[test]
    fn prints_issues_as_compilers_do_and_marks_its_own_lines() {
        assert_eq!(issue_line(Some("/w/A.swift:3:7"), Severity::Error, "boom"), "/w/A.swift:3:7: error: boom");
        assert_eq!(issue_line(None, Severity::Warning, "w"), "warning: w");
        assert_eq!(test_failure_line("S/t(): no"), "agb: test failed: S/t(): no");
        assert!(tail_header(30).starts_with("agb: ") && started_line("s", "/l").contains("/l"));
    }

    // A6.1
    #[test]
    fn exit_codes_say_what_went_wrong() {
        assert_eq!(exit_code(Outcome::Usage), 64);
        assert_eq!(exit_code(Outcome::SyncFailed), 125);
        assert_eq!(exit_code(Outcome::Built(0)), 0);
        assert_eq!(exit_code(Outcome::Built(65)), 65);
        assert_eq!(exit_code(Outcome::Signaled(15)), 143);
        assert_eq!(exit_code(Outcome::Built(300)), 255);
    }
}
