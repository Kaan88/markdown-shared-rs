//! One running instance per session through a named mutex, with no listening socket. Windows
//! only.
//!
//! [`crate::single`] does the same job through a loopback TCP port. An application that must not
//! open a listening socket (Purge, an uninstaller that runs elevated) uses this module instead. The
//! first instance creates a named mutex and keeps the [`Guard`]; a later launch finds the mutex
//! there, finds the first instance's window, hands it its command line in a `WM_COPYDATA`
//! message and brings it forward. Nothing listens, nothing is written to disk, and nothing is left
//! behind: every object here is a kernel object that goes away with the last process holding it,
//! however that process ended.
//!
//! What the caller owns: the decision to exit (a second launch is told [`Instance::Running`] and
//! decides), its window and its window procedure. The first instance tells the module which
//! window is its main one with [`Guard::publish_window`] once the window exists. Its window
//! procedure passes each `WM_COPYDATA` it receives to [`received`], which says whether the message
//! was a hand-over of this application and gives the command line it carried.
//!
//! ```text
//! match single_mutex::acquire("purge") {
//!     Instance::First(guard) => { /* make the window */ guard.publish_window(hwnd); /* run, keeping guard */ }
//!     Instance::Running(first) => {
//!         if let Some(window) = first.wait_window(Duration::from_secs(2)) {
//!             single_mutex::send("purge", window, &args);
//!             single_mutex::bring_forward(window);
//!         }
//!         return; // the caller's choice
//!     }
//! }
//! ```
//!
//! # The names
//!
//! The mutex is `Local\mdshared-single.<app id>.mutex` and the record of the first instance is a
//! named file mapping, `Local\mdshared-single.<app id>.window`. `Local\` is the namespace of the
//! session the process runs in. Each session (the console, each Remote Desktop session, each user
//! of fast user switching) has its own instance, which is what a window application wants: a
//! window in another session cannot be shown or brought forward from this one, so a launch that
//! found an instance there could only refuse to start. `Global\` would also let a process of
//! another user block this user's launch, and a file mapping there needs the privilege
//! `SeCreateGlobalPrivilege`, which a process outside session 0 that is not elevated lacks.
//!
//! Within a session, an elevated and a plain process see the same `Local\` names. An object made
//! by an elevated process is closed to a plain one: a plain launch beside an elevated first
//! instance gets "access denied" for the mutex, which is read as [`Instance::Running`] whose
//! [`Running::pid`] is `None` (the record cannot be opened either), so it starts nothing and can
//! bring nothing forward.
//!
//! # Finding the window
//!
//! The first instance's process id and main window are written in the small named file mapping
//! (16 bytes) that the [`Guard`] holds. A second launch opens it, reads them and closes it. The
//! mapping is the choice over the alternatives because it cannot go stale: it dies with the
//! process, so the id in it is never that of a process that has ended (whose number Windows may
//! have given to another). The owner of a mutex cannot be asked from documented interfaces, a
//! file beside the settings outlives a process that crashed, and `FindWindowW` by class or title
//! finds any window with that class or title, including another application's.
//!
//! The published window is used when it is still a window of that process. When none was
//! published, or it is gone, the top-level windows of the process are looked through
//! ([`main_window_of`]): the first one, in the order of the screen from the top, that is visible,
//! has no owner and is not a tool window. winit and the input method make hidden top-level windows
//! of their own in every process, which is why a hidden window is never picked that way.
//!
//! [`Running::wait_window`] waits for the published window only. A first instance that is still
//! starting may have a window of its own up (a splash, a hidden one) that is not its main window
//! yet, and a launch that took it would hand its command line to the wrong window and lose the
//! hand-over. So the looking through happens once, at the deadline, for an application that never
//! publishes a window; [`Running::window`] does it at once, for an application that asks now.
//!
//! # Bringing forward
//!
//! [`bring_forward`] restores the window if it is minimized (`ShowWindowAsync(SW_RESTORE)`, which
//! asks the window's own thread to do it and returns at once, so a hung first instance cannot hang
//! the second launch) and then calls `SetForegroundWindow`. Windows lets a process take the
//! foreground only in some cases; the ones that matter here are: the process is the foreground
//! process, it was started by the foreground process, or it received the last input event. A
//! second launch from Explorer, the Start menu, a shortcut or a terminal the user typed in is
//! started by the foreground process and may set the foreground window. One started from a
//! background process (a scheduled task, a service, a script running unseen) may not: Windows then
//! flashes the window's taskbar button instead, and `SetForegroundWindow` answers false.
//!
//! The second launch also calls `AllowSetForegroundWindow` for the first instance's process before
//! it sends anything, which passes its right on: the first instance may then bring forward a
//! window of its own choosing while it handles the `WM_COPYDATA` (a dialog that is open, or a
//! window that was hidden to the notification area, which [`bring_forward`] does not show).
//!
//! # The hand-over of the command line
//!
//! [`send`] packs an application id and a list of arguments into a `WM_COPYDATA` and sends it with
//! a time limit of [`SEND_TIMEOUT`], so a hung first instance does not hang the launch. Windows
//! copies the bytes into the receiver. [`received`] checks that a message is one of these, of the
//! same application id, and gives the arguments back exactly, as `OsString`s (a Windows command
//! line may hold text that is not valid Unicode; it travels unchanged).
//!
//! `WM_COPYDATA` is sent, not posted, so it reaches only the window procedure. winit 0.30's
//! `with_msg_hook` sees the messages taken from the queue, which are the posted ones: it does not
//! see this one. A winit application replaces its window's procedure (`SetWindowLongPtrW` with
//! `GWLP_WNDPROC`, chained with `CallWindowProcW`, as `mdshared_ui::snap` does) and passes the
//! message to [`received`] there, returning 1 when it gave `Some`.
//!
//! The application id in the message keeps one application's hand-over from being taken by
//! another one's window. It is not authentication: any process of the session at the same
//! integrity level can send such a message, so the arguments are to be treated as a command line
//! is, as input from the user.
//!
//! # Deliberately not here
//!
//! - Linux and macOS. The module is compiled on Windows only. A lock file taken with `flock` would
//!   refuse a second instance but bring nothing forward and hand nothing over, which
//!   [`crate::folder_lock`] and [`crate::single`] already do better there; and a per-user place
//!   for the lock file needs `XDG_RUNTIME_DIR`, which is not always set, or the user id, which
//!   needs libc.
//! - A machine-wide instance (`Global\`): see "The names".
//! - A plain launch beside an elevated first instance. It learns that one runs and nothing more:
//!   the record would need a security descriptor that lets a lower integrity level read it, and
//!   the window would need `ChangeWindowMessageFilterEx` for `WM_COPYDATA`. An application that
//!   runs elevated every time (its manifest asks for it) never meets the case.
//! - Protection from a squatter. A process of the same user that creates the mutex first makes
//!   every launch think the application runs. Private namespaces with a boundary descriptor would
//!   prevent it; an application that needs that is beyond this module. The same goes for the
//!   record: a process that makes the mapping of that name first is found by the next launch, and
//!   a first instance that finds a mapping already there does not write into it (see
//!   `create_record`): it keeps its mutex, publishes no window, and every launch finds the windows
//!   of the process instead. Neither is prevented, only not made worse.
//! - Waiting for the first instance to answer anything beyond the message: [`send`] says whether
//!   the window procedure took the hand-over, not what the application did with it.
//! - A listener thread or a message-only window of its own. The first instance's own window
//!   receives the message, so nothing runs while nothing is sent.
//! - Callbacks. The window procedure is the application's; it calls [`received`].
//!
//! Verified: the tests (`single_mutex_tests.rs`), run on Windows: two guards in one process, the
//! record, a hidden window of the test's own, a `WM_COPYDATA` sent to it, and a second process
//! (the test program started again) that finds the first, its window, and hands it its arguments.
//! Not verified: a real window brought forward by a launch from Explorer (it needs a person at
//! the desktop; the tests bring nothing forward), a winit window receiving the message through a
//! replaced procedure, and an elevated first instance with a plain second launch.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    BOOL, CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
    GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, SetLastError,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SMTO_ABORTIFHUNG, SW_RESTORE,
    SendMessageTimeoutW, SetForegroundWindow, ShowWindowAsync, WM_COPYDATA, WS_EX_TOOLWINDOW,
};

