//! Tests of `single_mutex`. Most run in one process: the mutex and its record are seen by a
//! second `acquire` exactly as by a second process. One starts the test program again as a
//! second process. The windows are the test's own: hidden ones, and for the tests that need a
//! window that counts as visible, one that is never drawn, is off the screen and has no taskbar
//! button, so nothing shows while the tests run.

use super::*;
use std::cell::RefCell;
use std::sync::mpsc::{self, RecvTimeoutError};
use windows_sys::Win32::Foundation::{LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_POPUP, WS_VISIBLE,
};

/// An id no other test, and no other run of the tests, uses.
fn id(name: &str) -> String {
    format!("mdshared-test.{}.{name}", std::process::id())
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn running(instance: Instance) -> Running {
    match instance {
        Instance::Running(running) => running,
        Instance::First(_) => panic!("expected another instance to be running"),
    }
}

fn first(instance: Instance) -> Guard {
    match instance {
        Instance::First(guard) => guard,
        Instance::Running(_) => panic!("expected the first instance"),
    }
}

/// The id the test window's procedure accepts hand-overs of.
const WINDOW_APP: &str = "mdshared-test-window";

thread_local! {
    /// What the test window's procedure was handed.
    static HANDED: RefCell<Vec<Vec<OsString>>> = const { RefCell::new(Vec::new()) };
}

/// The test window's procedure: what an application's procedure does with a hand-over.
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: the values are the ones this procedure was called with.
    if let Some(args) = unsafe { received(WINDOW_APP, msg, l) } {
        HANDED.with(|h| h.borrow_mut().push(args));
        return 1;
    }
    // SAFETY: the message is passed on as it came.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn handed() -> Vec<Vec<OsString>> {
    HANDED.with(|h| std::mem::take(&mut *h.borrow_mut()))
}

/// A hidden top-level window of this thread, with a class of its own.
fn window() -> isize {
    make_window(false)
}

/// A top-level window of this thread that Windows counts as visible (`WS_VISIBLE`, no owner, not
/// a tool window): the kind [`main_window_of`] picks when it looks through the windows of a
/// process. No person sees it: it is layered and never given its contents or its alpha, which
/// Windows takes for "not drawn yet"; it is one pixel at (-32000, -32000), off every screen; and
/// `WS_EX_NOACTIVATE` keeps it from the taskbar and from taking the focus.
fn shown_window() -> isize {
    make_window(true)
}

fn make_window(visible: bool) -> isize {
    let name: Vec<u16> = "mdshared_single_mutex_test\0".encode_utf16().collect();
    let class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(procedure),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: std::ptr::null_mut(),
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: name.as_ptr(),
    };
    let (style, ex_style, at, side) = if visible {
        (
            WS_POPUP | WS_VISIBLE,
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            -32000,
            1,
        )
    } else {
        (WS_POPUP, 0, 0, 100)
    };
    // SAFETY: `class` and the name outlive the calls; registering twice (another test in the same
    // process) fails harmlessly and the class is already there.
    let hwnd = unsafe {
        RegisterClassW(&class);
        CreateWindowExW(
            ex_style,
            name.as_ptr(),
            name.as_ptr(),
            style,
            at,
            at,
            side,
            side,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null(), "the test window was not created");
    hwnd as isize
}

fn destroy(hwnd: isize) {
    // SAFETY: a window of this thread that the test made.
    unsafe { DestroyWindow(hwnd as HWND) };
}

/// Held by a test that depends on which visible windows this process has: there is only one test
/// process, and a visible window of one test is a window of every other.
static LOOKING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn looking() -> std::sync::MutexGuard<'static, ()> {
    LOOKING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The bytes `send` hands Windows, given to `received` as a window procedure would get them.
fn through_copydata(app_id: &str, tag: usize, bytes: &[u8]) -> Option<Vec<OsString>> {
    let data = COPYDATASTRUCT {
        dwData: tag,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut core::ffi::c_void,
    };
    // SAFETY: `data` and its bytes are alive for the call, as Windows keeps them for a procedure.
    unsafe { received(app_id, WM_COPYDATA, &data as *const COPYDATASTRUCT as isize) }
}

