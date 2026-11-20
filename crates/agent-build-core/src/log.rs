//! Reading xcodebuild's output: cache hits and misses, where misses concentrate,
//! targets that cannot use the cache (P4), phase timings, and the test result bundle.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    /// Misses per target, most first; ties keep first-seen order.
    pub misses_by_target: Vec<(String, usize)>,
}

/// The `(in target 'X' ...)` suffix xcodebuild puts on task headers and remarks.
fn target_of(line: &str) -> Option<&str> {
    let start = line.find("(in target '")? + "(in target '".len();
    let len = line[start..].find('\'')?;
    Some(&line[start..start + len])
}

/// Counts the task-level `Cache hit` / `Cache miss` lines that
/// COMPILATION_CACHE_ENABLE_DIAGNOSTIC_REMARKS adds; a miss belongs to the last target named before it.
pub fn cache_stats<'a>(lines: impl IntoIterator<Item = &'a str>) -> CacheStats {
    let mut stats = CacheStats::default();
    let mut current: Option<String> = None;
    for line in lines {
        if let Some(t) = target_of(line) {
            current = Some(t.to_string());
        }
        match line.trim() {
            "Cache hit" => stats.hits += 1,
            "Cache miss" => {
                stats.misses += 1;
                let name = current.clone().unwrap_or_else(|| "(no target)".to_string());
                match stats.misses_by_target.iter_mut().find(|(t, _)| *t == name) {
                    Some((_, n)) => *n += 1,
                    None => stats.misses_by_target.push((name, 1)),
                }
            }
            _ => {}
        }
    }
    stats.misses_by_target.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    stats
}

/// Targets Xcode warns cannot be cached because explicit modules are off (precondition P4).
pub fn p4_targets<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        if line.contains("caching requires explicit module build") {
            if let Some(t) = target_of(line) {
                if !out.iter().any(|x| x == t) {
                    out.push(t.to_string());
                }
            }
        }
    }
    out
}

/// Phase durations in seconds from timestamped output lines, using the markers xcodebuild prints:
/// resolve (packages and scheme pre-actions) → graph (target dependency graph) → plan (build description)
/// → execute (tasks, until the BUILD/TEST result line). A phase whose markers are missing is left out.
pub fn phases(stamped: &[(f64, &str)]) -> Vec<(&'static str, f64)> {
    let first = |pred: &dyn Fn(&str) -> bool, after: f64| stamped.iter().find(|(t, l)| *t >= after && pred(l)).map(|(t, _)| *t);
    let resolve = first(&|l| l.starts_with("Resolve Package Graph"), 0.0);
    let graph = first(&|l| l.starts_with("ComputePackagePrebuildTargetDependencyGraph") || l.starts_with("Prepare packages"), 0.0);
    let plan = first(&|l| l.starts_with("CreateBuildDescription"), 0.0);
    let execute = plan.and_then(|p| first(&|l| is_task_header(l) && !l.starts_with("CreateBuildDescription"), p).filter(|t| *t > p));
    let done = first(&|l| l.starts_with("** BUILD ") || l.starts_with("** TEST ") || l.starts_with("** ARCHIVE "), 0.0);
    let marks = [("resolve", resolve, graph), ("graph", graph, plan), ("plan", plan, execute), ("execute", execute, done)];
    marks.iter().filter_map(|(name, a, b)| Some((*name, b.zip(*a).map(|(b, a)| b - a)?))).filter(|(_, d)| *d >= 0.0).collect()
}

const PLANNING: [&str; 19] = [
    "Command",
    "Resolve",
    "Resolved",
    "Prepare",
    "ComputePackagePrebuildTargetDependencyGraph",
    "CreateBuildRequest",
    "SendProjectDescription",
    "CreateBuildOperation",
    "ComputeTargetDependencyGraph",
    "GatherProvisioningInputs",
    "CreateBuildDescription",
    "Build",
    "Testing",
    "Test",
    "User",
    "Note",
    "Writing",
    "The",
    "Ignoring",
];

