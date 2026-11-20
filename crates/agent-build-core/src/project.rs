//! `.agent-build.yaml` at the root of the project's repository: the git-ignored paths its build
//! needs (precondition P8). Owned and committed by the project; the global config stays
//! project-agnostic. Same minimal YAML subset as the config, plus one block list:
//!
//! ```yaml
//! ignored_inputs:
//!   - rust-core/build/
//!   - App/Generated.swift
//! ```
use crate::config::{ConfigError, strip_comment, unquote};

pub const FILE: &str = ".agent-build.yaml";

/// Paths relative to the repository root, without a trailing `/`.
pub fn parse(text: &str) -> Result<Vec<String>, ConfigError> {
    let mut inputs = Vec::new();
    let mut in_list = false;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let err = |message: &str| ConfigError { line, message: message.to_string() };
        let content = strip_comment(raw);
        if content.trim().is_empty() {
            continue;
        }
        if let Some(item) = content.trim_start().strip_prefix("- ").filter(|_| content.starts_with([' ', '\t'])) {
            if !in_list {
                return Err(err("a list item must follow `ignored_inputs:`"));
            }
            inputs.push(relative_path(unquote(item.trim())).map_err(|m| err(&m))?);
            continue;
        }
        if content.starts_with([' ', '\t', '-']) {
            return Err(err("expected `ignored_inputs:` followed by indented `- path` lines"));
        }
        match content.split_once(':').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("ignored_inputs", "")) => in_list = true,
            Some(("ignored_inputs", _)) => return Err(err("write each path on its own `- path` line under `ignored_inputs:`")),
            Some((other, _)) => return Err(err(&format!("unknown key `{other}` (known: ignored_inputs)"))),
            None => return Err(err("expected `key:`")),
        }
    }
    Ok(inputs)
}

/// A path inside the repository: relative, no `..`, not the root itself.
fn relative_path(p: &str) -> Result<String, String> {
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Err("the path is empty or the repository root".to_string());
    }
    if trimmed.starts_with('/') || trimmed.split('/').any(|c| c == "..") {
        return Err(format!("`{p}` must be a path inside the repository, relative to its root"));
    }
    Ok(trimmed.to_string())
}

/// Paths under `slot_src` that a build's output mentions, relative to it, each once, in order.
/// A path ends at a quote, a `:` (as in `file:line:col`), a `)` or the end of the line.
pub fn mentioned_paths<'a>(lines: impl Iterator<Item = &'a str>, slot_src: &str) -> Vec<String> {
    let prefix = format!("{}/", slot_src.trim_end_matches('/'));
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let mut rest = line;
        while let Some(at) = rest.find(&prefix) {
            let tail = &rest[at + prefix.len()..];
            let end = tail.find(['\'', '"', ':', ')']).unwrap_or(tail.len());
            let path = tail[..end].trim_end_matches(['.', ' ']);
            if !path.is_empty() && !out.iter().any(|p| p == path) {
                out.push(path.to_string());
            }
            rest = &tail[end..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A11.1
    #[test]
    fn reads_the_list_of_ignored_inputs_and_rejects_anything_else_with_a_line_number() {
        assert_eq!(parse("").unwrap(), Vec::<String>::new());
        let text = "# what the build needs\nignored_inputs:\n  - rust-core/build/\n  - \"App/Gen erated.swift\"  # spaces\n";
        assert_eq!(parse(text).unwrap(), ["rust-core/build", "App/Gen erated.swift"]);
        let line_of = |t: &str| parse(t).unwrap_err().line;
        assert_eq!(line_of("ignored_inputs:\n  - /abs\n"), 2);
        assert_eq!(line_of("ignored_inputs:\n  - a/../../b\n"), 2);
        assert_eq!(line_of("ignored_inputs:\n  - ./\n"), 2);
        assert_eq!(line_of("  - a\n"), 1);
        assert_eq!(line_of("ignored_inputs: a\n"), 1);
        assert_eq!(line_of("ignored_inputs:\n- a\n"), 2);
        assert_eq!(line_of("ignored_inputs:\n  - a\nslots: 3\n"), 3);
        assert!(parse("cache_limit: 1G").unwrap_err().message.contains("unknown key `cache_limit`"));
    }

    // A11.2
    #[test]
    fn finds_slot_paths_in_build_output_whatever_surrounds_them() {
        let lines = [
            "error: Build input file cannot be found: '/s/src/App/Generated.swift'. Did you forget",
            "/s/src/App/A.swift:3:7: error: cannot find 'Greeting' in scope",
            "/s/src/App/A.swift:9:1: error: again",
            "note: (in /s/src/Gen Dir/x.h)",
            "/s/srcother/App/B.swift:1:1: error: a different directory",
            "no paths here",
        ];
        assert_eq!(mentioned_paths(lines.into_iter(), "/s/src"), ["App/Generated.swift", "App/A.swift", "Gen Dir/x.h"]);
    }
}