fn bytes_of(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

#[test]
fn a_second_launch_of_the_same_id_finds_the_first_running_and_its_process() {
    let app = id("second");
    let guard = first(acquire(&app));
    assert!(guard.held());
    let other = running(acquire(&app));
    // SAFETY: no pointers.
    let me = unsafe { GetCurrentProcessId() };
    assert_eq!(other.pid(), Some(me), "the record names the first instance");
    // A third is told the same, and the first still holds it.
    assert_eq!(running(acquire(&app)).pid(), Some(me));
    // Another id is another application.
    assert!(first(acquire(&id("second.other"))).held());
}

#[test]
fn after_the_first_is_dropped_the_next_launch_is_first() {
    let app = id("dropped");
    let guard = first(acquire(&app));
    let other = running(acquire(&app));
    drop(guard);
    assert_eq!(other.pid(), None, "the record went with the guard");
    assert_eq!(other.window(), None);
    let again = first(acquire(&app));
    assert!(again.held());
    // The new first instance's record is whole again.
    assert!(running(acquire(&app)).pid().is_some());
    drop(again);
    assert!(first(acquire(&app)).held());
}

#[test]
fn a_record_that_was_already_there_is_not_written_into() {
    let app = id("squat");
    let name = object_name(&app, "window");
    // The record a squatter made first: a mapping that is never written whole, so every launch
    // that finds it learns nothing.
    // SAFETY: a mapping backed by the paging file (`INVALID_HANDLE_VALUE`) of `RECORD` bytes,
    // named by a NUL-terminated string that outlives the call; default security. The handle is
    // closed at the end of the test.
    let squatter = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            std::ptr::null(),
            PAGE_READWRITE,
            0,
            RECORD as u32,
            name.as_ptr(),
        )
    };
    assert!(!squatter.is_null(), "the squatter's mapping was not made");
    let started = Instant::now();
    let guard = first(acquire(&app));
    assert!(
        started.elapsed() >= RECORD_PATIENCE,
        "it gave up on the record's name after {:?}, before its patience ran out",
        started.elapsed()
    );
    assert!(guard.held(), "the mutex is still this instance's");
    // The window stays unpublished: the record is the squatter's, and this instance left it alone.
    let hwnd = window();
    guard.publish_window(hwnd);
    // SAFETY: `squatter` is open until the end of the test; the view is page-aligned and `RECORD`
    // bytes long, which the three atomics fit, and it is unmapped below.
    unsafe {
        let view = MapViewOfFile(squatter, FILE_MAP_READ, 0, 0, RECORD);
        assert_ne!(view.Value as usize, 0);
        let (magic, pid, window) = record_fields(view.Value as usize);
        assert_eq!(magic.load(Ordering::Relaxed), 0, "no record was written");
        assert_eq!(pid.load(Ordering::Relaxed), 0);
        assert_eq!(window.load(Ordering::Relaxed), 0);
        UnmapViewOfFile(view);
    }
    let other = running(acquire(&app));
    assert_eq!(
        other.pid(),
        None,
        "a launch finds no record of this instance"
    );
    assert_eq!(other.published_window(), None);

    drop(guard);
    destroy(hwnd);
    // SAFETY: the handle was opened here and nothing else holds it.
    unsafe { CloseHandle(squatter) };
    // With the squatter's mapping gone, the next launch is the first instance, with a record.
    let again = first(acquire(&app));
    assert!(running(acquire(&app)).pid().is_some());
    drop(again);
}

/// The record's name taken for a moment, as by the instance before this one that has closed its
/// mutex but not yet its mapping, or by a second launch reading the record: the first instance
/// tries again and writes its record once the name is free.
#[test]
fn a_record_name_held_for_a_moment_is_tried_again_and_written() {
    let app = id("moment");
    let name = object_name(&app, "window");
    // SAFETY: as in `a_record_that_was_already_there_is_not_written_into`; the handle is closed
    // below, while the other thread's `acquire` tries again.
    let lingering = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            std::ptr::null(),
            PAGE_READWRITE,
            0,
            RECORD as u32,
            name.as_ptr(),
        )
    };
    assert!(!lingering.is_null(), "the lingering mapping was not made");
    let mutex_name = object_name(&app, "mutex");
    let (tx, rx) = mpsc::channel();
    let launch = {
        let app = app.clone();
        std::thread::spawn(move || {
            let guard = first(acquire(&app));
            let ok = guard.held() && guard.view != 0;
            tx.send(()).unwrap();
            // The guard lives until the main thread has looked at the record.
            std::thread::park();
            ok
        })
    };
    // Once the mutex is there, the launch is in `create_record`, finding the name taken.
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        // SAFETY: a NUL-terminated name that outlives the call; the handle is closed at once.
        let mutex = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, mutex_name.as_ptr()) };
        if !mutex.is_null() {
            // SAFETY: the handle was opened here and nothing else holds it.
            unsafe { CloseHandle(mutex) };
            break;
        }
        assert!(Instant::now() < end, "the launch never took the mutex");
        std::thread::yield_now();
    }
    // It keeps trying while the name is taken; a tenth of it is watched here, then the name
    // is freed well within its patience.
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(10)),
        Err(RecvTimeoutError::Timeout),
        "the launch gave up on the record's name at once"
    );
    // SAFETY: the handle was opened here and nothing else holds it.
    unsafe { CloseHandle(lingering) };
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the launch did not return");
    // SAFETY: no pointers.
    let me = unsafe { GetCurrentProcessId() };
    assert_eq!(
        running(acquire(&app)).pid(),
        Some(me),
        "the record was written once the name was free"
    );
    launch.thread().unpark();
    assert!(launch.join().unwrap(), "the launch has a record of its own");
}

