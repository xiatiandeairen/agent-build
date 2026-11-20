//! `git status --porcelain=v1 -z --no-renames`: which paths differ from HEAD and how.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The two status letters, e.g. " M", "D ", "??".
    pub code: String,
    pub path: String,
}

impl Entry {
    /// Gone from the working tree (deleted, staged or not).
    pub fn is_deletion(&self) -> bool {
        self.code.contains('D')
    }

    pub fn is_untracked(&self) -> bool {
        self.code == "??"
    }
}

/// Entries are `XY path` separated by NUL. The first status letter may be a space,
/// so nothing here trims the input.
pub fn parse(porcelain_z: &str) -> Vec<Entry> {
    porcelain_z.split('\0').filter(|e| e.len() > 3).map(|e| Entry { code: e[..2].to_string(), path: e[3..].to_string() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A3.1
    #[test]
    fn keeps_whole_paths_including_the_first_entry_and_spaces() {
        let e = parse(" M Modules/a.swift\0?? new dir/b 文件.swift\0D  gone.swift\0 D also gone.swift\0A  added.swift\0");
        let paths: Vec<_> = e.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(paths, ["Modules/a.swift", "new dir/b 文件.swift", "gone.swift", "also gone.swift", "added.swift"]);
        assert!(e[2].is_deletion() && e[3].is_deletion() && !e[0].is_deletion());
        assert!(e[1].is_untracked() && !e[4].is_untracked());
        assert!(parse("").is_empty());
    }
}
