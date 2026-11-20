//! What the caller sees while xcodebuild runs: slot paths shown as worktree paths,
//! and the thousands of per-task cache remarks kept out of the way (they stay in the log).
use std::borrow::Cow;

/// Replaces `slot` with `worktree` wherever it appears as a whole path (or a path prefix),
/// so errors point at files the caller can open. `/s/src10` is not rewritten for slot `/s/src`.
pub fn to_caller<'a>(line: &'a str, slot: &str, worktree: &str) -> Cow<'a, str> {
    if slot.is_empty() || !line.contains(slot) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(slot) {
        let before_ok = pos == 0 || !is_path_char(rest[..pos].chars().next_back().unwrap_or(' '));
        let after = &rest[pos + slot.len()..];
        let after_ok = after.chars().next().is_none_or(|c| c == '/' || !is_path_char(c));
        out.push_str(&rest[..pos]);
        out.push_str(if before_ok && after_ok { worktree } else { slot });
        rest = after;
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || "._-/~+@".contains(c)
}

/// Per-task cache remarks: kept in the log, left out of the terminal.
pub fn is_cache_remark(line: &str) -> bool {
    let t = line.trim();
    if t == "Cache hit" || t == "Cache miss" {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    ["note: replay cache", "note: replayed cache", "note: local cache", "note: cache miss", "note: cache hit", "note: using cas output"]
        .iter()
        .any(|p| lower.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A7.5
    #[test]
    fn rewrites_slot_paths_only_at_path_boundaries() {
        let r = |l: &str| to_caller(l, "/c/slots/app-1a2b3c4d/src", "/w/my app").into_owned();
        assert_eq!(r("/c/slots/app-1a2b3c4d/src/A.swift:3:1: error: x"), "/w/my app/A.swift:3:1: error: x");
        assert_eq!(r("cd /c/slots/app-1a2b3c4d/src"), "cd /w/my app");
        assert_eq!(r("SRCROOT=\"/c/slots/app-1a2b3c4d/src\""), "SRCROOT=\"/w/my app\"");
        assert_eq!(r("/c/slots/app-1a2b3c4d/src2/A.swift"), "/c/slots/app-1a2b3c4d/src2/A.swift");
        assert_eq!(r("/c/slots/app-1a2b3c4d/dd/Build/x.o"), "/c/slots/app-1a2b3c4d/dd/Build/x.o");
        assert_eq!(r("/x/c/slots/app-1a2b3c4d/src/A.swift"), "/x/c/slots/app-1a2b3c4d/src/A.swift");
        assert_eq!(r("a /c/slots/app-1a2b3c4d/src/A b /c/slots/app-1a2b3c4d/src/B"), "a /w/my app/A b /w/my app/B");
    }

    // A7.6
    #[test]
    fn hides_cache_remarks_but_never_errors_that_mention_a_cache() {
        for l in [
            "Cache hit",
            "  Cache miss",
            "note: replay cache hit",
            "note: Local cache miss for key: llvmcas://1 (in target 'A')",
            "note: using CAS output pcm: x",
            "note: replayed cache hit: llvmcas://2",
        ] {
            assert!(is_cache_remark(l), "{l}");
        }
        for l in ["/a.swift:1:1: error: cache miss handling", "note: Building targets in dependency order", "warning: cache"] {
            assert!(!is_cache_remark(l), "{l}");
        }
    }
}
