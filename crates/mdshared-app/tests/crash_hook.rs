//! The crash hook, in a test program of its own. The hook is the whole process's: in the
//! library's tests, any test that failed beside this one (a panic) wrote its report into this
//! test's folder, and this test read that report instead of its own (review of the audit 4
//! fixes). Keep this file to the one test.

use mdshared_app::crash::{closed_normally, install, pending, set_context};

#[test]
fn a_panic_writes_a_report_and_the_old_hook_still_runs() {
    let dir = std::env::temp_dir().join(format!("mdshared-crash-hook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
        // Windows writes the path with backslashes.
        text.replace('\\', "/")
            .contains("where:   crates/mdshared-app/tests/crash_hook.rs:"),
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
