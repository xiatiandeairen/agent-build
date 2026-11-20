//! Ctrl-C and termination: record the signal so the run can stop xcodebuild and exit cleanly.
use crate::ffi;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicI32, Ordering};

static PENDING: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: c_int) {
    PENDING.store(sig, Ordering::SeqCst);
}

pub fn install() {
    for sig in [ffi::SIGINT, ffi::SIGTERM, ffi::SIGHUP] {
        ffi::set_handler(sig, on_signal);
    }
}

/// The signal received so far, if any.
pub fn pending() -> Option<i32> {
    match PENDING.load(Ordering::SeqCst) {
        0 => None,
        s => Some(s),
    }
}