/// The most bytes a hand-over carries, both ways: 1 MiB. A Windows command line is at most 32,767
/// UTF-16 units (64 KiB), so this leaves room for arguments that did not come from one. [`send`]
/// refuses more, and [`received`] ignores a message that claims more.
pub const MAX_BYTES: usize = 1 << 20;

/// How long [`send`] waits for the first instance's window procedure to return.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(2);

/// The `dwData` of a hand-over: "MDS1". A message with another value is not one of these.
const TAG: usize = 0x4D44_5331;

/// The first word of the record, written last, so a record is read only once it is whole.
const MAGIC: u32 = 0x4D44_5352;

/// The size of the record: the magic, the process id and the window, 4 + 4 + 8 bytes.
const RECORD: usize = 16;

/// The most UTF-16 units of the application id that go into an object name. A name may be at most
/// 260 units long; the prefix and the suffix take 29.
const MAX_ID_UNITS: usize = 200;

/// What a launch is.
#[derive(Debug)]
pub enum Instance {
    /// This is the first instance. Keep the guard for as long as the application runs: dropping
    /// it lets the next launch be the first.
    First(Guard),
    /// Another instance of this application already runs in this session.
    Running(Running),
}

/// What the first instance keeps: the mutex, and the record of its process and window.
///
/// Dropping it closes both. The mutex then goes away (unless a second launch has it open at that
/// moment, for the microseconds it takes to look), and the next launch is the first.
#[derive(Debug)]
pub struct Guard {
    /// The mutex handle, as an integer; 0 when it could not be made.
    mutex: isize,
    /// The record's file mapping handle; 0 when it could not be made.
    mapping: isize,
    /// The address of the record's view in this process; 0 when there is none.
    view: usize,
}

