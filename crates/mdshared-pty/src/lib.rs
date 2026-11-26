//! A pseudo-terminal: a program started with a terminal of its own, whose output comes back as
//! bytes and whose keyboard is the bytes written to it. ConPTY on Windows, `openpty` on Unix.
//! It is the system side of a terminal emulator and nothing more: `mdshared-term` turns the
//! bytes into a grid, and an application draws that grid.
//!
//! The reading, the writing and the wait for the program's end are threads, so no call here
//! blocks. When there is something new (output, or the end) the wake closure given to
//! [`Pty::spawn`] is called from one of those threads; as with `mdshared_app::job`, it is a
//! plain closure and the caller decides what a wake means (usually: send the window's event loop
//! a message). The caller then takes what there is:
//!
//! ```no_run
//! use mdshared_pty::{Command, Pty};
//! use mdshared_term::Screen;
//!
//! let mut screen = Screen::new(80, 24, 10_000);
//! let mut pty = Pty::spawn(&Command::new("cmd.exe"), 80, 24, || { /* wake the event loop */ })?;
//! // On every wake:
//! screen.feed(&pty.take_output());
//! pty.write(&screen.take_reply())?; // what the program asked the terminal
//! if let Some(exit) = pty.exit() {
//!     println!("ended with {:?}", exit.code);
//! }
//! // A key, a paste, a mouse event (`mdshared_term::input`) are bytes for `pty.write`;
//! // a new size of the view is `screen.resize` and `pty.resize`.
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Deliberately not here:
//!
//! - no screen, no parser, no drawing: bytes in and bytes out;
//! - no event loop and no channel type in the interface: one wake closure, and the caller polls;
//! - no shell detection: the caller names the program ([`Command::new`]);
//! - no job control, no process groups beyond what the system's terminal gives, no way to reach
//!   the program's grandchildren other than closing the terminal on them;
//! - no flow control on writing: what is written is queued in memory until the program reads it.
//!   Output the caller does not take is held back at the program (see [`Pty::take_output`]).
//!
//! What was checked and what was not is in the crate's section of `docs/FEATURES.md`. In short:
//! the Windows side runs in the tests; the Unix side is only type-checked.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;

#[cfg(not(any(windows, unix)))]
mod sys {
    use super::{Command, Shared};
    use std::io;
    use std::sync::{Arc, mpsc};

    pub(crate) struct Inner;

    impl Inner {
        pub(crate) fn resize(&self, _cols: u16, _rows: u16) -> io::Result<()> {
            Err(io::ErrorKind::Unsupported.into())
        }
        pub(crate) fn kill(&self) -> io::Result<()> {
            Err(io::ErrorKind::Unsupported.into())
        }
        pub(crate) fn close(&self) {}
    }

    pub(crate) fn spawn(
        _command: &Command,
        _cols: u16,
        _rows: u16,
        _shared: Arc<Shared>,
        _input: mpsc::Receiver<Vec<u8>>,
    ) -> io::Result<(Inner, u32)> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// How much output may wait to be taken before the reading thread stops reading, which in turn
/// stops the program when the system's own buffer is full. A program that prints without end
/// therefore costs this much memory and no more.
pub const HIGH_WATER: usize = 4 * 1024 * 1024;

/// What to start: a program, its arguments, the folder it starts in and changes to the
/// environment it inherits. A plain value; nothing happens until [`Pty::spawn`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) cwd: Option<PathBuf>,
    /// In the order given; `None` removes the variable. A later entry for a name wins.
    pub(crate) env: Vec<(OsString, Option<OsString>)>,
}

