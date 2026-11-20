//! How much disk a directory takes, counted like `du`: allocated blocks, symbolic links not followed.
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Bytes allocated under `dir`; 0 when it does not exist.
pub fn disk_usage(dir: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(dir) else { return 0 };
    let own = meta.blocks() * 512;
    if !meta.is_dir() {
        return own;
    }
    let children = fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok);
    own + children.map(|e| disk_usage(&e.path())).sum::<u64>()
}
