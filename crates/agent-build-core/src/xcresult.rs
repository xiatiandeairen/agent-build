//! Reading the result bundle through `xcresulttool`'s JSON: errors and warnings with their
//! locations, failed tests, the compilation cache counters Xcode reports, where misses
//! concentrate, targets that cannot use the cache (P4), and phase timings.
use crate::json::Json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// `path:line:col` when Xcode knows the location, as compilers print it (1-based).
    pub location: Option<String>,
    pub message: String,
}

/// Errors then warnings from `xcresulttool get build-results`.
pub fn issues(build_results: &Json) -> Vec<Issue> {
    let read = |key: &str, severity: Severity| {
        build_results.get(key).items().iter().map(move |i| Issue {
            severity,
            location: i.get("sourceURL").str().and_then(location),
            message: i.get("message").str().unwrap_or_default().to_string(),
        })
    };
    read("errors", Severity::Error).chain(read("warnings", Severity::Warning)).collect()
}

/// `file:///a%20b/X.swift#...&StartingColumnNumber=29&StartingLineNumber=12&...` → `/a b/X.swift:13:30`.
/// The bundle counts lines and columns from 0.
fn location(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let (path, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let field =
        |name: &str| fragment.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=')?.parse::<u32>().ok()).map(|n| n + 1);
    let path = percent_decode(path);
    Some(match (field("StartingLineNumber"), field("StartingColumnNumber")) {
        (Some(l), Some(c)) => format!("{path}:{l}:{c}"),
        (Some(l), None) => format!("{path}:{l}"),
        _ => path,
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `Suite/test(): reason` for each failed test in `xcresulttool get test-results summary`.
pub fn test_failures(summary: &Json) -> Vec<String> {
    summary
        .get("testFailures")
        .items()
        .iter()
        .map(|f| {
            let name = f.get("testIdentifierString").str().or(f.get("testName").str()).unwrap_or("(unnamed test)");
            format!("{name}: {}", f.get("failureText").str().unwrap_or_default())
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    /// Tasks that missed, per target, most first; ties keep first-seen order.
    pub misses_by_target: Vec<(String, usize)>,
}

/// Hits and misses from the build's `BuildOperationMetrics` attachment (every counter named
/// `...CacheHits` / `...CacheMisses`); misses per target from the tasks carrying a cache-miss notice.
pub fn cache(log: &Json) -> CacheStats {
    let mut stats = CacheStats::default();
    for a in log.get("attachments").items() {
        if !a.get("uniformTypeIdentifier").str().unwrap_or_default().ends_with("BuildOperationMetrics") {
            continue;
        }
        let metrics = a.get("data").str().and_then(crate::json::parse).unwrap_or(Json::Null);
        for (name, n) in metrics.get("counters").members() {
            let n = n.num().unwrap_or(0.0) as usize;
            if name.ends_with("CacheHits") {
                stats.hits += n;
            } else if name.ends_with("CacheMisses") {
                stats.misses += n;
            }
        }
    }
    for task in tasks(log) {
        if notices(task).any(|t| t.starts_with("local cache miss")) {
            let name = target_of(task).unwrap_or("(no target)").to_string();
            match stats.misses_by_target.iter_mut().find(|(t, _)| *t == name) {
                Some((_, n)) => *n += 1,
                None => stats.misses_by_target.push((name, 1)),
            }
        }
    }
    stats.misses_by_target.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    stats
}

/// When Xcode warns that caching needs explicit modules (P4): the targets whose Swift compiles
/// carry no cache notice at all. `None` when there is no such warning.
pub fn p4_targets(build_results: &Json, log: &Json) -> Option<Vec<String>> {
    let warned = build_results
        .get("warnings")
        .items()
        .iter()
        .any(|w| w.get("message").str().is_some_and(|m| m.contains("caching requires explicit module build")));
    if !warned {
        return None;
    }
    let (mut swift, mut cached) = (Vec::new(), Vec::new());
    for task in tasks(log) {
        let Some(target) = target_of(task) else { continue };
        if command(task).starts_with("SwiftCompile ") && !swift.contains(&target) {
            swift.push(target);
        }
        if notices(task).any(|t| t.contains("cache hit") || t.contains("cache miss")) && !cached.contains(&target) {
            cached.push(target);
        }
    }
    Some(swift.into_iter().filter(|t| !cached.contains(t)).map(String::from).collect())
}

/// Seconds spent resolving packages, building the dependency graph, planning, and executing.
pub fn phases(log: &Json) -> Vec<(&'static str, f64)> {
    let mut sums = [("resolve", 0.0), ("graph", 0.0), ("plan", 0.0)];
    for step in log.get("subsections").items() {
        let title = step.get("title").str().unwrap_or_default();
        let slot = if title.starts_with("Resolve") || title == "Prepare packages" || title.ends_with("for package preparation") {
            0
        } else if title == "Compute target dependency graph" {
            1
        } else if title == "Create build description" {
            2
        } else {
            continue;
        };
        sums[slot].1 += step.get("duration").num().unwrap_or(0.0);
    }
    let total = log.get("duration").num().unwrap_or(0.0);
    let before: f64 = sums.iter().map(|(_, d)| d).sum();
    let mut out: Vec<(&'static str, f64)> = sums.into_iter().filter(|(_, d)| *d > 0.0).collect();
    if total > 0.0 {
        out.push(("execute", (total - before).max(0.0)));
    }
    out
}

/// The build's task sections: the direct subsections of the log's root.
fn tasks(log: &Json) -> impl Iterator<Item = &Json> {
    log.get("subsections").items().iter()
}

fn command(task: &Json) -> &str {
    task.get("commandInvocationDetails").get("commandDetails").str().unwrap_or_default()
}

/// The `(in target 'X' ...)` suffix of a task's command.
fn target_of(task: &Json) -> Option<&str> {
    let c = command(task);
    let start = c.find("(in target '")? + "(in target '".len();
    let len = c[start..].find('\'')?;
    Some(&c[start..start + len])
}

/// Lowercased titles of a task's notices, where the cache remarks are.
fn notices(task: &Json) -> impl Iterator<Item = String> + '_ {
    task.get("messages").items().iter().filter_map(|m| m.get("title").str()).map(str::to_ascii_lowercase)
}

#[cfg(test)]
#[path = "xcresult_tests.rs"]
mod tests;
