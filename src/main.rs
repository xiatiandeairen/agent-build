//! `agb xcodebuild <args...>` and `agb cargo <args...>`: run the build tool for any worktree in its
//! project's fixed build slot, so incremental builds carry over between worktrees.
mod app;
mod build;
mod cargo;

use agent_build_core::report;
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(report::exit_code(app::run(&argv)))
}
