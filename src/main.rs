//! `agent-build xcodebuild <args...>`: run xcodebuild for any worktree in its project's fixed build
//! slot, so Xcode's incremental build and compilation cache carry over between worktrees.
mod app;
mod build;

use agent_build_core::report;
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(report::exit_code(app::run(&argv)))
}
