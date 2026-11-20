//! Decisions only: argument parsing, config parsing, slot naming, git status parsing,
//! log parsing, path rewriting and output formatting. No IO, no clock, no processes,
//! so every rule here is tested without git or Xcode.
pub mod args;
pub mod config;
pub mod log;
pub mod output;
pub mod project;
pub mod report;
pub mod slot;
pub mod status;
