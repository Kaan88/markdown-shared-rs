//! Shows a few notifications and prints what the person does with them:
//! `cargo run -p mdshared-ui --features notify --example notify -- [seconds]`.
//!
//! It shows "Now playing" with Previous and Next buttons, replaces it a second later with the
//! next track (same tag: one toast, not two), and shows a quiet "Copy finished". Click a toast,
//! press its buttons or close it; each event is printed. It exits after the given seconds
//! (default 30). Without the feature it prints why there are no notifications.
//!
//! On Windows this writes `HKEY_CURRENT_USER\Software\Classes\AppUserModelId\Mdshared.Example`
//! (the name the toasts show); remove it with
//! `reg delete HKCU\Software\Classes\AppUserModelId\Mdshared.Example /f`.

use std::time::{Duration, Instant};

use mdshared_ui::notify::{Identity, Notification, Notifier, Urgency};

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let identity = Identity {
        name: "mdshared example",
        app_id: "Mdshared.Example",
        icon: None,
    };
    let mut notifier = match Notifier::new(&identity, || {}) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("no notifications: {e}");
            return;
        }
    };
    let now_playing = |track: &str| {
        Notification::new("Now playing", track)
            .action("previous", "Previous")
            .action("next", "Next")
            .tagged("now playing")
    };
    match notifier.show(&now_playing("First Song — Some Artist")) {
        Ok(id) => println!("shown {id:?}"),
        Err(e) => eprintln!("show: {e}"),
    }
    std::thread::sleep(Duration::from_secs(1));
    match notifier.show(&now_playing("Second Song — Some Artist")) {
        Ok(id) => println!("shown {id:?} in its place"),
        Err(e) => eprintln!("show: {e}"),
    }
    let mut done = Notification::new("Copy finished", "12 files to Music");
    done.urgency = Urgency::Low;
    match notifier.show(&done) {
        Ok(id) => println!("shown {id:?}"),
        Err(e) => eprintln!("show: {e}"),
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        for event in notifier.events() {
            println!("{event:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
