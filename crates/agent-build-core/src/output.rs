//! Slot paths shown as worktree paths, so what agb prints points at files the caller can open.
use std::borrow::Cow;

/// Replaces `slot` with `worktree` wherever it appears as a whole path (or a path prefix),
/// so errors point at files the caller can open. `/s/src10` is not rewritten for slot `/s/src`.
/// `slot` is spelled the way Xcode prints it (`slot::as_xcode_prints`), so one spelling is enough.
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
}