impl Command {
    /// A program by path, or by name to be looked for on the `PATH` (the one the program will
    /// get, so an [`env`](Self::env) for `PATH` counts). On Windows a name without an extension
    /// is tried as `.exe`, `.com`, `.cmd` and `.bat`, and a `.cmd` or `.bat` is run through
    /// `cmd.exe`.
    pub fn new(program: impl AsRef<OsStr>) -> Command {
        Command {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    /// One more argument, passed as it is (the crate does the quoting the system needs).
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Command {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    /// Several more arguments.
    pub fn args<I, S>(mut self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// The folder the program starts in. Without it: the caller's own.
    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Command {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets a variable on top of the caller's environment.
    pub fn env(mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Command {
        self.env.push((
            name.as_ref().to_os_string(),
            Some(value.as_ref().to_os_string()),
        ));
        self
    }

    /// Takes a variable of the caller's environment away from the program.
    pub fn env_remove(mut self, name: impl AsRef<OsStr>) -> Command {
        self.env.push((name.as_ref().to_os_string(), None));
        self
    }

    /// The program as it was named.
    pub fn program(&self) -> &OsStr {
        &self.program
    }
}

/// How the program ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    /// Its exit code. `None` on Unix when a signal ended it. On Windows always there, and a
    /// program that was killed or crashed has a large code such as `0xC000013A`.
    pub code: Option<u32>,
    /// The signal that ended it. Unix only; always `None` on Windows.
    pub signal: Option<i32>,
}

impl Exit {
    /// True for exit code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// What the threads and the handle share.
pub(crate) struct Shared {
    state: Mutex<State>,
    /// Signalled when output was taken or the handle was dropped, for a reader held at
    /// [`HIGH_WATER`].
    room: Condvar,
    wake: Box<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct State {
    out: Vec<u8>,
    /// No more output will come.
    eof: bool,
    exit: Option<Exit>,
    /// The handle was dropped: output is thrown away from here on.
    closed: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in the caller's wake closure must not make every later call panic too.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Adds output, waiting while [`HIGH_WATER`] bytes are untaken. False once the handle is
    /// gone; the bytes are then dropped and the reader goes on reading, so that the program and
    /// the system's terminal can still end.
    pub(crate) fn push(&self, bytes: &[u8]) -> bool {
        let mut s = self.lock();
        while s.out.len() >= HIGH_WATER && !s.closed {
            s = self.room.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        if s.closed {
            return false;
        }
        // One wake per batch: while the caller has not taken the last one it knows already.
        let first = s.out.is_empty();
        s.out.extend_from_slice(bytes);
        drop(s);
        if first {
            (self.wake)();
        }
        true
    }

    /// The output has ended. May be called more than once.
    pub(crate) fn end_of_output(&self) {
        let mut s = self.lock();
        let was = s.eof;
        s.eof = true;
        let closed = s.closed;
        drop(s);
        if !was && !closed {
            (self.wake)();
        }
    }

    /// The program has ended.
    pub(crate) fn exited(&self, exit: Exit) {
        let mut s = self.lock();
        s.exit = Some(exit);
        let closed = s.closed;
        drop(s);
        if !closed {
            (self.wake)();
        }
    }

    /// Whether the program's end has been recorded.
    pub(crate) fn has_exited(&self) -> bool {
        self.lock().exit.is_some()
    }
}

/// A running program and its terminal. Dropping it closes the terminal and ends the program:
/// Windows closes the console on everything attached to it and then terminates the program
/// itself; Unix sends it `SIGHUP`, as a closed terminal window does, which a program may ignore.
pub struct Pty {
    shared: Arc<Shared>,
    /// To the writing thread. `None` only during drop.
    input: Option<mpsc::Sender<Vec<u8>>>,
    inner: sys::Inner,
    pid: u32,
    size: (u16, u16),
}

impl Pty {
    /// Starts `command` in a new terminal of `cols` by `rows` cells (each at least 1; a 0 is
    /// taken as 1).
    ///
    /// `wake` is called from another thread when output has arrived that the caller has not been
    /// told of, when the output has ended and when the program has ended. It is called once per
    /// batch, not once per read: after a wake nothing more comes until [`take_output`]
    /// (Self::take_output) was called. It must return quickly and must not call into this `Pty`.
    ///
    /// Errors: `NotFound` when the program is not there, `InvalidInput` for a name or argument
    /// the system cannot take (a NUL inside; on Windows a `"` or `%` in the arguments of a
    /// `.cmd` or `.bat`), otherwise what the system said. `Unsupported` where there is no
    /// pseudo-terminal (Windows before 10 1809).
    pub fn spawn(
        command: &Command,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Pty> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            room: Condvar::new(),
            wake: Box::new(wake),
        });
        let (tx, rx) = mpsc::channel();
        let (inner, pid) = sys::spawn(command, cols, rows, shared.clone(), rx)?;
        Ok(Pty {
            shared,
            input: Some(tx),
            inner,
            pid,
            size: (cols, rows),
        })
    }

    /// Everything the program has written since the last call, in order; empty when there is
    /// nothing. The bytes are the raw stream (text and escape sequences) for
    /// `mdshared_term::Screen::feed`.
    ///
    /// Output that is not taken is kept, up to [`HIGH_WATER`] bytes; beyond that the program is
    /// held back until this is called.
    pub fn take_output(&mut self) -> Vec<u8> {
        let mut s = self.shared.lock();
        let out = std::mem::take(&mut s.out);
        drop(s);
        if !out.is_empty() {
            self.shared.room.notify_all();
        }
        out
    }

    /// Queues bytes for the program's input (keys, a paste, a reply the screen owes it) and
    /// returns at once. Empty input is nothing. `BrokenPipe` once the program's input is closed.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        match &self.input {
            Some(tx) => tx
                .send(bytes.to_vec())
                .map_err(|_| io::ErrorKind::BrokenPipe.into()),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }

    /// Tells the terminal its new size in cells (a 0 is taken as 1). The same size as before is
    /// nothing. The program hears of it the way the system tells it (a console event,
    /// `SIGWINCH`). An error once the terminal is closed.
    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        let size = (cols.max(1), rows.max(1));
        if size == self.size {
            return Ok(());
        }
        self.inner.resize(size.0, size.1)?;
        self.size = size;
        Ok(())
    }

    /// The size last set, as columns and rows.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// How the program ended: `None` while it runs, and also while output it wrote before
    /// ending has not all been taken, so a caller that draws on every wake and then asks this
    /// never loses the last lines.
    pub fn exit(&self) -> Option<Exit> {
        let s = self.shared.lock();
        if s.eof && s.out.is_empty() {
            s.exit
        } else {
            None
        }
    }

    /// True until the program's end was recorded (earlier than [`exit`](Self::exit) says so,
    /// which also waits for the output).
    pub fn running(&self) -> bool {
        !self.shared.has_exited()
    }

    /// The program's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Ends the program by force (`TerminateProcess`; `SIGKILL`). Its children that share the
    /// terminal are ended when the `Pty` is dropped. Nothing when it has ended already.
    pub fn kill(&self) -> io::Result<()> {
        if self.shared.has_exited() {
            return Ok(());
        }
        self.inner.kill()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let mut s = self.shared.lock();
        s.closed = true;
        s.out = Vec::new();
        drop(s);
        self.shared.room.notify_all();
        // Closes the program's input, which ends the writing thread.
        self.input = None;
        self.inner.close();
    }
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pty")
            .field("pid", &self.pid)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
