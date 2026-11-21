//! `.agents/build/` at the root of a worktree: what agb keeps for this project and this worktree.
//!
//! `config.yaml` lists the git-ignored paths the build needs (precondition P8). Owned and committed
//! by the project; the global config stays project-agnostic. Same minimal YAML subset as the
//! config, plus one block list:
//!
//! ```yaml
//! ignored_inputs:
//!   - rust-core/build/
//!   - App/Generated.swift
//! ```
//!
//! `output/` holds what a run hands to its caller. The slot is shared by every worktree, so
//! anything left there changes with another worktree's build; here it changes only with this
//! worktree's next run that makes the same kind of output. It ignores itself in git, so it is
//! neither committed nor synced into the slot.
use crate::config::{ConfigError, strip_comment, unquote};
use std::path::{Path, PathBuf};

pub const FILE: &str = ".agents/build/config.yaml";
const OUTPUT: &str = ".agents/build/output";
/// Written as `output/.gitignore`: ignores the directory, itself included.
pub const OUTPUT_GITIGNORE: &str = "# Automatically created by agent-build.\n*\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub dir: PathBuf,
    /// A copy of the slot's products as this run left them (xcodebuild).
    pub products: PathBuf,
    /// cargo's products, at the same paths as in its target directory.
    pub target: PathBuf,
    /// Where the copy is made before it becomes `products`, so a cut-off copy is never mistaken for one.
    pub staging: PathBuf,
    pub result_bundle: PathBuf,
    /// xcodebuild's full output.
    pub log: PathBuf,
}

pub fn output(worktree: &Path) -> Output {
    let dir = worktree.join(OUTPUT);
    Output {
        products: dir.join("Products"),
        target: dir.join("target"),
        staging: dir.join(".staging"),
        result_bundle: dir.join("result.xcresult"),
        log: dir.join("build.log"),
        dir,
    }
}

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

    // A19
    #[test]
    fn the_output_lives_in_the_worktree_and_ignores_itself() {
        let out = output(Path::new("/w"));
        assert_eq!(out.dir, PathBuf::from("/w/.agents/build/output"));
        assert!(
            [&out.products, &out.target, &out.staging, &out.result_bundle, &out.log].iter().all(|p| p.parent() == Some(out.dir.as_path()))
        );
        assert_eq!(OUTPUT_GITIGNORE.lines().last(), Some("*"));
        assert!(Path::new(FILE).starts_with(".agents/build"));
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
