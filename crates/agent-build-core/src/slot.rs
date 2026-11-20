//! Where a project's build slot lives. One slot per project: every worktree of the same
//! repository maps to the same directory, and different repositories never share one.
use std::path::{Path, PathBuf};

/// `<last segment of origin>-<8 hex of a hash of origin>`; without an origin, the hash is of the
/// main repository's git directory, which every worktree of that repository shares.
pub fn slot_name(origin: Option<&str>, common_git_dir: &str) -> String {
    let identity = origin.unwrap_or(common_git_dir);
    let base = match origin {
        Some(url) => last_segment(url),
        None => last_segment(common_git_dir.trim_end_matches("/.git")),
    };
    let base: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
    let base = if base.is_empty() { "repo".to_string() } else { base };
    format!("{base}-{:08x}", (fnv1a(identity.as_bytes()) >> 32) as u32)
}

fn last_segment(s: &str) -> &str {
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.rsplit(['/', ':']).next().unwrap_or(s)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPaths {
    pub root: PathBuf,
    /// The slot's clone of the code.
    pub src: PathBuf,
    /// The slot's DerivedData.
    pub derived_data: PathBuf,
    pub archives: PathBuf,
    /// The last run's result bundle; replaced by each run.
    pub result_bundle: PathBuf,
    /// Held with flock for the whole run; the kernel releases it when the last process holding it exits.
    pub lock: PathBuf,
    /// Who holds the lock, for the waiting message.
    pub owner: PathBuf,
    /// The ignored inputs the last run mirrored, so paths no longer declared are removed.
    pub ignored_record: PathBuf,
}

pub fn slot_paths(cache_dir: &Path, name: &str) -> SlotPaths {
    let root = cache_dir.join("slots").join(name);
    SlotPaths {
        src: root.join("src"),
        derived_data: root.join("dd"),
        archives: root.join("archives"),
        result_bundle: root.join("result.xcresult"),
        lock: root.join("lock"),
        owner: root.join("owner"),
        ignored_record: root.join("ignored_inputs"),
        root,
    }
}

/// `path` as Xcode prints it. macOS links `/var`, `/tmp` and `/etc` to `/private/...`, and Xcode
/// prints such paths without `/private`; the slot is spelled that way from the start, so the
/// build runs there and its output is rewritten from there with a single spelling.
pub fn as_xcode_prints(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix("/private").filter(|rest| ["/var/", "/tmp/", "/etc/"].iter().any(|p| rest.starts_with(p))) {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// The shared compilation cache directory.
pub fn cas_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("cas")
}

#[cfg(test)]
mod tests {
    use super::*;

    // A2.1
    #[test]
    fn names_follow_the_repository_not_the_worktree() {
        let a = slot_name(Some("git@gitlab.example.com:team/my-app.git"), "/w/my-app/.git");
        assert!(a.starts_with("my-app-") && a.len() == "my-app-".len() + 8, "{a}");
        assert_eq!(a, slot_name(Some("git@gitlab.example.com:team/my-app.git"), "/elsewhere/.git"), "same origin, same slot");
        let fork = slot_name(Some("https://github.com/someone/my-app.git"), "/w/my-app/.git");
        assert!(fork.starts_with("my-app-") && fork != a, "a fork with the same name gets its own slot");
        let local = slot_name(None, "/Users/me/code/My App/.git");
        assert!(local.starts_with("My-App-"), "{local}");
        assert_eq!(local, slot_name(None, "/Users/me/code/My App/.git"));
        assert_ne!(local, slot_name(None, "/Users/me/other/My App/.git"));
        assert!(slot_name(Some("https://host/x/"), "/g").starts_with("x-"));
    }

    // A5.2
    #[test]
    fn each_project_has_its_own_lock() {
        let c = Path::new("/c");
        let a = slot_paths(c, &slot_name(Some("u:a/one.git"), "/g"));
        let b = slot_paths(c, &slot_name(Some("u:a/two.git"), "/g"));
        assert_ne!(a.lock, b.lock);
        assert_eq!(a.lock, slot_paths(c, &slot_name(Some("u:a/one.git"), "/other")).lock);
        assert!(a.src.starts_with(&a.root) && a.derived_data.starts_with(&a.root));
    }

    // A7.5
    #[test]
    fn slot_paths_are_spelled_the_way_xcode_prints_them() {
        assert_eq!(as_xcode_prints(Path::new("/private/var/folders/x/T/c")), PathBuf::from("/var/folders/x/T/c"));
        assert_eq!(as_xcode_prints(Path::new("/private/tmp/c")), PathBuf::from("/tmp/c"));
        assert_eq!(as_xcode_prints(Path::new("/Users/me/.cache")), PathBuf::from("/Users/me/.cache"));
        assert_eq!(as_xcode_prints(Path::new("/private/varnish/c")), PathBuf::from("/private/varnish/c"));
        assert_eq!(as_xcode_prints(Path::new("/private/Users/c")), PathBuf::from("/private/Users/c"));
    }
}
