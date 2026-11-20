//! Reading what cargo prints with `--message-format=json`: one JSON message per line on stdout
//! (diagnostics, built units), mixed with the plain output of programs and test harnesses it runs.
//! Cargo documents these messages as stable for tools, as xcresulttool's JSON is for Xcode.
use crate::json::{self, Json};
use crate::report::PREFIX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub error: bool,
    /// `path:line:column` of the primary span, the path absolute.
    pub location: Option<String>,
    pub message: String,
    /// rustc's own text, with notes and help, for the log.
    pub rendered: Option<String>,
}

/// One compiled unit (`compiler-artifact`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub package: String,
    /// A package from a path (the project's own) rather than a registry or git.
    pub local: bool,
    pub fresh: bool,
    pub kinds: Vec<String>,
    /// Built for `cargo test` or `bench`: a test harness, not a product.
    pub test: bool,
    pub files: Vec<String>,
    pub executable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Diagnostic(Diagnostic),
    Unit(Unit),
    /// A JSON message agb does not use (build scripts, `build-finished`).
    Other,
}

/// `None` for a line that is not one of cargo's messages: a program's or test harness's own output.
/// `root` is the workspace root, which rustc's relative paths are relative to.
pub fn message(line: &str, root: &str) -> Option<Message> {
    if !line.starts_with('{') {
        return None;
    }
    let j = json::parse(line)?;
    Some(match j.get("reason").str()? {
        "compiler-message" => diagnostic(j.get("message"), root).map_or(Message::Other, Message::Diagnostic),
        "compiler-artifact" => Message::Unit(unit(&j)),
        _ => Message::Other,
    })
}

/// Errors and warnings; notes and the "for more information" lines are not diagnostics of their own.
fn diagnostic(m: &Json, root: &str) -> Option<Diagnostic> {
    let level = m.get("level").str()?;
    let error = level.starts_with("error");
    if !error && level != "warning" {
        return None;
    }
    let primary = m.get("spans").items().iter().find(|s| matches!(s.get("is_primary"), Json::Bool(true)));
    let location = primary.and_then(|s| {
        let file = s.get("file_name").str()?;
        let path = if file.starts_with('/') { file.to_string() } else { format!("{}/{file}", root.trim_end_matches('/')) };
        Some(format!("{path}:{}:{}", s.get("line_start").num()?, s.get("column_start").num()?))
    });
    let rendered = m.get("rendered").str().map(str::to_string);
    Some(Diagnostic { error, location, message: m.get("message").str()?.to_string(), rendered })
}

fn unit(j: &Json) -> Unit {
    let strings = |v: &Json| v.items().iter().filter_map(|x| x.str().map(str::to_string)).collect::<Vec<_>>();
    let id = j.get("package_id").str().unwrap_or_default();
    let target = j.get("target");
    Unit {
        package: package_name(id, target.get("name").str().unwrap_or_default()),
        local: id.starts_with("path+"),
        fresh: matches!(j.get("fresh"), Json::Bool(true)),
        kinds: strings(target.get("kind")),
        test: matches!(j.get("profile").get("test"), Json::Bool(true)),
        files: strings(j.get("filenames")),
        executable: j.get("executable").str().map(str::to_string),
    }
}

/// `path+file:///w/core#sample-core@0.1.0` → `sample-core`; the target's name when the id has none.
fn package_name(id: &str, fallback: &str) -> String {
    let after = id.rsplit_once('#').map_or("", |(_, r)| r);
    let name = after.split('@').next().unwrap_or_default();
    if name.is_empty() || name.contains('/') { fallback.to_string() } else { name.to_string() }
}

/// How much of the build Cargo reused, from its own `fresh` flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub fresh: usize,
    pub rebuilt: usize,
    /// The project's own packages that were rebuilt, each once, in build order.
    pub rebuilt_local: Vec<String>,
}

impl Stats {
    pub fn add(&mut self, u: &Unit) {
        if u.fresh {
            self.fresh += 1;
            return;
        }
        self.rebuilt += 1;
        if u.local && !self.rebuilt_local.contains(&u.package) {
            self.rebuilt_local.push(u.package.clone());
        }
    }
}

