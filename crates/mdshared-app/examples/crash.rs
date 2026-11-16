//! Crashes on purpose, for checking crash reports by hand or in CI:
//!
//!     cargo run -p mdshared-app --example crash -- <dir> panic|fault
//!
//! "panic" panics on a thread named "preview"; "fault" writes through a null pointer (on Windows
//! the unhandled-exception filter reports it; elsewhere the process just dies). The report goes
//! into <dir>.

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("a folder for the report"));
    let how = args.next().unwrap_or_else(|| "panic".into());
    mdshared_app::crash::install("crash-example", "1.0", dir);
    mdshared_app::crash::set_context("on purpose");
    match how.as_str() {
        "fault" => {
            let p = std::hint::black_box(std::ptr::null_mut::<u32>());
            // SAFETY: none; this is the crash being tested.
            unsafe { p.write_volatile(1) };
        }
        _ => {
            let _ = std::thread::Builder::new()
                .name("preview".into())
                .spawn(|| panic!("on purpose"))
                .unwrap()
                .join();
            std::process::exit(1);
        }
    }
}