#[test]
fn odd_ids_still_name_one_instance() {
    let long = "x".repeat(1000);
    for app in [
        format!("{}\\with\\backslashes", id("odd")),
        format!("{}{long}", id("long")),
        format!("{} \u{e9}\u{1f600} spaces", id("unicode")),
    ] {
        let guard = first(acquire(&app));
        assert!(guard.held(), "{app:?}");
        running(acquire(&app));
        drop(guard);
        assert!(first(acquire(&app)).held(), "{app:?}");
    }
}

#[test]
fn object_names_are_in_the_session_and_within_the_limit() {
    let name = String::from_utf16(&object_name("a\\b", "mutex")).unwrap();
    assert_eq!(name, "Local\\mdshared-single.a_b.mutex\0");
    let long = object_name(&"y".repeat(5000), "window");
    assert!(long.len() <= 260, "{}", long.len());
    // Ids that differ only past the cut are one id; ids that differ before it are not.
    assert_eq!(
        object_name(&format!("{}1", "z".repeat(300)), "mutex"),
        object_name(&format!("{}2", "z".repeat(300)), "mutex")
    );
    assert_ne!(object_name("a", "mutex"), object_name("b", "mutex"));
    assert_ne!(object_name("a", "mutex"), object_name("a", "window"));
    // The empty id names one object like any other id; it is tested here only, because through
    // `acquire` it would be one instance for the whole machine, and two test runs would race.
    assert_eq!(
        String::from_utf16(&object_name("", "mutex")).unwrap(),
        "Local\\mdshared-single..mutex\0"
    );
    assert_eq!(
        String::from_utf16(&object_name("", "window")).unwrap(),
        "Local\\mdshared-single..window\0"
    );
}

#[test]
fn the_hand_over_round_trips_through_a_copydata_structure() {
    // An unpaired surrogate is not Unicode, and a Windows command line may still hold one.
    let odd = OsString::from_wide(&[0x61, 0xD800, 0x62]);
    let cases: Vec<Vec<OsString>> = vec![
        Vec::new(),
        vec![OsString::new()],
        os(&["C:\\Program Files\\app\\x.exe", "--flag", "a b \"c\""]),
        os(&["", "", "middle", ""]),
        os(&["\u{e9}t\u{e9} \u{1f600}"]),
        vec![odd.clone(), OsString::from("after")],
    ];
    for args in &cases {
        let units = encode("app", args).expect("encodes");
        let back = through_copydata("app", TAG, &bytes_of(&units));
        assert_eq!(back.as_ref(), Some(args), "{args:?}");
        assert_eq!(decode("app", &bytes_of(&units)).as_ref(), Some(args));
    }
    // An empty id is an id.
    let units = encode("", &os(&["x"])).unwrap();
    assert_eq!(
        through_copydata("", TAG, &bytes_of(&units)),
        Some(os(&["x"]))
    );
    assert_eq!(through_copydata("app", TAG, &bytes_of(&units)), None);
}

