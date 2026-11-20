//! Git commands. Reads of the caller's worktree use --no-optional-locks so they never rewrite its
//! index; hooks are disabled; nothing here talks to a remote (fetches and clones use local paths).
use agent_build_core::status::{self, Entry};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let mut child = Command::new("git")
        .args(["--no-optional-locks", "-c", "core.hooksPath=/dev/null", "-c", "advice.detachedHead=false"])
        .args(args)
        .current_dir(dir)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input)?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let msg = String::from_utf8_lossy(&out.stderr);
        Err(io::Error::other(format!("git {} in {}: {}", args.join(" "), dir.display(), msg.trim())))
    }
}

fn text(dir: &Path, args: &[&str]) -> io::Result<String> {
    Ok(String::from_utf8_lossy(&git(dir, args, None)?).trim_end_matches('\n').to_string())
}

/// Root of the worktree containing `dir`, as a physical path.
pub fn toplevel(dir: &Path) -> io::Result<PathBuf> {
    PathBuf::from(text(dir, &["rev-parse", "--show-toplevel"])?).canonicalize()
}

/// `dir` relative to its worktree root: "" or "Modules/".
pub fn prefix(dir: &Path) -> io::Result<String> {
    text(dir, &["rev-parse", "--show-prefix"])
}

/// The main repository's git directory, shared by all its worktrees.
pub fn common_dir(dir: &Path) -> io::Result<PathBuf> {
    Ok(PathBuf::from(text(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?))
}

pub fn origin(dir: &Path) -> Option<String> {
    text(dir, &["config", "--get", "remote.origin.url"]).ok().filter(|s| !s.is_empty())
}

pub fn head(dir: &Path) -> io::Result<String> {
    text(dir, &["rev-parse", "--verify", "HEAD"])
}

pub fn status(dir: &Path) -> io::Result<Vec<Entry>> {
    let out = git(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=all", "--no-renames"], None)?;
    Ok(status::parse(&String::from_utf8_lossy(&out)))
}

pub fn has_commit(dir: &Path, sha: &str) -> bool {
    git(dir, &["cat-file", "-e", &format!("{sha}^{{commit}}")], None).is_ok()
}

/// Bring the worktree's HEAD commit into the slot, from the worktree itself (a local path).
pub fn fetch_head(slot: &Path, worktree: &Path) -> io::Result<()> {
    git(slot, &["fetch", "-q", "--no-tags", "--no-recurse-submodules", &worktree.to_string_lossy(), "HEAD"], None).map(drop)
}

/// Move the slot to `sha`, dropping any local change and untracked file (ignored files stay).
pub fn checkout(slot: &Path, sha: &str) -> io::Result<()> {
    git(slot, &["checkout", "-q", "-f", "--detach", sha], None)?;
    git(slot, &["clean", "-q", "-ffd"], None).map(drop)
}

/// Put these tracked paths back to their HEAD content.
pub fn restore(slot: &Path, paths: &[String]) -> io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let list = paths.join("\0");
    git(slot, &["checkout", "-q", "HEAD", "--pathspec-from-file=-", "--pathspec-file-nul"], Some(list.as_bytes())).map(drop)
}

/// Paths of submodules (gitlinks) recorded in the index.
pub fn gitlinks(dir: &Path) -> io::Result<Vec<String>> {
    let out = git(dir, &["ls-files", "-s", "-z"], None)?;
    Ok(String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|e| e.starts_with("160000 "))
        .filter_map(|e| e.split_once('\t').map(|(_, p)| p.to_string()))
        .collect())
}

/// True when `dir` is the root of its own checkout (an initialised submodule), not a plain directory.
pub fn is_own_checkout(dir: &Path) -> bool {
    dir.join(".git").exists() && toplevel(dir).ok() == dir.canonicalize().ok()
}

/// A slot clone: objects from a local repository, no checkout, no network.
pub fn clone_local(from_git_dir: &Path, to: &Path) -> io::Result<()> {
    let parent = to.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(parent)?;
    git(parent, &["clone", "-q", "--no-checkout", &from_git_dir.to_string_lossy(), &to.to_string_lossy()], None).map(drop)
}

/// Whether git ignores `path` (relative to the worktree root) in this worktree.
pub fn is_ignored(worktree: &Path, path: &str) -> bool {
    git(worktree, &["check-ignore", "-q", "--", path], None).is_ok()
}
