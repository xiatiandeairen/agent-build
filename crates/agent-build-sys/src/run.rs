//! Run the build tool in its own process group, stream its output line by line, and on Ctrl-C or
//! termination stop the whole group before returning.
use crate::lock::SlotLock;
use crate::{ffi, signals};
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Exited(i32),
    /// We stopped it because agent-build received this signal.
    Stopped(i32),
}

fn pump(reader: impl Read + Send + 'static, stream: Stream, tx: mpsc::Sender<(Stream, String)>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut r = BufReader::new(reader);
        let mut buf = Vec::new();
        while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
            if tx.send((stream, line)).is_err() {
                return;
            }
            buf.clear();
        }
    })
}

/// The slot lock is handed down so it stays held while xcodebuild or anything it started runs.
/// `env` is added to (and overrides) agb's own environment.
pub fn run(
    program: &str,
    args: &[String],
    env: &[(String, String)],
    cwd: &Path,
    lock: &SlotLock,
    mut on_line: impl FnMut(Stream, &str),
) -> io::Result<Ended> {
    lock.hand_down()?;
    let mut child = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let pgid = child.id();
    let (tx, rx) = mpsc::channel();
    let readers =
        [child.stdout.take().map(|o| pump(o, Stream::Out, tx.clone())), child.stderr.take().map(|e| pump(e, Stream::Err, tx.clone()))];
    drop(tx);
    let mut stopped = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((stream, line)) => on_line(stream, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if stopped.is_none() {
            if let Some(sig) = signals::pending() {
                ffi::kill_group(pgid, ffi::SIGTERM);
                stopped = Some(sig);
            }
        }
    }
    for r in readers.into_iter().flatten() {
        let _ = r.join(); // a reader only ends when its pipe closes
    }
    let status = child.wait()?;
    if stopped.is_none() {
        stopped = signals::pending();
    }
    Ok(match stopped {
        Some(sig) => {
            ffi::kill_group(pgid, ffi::SIGTERM); // anything it left behind in its group
            Ended::Stopped(sig)
        }
        None => Ended::Exited(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))),
    })
}
