//! Decisions only: argument parsing, config parsing, slot naming, git status parsing,
//! result bundle reading (JSON), path rewriting and output formatting. No IO, no clock, no processes,
//! so every rule here is tested without git or Xcode.
pub mod args;
pub mod cargo;
pub mod cargo_out;
pub mod config;
pub mod json;
pub mod output;
pub mod project;
pub mod report;
pub mod slot;
pub mod status;
pub mod xcresult;