/// A second launch's view of the instance that runs. Holds nothing open: each question opens the
/// record, reads it and closes it again, so the answers follow the first instance as it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    app_id: String,
}

/// Takes the single-instance mutex of `app_id` in this session, or finds that another instance
/// holds it.
///
/// `app_id` names the application; the same id in two programs makes them one application. A `\`
/// in it, which object names do not allow, becomes `_`, and only its first 200 UTF-16 units are
/// used. An empty id is an id like any other.
///
/// The mutex is never waited on: its existence is the signal. `CreateMutexW` answers
/// `ERROR_ALREADY_EXISTS` when it is there, which gives [`Instance::Running`], and so does
/// "access denied" (an elevated instance's mutex seen from a plain launch). Any other failure of
/// the system gives a [`Guard`] that holds nothing, so the application runs as it would without
/// this module rather than not at all; [`Guard::held`] tells the two apart.
pub fn acquire(app_id: &str) -> Instance {
    let name = object_name(app_id, "mutex");
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; no security
    // attributes (null) means the default ones; the handle returned is owned here.
    let (mutex, error) = unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        (handle, GetLastError())
    };
    if mutex.is_null() {
        if error == ERROR_ACCESS_DENIED {
            return Instance::Running(Running::new(app_id));
        }
        return Instance::First(Guard {
            mutex: 0,
            mapping: 0,
            view: 0,
        });
    }
    if error == ERROR_ALREADY_EXISTS {
        // SAFETY: `mutex` is a handle this function opened and nothing else holds.
        unsafe { CloseHandle(mutex) };
        return Instance::Running(Running::new(app_id));
    }
    let (mapping, view) = create_record(app_id);
    Instance::First(Guard {
        mutex: mutex as isize,
        mapping: mapping as isize,
        view,
    })
}

/// Makes the record of this process and maps it: (mapping, address of the view), each 0 or null
/// when it could not be made, and also when a mapping of that name was already there. Writes this
/// process's id and no window.
fn create_record(app_id: &str) -> (HANDLE, usize) {
    let name = object_name(app_id, "window");
    // SAFETY: `SetLastError` takes a value and has no other effect. Clearing it makes the
    // `ERROR_ALREADY_EXISTS` read below about this call alone: `CreateFileMappingW` sets it only
    // when the mapping existed before, and leaves the last error alone when it did not.
    unsafe { SetLastError(0) };
    // SAFETY: a mapping backed by the paging file (`INVALID_HANDLE_VALUE`) of `RECORD` bytes,
    // named by a NUL-terminated string that outlives the call; default security.
    let mapping = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            std::ptr::null(),
            PAGE_READWRITE,
            0,
            RECORD as u32,
            name.as_ptr(),
        )
    };
    if mapping.is_null() {
        return (std::ptr::null_mut(), 0);
    }
    // SAFETY: `mapping` is the handle just opened.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if existed {
        // The mapping was there before this call, so this instance did not make it: a squatter
        // did (see the module's "Deliberately not here"). Writing the record into it would let a
        // stranger name the window a launch hands its command line to, so the handle is closed
        // and nothing is published; the windows of the process are looked through instead.
        // SAFETY: the handle was opened here and nothing else holds it.
        unsafe { CloseHandle(mapping) };
        return (std::ptr::null_mut(), 0);
    }
    // SAFETY: `mapping` is the handle just made, at least `RECORD` bytes long.
    let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, RECORD) };
    let address = view.Value as usize;
    if address == 0 {
        // SAFETY: the handle was made above and is not kept.
        unsafe { CloseHandle(mapping) };
        return (std::ptr::null_mut(), 0);
    }
    // SAFETY: a view is page-aligned and `RECORD` bytes long, which the three atomics fit; the
    // memory is shared with other processes, which is what atomics are for.
    unsafe {
        let (magic, pid, window) = record_fields(address);
        // A mapping left open by a second launch that was reading it has an earlier record: every
        // field is written again.
        magic.store(0, Ordering::Release);
        window.store(0, Ordering::Relaxed);
        pid.store(GetCurrentProcessId(), Ordering::Relaxed);
        magic.store(MAGIC, Ordering::Release);
    }
    (mapping, address)
}

