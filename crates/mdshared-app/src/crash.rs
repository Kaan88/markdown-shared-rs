//! Crash reports. A panic, and on Windows a fault the process cannot survive (an access violation,
//! a stack overflow), writes a short text report into the application's data folder just before
//! the process ends; the next start finds it with [`pending`] and offers it to the user, who may
//! send it by hand. Nothing is sent anywhere.
//!
//! A report holds the application and its version, the system, the thread, the message and the
//! place in the source (or the fault's code and address), and the context the application last
//! set with [`set_context`]. Applications put only kinds and sizes there: never the text of a
//! document or the name of a file. Release builds strip symbols, so no backtrace is taken; the
//! source location of a panic survives stripping.
//!
//! [`install`] costs a few microseconds at start; nothing runs until a crash.
//!
//! A panic that is caught (`catch_unwind`, a thread that ends while the process goes on) writes a
//! report too, since the hook cannot know. An application that ends normally calls
//! [`closed_normally`] last, which deletes the reports this run wrote: it lived through them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Reports kept in the folder, seen or not; older ones are deleted when a new one is written.
const KEEP: usize = 5;

struct Setup {
    app: &'static str,
    version: &'static str,
    dir: PathBuf,
}

static SETUP: OnceLock<Setup> = OnceLock::new();
static CONTEXT: Mutex<String> = Mutex::new(String::new());
/// The reports this run wrote, for [`closed_normally`].
static WRITTEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Starts writing reports for `app` at `version` into `dir` (made when a report is written).
/// Call once, first thing in `main`; a second call changes nothing. The panic hook that was set
/// before still runs after the report is written.
pub fn install(app: &'static str, version: &'static str, dir: PathBuf) {
    if SETUP.set(Setup { app, version, dir }).is_err() {
        return;
    }
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = payload_text(info.payload());
        let place = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        write_report(&format!("panic: {message}"), place.as_deref());
        before(info);
    }));
    #[cfg(windows)]
    windows::install();
}

/// What the application is doing, for the next report: kinds and sizes only (see the module
/// documentation). Cheap; skipped when a report is being written at that moment.
pub fn set_context(text: &str) {
    if let Ok(mut c) = CONTEXT.try_lock()
        && *c != text
    {
        c.clear();
        c.push_str(text);
    }
}

/// The application is ending normally: deletes the reports written by this run, whose panics
/// were caught and lived through. Call it last, after the window and the work are gone.
pub fn closed_normally() {
    let written = match WRITTEN.lock() {
        Ok(mut w) => std::mem::take(&mut *w),
        Err(_) => return,
    };
    for report in written {
        let _ = std::fs::remove_file(report);
    }
}

/// The newest report not yet seen, in `dir`.
pub fn pending(dir: &Path) -> Option<PathBuf> {
    reports(dir)
        .into_iter()
        .filter(|p| !is_seen(p))
        .max_by_key(|p| p.file_name().map(|n| n.to_os_string()))
}

/// Marks a report seen: it stays in the folder (renamed `….seen.txt`) and is not offered again.
/// Returns the report's new path.
pub fn mark_seen(report: &Path) -> std::io::Result<PathBuf> {
    let name = report
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".txt"))
        .ok_or_else(|| std::io::Error::other("not a crash report"))?;
    let seen = report.with_file_name(format!("{name}.seen.txt"));
    std::fs::rename(report, &seen)?;
    Ok(seen)
}

/// The text of a report. `what` is the first line ("panic: …", "fault: …"); `place` the source
/// location of a panic; `when` seconds since 1970.
pub fn report_text(
    app: &str,
    version: &str,
    what: &str,
    place: Option<&str>,
    thread: &str,
    context: &str,
    when: u64,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{app} {version} closed because of an error.");
    let _ = writeln!(s);
    let _ = writeln!(s, "what:    {what}");
    if let Some(p) = place {
        let _ = writeln!(s, "where:   {p}");
    }
    let _ = writeln!(s, "thread:  {thread}");
    let _ = writeln!(s, "when:    {} UTC", utc(when));
    let _ = writeln!(
        s,
        "system:  {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if !context.is_empty() {
        let _ = writeln!(s, "doing:   {context}");
    }
    s
}

fn write_report(what: &str, place: Option<&str>) {
    let Some(setup) = SETUP.get() else { return };
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("unnamed");
    let context = CONTEXT.try_lock().map(|c| c.clone()).unwrap_or_default();
    let when = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let text = report_text(
        setup.app,
        setup.version,
        what,
        place,
        thread,
        &context,
        when,
    );
    if let Ok(path) = save(&setup.dir, &text, when)
        && let Ok(mut w) = WRITTEN.try_lock()
    {
        w.push(path);
    }
}

