//! What the caller asked for: `agb xcodebuild <args...>`.
//! Output locations belong to agb, so the flags that set them are dropped here,
//! and the settings agb adds are listed in one place (`injected`).

/// The xcodebuild action that decides which output path the caller is told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Build,
    Test,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Arguments for xcodebuild, in the caller's order, minus the dropped flags.
    pub args: Vec<String>,
    /// Each dropped flag with its value, e.g. `-derivedDataPath /tmp/dd`.
    pub dropped: Vec<String>,
    pub action: Action,
    pub scheme: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Usage,
    NotXcodebuild(String),
    Unsupported(String),
    MissingValue(String),
}

pub const USAGE: &str = "usage: agb xcodebuild <xcodebuild arguments...>";

impl ArgError {
    pub fn message(&self) -> String {
        match self {
            ArgError::Usage => USAGE.to_string(),
            ArgError::NotXcodebuild(w) if w == "xcrun" || w.ends_with("/xcodebuild") => {
                format!("write the command as `agb xcodebuild ...` (got `{w}`)")
            }
            ArgError::NotXcodebuild(w) => format!("only xcodebuild is supported in this version (got `{w}`)\n{USAGE}"),
            ArgError::Unsupported(f) => format!("{f} is not supported in this version: it does not build"),
            ArgError::MissingValue(f) => format!("{f} needs a value"),
        }
    }
}

const DROPPED: [&str; 3] = ["-derivedDataPath", "-archivePath", "-resultBundlePath"];
const UNSUPPORTED: [&str; 1] = ["-exportArchive"];
const ACTIONS: [&str; 10] =
    ["build", "test", "archive", "clean", "analyze", "build-for-testing", "test-without-building", "install", "installsrc", "docbuild"];
/// Flags that take no value: a bare word after them can still be an action.
const BOOL_FLAGS: [&str; 16] = [
    "-quiet",
    "-verbose",
    "-json",
    "-list",
    "-showBuildSettings",
    "-showdestinations",
    "-showsdks",
    "-allowProvisioningUpdates",
    "-allowProvisioningDeviceRegistration",
    "-skipUnavailableActions",
    "-hideShellScriptEnvironment",
    "-skipPackagePluginValidation",
    "-skipMacroValidation",
    "-disableAutomaticPackageResolution",
    "-onlyUsePackageVersionsFromResolvedFile",
    "-parallelizeTargets",
];

/// `argv` is everything after the program name.
pub fn parse(argv: &[String]) -> Result<Invocation, ArgError> {
    let (first, rest) = argv.split_first().ok_or(ArgError::Usage)?;
    if first != "xcodebuild" {
        return Err(ArgError::NotXcodebuild(first.clone()));
    }
    let mut args = Vec::new();
    let mut dropped = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if UNSUPPORTED.contains(&a.as_str()) {
            return Err(ArgError::Unsupported(a.clone()));
        }
        if DROPPED.contains(&a.as_str()) {
            let v = rest.get(i + 1).ok_or_else(|| ArgError::MissingValue(a.clone()))?;
            dropped.push(format!("{a} {v}"));
            i += 2;
            continue;
        }
        args.push(a.clone());
        i += 1;
    }
    let action = action_of(&args);
    let scheme = args.iter().position(|a| a == "-scheme").and_then(|p| args.get(p + 1)).cloned();
    Ok(Invocation { args, dropped, action, scheme })
}

fn action_of(args: &[String]) -> Action {
    let mut found = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let after_value_flag = i > 0 && args[i - 1].starts_with('-') && !BOOL_FLAGS.contains(&args[i - 1].as_str());
        if ACTIONS.contains(&a.as_str()) && !after_value_flag {
            found.push(a.as_str());
        }
    }
    if found.contains(&"archive") {
        Action::Archive
    } else if found.iter().any(|a| a.starts_with("test")) {
        Action::Test
    } else {
        Action::Build
    }
}