const PRODUCT_KINDS: [&str; 4] = ["bin", "staticlib", "cdylib", "dylib"];

/// The files of the project's own products (libraries other languages link, programs) under
/// `target_dir`, relative to it; intermediate files (`.rlib`, `deps/`, build scripts) are not products.
pub fn products(units: &[Unit], target_dir: &str) -> Vec<String> {
    let prefix = format!("{}/", target_dir.trim_end_matches('/'));
    let mut found = Vec::new();
    for u in units.iter().filter(|u| u.local && !u.test && u.kinds.iter().any(|k| PRODUCT_KINDS.contains(&k.as_str()))) {
        for f in &u.files {
            let wanted = u.executable.as_deref() == Some(f.as_str()) || [".a", ".dylib", ".so"].iter().any(|x| f.ends_with(x));
            let rel = f.strip_prefix(&prefix);
            if let Some(rel) = rel.filter(|r| wanted && !r.split('/').any(|c| c == "deps" || c == "build")) {
                if !found.iter().any(|x: &String| x == rel) {
                    found.push(rel.to_string());
                }
            }
        }
    }
    found
}

/// Failed tests from a test harness's plain output: `name: path:line:column: message` when the
/// test panicked with a location (made absolute against `root`), else the name alone.
pub fn test_failures(lines: &[String], root: &str) -> Vec<String> {
    let mut failed: Vec<String> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line.strip_prefix("test ").and_then(|r| r.strip_suffix(" ... FAILED")) else { continue };
        let header = format!("---- {name} stdout ----");
        let block = lines.iter().skip(i).position(|l| *l == header).map(|p| i + p + 1);
        let panic = block
            .and_then(|b| (b..lines.len()).take_while(|&k| !lines[k].starts_with("---- ")).find(|&k| lines[k].contains(" panicked at ")));
        let reason = panic.and_then(|k| {
            let at = lines[k].split(" panicked at ").nth(1)?.trim_end_matches(':');
            let msg = lines.get(k + 1).map(String::as_str).unwrap_or_default();
            let path = if at.starts_with('/') { at.to_string() } else { format!("{}/{at}", root.trim_end_matches('/')) };
            Some(format!("{path}: {msg}"))
        });
        failed.push(reason.map_or_else(|| name.to_string(), |r| format!("{name}: {r}")));
    }
    failed
}

pub struct Summary<'a> {
    pub ok: bool,
    pub slot: &'a str,
    pub sync_secs: f64,
    pub build_secs: f64,
    pub stats: &'a Stats,
    pub errors: usize,
    pub warnings: usize,
    pub log: &'a str,
}

/// Same fields as the xcodebuild summary, with Cargo's reuse counts in place of Xcode's cache.
pub fn summary_line(s: &Summary) -> String {
    format!(
        "{PREFIX} {} slot={} sync={:.1}s build={:.1}s units fresh={} rebuilt={} errors={} warnings={} log={}",
        if s.ok { "ok" } else { "failed" },
        s.slot,
        s.sync_secs,
        s.build_secs,
        s.stats.fresh,
        s.stats.rebuilt,
        s.errors,
        s.warnings,
        s.log
    )
}

/// The project's own packages that were rebuilt: where this run's time went.
pub fn rebuilt_line(stats: &Stats) -> Option<String> {
    let names = &stats.rebuilt_local;
    if names.is_empty() {
        return None;
    }
    let shown: Vec<&str> = names.iter().take(10).map(String::as_str).collect();
    let more = if names.len() > 10 { format!(" and {} more", names.len() - 10) } else { String::new() };
    Some(format!("{PREFIX} rebuilt: {}{more}", shown.join(", ")))
}

pub fn wrappers_off_line(set: &[String]) -> Option<String> {
    (!set.is_empty()).then(|| {
        format!("{PREFIX} {} cleared: in the slot rustc runs without a wrapper; Cargo's own tracking decides reuse", set.join(", "))
    })
}

pub fn started_line(slot: &str, log: &str) -> String {
    format!("{PREFIX} building in slot {slot}; cargo's full output: {log}")
}

#[cfg(test)]
#[path = "cargo_out_tests.rs"]
mod tests;
