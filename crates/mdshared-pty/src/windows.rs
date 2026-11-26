//! ConPTY: two pipes, a pseudo-console between them, and a process attached to it.
//!
//! Three threads per terminal: one reads the output pipe, one writes the input pipe, one waits
//! for the process. The pseudo-console is closed when the process ends (or the handle is
//! dropped, whichever is first); closing it is what makes the output pipe end, and it ends any
//! other program still attached to the console.

use crate::{Command, Exit, Shared};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

/// A handle this crate owns and closes.
struct Owned(HANDLE);

// SAFETY: a kernel handle is a process-wide name for an object; it may be used and closed from
// any thread, and the calls made on these (ReadFile, WriteFile, WaitForSingleObject,
// TerminateProcess) are themselves safe to make from several threads.
unsafe impl Send for Owned {}
// SAFETY: as above; nothing is reached through the handle without a system call.
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreatePipe or CreateProcessW, is owned by this value
        // alone and is closed exactly once, here.
        unsafe { CloseHandle(self.0) };
    }
}

/// The pseudo-console. Dropping it closes it.
struct Console(HPCON);

// SAFETY: an HPCON is a handle like the others; ResizePseudoConsole and ClosePseudoConsole may
// be called from any thread, and the Mutex around it keeps a resize from racing the close.
unsafe impl Send for Console {}