/// The three fields of the record at `address`.
///
/// # Safety
/// `address` must be a live view of at least `RECORD` bytes, aligned to 8.
unsafe fn record_fields<'a>(address: usize) -> (&'a AtomicU32, &'a AtomicU32, &'a AtomicU64) {
    // SAFETY: the caller's promise; the fields are at 0, 4 and 8, each aligned to its size.
    unsafe {
        (
            &*(address as *const AtomicU32),
            &*((address + 4) as *const AtomicU32),
            &*((address + 8) as *const AtomicU64),
        )
    }
}

impl Guard {
    /// Whether the mutex is really held. False only when the system refused to make it (see
    /// [`acquire`]): the application then runs unguarded, and a later launch will be first too.
    pub fn held(&self) -> bool {
        self.mutex != 0
    }

    /// Records `hwnd` as this instance's main window, which a second launch's
    /// [`Running::window`] then finds directly. Call it once the window exists, and again if the
    /// main window changes; 0 takes the record back, and the windows of the process are looked
    /// through instead. Does nothing when there is no record.
    ///
    /// The handle is taken as it is given (winit's `Win32WindowHandle::hwnd`), and is not checked
    /// here: [`Running::window`] checks it belongs to this process before using it.
    pub fn publish_window(&self, hwnd: isize) {
        if self.view == 0 {
            return;
        }
        // SAFETY: `view` is this guard's live view of `RECORD` bytes, page-aligned.
        let (_, _, window) = unsafe { record_fields(self.view) };
        window.store(hwnd as u64, Ordering::Release);
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // The mutex goes first, the mapping last: `wait_window` takes "no record and no mutex" for
        // the instance having gone, so the mutex must not outlive the record.
        // SAFETY: each value is a handle or a view this guard made and nothing else releases;
        // 0 means there is none.
        unsafe {
            if self.mutex != 0 {
                CloseHandle(self.mutex as HANDLE);
            }
            if self.view != 0 {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as *mut core::ffi::c_void,
                });
            }
            if self.mapping != 0 {
                CloseHandle(self.mapping as HANDLE);
            }
        }
    }
}

impl Running {
    fn new(app_id: &str) -> Running {
        Running {
            app_id: app_id.to_string(),
        }
    }

