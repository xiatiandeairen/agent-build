//! Asking `xcrun xcresulttool` for the result bundle's JSON. `AGENT_BUILD_XCRESULTTOOL` replaces
//! `xcrun xcresulttool` in tests.
use std::path::Path;
use std::process::{Command, Stdio};

/// `get build-results`, `get log --type build` or `get test-results summary`; `None` when the
/// bundle is missing or unreadable.
pub fn get(bundle: &Path, what: &[&str]) -> Option<String> {
    let mut cmd = match std::env::var("AGENT_BUILD_XCRESULTTOOL") {
        Ok(tool) => Command::new(tool),
        Err(_) => {
            let mut c = Command::new("xcrun");
            c.arg("xcresulttool");
            c
        }
    };
    let out = cmd.arg("get").args(what).arg("--path").arg(bundle).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