/// The only arguments agb adds: the slot's output locations and the compilation cache.
/// They go before the caller's arguments, so a setting the caller passes explicitly wins.
pub fn injected(action: Action, derived_data: &str, archive_path: &str, cas_path: &str, cache_limit: &str) -> Vec<String> {
    let mut v = vec!["-derivedDataPath".to_string(), derived_data.to_string()];
    if action == Action::Archive {
        v.extend(["-archivePath".to_string(), archive_path.to_string()]);
    }
    v.extend([
        "COMPILATION_CACHE_ENABLE_CACHING=YES".to_string(),
        format!("COMPILATION_CACHE_CAS_PATH={cas_path}"),
        "COMPILATION_CACHE_KEEP_CAS_DIRECTORY=YES".to_string(),
        format!("COMPILATION_CACHE_LIMIT_SIZE={cache_limit}"),
        "COMPILATION_CACHE_ENABLE_DIAGNOSTIC_REMARKS=YES".to_string(),
    ]);
    v
}

/// File name for an archive: the scheme, made safe for a path.
pub fn archive_name(scheme: Option<&str>) -> String {
    let base: String =
        scheme.unwrap_or("archive").chars().map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
    format!("{base}.xcarchive")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    // A1.1
    #[test]
    fn rejects_anything_but_xcodebuild_and_commands_that_do_not_build() {
        assert_eq!(parse(&[]), Err(ArgError::Usage));
        for first in ["xcrun", "/usr/bin/xcodebuild", "cargo", "swift"] {
            assert_eq!(parse(&v(&[first, "build"])), Err(ArgError::NotXcodebuild(first.into())));
        }
        assert!(ArgError::NotXcodebuild("xcrun".into()).message().contains("agb xcodebuild"));
        assert!(ArgError::NotXcodebuild("cargo".into()).message().contains("only xcodebuild"));
        assert_eq!(parse(&v(&["xcodebuild", "-exportArchive", "-archivePath", "a"])), Err(ArgError::Unsupported("-exportArchive".into())));
        assert!(parse(&v(&["xcodebuild"])).is_ok(), "bare xcodebuild is passed through");
    }

    // A1.2
    #[test]
    fn drops_output_locations_with_their_values_and_keeps_the_rest_in_order() {
        let inv = parse(&v(&[
            "xcodebuild",
            "-derivedDataPath",
            "/x",
            "build",
            "-scheme",
            "App",
            "-resultBundlePath",
            "r.xcresult",
            "-archivePath",
            "/a",
            "ARCHS=arm64",
        ]))
        .unwrap();
        assert_eq!(inv.args, v(&["build", "-scheme", "App", "ARCHS=arm64"]));
        assert_eq!(inv.dropped, v(&["-derivedDataPath /x", "-resultBundlePath r.xcresult", "-archivePath /a"]));
        assert_eq!(parse(&v(&["xcodebuild", "build", "-derivedDataPath"])), Err(ArgError::MissingValue("-derivedDataPath".into())));
    }

    // A1.3
    #[test]
    fn injects_only_the_slot_locations_and_cache_settings() {
        let build = injected(Action::Build, "/s/dd", "/s/archives/App.xcarchive", "/c/cas", "40G");
        assert_eq!(
            build,
            v(&[
                "-derivedDataPath",
                "/s/dd",
                "COMPILATION_CACHE_ENABLE_CACHING=YES",
                "COMPILATION_CACHE_CAS_PATH=/c/cas",
                "COMPILATION_CACHE_KEEP_CAS_DIRECTORY=YES",
                "COMPILATION_CACHE_LIMIT_SIZE=40G",
                "COMPILATION_CACHE_ENABLE_DIAGNOSTIC_REMARKS=YES",
            ])
        );
        let archive = injected(Action::Archive, "/s/dd", "/s/archives/App.xcarchive", "/c/cas", "40G");
        assert_eq!(&archive[2..4], &v(&["-archivePath", "/s/archives/App.xcarchive"])[..]);
        assert_eq!(archive.len(), build.len() + 2);
    }

    // A7.7
    #[test]
    fn finds_the_action_without_mistaking_option_values_for_it() {
        let action = |s: &[&str]| parse(&v(s)).unwrap().action;
        assert_eq!(action(&["xcodebuild", "-scheme", "App"]), Action::Build);
        assert_eq!(action(&["xcodebuild", "test", "-scheme", "App"]), Action::Test);
        assert_eq!(action(&["xcodebuild", "-scheme", "test", "build"]), Action::Build);
        assert_eq!(action(&["xcodebuild", "-quiet", "archive", "-scheme", "App"]), Action::Archive);
        assert_eq!(action(&["xcodebuild", "clean", "test-without-building"]), Action::Test);
        assert_eq!(archive_name(Some("My App")), "My-App.xcarchive");
        assert_eq!(archive_name(None), "archive.xcarchive");
    }
}
