//! The lines agb prints itself, the exit code, and which old logs to remove.
//! Every line starts with `agb:` so agents can find them in a long build log.
use crate::args::Action;
use crate::log::CacheStats;

pub const PREFIX: &str = "agb:";

pub struct Summary<'a> {
    pub ok: bool,
    pub slot: &'a str,
    pub sync_secs: f64,
    pub build_secs: f64,
    pub cache: &'a CacheStats,
    pub log: &'a str,
}

pub fn summary_line(s: &Summary) -> String {
    format!(
        "{PREFIX} {} slot={} sync={:.1}s build={:.1}s cache hit={} miss={} log={}",
        if s.ok { "ok" } else { "failed" },
        s.slot,
        s.sync_secs,
        s.build_secs,
        s.cache.hits,
        s.cache.misses,
        s.log
    )
}

pub fn phases_line(phases: &[(&str, f64)]) -> Option<String> {
    if phases.is_empty() {
        return None;
    }
    let parts: Vec<String> = phases.iter().map(|(n, d)| format!("{n}={d:.1}s")).collect();
    Some(format!("{PREFIX} phases {}", parts.join(" ")))
}

pub fn p4_line(targets: &[String]) -> Option<String> {
    (!targets.is_empty()).then(|| {
        format!("{PREFIX} precondition P4 not met: {} (explicit modules off) — their Swift compiles are not cached", targets.join(", "))
    })
}

pub fn p5_line(files: &[String]) -> Option<String> {
    (!files.is_empty()).then(|| {
        format!(
            "{PREFIX} precondition P5 not met: the build rewrote tracked files: {} — they are restored on the next run",
            files.join(", ")
        )
    })
}

pub fn top_misses_line(cache: &CacheStats) -> Option<String> {
    if cache.misses == 0 {
        return None;
    }
    let top: Vec<String> = cache.misses_by_target.iter().take(5).map(|(t, n)| format!("{t} {n}")).collect();
    Some(format!("{PREFIX} top misses: {}", top.join(", ")))
}

/// Where the outputs of this run are. They live in the slot and change on the project's next run.
pub fn output_lines(action: Action, products: &str, archive: &str, xcresult: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    match action {
        Action::Archive => v.push(format!("{PREFIX} archive={archive}")),
        Action::Test => {
            if let Some(x) = xcresult {
                v.push(format!("{PREFIX} test results={x}"));
            }
        }
        Action::Build => {}
    }
    v.push(format!("{PREFIX} products={products} (valid until this project's next agb run; copy what you keep)"));
    v
}

pub fn dropped_line(flag_and_value: &str) -> String {
    format!("{PREFIX} ignored {flag_and_value}: output locations are managed by agb")
}

pub fn waiting_line(owner: &str) -> String {
    format!("{PREFIX} waiting for the slot: {owner}")
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Usage,
    SyncFailed,
    Built(i32),
    Signaled(i32),
}

pub fn exit_code(outcome: Outcome) -> u8 {
    match outcome {
        Outcome::Usage => 2,
        Outcome::SyncFailed => 3,
        Outcome::Built(code) => u8::try_from(code.clamp(0, 255)).unwrap_or(1),
        Outcome::Signaled(sig) => u8::try_from((128 + sig).clamp(0, 255)).unwrap_or(1),
    }
}

/// Logs are named so that they sort oldest first; keep the newest `keep`.
pub fn logs_to_delete(mut names: Vec<String>, keep: usize) -> Vec<String> {
    names.sort();
    let excess = names.len().saturating_sub(keep);
    names.truncate(excess);
    names
}

/// `YYYYMMDD-HHMMSS-<pid>-<slot>.log` in UTC, so names sort oldest first.
pub fn log_file_name(unix_secs: u64, pid: u32, slot: &str) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let secs = unix_secs % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{pid}-{slot}.log", secs / 3600, secs % 3600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A7.1
    #[test]
    fn summary_carries_result_slot_timings_cache_and_log() {
        let cache = CacheStats { hits: 46, misses: 23, misses_by_target: vec![] };
        let s = Summary { ok: true, slot: "app-1a2b3c4d", sync_secs: 1.06, build_secs: 35.94, cache: &cache, log: "/l/x.log" };
        assert_eq!(summary_line(&s), "agb: ok slot=app-1a2b3c4d sync=1.1s build=35.9s cache hit=46 miss=23 log=/l/x.log");
        assert!(summary_line(&Summary { ok: false, ..s }).starts_with("agb: failed "));
        assert_eq!(phases_line(&[("resolve", 7.7), ("execute", 18.6)]).as_deref(), Some("agb: phases resolve=7.7s execute=18.6s"));
        assert_eq!(phases_line(&[]), None);
    }

    // A7.2
    #[test]
    fn top_misses_lists_at_most_five_targets_and_nothing_without_misses() {
        let by: Vec<(String, usize)> = (0..7).map(|i| (format!("T{i}"), 10 - i)).collect();
        let c = CacheStats { hits: 0, misses: 49, misses_by_target: by };
        assert_eq!(top_misses_line(&c).as_deref(), Some("agb: top misses: T0 10, T1 9, T2 8, T3 7, T4 6"));
        assert_eq!(top_misses_line(&CacheStats::default()), None);
        assert!(p4_line(&["App".into()]).is_some_and(|l| l.contains("P4 not met: App")));
        assert!(p5_line(&["App/Gen.swift".into()]).is_some_and(|l| l.contains("P5 not met") && l.contains("App/Gen.swift")));
        assert_eq!(p4_line(&[]), None);
    }

    // A7.7
    #[test]
    fn tells_where_each_action_left_its_output() {
        let build = output_lines(Action::Build, "/s/dd/Build/Products", "/s/archives/App.xcarchive", None);
        assert_eq!(build.len(), 1);
        assert!(build[0].contains("products=/s/dd/Build/Products") && build[0].contains("valid until"));
        let test = output_lines(Action::Test, "/p", "/a", Some("/s/dd/Logs/Test/T.xcresult"));
        assert_eq!(test[0], "agb: test results=/s/dd/Logs/Test/T.xcresult");
        assert_eq!(output_lines(Action::Archive, "/p", "/s/archives/App.xcarchive", None)[0], "agb: archive=/s/archives/App.xcarchive");
    }

    // A6.1
    #[test]
    fn exit_codes_say_what_went_wrong() {
        assert_eq!(exit_code(Outcome::Usage), 2);
        assert_eq!(exit_code(Outcome::SyncFailed), 3);
        assert_eq!(exit_code(Outcome::Built(0)), 0);
        assert_eq!(exit_code(Outcome::Built(65)), 65);
        assert_eq!(exit_code(Outcome::Signaled(15)), 143);
        assert_eq!(exit_code(Outcome::Built(300)), 255);
    }

    // A7.8
    #[test]
    fn keeps_only_the_newest_logs() {
        let names = ["20261119-120000-a.log", "20261118-090000-a.log", "20261120-080000-b.log"].map(String::from).to_vec();
        assert_eq!(logs_to_delete(names.clone(), 2), ["20261118-090000-a.log"]);
        assert!(logs_to_delete(names, 5).is_empty());
        assert_eq!(log_file_name(0, 7, "s"), "19700101-000000-7-s.log");
        assert_eq!(log_file_name(1_795_115_660, 42, "app-1a2b3c4d"), "20261119-191420-42-app-1a2b3c4d.log");
        assert_eq!(log_file_name(951_782_400, 1, "s"), "20000229-000000-1-s.log");
    }
}