#[test]
fn what_is_not_a_hand_over_of_this_application_gives_nothing() {
    let units = encode("app", &os(&["a"])).unwrap();
    let bytes = bytes_of(&units);
    assert_eq!(through_copydata("other", TAG, &bytes), None, "another id");
    assert_eq!(
        through_copydata("ap", TAG, &bytes),
        None,
        "a prefix of the id"
    );
    assert_eq!(
        through_copydata("app", TAG + 1, &bytes),
        None,
        "another tag"
    );
    assert_eq!(
        through_copydata("app", TAG, &bytes[1..]),
        None,
        "odd length"
    );
    assert_eq!(
        through_copydata("app", TAG, &bytes[..bytes.len() - 2]),
        None,
        "no final NUL"
    );
    assert_eq!(through_copydata("app", TAG, &[]), None, "no bytes at all");
    // Another message, or no structure, is not looked at.
    // SAFETY: neither pair is dereferenced: the message is not WM_COPYDATA, or lparam is 0.
    unsafe {
        assert_eq!(received("app", WM_COPYDATA + 1, 12345), None);
        assert_eq!(received("app", WM_COPYDATA, 0), None);
    }
    // A structure that claims more than the limit is refused before its bytes are read.
    let data = COPYDATASTRUCT {
        dwData: TAG,
        cbData: (MAX_BYTES + 2) as u32,
        lpData: std::ptr::null_mut(),
    };
    // SAFETY: a live structure; its null data pointer is never read, the length is refused first.
    let got = unsafe { received("app", WM_COPYDATA, &data as *const COPYDATASTRUCT as isize) };
    assert_eq!(got, None);
    // A NUL inside an argument or the id, and too much in all, are not encoded.
    assert_eq!(encode("app", &os(&["a\0b"])), None);
    assert_eq!(encode("a\0pp", &[]), None);
    let fits = "a".repeat(MAX_BYTES / 2 - "app".len() - 2);
    assert!(encode("app", &os(&[&fits])).is_some(), "exactly the limit");
    let over = format!("{fits}a");
    assert_eq!(encode("app", &os(&[&over])), None, "one unit over");
    assert_eq!(decode("app", &vec![0; MAX_BYTES + 2]), None);
}

#[test]
fn a_hand_over_sent_to_a_window_reaches_its_procedure() {
    let hwnd = window();
    handed();
    let args = os(&["--open", "C:\\x y\\\u{e9}.txt", ""]);
    assert!(send(WINDOW_APP, hwnd, &args), "the procedure took it");
    assert_eq!(handed(), vec![args]);
    assert!(send(WINDOW_APP, hwnd, &[]), "a launch with no arguments");
    assert_eq!(handed(), vec![Vec::<OsString>::new()]);
    // Another application's hand-over is not taken, and nothing is sent that cannot be encoded.
    assert!(!send("another-app", hwnd, &os(&["x"])));
    assert!(!send(WINDOW_APP, hwnd, &os(&["a\0b"])));
    assert!(handed().is_empty());
    destroy(hwnd);
    assert!(!send(WINDOW_APP, hwnd, &os(&["x"])), "the window is gone");
    assert!(!send(WINDOW_APP, 0, &os(&["x"])));
}

#[test]
fn the_window_of_this_process_is_found_published_or_among_its_windows() {
    let app = id("window");
    // SAFETY: no pointers.
    let me = unsafe { GetCurrentProcessId() };
    let hwnd = window();
    assert!(
        windows_of(me).contains(&hwnd),
        "a hidden top-level window is listed"
    );
    assert_ne!(
        main_window_of(me),
        Some(hwnd),
        "but never picked by looking"
    );

    let guard = first(acquire(&app));
    let other = running(acquire(&app));
    assert_ne!(other.window(), Some(hwnd), "not published yet");
    guard.publish_window(hwnd);
    assert_eq!(
        other.window(),
        Some(hwnd),
        "published, so found though hidden"
    );
    assert_eq!(other.wait_window(Duration::ZERO), Some(hwnd));
    // A hidden window is not brought forward, and nothing is shown.
    assert!(!bring_forward(hwnd));
    // SAFETY: takes a handle by value.
    assert_eq!(unsafe { IsWindowVisible(hwnd as HWND) }, 0);

    // A published window that is gone is not used.
    destroy(hwnd);
    assert!(!windows_of(me).contains(&hwnd));
    assert_ne!(other.window(), Some(hwnd));
    guard.publish_window(0);
    assert_ne!(other.window(), Some(hwnd));
    assert!(!bring_forward(hwnd));
    assert!(!bring_forward(0));
}

#[test]
fn waiting_for_a_window_that_never_comes_ends_at_the_timeout() {
    let _looking = looking();
    let app = id("wait");
    let _guard = first(acquire(&app));
    let other = running(acquire(&app));
    // This test process has no visible window of its own, so the first instance has none.
    // SAFETY: no pointers.
    if main_window_of(unsafe { GetCurrentProcessId() }).is_some() {
        return;
    }
    let started = Instant::now();
    assert_eq!(other.wait_window(Duration::from_millis(60)), None);
    assert!(started.elapsed() >= Duration::from_millis(60));
    assert_eq!(other.wait_window(Duration::ZERO), None);
}

