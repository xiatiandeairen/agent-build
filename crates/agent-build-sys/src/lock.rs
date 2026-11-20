//! One build at a time per slot. The lock is a flock on a file: the kernel releases it when the
//! last process holding the file open exits. xcodebuild inherits the file, so a killed agent-build
//! (even kill -9) keeps the slot locked until its xcodebuild and that build's tools have exited too.
use crate::{ffi, signals};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::Duration;

pub struct SlotLock {
    file: File,
}

#[derive(Debug)]
pub enum LockError {
    Io(io::Error),
    Interrupted(i32),
}

impl From<io::Error> for LockError {
    fn from(e: io::Error) -> Self {
        LockError::Io(e)
    }
}

impl SlotLock {
    /// Blocks until the slot is free. `on_wait` is called once, with who holds it, if we have to wait.
    pub fn acquire(lock: &Path, owner: &Path, me: &str, mut on_wait: impl FnMut(&str)) -> Result<SlotLock, LockError> {
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock)?;
        let mut told = false;
        loop {
            if ffi::try_lock_exclusive(file.as_raw_fd())? {
                fs::write(owner, me)?;
                return Ok(SlotLock { file });
            }
            if !told {
                let holder = fs::read_to_string(owner).unwrap_or_default(); // empty if it has not written yet
                on_wait(holder.trim());
                told = true;
            }
            if let Some(sig) = signals::pending() {
                return Err(LockError::Interrupted(sig));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Make the next spawned process (xcodebuild) inherit the lock.
    pub fn hand_down(&self) -> io::Result<()> {
        ffi::set_inheritable(self.file.as_raw_fd())
    }
}
