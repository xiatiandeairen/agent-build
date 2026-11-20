//! The worktree's output directory (`project::Output`): readied before a run, and given a copy of
//! the slot's products after a build that made them.
use crate::sync::remove;
use agent_build_core::project::{OUTPUT_GITIGNORE, Output};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Before a run: the directory exists and ignores itself in git, a copy an earlier run was cut off
/// in is gone, and so is each path in `replaced`, the outputs this run makes anew.
pub fn prepare(out: &Output, replaced: &[PathBuf]) -> io::Result<()> {
    fs::create_dir_all(&out.dir)?;
    let ignore = out.dir.join(".gitignore");
    if fs::read_to_string(&ignore).ok().as_deref() != Some(OUTPUT_GITIGNORE) {
        fs::write(&ignore, OUTPUT_GITIGNORE)?;
    }
    remove(&out.staging)?;
    replaced.iter().try_for_each(|p| remove(p))
}

/// The archives in the output directory, so an archive run replaces them all and leaves only its own.
pub fn archives(out: &Output) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(&out.dir) else { return Vec::new() };
    entries.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|x| x == "xcarchive")).collect()
}

/// After a successful build: `out.products` becomes a copy of the slot's `products`; `false` when
/// the build left none. APFS clones the files (0.43 s for 1.1 GB of products, measured), so no data
/// is copied until either side changes; across volumes it falls back to a plain copy. The copy is
/// made beside the old one and renamed over it, so a cut-off copy is never taken for the products.
pub fn publish(products: &Path, out: &Output) -> io::Result<bool> {
    if !products.is_dir() {
        return Ok(false);
    }
    remove(&out.staging)?;
    if !copy(&["-cR"], products, &out.staging)? {
        remove(&out.staging)?;
        if !copy(&["-R"], products, &out.staging)? {
            remove(&out.staging)?;
            return Err(io::Error::other(format!("cp could not copy {}", products.display())));
        }
    }
    remove(&out.products)?;
    fs::rename(&out.staging, &out.products)?;
    Ok(true)
}

/// After a successful cargo build: `out.target` becomes a copy of `files` (relative to the slot's
/// `target_dir`, at the same relative paths), made and swapped in the same way as `publish`;
/// `false` when the build made no products.
pub fn publish_files(target_dir: &Path, files: &[String], out: &Output) -> io::Result<bool> {
    if files.is_empty() {
        return Ok(false);
    }
    remove(&out.staging)?;
    for rel in files {
        let to = out.staging.join(rel);
        fs::create_dir_all(to.parent().unwrap_or(&out.staging))?;
        let from = target_dir.join(rel);
        if !copy(&["-c"], &from, &to)? && !copy(&[], &from, &to)? {
            remove(&out.staging)?;
            return Err(io::Error::other(format!("cp could not copy {}", from.display())));
        }
    }
    remove(&out.target)?;
    fs::rename(&out.staging, &out.target)?;
    Ok(true)
}

/// `cp` keeps symbolic links inside app bundles as links and, with `-c`, clones instead of copying.
fn copy(flags: &[&str], from: &Path, to: &Path) -> io::Result<bool> {
    Ok(Command::new("cp").args(flags).arg(from).arg(to).output()?.status.success())
}