/// Writes `crash-<seconds>.txt` (a suffix when that name is taken) and deletes the oldest
/// reports beyond [`KEEP`].
fn save(dir: &Path, text: &str, when: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("crash-{when:011}.txt"));
    let mut n = 1;
    while path.exists() || seen_name(&path).exists() {
        path = dir.join(format!("crash-{when:011}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    let mut all = reports(dir);
    all.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    let extra = all.len().saturating_sub(KEEP);
    for old in all.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

fn seen_name(path: &Path) -> PathBuf {
    let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
    path.with_file_name(format!("{name}.seen.txt"))
}

/// Every report in `dir`, seen or not.
fn reports(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect()
}

fn is_seen(path: &Path) -> bool {
    path.to_str().is_some_and(|p| p.ends_with(".seen.txt"))
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message)".to_string()
    }
}

/// `YYYY-MM-DD HH:MM:SS` for seconds since 1970 (UTC; the civil-from-days algorithm).
fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Windows: faults that end the process without a panic (an access violation in a system
/// library, a stack overflow) reach the unhandled-exception filter, which writes a report with
/// the fault's code and its address as an offset into the module, then lets Windows end the
/// process as before.
#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{EXCEPTION_ACCESS_VIOLATION, EXCEPTION_STACK_OVERFLOW};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    /// Lets the system go on to end the process.
    const CONTINUE_SEARCH: i32 = 0;

    pub(super) fn install() {
        // SAFETY: the filter is a plain function that lives as long as the process.
        unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: the system passes valid pointers for the duration of the call.
        let (code, address) = unsafe {
            info.as_ref()
                .and_then(|i| i.ExceptionRecord.as_ref())
                .map_or((0, 0), |r| (r.ExceptionCode, r.ExceptionAddress as usize))
        };
        // SAFETY: a null name asks for this executable's own module.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let name = match code {
            EXCEPTION_ACCESS_VIOLATION => " (access violation)",
            EXCEPTION_STACK_OVERFLOW => " (stack overflow)",
            _ => "",
        };
        let at = if base != 0 && address >= base {
            format!("exe+0x{:x}", address - base)
        } else {
            format!("0x{address:x}")
        };
        let what = format!("fault: code 0x{:08x}{name} at {at}", code as u32);
        super::write_report(&what, None);
        CONTINUE_SEARCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdshared-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_report_says_what_where_and_when_without_more() {
        let text = report_text(
            "mdedit",
            "0.8.0",
            "panic: index out of bounds",
            Some("src/view.rs:10:5"),
            "preview",
            "Markdown, 120 KB, preview pane open",
            1_763_300_000,
        );
        assert!(text.starts_with("mdedit 0.8.0 closed because of an error.\n"));
        assert!(text.contains("what:    panic: index out of bounds\n"));
        assert!(text.contains("where:   src/view.rs:10:5\n"));
        assert!(text.contains("thread:  preview\n"));
        assert!(text.contains("when:    2025-11-16 13:33:20 UTC\n"));
        assert!(text.contains("doing:   Markdown, 120 KB, preview pane open\n"));
        let bare = report_text("mdview", "1", "fault: code 0xc0000005", None, "main", "", 0);
        assert!(!bare.contains("where:") && !bare.contains("doing:"));
        assert!(bare.contains("1970-01-01 00:00:00 UTC"));
    }

    #[test]
    fn dates_across_leap_years() {
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(utc(4_107_542_399), "2100-02-28 23:59:59");
        assert_eq!(utc(1_709_164_800), "2024-02-29 00:00:00");
    }

    #[test]
    fn the_newest_unseen_report_is_pending_until_seen_and_five_are_kept() {
        let dir = scratch("keep");
        assert_eq!(pending(&dir), None);
        for when in 1..=7u64 {
            save(&dir, "r", when).unwrap();
        }
        assert_eq!(reports(&dir).len(), KEEP);
        let newest = pending(&dir).unwrap();
        assert!(newest.ends_with("crash-00000000007.txt"));
        let seen = mark_seen(&newest).unwrap();
        assert!(seen.ends_with("crash-00000000007.seen.txt"));
        assert!(pending(&dir).unwrap().ends_with("crash-00000000006.txt"));
        // The same second again: another name, not the seen report overwritten.
        let again = save(&dir, "r2", 7).unwrap();
        assert!(again.ends_with("crash-00000000007-1.txt"));
        assert_eq!(std::fs::read_to_string(&seen).unwrap(), "r");
        assert_eq!(reports(&dir).len(), KEEP);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panic_writes_a_report_and_the_old_hook_still_runs() {
        let dir = scratch("hook");
        install("testapp", "9.9", dir.clone());
        set_context("Markdown, 3 KB");
        let t = std::thread::Builder::new()
            .name("worker".into())
            .spawn(|| panic!("boom {}", 42))
            .unwrap();
        assert!(t.join().is_err());
        let report = pending(&dir).expect("a report");
        let text = std::fs::read_to_string(&report).unwrap();
        assert!(
            text.starts_with("testapp 9.9 closed because of an error.\n"),
            "{text}"
        );
        assert!(text.contains("what:    panic: boom 42\n"), "{text}");
        assert!(
            text.contains("where:   crates/mdshared-app/src/crash.rs:"),
            "{text}"
        );
        assert!(text.contains("thread:  worker\n"), "{text}");
        assert!(text.contains("doing:   Markdown, 3 KB\n"), "{text}");
        // The process lived through it and ends normally: the report goes.
        closed_normally();
        assert!(!report.exists());
        assert_eq!(pending(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