    /// The (process id, published window) in the record, if it can be opened and is whole.
    fn record(&self) -> Option<(u32, isize)> {
        let name = object_name(&self.app_id, "window");
        // SAFETY: a NUL-terminated name that outlives the call; the handle is closed below.
        let mapping = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr()) };
        if mapping.is_null() {
            return None;
        }
        // SAFETY: `mapping` was just opened; the view is unmapped and the handle closed before
        // returning; a view is page-aligned and the record is `RECORD` bytes, which the three
        // atomics fit. The memory is shared with other processes: the writer stores every field
        // with `Relaxed` and the magic last with `Release` (`create_record`), so the reader loads
        // the magic with `Relaxed`, and the `Acquire` fence after that load pairs with the release
        // store: the two `Relaxed` loads after the fence cannot be moved before it, and a record
        // whose magic reads whole has whole fields. Without the fence a relaxed read could see the
        // new magic beside an old window.
        unsafe {
            let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, RECORD);
            let address = view.Value as usize;
            let found = if address == 0 {
                None
            } else {
                let (magic, pid, window) = record_fields(address);
                let whole = magic.load(Ordering::Relaxed) == MAGIC;
                fence(Ordering::Acquire);
                let found = whole.then(|| {
                    (
                        pid.load(Ordering::Relaxed),
                        window.load(Ordering::Relaxed) as isize,
                    )
                });
                UnmapViewOfFile(view);
                found
            };
            CloseHandle(mapping);
            found.filter(|&(pid, _)| pid != 0)
        }
    }

    /// The process id of the instance that runs. `None` while that instance is between taking
    /// the mutex and writing its record (microseconds), and when its record is closed to this
    /// process (an elevated first instance seen from a plain launch).
    pub fn pid(&self) -> Option<u32> {
        self.record().map(|(pid, _)| pid)
    }

    /// The main window of the instance that runs, now: the one it published with
    /// [`Guard::publish_window`] while that is still a window of its process, otherwise
    /// [`main_window_of`] its process. `None` when there is no process id or no such window (the
    /// first instance may not have made its window yet: see [`Running::wait_window`]).
    ///
    /// This is the "what is there now" question: the windows of the process are looked through at
    /// once. A launch that waits for a first instance is better served by
    /// [`Running::wait_window`], which takes only a published window until its deadline, so that a
    /// window the first instance happens to have up is not taken for its main one.
    pub fn window(&self) -> Option<isize> {
        self.published_window()
            .or_else(|| main_window_of(self.record()?.0))
    }

    /// The window the first instance published with [`Guard::publish_window`], while it is still a
    /// window of that process. `None` when none is published, it is gone, or there is no record.
    /// Never looks at the other windows of the process; [`Running::window`] does that.
    fn published_window(&self) -> Option<isize> {
        let (pid, published) = self.record()?;
        (published != 0 && window_pid(published) == Some(pid)).then_some(published)
    }

    /// The published window of the first instance, waited for: asked again every 20 ms for at most
    /// `timeout`, until the window it published is there and is one of its process. Only at the
    /// deadline are the other windows of the process looked through, once, for a first instance
    /// that never publishes one ([`main_window_of`]).
    ///
    /// The difference from [`Running::window`] is the point of this function: a window the first
    /// instance happens to have up while it is still starting (a splash, or winit's hidden ones,
    /// which are not picked at all) is not taken for its main window, so the hand-over reaches the
    /// window that will handle it.
    ///
    /// Blocks the calling thread; a second launch has nothing else to do. A zero timeout asks
    /// once; one too long to be added to the time now (`Duration::MAX`) waits until a window
    /// comes.
    ///
    /// `None` also when the instance has gone while this waited: neither its record nor its mutex
    /// can be opened any more. Call [`acquire`] again then; if that gives [`Instance::First`], this
    /// process is the first instance after all.
    pub fn wait_window(&self, timeout: Duration) -> Option<isize> {
        let end = Instant::now().checked_add(timeout);
        loop {
            if let Some(window) = self.published_window() {
                return Some(window);
            }
            if self.gone() {
                return None;
            }
            if end.is_some_and(|end| Instant::now() >= end) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        main_window_of(self.record()?.0)
    }

    /// Whether the instance has gone: neither its record nor its mutex can be opened any more.
    /// False while either can be: between taking the mutex and writing the record the first
    /// instance has the mutex and no record, and a plain launch cannot open an elevated one's
    /// record but can open its mutex.
    fn gone(&self) -> bool {
        if self.record().is_some() {
            return false;
        }
        let name = object_name(&self.app_id, "mutex");
        // SAFETY: a NUL-terminated name that outlives the call; `SYNCHRONIZE` is asked for because
        // it is the least that opens a mutex, and only whether one is there matters. The handle is
        // closed below when there is one.
        let mutex = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        if mutex.is_null() {
            // Only a mutex that is not there is gone. One this process may not open (an elevated
            // first instance, whose record cannot be read either) is still running.
            // SAFETY: reads this thread's last error, set by the call just above.
            return unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND;
        }
        // SAFETY: the handle was opened here and nothing else holds it.
        unsafe { CloseHandle(mutex) };
        false
    }
}

/// The process that owns the window `hwnd`, or `None` when it is not a window.
fn window_pid(hwnd: isize) -> Option<u32> {
    let window = hwnd as HWND;
    let mut pid = 0u32;
    // SAFETY: both calls take any value as a handle and fail for one that is not a window; `pid`
    // is a live `u32`.
    let thread = unsafe {
        if IsWindow(window) == 0 {
            return None;
        }
        GetWindowThreadProcessId(window, &mut pid)
    };
    (thread != 0 && pid != 0).then_some(pid)
}

