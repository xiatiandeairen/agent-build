//! Git-ignored paths: the ones a project declares in `.agent-build.yaml` are mirrored into the
//! slot like tracked files (only differing files written, extras removed); the ones it does not
//! declare are named when a failed build was looking for them (precondition P8).
use crate::git;
use crate::sync::{copy_if_differs, remove};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

/// Mirrors each declared path from the worktree into the slot, and removes the paths an earlier
/// run declared but this one does not. `record` keeps the declared list between runs.
pub fn sync(worktree: &Path, slot: &Path, declared: &[String], record: &Path) -> io::Result<usize> {
    let before = fs::read_to_string(record).unwrap_or_default();
    for gone in before.lines().filter(|p| !p.is_empty() && !declared.iter().any(|d| d == p)) {
        remove(&slot.join(gone))?;
    }
    let mut written = 0;
    for path in declared {
        written += mirror(&worktree.join(path), &slot.join(path))?;
    }
    fs::write(record, declared.join("\n"))?;
    Ok(written)
}

fn mirror(src: &Path, dst: &Path) -> io::Result<usize> {
    let meta = match fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return remove(dst).map(|()| 0),
        Err(e) => return Err(e),
    };
    if !meta.is_dir() {
        return copy_if_differs(src, dst).map(usize::from);
    }
    if fs::symlink_metadata(dst).is_ok_and(|m| !m.is_dir()) {
        remove(dst)?;
    }
    fs::create_dir_all(dst)?;
    let mut written = 0;
    let mut names = HashSet::new();
    for entry in fs::read_dir(src)? {
        let name = entry?.file_name();
        written += mirror(&src.join(&name), &dst.join(&name))?;
        names.insert(name);
    }
    for entry in fs::read_dir(dst)? {
        let name = entry?.file_name();
        if !names.contains(&name) {
            remove(&dst.join(&name))?;
        }
    }
    Ok(written)
}

/// Of the paths a failed build mentioned, those that exist in the worktree, are missing from the
/// slot and are git-ignored there: the build needed a file sync does not carry.
pub fn missing_in_slot(worktree: &Path, slot: &Path, mentioned: &[String]) -> Vec<String> {
    mentioned
        .iter()
        .filter(|p| worktree.join(p).exists() && fs::symlink_metadata(slot.join(p)).is_err() && git::is_ignored(worktree, p))
        .take(5)
        .cloned()
        .collect()
}
