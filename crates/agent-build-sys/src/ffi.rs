//! The four libc calls the standard library does not expose: flock, fcntl, kill, signal.
//! Declared here instead of pulling in a crate, to keep zero third-party dependencies.
use std::io;
use std::os::raw::c_int;

unsafe extern "C" {
    fn flock(fd: c_int, operation: c_int) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn signal(signum: c_int, handler: usize) -> usize;
}

const LOCK_EX: c_int = 2;
const LOCK_NB: c_int = 4;
const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const FD_CLOEXEC: c_int = 1;

pub const SIGHUP: i32 = 1;
pub const SIGINT: i32 = 2;
pub const SIGTERM: i32 = 15;

/// Exclusive, non-blocking flock. Ok(false) when another open file description holds it.
pub fn try_lock_exclusive(fd: c_int) -> io::Result<bool> {
    // SAFETY: flock only reads its integer arguments; an invalid fd is reported through errno.
    if unsafe { flock(fd, LOCK_EX | LOCK_NB) } == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.kind() == io::ErrorKind::WouldBlock { Ok(false) } else { Err(err) }
}

/// Let child processes inherit `fd` (clear FD_CLOEXEC), so they keep a flock alive after we exit.
pub fn set_inheritable(fd: c_int) -> io::Result<()> {
    // SAFETY: F_GETFD/F_SETFD take and return plain integers.
    let flags = unsafe { fcntl(fd, F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above.
    if unsafe { fcntl(fd, F_SETFD, flags & !FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Send `sig` to every process in process group `pgid`. A group that is already gone is not an error.
pub fn kill_group(pgid: u32, sig: i32) {
    if let Ok(pid) = c_int::try_from(pgid) {
        // SAFETY: kill only reads its integer arguments.
        let _ = unsafe { kill(-pid, sig) }; // ESRCH (already gone) is the expected outcome after it exits
    }
}

/// Install `handler` for `sig`.
pub fn set_handler(sig: i32, handler: extern "C" fn(c_int)) {
    // SAFETY: the handler is a plain function that only stores into an atomic, which is async-signal-safe.
    let _ = unsafe { signal(sig, handler as usize) }; // the previous handler is not needed
}