/// What [`windows_of`] gathers while `EnumWindows` runs.
struct Gather {
    pid: u32,
    found: Vec<isize>,
}

/// Called by `EnumWindows` for each top-level window.
unsafe extern "system" fn gather(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut Gather` that `windows_of` passed, alive for the whole
    // enumeration, and this is the only reference made from it at a time.
    let gather = unsafe { &mut *(lparam as *mut Gather) };
    let mut pid = 0u32;
    // SAFETY: `hwnd` is a window the system is enumerating; `pid` is a live `u32`.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == gather.pid {
        gather.found.push(hwnd as isize);
    }
    1
}

/// Every top-level window of the process `pid`, visible or not, in the order of the screen from
/// the top. Empty for a process with none, or for a number that is no process. Message-only
/// windows are not top-level and are never in it.
pub fn windows_of(pid: u32) -> Vec<isize> {
    if pid == 0 {
        return Vec::new();
    }
    let mut gather = Gather {
        pid,
        found: Vec::new(),
    };
    // SAFETY: `gather` is a function with the signature `EnumWindows` calls, and the pointer
    // passed is to `gather` (the local), which outlives the call.
    unsafe { EnumWindows(Some(self::gather), &mut gather as *mut Gather as LPARAM) };
    gather.found
}

/// The main window of the process `pid`: the first of [`windows_of`] that is visible, has no
/// owner and is not a tool window. `None` when it has no such window. Hidden windows are never
/// picked: every winit process has hidden top-level windows of winit's and of the input method.
pub fn main_window_of(pid: u32) -> Option<isize> {
    windows_of(pid).into_iter().find(|&hwnd| {
        let window = hwnd as HWND;
        // SAFETY: the calls take any handle and give 0 for one that is gone.
        unsafe {
            IsWindowVisible(window) != 0
                && GetWindow(window, GW_OWNER).is_null()
                && (GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW) == 0
        }
    })
}

/// Brings the window `hwnd` of another process forward: lets its process take the foreground
/// (`AllowSetForegroundWindow`), restores it if it is minimized, and makes it the foreground
/// window. True when Windows made it the foreground window.
///
/// False, and nothing shown, for a handle that is not a window and for a hidden window (one hidden
/// to the notification area is the application's to show, which it may do on the hand-over: its
/// process was allowed the foreground first). False also when this process may not set the
/// foreground window (see the module's text); Windows then flashes the window's taskbar button.
pub fn bring_forward(hwnd: isize) -> bool {
    let Some(pid) = window_pid(hwnd) else {
        return false;
    };
    let window = hwnd as HWND;
    // SAFETY: the calls take a process id and a window handle by value and fail harmlessly for
    // ones that are gone. `ShowWindowAsync` only asks the window's thread to restore it and
    // returns at once, so a hung first instance cannot hang this launch; its answer is not used.
    unsafe {
        AllowSetForegroundWindow(pid);
        if IsWindowVisible(window) == 0 {
            return false;
        }
        if IsIconic(window) != 0 {
            ShowWindowAsync(window, SW_RESTORE);
        }
        SetForegroundWindow(window) != 0
    }
}

/// Hands `args` to the window `hwnd` in a `WM_COPYDATA` of the application `app_id`, waiting at
/// most [`SEND_TIMEOUT`] for its window procedure. True when the procedure returned non-zero, which
/// is what it does for a message [`received`] accepted (see the module's text). Lets the window's
/// process take the foreground first, so it may show what it likes while it handles the message.
///
/// False without sending for a handle that is not a window, for an id or an argument that holds a
/// NUL (a command line cannot), and for more than [`MAX_BYTES`] in all. False after sending when
/// the window does not take it: another kind of window, a hung one, one that has not answered in
/// time, or a window of a higher integrity level (Windows drops the message). An empty list of arguments is sent; it means "a launch with no arguments".
pub fn send(app_id: &str, hwnd: isize, args: &[OsString]) -> bool {
    let Some(pid) = window_pid(hwnd) else {
        return false;
    };
    let Some(units) = encode(app_id, args) else {
        return false;
    };
    let data = COPYDATASTRUCT {
        dwData: TAG,
        cbData: (units.len() * 2) as u32,
        lpData: units.as_ptr() as *mut core::ffi::c_void,
    };
    let mut result = 0usize;
    // SAFETY: `data` and the bytes it points to outlive the call, which returns only once the
    // receiver is done (Windows copies the bytes into its process); the receiver only reads them.
    // `AllowSetForegroundWindow` takes a process id by value.
    let sent = unsafe {
        AllowSetForegroundWindow(pid);
        SendMessageTimeoutW(
            hwnd as HWND,
            WM_COPYDATA,
            0,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT.as_millis() as u32,
            &mut result,
        )
    };
    sent != 0 && result != 0
}

/// The arguments of a hand-over of the application `app_id`, from a message the window procedure
/// received: `Some` when `msg` is `WM_COPYDATA` and `lparam` holds a hand-over made by [`send`]
/// with the same id; `None` for any other message, another application's hand-over, a message
/// that claims more than [`MAX_BYTES`] and bytes that are not one. The window procedure returns 1
/// for `Some` and passes the message on otherwise.
///
/// # Safety
/// `msg` and `lparam` must be the ones the window procedure was called with. For `WM_COPYDATA`,
/// Windows makes `lparam` point to a `COPYDATASTRUCT` that is valid until the procedure returns;
/// any other pair of values is not looked at beyond `msg`.
pub unsafe fn received(app_id: &str, msg: u32, lparam: isize) -> Option<Vec<OsString>> {
    if msg != WM_COPYDATA || lparam == 0 {
        return None;
    }
    // SAFETY: the caller's promise: for `WM_COPYDATA`, `lparam` is a live `COPYDATASTRUCT`.
    let data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
    let length = data.cbData as usize;
    if data.dwData != TAG || length > MAX_BYTES {
        return None;
    }
    let bytes: &[u8] = if length == 0 {
        &[]
    } else if data.lpData.is_null() {
        return None;
    } else {
        // SAFETY: Windows copied `cbData` bytes to `lpData` in this process, valid until the
        // window procedure returns, which is after this function does.
        unsafe { std::slice::from_raw_parts(data.lpData as *const u8, length) }
    };
    decode(app_id, bytes)
}

/// The NUL-terminated object name `Local\mdshared-single.<id>.<what>`. A `\` in the id becomes
/// `_`, and the id is cut to [`MAX_ID_UNITS`] UTF-16 units.
fn object_name(app_id: &str, what: &str) -> Vec<u16> {
    let mut name: Vec<u16> = "Local\\mdshared-single.".encode_utf16().collect();
    name.extend(
        app_id
            .encode_utf16()
            .map(|u| {
                if u == u16::from(b'\\') {
                    u16::from(b'_')
                } else {
                    u
                }
            })
            .take(MAX_ID_UNITS),
    );
    name.push(u16::from(b'.'));
    name.extend(what.encode_utf16());
    name.push(0);
    name
}

/// The UTF-16 units of a hand-over: the id, then each argument, each followed by a NUL. `None`
/// when one of them holds a NUL or the whole is longer than [`MAX_BYTES`].
fn encode(app_id: &str, args: &[OsString]) -> Option<Vec<u16>> {
    let mut units: Vec<u16> = Vec::new();
    let pieces = std::iter::once(OsStr::new(app_id)).chain(args.iter().map(OsString::as_os_str));
    for piece in pieces {
        let start = units.len();
        units.extend(piece.encode_wide());
        if units[start..].contains(&0) {
            return None;
        }
        units.push(0);
        if units.len() * 2 > MAX_BYTES {
            return None;
        }
    }
    Some(units)
}

/// The arguments of the bytes of a hand-over (UTF-16, little-endian, as [`encode`] makes them)
/// of the application `app_id`. `None` for an odd length, a missing final NUL, another id or more
/// than [`MAX_BYTES`].
fn decode(app_id: &str, bytes: &[u8]) -> Option<Vec<OsString>> {
    if !bytes.len().is_multiple_of(2) || bytes.len() > MAX_BYTES {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    let (&last, body) = units.split_last()?;
    if last != 0 {
        return None;
    }
    let mut pieces = body.split(|&u| u == 0);
    let id = pieces.next()?;
    if !id.iter().copied().eq(app_id.encode_utf16()) {
        return None;
    }
    Some(pieces.map(OsString::from_wide).collect())
}

#[cfg(test)]
#[path = "single_mutex_tests.rs"]
mod tests;