impl Drop for Console {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreatePseudoConsole and is closed exactly once, here.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

/// The system side of a [`crate::Pty`].
pub(crate) struct Inner {
    /// `None` once closed, by the end of the process or by [`Inner::close`].
    console: Arc<Mutex<Option<Console>>>,
    process: Arc<Owned>,
}

impl Inner {
    pub(crate) fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let guard = self.console.lock().unwrap_or_else(|e| e.into_inner());
        let Some(console) = guard.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the terminal is closed",
            ));
        };
        // SAFETY: the console is open: it is closed only by taking it out of this Mutex, which
        // is held here.
        let hr = unsafe { ResizePseudoConsole(console.0, coord(cols, rows)) };
        if hr < 0 { Err(hresult(hr)) } else { Ok(()) }
    }

    pub(crate) fn kill(&self) -> io::Result<()> {
        // SAFETY: the process handle is open for as long as this Arc lives.
        if unsafe { TerminateProcess(self.process.0, 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Closes the console on whatever is still attached to it, and ends the program itself if
    /// that did not: a program may sit out the console's closing (measured: `cmd.exe` waiting
    /// in `pause` outlived it by more than 30 seconds).
    pub(crate) fn close(&self) {
        self.close_console();
        // SAFETY: the process handle is open for as long as this Arc lives. On a process that
        // has ended the call fails and changes nothing.
        unsafe { TerminateProcess(self.process.0, 1) };
    }

    fn close_console(&self) {
        let console = self
            .console
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        // Dropped outside the lock: before Windows 11 22H2 the close waits for the output to
        // be read, which the reading thread does.
        drop(console);
    }
}

fn coord(cols: u16, rows: u16) -> COORD {
    COORD {
        X: cols.min(i16::MAX as u16) as i16,
        Y: rows.min(i16::MAX as u16) as i16,
    }
}

fn hresult(hr: i32) -> io::Error {
    io::Error::other(format!(
        "the pseudo-console call failed (HRESULT {:#010X})",
        hr as u32
    ))
}

fn pipe() -> io::Result<(Owned, Owned)> {
    let mut read: HANDLE = null_mut();
    let mut write: HANDLE = null_mut();
    // SAFETY: both out-pointers are valid for the call; no security attributes, so the handles
    // are not inheritable, which is right: the pseudo-console duplicates what it needs.
    if unsafe { CreatePipe(&mut read, &mut write, null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((Owned(read), Owned(write)))
}

pub(crate) fn spawn(
    command: &Command,
    cols: u16,
    rows: u16,
    shared: Arc<Shared>,
    input: mpsc::Receiver<Vec<u8>>,
) -> io::Result<(Inner, u32)> {
    let env = environment(&command.env);
    let (application, mut line) = command_line(command, &env)?;
    let application = wide(application.as_os_str())?;
    let block = env_block(&env)?;
    let cwd = match &command.cwd {
        Some(dir) => Some(wide(dir.as_os_str())?),
        None => None,
    };

    let (in_read, in_write) = pipe()?;
    let (out_read, out_write) = pipe()?;

    // SAFETY: an all-zero HPCON is a valid "no handle" value to be overwritten by the call.
    let mut hpc: HPCON = unsafe { std::mem::zeroed() };
    // SAFETY: both pipe handles are open, and `hpc` is a valid out-pointer.
    let hr = unsafe { CreatePseudoConsole(coord(cols, rows), in_read.0, out_write.0, 0, &mut hpc) };
    if hr < 0 {
        return Err(hresult(hr));
    }
    let console = Console(hpc);

    // The attribute list that hands the console to the new process. Its size is the system's
    // to say; the buffer is of usize so that it is aligned for whatever the list holds.
    let mut size = 0usize;
    // SAFETY: the documented way to ask for the size: a null list, and the call fails with
    // the size written.
    unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size) };
    let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>()).max(1)];
    let list = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
    // SAFETY: `list` points at `size` writable bytes, as the call above asked for.
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the list was initialized just above; for this attribute the value is the console
    // handle itself and the size that of the handle, as CreatePseudoConsole's documentation
    // says; the console outlives the list.
    let updated = unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            console.0 as *const c_void,
            size_of::<HPCON>(),
            null_mut(),
            null(),
        )
    };
    if updated == 0 {
        let error = io::Error::last_os_error();
        // SAFETY: the list was initialized and is deleted once.
        unsafe { DeleteProcThreadAttributeList(list) };
        return Err(error);
    }

    // SAFETY: both are plain C structs for which all-zero is a valid value.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // Without this a caller whose own output is redirected (a test runner, a program started
    // from an editor) would pass its redirected handles on, and the program would write there
    // and not to the console.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = list;
    // SAFETY: as above.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    // SAFETY: every pointer is to a NUL-terminated buffer that lives across the call (the
    // command line is mutable, as the call requires); the environment block is UTF-16 and ends
    // with two NULs, matching CREATE_UNICODE_ENVIRONMENT; the startup structure is the extended
    // one, matching EXTENDED_STARTUPINFO_PRESENT, with an initialized attribute list.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            block.as_ptr() as *const c_void,
            cwd.as_ref().map_or(null(), |c| c.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        )
    };
    let error = io::Error::last_os_error();
    // SAFETY: the list was initialized and is deleted once, after its only use.
    unsafe { DeleteProcThreadAttributeList(list) };
    if created == 0 {
        return Err(error);
    }
    drop(Owned(info.hThread));
    let process = Arc::new(Owned(info.hProcess));
    // The console has its own copies of these two ends. Ours must go, or the output pipe
    // would never end.
    drop(in_read);
    drop(out_write);

    let console = Arc::new(Mutex::new(Some(console)));

    let reader = shared.clone();
    thread::Builder::new()
        .name("pty-read".into())
        .spawn(move || {
            // The whole handle moves in, so that it is closed when this thread ends.
            let out_read = out_read;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let mut n = 0u32;
                // SAFETY: the buffer is valid for its length, `n` for the count, and the handle is
                // owned by this thread.
                let ok = unsafe {
                    ReadFile(
                        out_read.0,
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        &mut n,
                        null_mut(),
                    )
                };
                if ok == 0 || n == 0 {
                    break;
                }
                // When the handle is gone the bytes are dropped, and reading goes on so that the
                // console can finish closing.
                reader.push(&buffer[..n as usize]);
            }
            reader.end_of_output();
        })?;

    thread::Builder::new()
        .name("pty-write".into())
        .spawn(move || {
            let in_write = in_write;
            for chunk in input {
                let mut rest = &chunk[..];
                while !rest.is_empty() {
                    let mut n = 0u32;
                    let len = rest.len().min(u32::MAX as usize) as u32;
                    // SAFETY: `rest` is valid for `len` bytes, `n` for the count, and the handle is
                    // owned by this thread.
                    if unsafe { WriteFile(in_write.0, rest.as_ptr(), len, &mut n, null_mut()) } == 0
                        || n == 0
                    {
                        return;
                    }
                    rest = &rest[n as usize..];
                }
            }
        })?;

    let waited = process.clone();
    let closing = console.clone();
    thread::Builder::new()
        .name("pty-wait".into())
        .spawn(move || {
            let mut code = 0u32;
            // SAFETY: the process handle is open for as long as this thread's Arc lives.
            unsafe { WaitForSingleObject(waited.0, INFINITE) };
            // SAFETY: as above; `code` is a valid out-pointer.
            unsafe { GetExitCodeProcess(waited.0, &mut code) };
            shared.exited(Exit {
                code: Some(code),
                signal: None,
            });
            // Closing the console is what ends the output pipe, and so the reading thread.
            let console = closing.lock().unwrap_or_else(|e| e.into_inner()).take();
            drop(console);
        })?;

    Ok((Inner { console, process }, info.dwProcessId))
}

