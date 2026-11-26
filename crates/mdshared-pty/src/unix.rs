//! `openpty`: a master and a slave end, and a child process whose terminal is the slave.
//!
//! The child is started by the standard library, which forks and executes safely from a
//! program with threads; the one step done between the two is making the slave the child's
//! controlling terminal. Three threads per terminal, as on Windows: read, write, wait.
//!
//! Not run by any test: this side is only type-checked (`docs/FEATURES.md`).

use crate::{Command, Exit, Shared};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Stdio;
use std::ptr::null_mut;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

/// The system side of a [`crate::Pty`].
pub(crate) struct Inner {
    master: OwnedFd,
    pid: libc::pid_t,
    shared: Arc<Shared>,
}

impl Inner {
    pub(crate) fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        set_size(&self.master, cols, rows)
    }

    pub(crate) fn kill(&self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    /// The terminal is going away: what a closed terminal window sends.
    pub(crate) fn close(&self) {
        let _ = self.signal(libc::SIGHUP);
    }

    fn signal(&self, signal: libc::c_int) -> io::Result<()> {
        // Once the child was waited for, its id may belong to another process.
        if self.shared.has_exited() {
            return Ok(());
        }
        // SAFETY: a plain system call on a process id; the worst a stale id can do is covered
        // by the check above.
        if unsafe { libc::kill(self.pid, signal) } == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

fn set_size(master: &OwnedFd, cols: u16, rows: u16) -> io::Result<()> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the descriptor is open and `size` is a valid winsize for the call to read.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn spawn(
    command: &Command,
    cols: u16,
    rows: u16,
    shared: Arc<Shared>,
    input: mpsc::Receiver<Vec<u8>>,
) -> io::Result<(Inner, u32)> {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both out-pointers are valid; the name, the terminal settings and the size are
    // left out (null), the size being set just below.
    if unsafe { libc::openpty(&mut master, &mut slave, null_mut(), null_mut(), null_mut()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty succeeded, so both are open descriptors that nothing else owns.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // The child must not keep the master open, or the terminal would never close.
    // SAFETY: plain descriptor-flag calls on an open descriptor.
    unsafe {
        let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFD);
        libc::fcntl(master.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC);
    }
    set_size(&master, cols, rows)?;

    let mut child = std::process::Command::new(&command.program);
    child.args(&command.args);
    if let Some(dir) = &command.cwd {
        child.current_dir(dir);
    }
    // What this crate's companion understands. A caller's own TERM, set below, wins.
    child.env("TERM", "xterm-256color");
    for (name, value) in &command.env {
        match value {
            Some(value) => child.env(name, value),
            None => child.env_remove(name),
        };
    }
    child.stdin(Stdio::from(slave.try_clone()?));
    child.stdout(Stdio::from(slave.try_clone()?));
    child.stderr(Stdio::from(slave));
    // SAFETY: the closure runs in the child between fork and exec and makes only two
    // async-signal-safe system calls: a new session, and the terminal on descriptor 0 (the
    // slave by then) as its controlling terminal. It allocates nothing and takes no lock.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut running = child.spawn()?;
    // The builder holds the parent's copies of the slave; they must go, or reading the master
    // would never end.
    drop(child);
    let pid = running.id();

    let mut from = File::from(master.try_clone()?);
    let reader = shared.clone();
    thread::Builder::new()
        .name("pty-read".into())
        .spawn(move || {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match from.read(&mut buffer) {
                    // Linux answers EIO, others 0, when the last slave descriptor is closed.
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        reader.push(&buffer[..n]);
                    }
                }
            }
            reader.end_of_output();
        })?;

    let mut to = File::from(master.try_clone()?);
    thread::Builder::new()
        .name("pty-write".into())
        .spawn(move || {
            for chunk in input {
                if to.write_all(&chunk).is_err() {
                    return;
                }
            }
        })?;

    let waiter = shared.clone();
    thread::Builder::new()
        .name("pty-wait".into())
        .spawn(move || {
            let exit = match running.wait() {
                Ok(status) => Exit {
                    code: status.code().map(|c| c as u32),
                    signal: status.signal(),
                },
                Err(_) => Exit {
                    code: None,
                    signal: None,
                },
            };
            waiter.exited(exit);
            // A background child of the program may hold the slave open for ever. The program's
            // own last output is in the system's buffer by now; give the reader a moment for it,
            // then call the output ended whether or not the slave is closed.
            thread::sleep(Duration::from_millis(50));
            waiter.end_of_output();
        })?;

    Ok((
        Inner {
            master,
            pid: pid as libc::pid_t,
            shared,
        },
        pid,
    ))
}
