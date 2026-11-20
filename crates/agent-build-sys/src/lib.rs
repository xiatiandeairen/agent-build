//! The outside world: git, file copying, the slot lock, running xcodebuild and signals.
//! No decisions here; the rules live in `agent-build-core`.
mod ffi;
pub mod git;
pub mod lock;
pub mod run;
pub mod signals;
pub mod sync;
