//! What the caller asked for: `agb xcodebuild <args...>` (here) or `agb cargo <args...>` (`cargo`).
//! Output locations belong to agb, so the flags that set them are dropped here,
//! and the settings agb adds are listed in one place (`injected`).

/// What the invocation does: builds (with a result bundle the report is read from), or only
/// prints information such as `-showBuildSettings`, whose output the caller needs as is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Build,
    Test,
    Archive,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Arguments for xcodebuild, in the caller's order, minus the dropped flags.
    pub args: Vec<String>,
    /// Each dropped flag with its value, e.g. `-derivedDataPath /tmp/dd`.
    pub dropped: Vec<String>,
    pub action: Action,
    /// Whether xcodebuild builds new products: every action but `test-without-building` and
    /// `clean`, and no action at all (xcodebuild then builds). Archives keep theirs in the archive.
    pub builds: bool,
    pub scheme: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    Usage,
    NotXcodebuild(String),
    Unsupported(String),
    MissingValue(String),
}

pub const USAGE: &str =
    "usage: agb xcodebuild <xcodebuild arguments...>\n       agb cargo <build|check|test|bench|run|doc|clippy|rustc|clean> [arguments...]";

/// Which build tool the caller named, with its parsed arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Xcodebuild(Invocation),
    Cargo(crate::cargo::Invocation),
}

/// `argv` is everything after the program name; the error is the message to print.
pub fn request(argv: &[String]) -> Result<Request, String> {
    match argv.split_first() {
        Some((first, rest)) if first == "cargo" => crate::cargo::parse(rest).map(Request::Cargo).map_err(|e| e.message()),
        _ => parse(argv).map(Request::Xcodebuild).map_err(|e| e.message()),
    }
}

impl ArgError {
    pub fn message(&self) -> String {
        match self {
            ArgError::Usage => USAGE.to_string(),
            ArgError::NotXcodebuild(w) if w == "xcrun" || w.ends_with("/xcodebuild") => {
                format!("write the command as `agb xcodebuild ...` (got `{w}`)")
            }
            ArgError::NotXcodebuild(w) => format!("only xcodebuild and cargo are supported (got `{w}`)\n{USAGE}"),
            ArgError::Unsupported(f) => format!("{f} is not supported in this version: it does not build"),
            ArgError::MissingValue(f) => format!("{f} needs a value"),
        }
    }
}

const DROPPED: [&str; 4] = ["-derivedDataPath", "-archivePath", "-resultBundlePath", "-clonedSourcePackagesDirPath"];
const UNSUPPORTED: [&str; 1] = ["-exportArchive"];
const ACTIONS: [&str; 10] =
    ["build", "test", "archive", "clean", "analyze", "build-for-testing", "test-without-building", "install", "installsrc", "docbuild"];
/// Flags that make xcodebuild print information instead of building.
/// `-resolvePackageDependencies` builds nothing either; its package checkouts land in the slot's DerivedData.
const INFO_FLAGS: [&str; 10] = [
    "-list",
    "-showBuildSettings",
    "-showdestinations",
    "-showsdks",
    "-showTestPlans",
    "-version",
    "-help",
    "-usage",
    "-showComponent",
    "-resolvePackageDependencies",
];
/// Flags that take no value: a bare word after them can still be an action.
const BOOL_FLAGS: [&str; 17] = [
    "-resolvePackageDependencies",
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
    let builds = action != Action::Info && {
        let named = actions(&args);
        named.is_empty() || named.iter().any(|a| !["test-without-building", "clean"].contains(a))
    };
    let scheme = args.iter().position(|a| a == "-scheme").and_then(|p| args.get(p + 1)).cloned();
    Ok(Invocation { args, dropped, action, builds, scheme })
}

fn action_of(args: &[String]) -> Action {
    if args.iter().any(|a| INFO_FLAGS.contains(&a.as_str())) {
        return Action::Info;
    }
    let found = actions(args);
    if found.contains(&"archive") {
        Action::Archive
    } else if found.iter().any(|a| a.starts_with("test")) {
        Action::Test
    } else {
        Action::Build
    }
}

/// The actions named on the command line, skipping words that are an option's value.
fn actions(args: &[String]) -> Vec<&str> {
    let mut found = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let after_value_flag = i > 0 && args[i - 1].starts_with('-') && !BOOL_FLAGS.contains(&args[i - 1].as_str());
        if ACTIONS.contains(&a.as_str()) && !after_value_flag {
            found.push(a.as_str());
        }
    }
    found
}

/// Where an invocation's outputs go: DerivedData in the slot; the archive and the result bundle in
/// the worktree's output, which only this worktree's runs replace.
pub struct Outputs<'a> {
    pub derived_data: &'a str,
    pub archive: &'a str,
    pub result_bundle: &'a str,
}

