//! Opens one dialog through the async API and prints the result: `cargo run -p mdshared-ui
//! --example dialog -- [files|folder|save]`. Also tries to open a second dialog at once, which
//! must be refused.

use std::sync::mpsc;
use std::time::Duration;

use mdshared_ui::platform;

fn main() {
    let kind = std::env::args().nth(1).unwrap_or_else(|| "files".into());
    let (tx, rx) = mpsc::channel();
    let start = std::env::temp_dir();
    let t = tx.clone();
    let started = match kind.as_str() {
        "folder" => platform::pick_folder_async(Some(&start), move |r| {
            let _ = t.send(format!("{r:?}"));
        }),
        "save" => platform::pick_save_async(&start.join("untitled.md"), move |r| {
            let _ = t.send(format!("{r:?}"));
        }),
        _ => platform::pick_files_async(Some(&start), "Markdown", &["md"], move |r| {
            let _ = t.send(format!("{r:?}"));
        }),
    };
    println!(
        "started: {started}; dialog_open: {}",
        platform::dialog_open()
    );
    let second = platform::pick_folder_async(None, move |r| {
        let _ = tx.send(format!("second dialog's result: {r:?}"));
    });
    println!("second request started: {second} (must be false)");
    // The refused request's `done` ran at once; then wait for the real dialog.
    println!("{}", rx.recv_timeout(Duration::from_secs(1)).unwrap());
    println!("waiting for the dialog to be closed...");
    println!("result: {}", rx.recv().unwrap());
}