#[test]
fn waiting_gives_the_window_published_late_and_not_a_visible_one_of_the_process() {
    let _looking = looking();
    let app = id("late");
    // A visible window of this process, the kind looking through the process would pick, and the
    // hidden one the first instance publishes 100 ms in.
    let visible = shown_window();
    let late = window();
    let guard = first(acquire(&app));
    let other = running(acquire(&app));
    // SAFETY: no pointers.
    let me = unsafe { GetCurrentProcessId() };
    assert_eq!(
        main_window_of(me),
        Some(visible),
        "the visible window is what looking through the process gives"
    );
    assert_ne!(other.window(), Some(late), "nothing published yet");
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let other = &other;
        scope.spawn(move || tx.send(other.wait_window(Duration::from_secs(10))).unwrap());
        // While nothing is published the waiter keeps waiting: it does not take the visible
        // window. The time limit only says how long this test watches it wait.
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout),
            "the waiter took a window before one was published"
        );
        guard.publish_window(late);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(Some(late)),
            "the published window, not the visible one, and well before the waiter's timeout"
        );
    });
    assert_eq!(other.window(), Some(late), "and still the published one");
    assert_eq!(other.wait_window(Duration::ZERO), Some(late));
    destroy(late);
    destroy(visible);
}

#[test]
fn waiting_ends_at_once_when_the_instance_goes() {
    let app = id("gone");
    let guard = first(acquire(&app));
    let other = running(acquire(&app));
    let (tx, rx) = mpsc::channel();
    let waiter = {
        let other = other.clone();
        std::thread::spawn(move || tx.send(other.wait_window(Duration::from_secs(10))).unwrap())
    };
    // While the instance runs and publishes nothing, the waiter waits.
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(50)),
        Err(RecvTimeoutError::Timeout),
        "the waiter ended while the instance still ran"
    );
    drop(guard);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)),
        Ok(None),
        "the waiter did not end soon after the instance went, well before its timeout"
    );
    waiter.join().unwrap();
    // The instance is gone, so the next acquire is the first instance.
    assert_eq!(other.pid(), None);
    assert!(first(acquire(&app)).held());
}

#[test]
fn no_process_has_no_windows() {
    assert!(windows_of(0).is_empty());
    assert!(windows_of(u32::MAX).is_empty());
    assert_eq!(main_window_of(0), None);
}

/// The environment variable that makes [`as_a_second_process`] act: the app id, a space, the
/// window to expect.
const CHILD: &str = "MDSHARED_SINGLE_MUTEX_CHILD";

/// Does nothing in an ordinary run. Started by [`a_second_process_finds_the_first_and_hands_over`]
/// as a process of its own, it is the second launch: it must find the parent running, find its
/// window and hand it its arguments. It fails (and the process exits non-zero) otherwise.
#[test]
fn as_a_second_process() {
    let Ok(value) = std::env::var(CHILD) else {
        return;
    };
    let (app, expected) = value.split_once(' ').unwrap();
    let expected: isize = expected.parse().unwrap();
    let other = running(acquire(app));
    assert!(other.pid().is_some_and(|pid| pid != std::process::id()));
    let window = other.wait_window(Duration::from_secs(5)).expect("window");
    assert_eq!(window, expected);
    assert!(send(WINDOW_APP, window, &os(&["from", "the child"])));
}

#[test]
fn a_second_process_finds_the_first_and_hands_over() {
    if std::env::var(CHILD).is_ok() {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
    let app = id("process");
    let guard = first(acquire(&app));
    let hwnd = window();
    guard.publish_window(hwnd);
    handed();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "single_mutex::tests::as_a_second_process"])
        .env(CHILD, format!("{app} {hwnd}"))
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // The child's `WM_COPYDATA` is delivered while this thread looks at its messages.
    let end = Instant::now() + Duration::from_secs(20);
    let status = loop {
        // SAFETY: MSG is plain data, for which all zeroes is a value.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `msg` is a live MSG; a null window means any window of this thread.
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {}
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "the second process did not end");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "the second process failed: {status}");
    assert_eq!(handed(), vec![os(&["from", "the child"])]);
    destroy(hwnd);
}
