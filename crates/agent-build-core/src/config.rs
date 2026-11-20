//! `$XDG_CONFIG_HOME/agent-build/config.yaml`: optional, one key, a minimal YAML subset.
//! Anything outside `key: value` lines and `#` comments is an error with its line number,
//! never a guess.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Size limit of each project's compilation cache (e.g. `20G`, binary units). Xcode is told
    /// too, but does not enforce it, so agb clears a cache that a build left over the limit.
    pub cache_limit: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { cache_limit: "20G".to_string() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let err = |message: &str| ConfigError { line, message: message.to_string() };
        let content = strip_comment(raw);
        if content.trim().is_empty() {
            continue;
        }
        if content.starts_with([' ', '\t']) {
            return Err(err("nested values are not supported; write `key: value` at the start of the line"));
        }
        if content.starts_with('-') {
            return Err(err("lists are not supported"));
        }
        let (key, value) = content.split_once(':').ok_or_else(|| err("expected `key: value`"))?;
        let value = unquote(value.trim());
        match key.trim() {
            "cache_limit" if is_size(value) => config.cache_limit = value.to_string(),
            "cache_limit" => return Err(err("cache_limit must be a size such as 40G, 512M or 1T")),
            other => return Err(err(&format!("unknown key `{other}` (known: cache_limit)"))),
        }
    }
    Ok(config)
}

pub(crate) fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    match line.find(" #") {
        Some(p) => &line[..p],
        None => line,
    }
}

pub(crate) fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

/// `20G` → bytes, in binary units (K = 1024), as `du -h` counts.
pub fn size_bytes(v: &str) -> Option<u64> {
    let (digits, shift) = match v.as_bytes().last()? {
        b'K' => (&v[..v.len() - 1], 10),
        b'M' => (&v[..v.len() - 1], 20),
        b'G' => (&v[..v.len() - 1], 30),
        b'T' => (&v[..v.len() - 1], 40),
        _ => (v, 0),
    };
    digits.parse::<u64>().ok()?.checked_mul(1 << shift)
}

fn is_size(v: &str) -> bool {
    let digits = v.trim_end_matches(['K', 'M', 'G', 'T']);
    let unit_len = v.len() - digits.len();
    !digits.is_empty() && unit_len <= 1 && digits.chars().all(|c| c.is_ascii_digit())
}

/// Base directories per the XDG Base Directory spec: the variable when it is an absolute path,
/// otherwise `$HOME/<default>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub config: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
}

pub fn dirs(lookup: impl Fn(&str) -> Option<String>) -> Option<Dirs> {
    let home = PathBuf::from(lookup("HOME")?);
    let base = |var: &str, default: &str| match lookup(var) {
        Some(v) if v.starts_with('/') => PathBuf::from(v),
        _ => home.join(default),
    };
    Some(Dirs {
        config: base("XDG_CONFIG_HOME", ".config").join("agent-build"),
        cache: base("XDG_CACHE_HOME", ".cache").join("agent-build"),
        state: base("XDG_STATE_HOME", ".local/state").join("agent-build"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A8.1
    #[test]
    fn reads_the_one_key_and_rejects_everything_else_with_a_line_number() {
        assert_eq!(parse("").unwrap(), Config::default());
        assert_eq!(parse("# c\n\ncache_limit: 20G  # smaller\n").unwrap().cache_limit, "20G");
        assert_eq!(parse("cache_limit: \"512M\"").unwrap().cache_limit, "512M");
        let line_of = |t: &str| parse(t).unwrap_err().line;
        assert_eq!(line_of("cache_limit: 1G\nslots: 3\n"), 2);
        assert_eq!(line_of("cache_limit: 1G\n  limit: 40G\n"), 2);
        assert_eq!(line_of("cache:\n  limit: 40G\n"), 1);
        assert_eq!(line_of("- a\n"), 1);
        assert_eq!(line_of("cache_limit: lots\n"), 1);
        assert_eq!(line_of("cache_limit: 40GB\n"), 1);
        assert_eq!(line_of("just text\n"), 1);
        assert!(parse("slots: 3").unwrap_err().message.contains("unknown key `slots`"));
    }

    // A8.1
    #[test]
    fn xdg_variables_move_the_directories_and_relative_ones_are_ignored() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let d = dirs(env(&[("HOME", "/h")])).unwrap();
        assert_eq!(d.config, PathBuf::from("/h/.config/agent-build"));
        assert_eq!(d.cache, PathBuf::from("/h/.cache/agent-build"));
        assert_eq!(d.state, PathBuf::from("/h/.local/state/agent-build"));
        let d = dirs(env(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x"), ("XDG_CACHE_HOME", "rel")])).unwrap();
        assert_eq!(d.config, PathBuf::from("/x/agent-build"));
        assert_eq!(d.cache, PathBuf::from("/h/.cache/agent-build"));
        assert!(dirs(env(&[])).is_none());
    }

    // A16.1
    #[test]
    fn sizes_are_binary_units() {
        assert_eq!(size_bytes("20G"), Some(20 << 30));
        assert_eq!(size_bytes("512M"), Some(512 << 20));
        assert_eq!(size_bytes("1T"), Some(1 << 40));
        assert_eq!(size_bytes("100K"), Some(100 << 10));
        assert_eq!(size_bytes("123"), Some(123));
        assert_eq!(size_bytes("lots"), None);
        assert_eq!(Config::default().cache_limit, "20G");
    }
}