/// A task line such as `SwiftCompile normal arm64 ...`: a capitalised word at column 0 that is not a planning step.
fn is_task_header(line: &str) -> bool {
    let word: String = line.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    word.len() > 2
        && word.starts_with(|c: char| c.is_ascii_uppercase())
        && line[word.len()..].starts_with(' ')
        && !PLANNING.contains(&word.as_str())
}

/// The result bundle path xcodebuild prints after a test run.
pub fn xcresult_path<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    lines.into_iter().map(str::trim).filter(|l| l.starts_with('/') && l.ends_with(".xcresult")).last().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines recorded from a real Xcode 27.2 build of my-app (timestamps removed).
    const RECORDED: &str = "\
SwiftExplicitDependencyGeneratePcm arm64 /s/dd/SDKExplicitPrecompiledModules/_Builtin_stdarg.pcm
note: local cache found for key: llvmcas://e61f
note: replay cache hit
Cache hit
SwiftCompile normal arm64 /s/src/A.swift (in target 'FeatureUI' from project 'FeatureUI')
note: Local cache miss for key: llvmcas://ed35 (in target 'FeatureUI' from project 'FeatureUI')
note: Replay cache miss (in target 'FeatureUI' from project 'FeatureUI')
Cache miss
note: Replay cache miss (in target 'App' from project 'App')
Cache miss
note: Replay cache miss (in target 'FeatureUI' from project 'FeatureUI')
Cache miss
/s/src/B.swift:3:1: error: cache miss handling is wrong
warning: swift compiler caching requires explicit module build (SWIFT_ENABLE_EXPLICIT_MODULES=YES) (in target 'App' from project 'App')
warning: swift compiler caching requires explicit module build (SWIFT_ENABLE_EXPLICIT_MODULES=YES) (in target 'UILibrary' from project 'X')
warning: swift compiler caching requires explicit module build (SWIFT_ENABLE_EXPLICIT_MODULES=YES) (in target 'App' from project 'App')";

    // A7.2
    #[test]
    fn counts_task_level_cache_lines_and_groups_misses_by_target() {
        let s = cache_stats(RECORDED.lines());
        assert_eq!((s.hits, s.misses), (1, 3));
        assert_eq!(s.misses_by_target, vec![("FeatureUI".to_string(), 2), ("App".to_string(), 1)]);
        assert_eq!(cache_stats(["Cache miss"]).misses_by_target, vec![("(no target)".to_string(), 1)]);
    }

    // A7.4
    #[test]
    fn names_each_target_that_cannot_be_cached_once() {
        assert_eq!(p4_targets(RECORDED.lines()), ["App", "UILibrary"]);
        assert!(p4_targets(["warning: something else (in target 'A' from project 'A')"]).is_empty());
    }

    // A7.3
    #[test]
    fn splits_phases_at_the_markers_and_leaves_out_missing_ones() {
        let lines = [
            (0.5, "Command line invocation:"),
            (1.0, "Resolve Package Graph"),
            (8.0, "ComputePackagePrebuildTargetDependencyGraph"),
            (12.0, "CreateBuildDescription"),
            (12.5, "Build description path: /x"),
            (14.0, "SwiftCompile normal arm64 /s/A.swift (in target 'A' from project 'A')"),
            (30.0, "** BUILD SUCCEEDED **"),
        ];
        assert_eq!(phases(&lines), vec![("resolve", 7.0), ("graph", 4.0), ("plan", 2.0), ("execute", 16.0)]);
        let no_packages = [(1.0, "CreateBuildDescription"), (2.0, "CompileC x"), (5.0, "** TEST SUCCEEDED **")];
        assert_eq!(phases(&no_packages), vec![("plan", 1.0), ("execute", 3.0)]);
        assert!(phases(&[]).is_empty());
    }

    // A7.7
    #[test]
    fn takes_the_result_bundle_path_from_the_test_output() {
        let out =
            ["Test session results, code coverage, and logs:", "\t/s/dd/Logs/Test/Test-App-2026.11.19.xcresult", "** TEST SUCCEEDED **"];
        assert_eq!(xcresult_path(out).as_deref(), Some("/s/dd/Logs/Test/Test-App-2026.11.19.xcresult"));
        assert_eq!(xcresult_path(["** BUILD SUCCEEDED **"]), None);
    }
}