/// A NUL-terminated UTF-16 copy. A NUL inside is refused: the system would cut the text there.
fn wide(text: &OsStr) -> io::Result<Vec<u16>> {
    let mut w: Vec<u16> = text.encode_wide().collect();
    if w.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a NUL inside a name or an argument",
        ));
    }
    w.push(0);
    Ok(w)
}

/// The environment the program gets: the caller's, with the command's changes. Keyed by the
/// name in upper case, as Windows treats names without regard to case and wants the block
/// sorted so.
pub(crate) fn environment(
    changes: &[(OsString, Option<OsString>)],
) -> BTreeMap<String, (OsString, OsString)> {
    let mut env = BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        env.insert(name.to_string_lossy().to_uppercase(), (name, value));
    }
    for (name, value) in changes {
        let key = name.to_string_lossy().to_uppercase();
        match value {
            Some(value) => env.insert(key, (name.clone(), value.clone())),
            None => env.remove(&key),
        };
    }
    env
}

/// The block CreateProcessW takes: `name=value` strings, each ended by a NUL, and one more NUL.
pub(crate) fn env_block(env: &BTreeMap<String, (OsString, OsString)>) -> io::Result<Vec<u16>> {
    let mut block = Vec::new();
    for (name, value) in env.values() {
        let name: Vec<u16> = name.encode_wide().collect();
        let value: Vec<u16> = value.encode_wide().collect();
        if name.is_empty() || name.contains(&0) || value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a NUL inside an environment variable",
            ));
        }
        block.extend(name);
        block.push(b'=' as u16);
        block.extend(value);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