/// The only arguments agb adds: the output locations and the compilation cache.
/// They go before the caller's arguments, so a setting the caller passes explicitly wins.
pub fn injected(action: Action, out: &Outputs, cas_path: &str, cache_limit: &str) -> Vec<String> {
    let mut v = vec!["-derivedDataPath".to_string(), out.derived_data.to_string()];
    if action == Action::Archive {
        v.extend(["-archivePath".to_string(), out.archive.to_string()]);
    }
    if action != Action::Info {
        v.extend(["-resultBundlePath".to_string(), out.result_bundle.to_string()]);
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

/// Flags whose value is a path xcodebuild builds from.
const PATH_FLAGS: [&str; 3] = ["-workspace", "-project", "-xctestrun"];

/// Points the values of `-workspace`, `-project` and `-xctestrun` that `to_slot` maps (paths inside
/// the worktree) at the slot, so the slot's copy is built. Returns each change as (flag, from, to).
pub fn relocate(args: &mut [String], to_slot: impl Fn(&str) -> Option<String>) -> Vec<(String, String, String)> {
    let mut moved = Vec::new();
    for i in 1..args.len() {
        if !PATH_FLAGS.contains(&args[i - 1].as_str()) {
            continue;
        }
        if let Some(to) = to_slot(&args[i]) {
            moved.push((args[i - 1].clone(), std::mem::replace(&mut args[i], to.clone()), to));
        }
    }
    moved
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
        assert!(ArgError::NotXcodebuild("swift".into()).message().contains("only xcodebuild and cargo"));
        assert!(matches!(request(&v(&["cargo", "build"])), Ok(Request::Cargo(_))));
        assert!(matches!(request(&v(&["xcodebuild", "build"])), Ok(Request::Xcodebuild(_))));
        assert!(request(&v(&["cargo", "fmt"])).is_err_and(|m| m.contains("agb only builds")));
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
        let out = Outputs {
            derived_data: "/s/DerivedData",
            archive: "/w/.agent-build/output/App.xcarchive",
            result_bundle: "/w/.agent-build/output/result.xcresult",
        };
        let build = injected(Action::Build, &out, "/c/cas", "40G");
        assert_eq!(
            build,
            v(&[
                "-derivedDataPath",
                "/s/DerivedData",
                "-resultBundlePath",
                "/w/.agent-build/output/result.xcresult",
                "COMPILATION_CACHE_ENABLE_CACHING=YES",
                "COMPILATION_CACHE_CAS_PATH=/c/cas",
                "COMPILATION_CACHE_KEEP_CAS_DIRECTORY=YES",
                "COMPILATION_CACHE_LIMIT_SIZE=40G",
                "COMPILATION_CACHE_ENABLE_DIAGNOSTIC_REMARKS=YES",
            ])
        );
        let archive = injected(Action::Archive, &out, "/c/cas", "40G");
        assert_eq!(&archive[2..4], &v(&["-archivePath", "/w/.agent-build/output/App.xcarchive"])[..]);
        assert_eq!(archive.len(), build.len() + 2);
        let info = injected(Action::Info, &out, "/c/cas", "40G");
        assert!(!info.contains(&"-resultBundlePath".to_string()) && info.len() == build.len() - 2, "{info:?}");
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
        assert_eq!(action(&["xcodebuild", "-showBuildSettings", "-scheme", "App"]), Action::Info);
        assert_eq!(action(&["xcodebuild", "-list"]), Action::Info);
        let builds = |s: &[&str]| parse(&v(s)).unwrap().builds;
        assert!(builds(&["xcodebuild", "-scheme", "App"]) && builds(&["xcodebuild", "build-for-testing"]));
        assert!(builds(&["xcodebuild", "clean", "build"]) && builds(&["xcodebuild", "archive"]));
        assert!(!builds(&["xcodebuild", "test-without-building", "-xctestrun", "x"]) && !builds(&["xcodebuild", "clean"]));
        assert!(!builds(&["xcodebuild", "-list"]));
        assert_eq!(archive_name(Some("My App")), "My-App.xcarchive");
        assert_eq!(archive_name(None), "archive.xcarchive");
    }

    // A1.6
    #[test]
    fn resolving_packages_is_an_information_command() {
        let action = |s: &[&str]| parse(&v(s)).unwrap().action;
        assert_eq!(action(&["xcodebuild", "-resolvePackageDependencies"]), Action::Info);
        assert_eq!(action(&["xcodebuild", "-resolvePackageDependencies", "-workspace", "W.xcworkspace", "-scheme", "App"]), Action::Info);
    }

    // A1.8
    #[test]
    fn package_checkouts_stay_in_the_slot() {
        let inv = parse(&v(&["xcodebuild", "build", "-clonedSourcePackagesDirPath", "/tmp/spm", "-scheme", "App"])).unwrap();
        assert_eq!(inv.args, v(&["build", "-scheme", "App"]));
        assert_eq!(inv.dropped, v(&["-clonedSourcePackagesDirPath /tmp/spm"]));
    }

    // A1.7
    #[test]
    fn worktree_paths_given_to_xcodebuild_point_into_the_slot() {
        let mut args = v(&["build", "-workspace", "/w/ios/App.xcworkspace", "-project", "/elsewhere/P.xcodeproj", "-scheme", "/w/x"]);
        args.extend(v(&["-xctestrun", "/w/dd/T.xctestrun", "-project", "Rel.xcodeproj"]));
        let to_slot = |p: &str| p.strip_prefix("/w/").map(|rest| format!("/s/src/{rest}"));
        let moved = relocate(&mut args, to_slot);
        assert_eq!(
            args,
            v(&[
                "build",
                "-workspace",
                "/s/src/ios/App.xcworkspace",
                "-project",
                "/elsewhere/P.xcodeproj",
                "-scheme",
                "/w/x",
                "-xctestrun",
                "/s/src/dd/T.xctestrun",
                "-project",
                "Rel.xcodeproj"
            ])
        );
        assert_eq!(
            moved,
            [
                ("-workspace".to_string(), "/w/ios/App.xcworkspace".to_string(), "/s/src/ios/App.xcworkspace".to_string()),
                ("-xctestrun".to_string(), "/w/dd/T.xctestrun".to_string(), "/s/src/dd/T.xctestrun".to_string()),
            ]
        );
    }
}
