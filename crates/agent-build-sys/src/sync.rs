//! Make the slot hold exactly what the worktree holds: same commit, same uncommitted changes,
//! same submodule state. Only files whose content (or type, or executable bit) differs are written,
//! so Xcode's incremental build sees untouched files as untouched.
use crate::git;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

/// Returns how many files were written.
pub fn sync(worktree: &Path, slot: &Path) -> io::Result<usize> {
    // A fresh clone has HEAD at the right commit but an empty index, so it always needs a checkout.
    let fresh = !slot.join(".git").exists();
    if fresh {
        let _ = fs::remove_dir_all(slot); // a half-made slot is useless; a missing one is fine
        git::clone_local(&git::common_dir(worktree)?, slot)?;
    }
    let want = git::head(worktree)?;
    if fresh || git::head(slot).ok().as_deref() != Some(want.as_str()) {
        if !git::has_commit(slot, &want) {
            git::fetch_head(slot, worktree)?;
        }
        git::checkout(slot, &want)?;
    }
    let mut written = overlay(worktree, slot)?;
    for path in git::gitlinks(worktree)? {
        written += submodule(&worktree.join(&path), &slot.join(&path))?;
    }
    Ok(written)
}

/// Uncommitted state: undo the slot's own changes the worktree does not have, then copy the worktree's.
fn overlay(worktree: &Path, slot: &Path) -> io::Result<usize> {
    let wanted = git::status(worktree)?;
    let wanted_paths: HashSet<&str> = wanted.iter().map(|e| e.path.as_str()).collect();
    let mut restore = Vec::new();
    for e in git::status(slot)? {
        if wanted_paths.contains(e.path.as_str()) {
            continue;
        }
        if e.is_untracked() {
            remove(&slot.join(&e.path))?;
        } else {
            restore.push(e.path);
        }
    }
    git::restore(slot, &restore)?;
    let mut written = restore.len();
    // `dir/` is an untracked nested repository: a separate repository, which git does not look into either.
    for e in wanted.iter().filter(|e| !e.path.ends_with('/')) {
        let dst = slot.join(&e.path);
        if e.is_deletion() {
            remove(&dst)?;
        } else if copy_if_differs(&worktree.join(&e.path), &dst)? {
            written += 1;
        }
    }
    Ok(written)
}

/// An initialised submodule is mirrored recursively; an uninitialised one stays empty in the slot.
fn submodule(src: &Path, dst: &Path) -> io::Result<usize> {
    if git::is_own_checkout(src) {
        if !git::is_own_checkout(dst) {
            let _ = fs::remove_dir_all(dst); // an empty gitlink directory, or a stale non-checkout
        }
        return sync(src, dst);
    }
    if dst.join(".git").exists() {
        fs::remove_dir_all(dst)?;
        fs::create_dir_all(dst)?;
    }
    Ok(0)
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Writes `dst` only when it differs from `src`. Returns whether it wrote.
pub fn copy_if_differs(src: &Path, dst: &Path) -> io::Result<bool> {
    let meta = fs::symlink_metadata(src)?;
    let current = fs::symlink_metadata(dst).ok();
    if meta.file_type().is_symlink() {
        let target = fs::read_link(src)?;
        if current.as_ref().is_some_and(|m| m.file_type().is_symlink()) && fs::read_link(dst)? == target {
            return Ok(false);
        }
        remove(dst)?;
        make_parent(dst)?;
        symlink(target, dst)?;
        return Ok(true);
    }
    let bytes = fs::read(src)?;
    let mode = meta.permissions().mode() & 0o777;
    if let Some(m) = &current {
        if m.is_file() && m.permissions().mode() & 0o777 == mode && fs::read(dst)? == bytes {
            return Ok(false);
        }
        if !m.is_file() {
            remove(dst)?;
        }
    }
    make_parent(dst)?;
    fs::write(dst, &bytes)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(mode))?;
    Ok(true)
}

fn make_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(p) => fs::create_dir_all(p),
        None => Ok(()),
    }
}

/// Tracked files the build itself changed in the slot (precondition P5): the slot differs from
/// the worktree although sync had made them equal.
pub fn rewritten_by_build(worktree: &Path, slot: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for e in git::status(slot)? {
        if e.is_untracked() {
            continue;
        }
        let same = match (fs::read(worktree.join(&e.path)), fs::read(slot.join(&e.path))) {
            (Ok(a), Ok(b)) => a == b,
            (Err(_), Err(_)) => true,
            _ => false,
        };
        if !same {
            out.push(e.path);
        }
    }
    Ok(out)
}