/// The application to start and its command line (NUL-terminated). A `.cmd` or `.bat` becomes
/// `cmd.exe /d /s /c "<file> <arguments>"`.
fn command_line(
    command: &Command,
    env: &BTreeMap<String, (OsString, OsString)>,
) -> io::Result<(PathBuf, Vec<u16>)> {
    let path = env.get("PATH").map(|(_, v)| v.clone()).unwrap_or_default();
    let found = resolve(&command.program, &path, command.cwd.as_deref())?;
    let script = found
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));

    let mut line: Vec<u16> = Vec::new();
    if !script {
        quote(&command.program, &mut line);
        for arg in &command.args {
            line.push(b' ' as u16);
            quote(arg, &mut line);
        }
        if line.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a NUL inside a name or an argument",
            ));
        }
        line.push(0);
        return Ok((found, line));
    }

    // cmd.exe reads its line by rules of its own, which no quoting makes safe for every text.
    // What cannot be passed safely is refused, not guessed at.
    for arg in &command.args {
        let unsafe_for_cmd = arg
            .encode_wide()
            .any(|c| matches!(c, 0x22 | 0x25 | 0x0A | 0x0D | 0));
        if unsafe_for_cmd {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a quote, a percent sign or a line break in an argument of a .cmd or .bat file",
            ));
        }
    }
    let shell = env
        .get("COMSPEC")
        .map(|(_, v)| PathBuf::from(v))
        .filter(|p| p.is_file())
        .or_else(|| resolve(OsStr::new("cmd.exe"), &path, None).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cmd.exe was not found"))?;
    line.extend("cmd.exe /d /s /c \"".encode_utf16());
    line.push(b'"' as u16);
    line.extend(found.as_os_str().encode_wide());
    line.push(b'"' as u16);
    for arg in &command.args {
        line.push(b' ' as u16);
        line.push(b'"' as u16);
        line.extend(arg.encode_wide());
        line.push(b'"' as u16);
    }
    line.push(b'"' as u16);
    if line.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a NUL inside a name or an argument",
        ));
    }
    line.push(0);
    Ok((shell, line))
}

/// Finds the program's file. A name with a folder in it is taken as it is (relative to `cwd`
/// when there is one); a bare name is looked for in each folder of `path`. A name without an
/// extension is tried as `.exe`, `.com`, `.cmd`, `.bat`, in that order, in each folder.
pub(crate) fn resolve(program: &OsStr, path: &OsStr, cwd: Option<&Path>) -> io::Result<PathBuf> {
    let missing = || {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("the program {} was not found", program.to_string_lossy()),
        )
    };
    if program.is_empty() {
        return Err(missing());
    }
    let given = Path::new(program);
    let candidates = |base: &Path| -> Vec<PathBuf> {
        let mut all = Vec::new();
        if base.extension().is_some() {
            all.push(base.to_path_buf());
        }
        for ext in ["exe", "com", "cmd", "bat"] {
            let mut name = base.as_os_str().to_os_string();
            name.push(".");
            name.push(ext);
            all.push(PathBuf::from(name));
        }
        all
    };
    let has_folder = given.components().count() > 1 || given.is_absolute();
    if has_folder {
        let base = match cwd {
            Some(dir) if given.is_relative() => dir.join(given),
            _ => given.to_path_buf(),
        };
        return candidates(&base)
            .into_iter()
            .find(|c| c.is_file())
            .ok_or_else(missing);
    }
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if let Some(found) = candidates(&dir.join(given))
            .into_iter()
            .find(|c| c.is_file())
        {
            return Ok(found);
        }
    }
    Err(missing())
}

/// Appends one argument, quoted as the C runtime's argument parser (and so nearly every
/// program) reads it back: quotes around anything with a space, a tab or a quote in it or
/// nothing in it, a backslash before each quote, and the backslashes before a quote doubled.
pub(crate) fn quote(arg: &OsStr, line: &mut Vec<u16>) {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let units: Vec<u16> = arg.encode_wide().collect();
    let plain = !units.is_empty()
        && !units
            .iter()
            .any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == QUOTE);
    if plain {
        line.extend(units);
        return;
    }
    line.push(QUOTE);
    let mut backslashes = 0usize;
    for c in units {
        if c == BACKSLASH {
            backslashes += 1;
            continue;
        }
        if c == QUOTE {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
        } else {
            line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        }
        backslashes = 0;
        line.push(c);
    }
    line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    line.push(QUOTE);
}

/// The quoted line as text, for the tests.
#[cfg(test)]
pub(crate) fn quoted(args: &[&str]) -> String {
    let mut line = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            line.push(b' ' as u16);
        }
        quote(OsStr::new(arg), &mut line);
    }
    String::from_utf16_lossy(&line)
}
