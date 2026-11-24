//! Puts an icon in the system's tray and prints what the person does with it:
//! `cargo run -p mdshared-ui --example tray -- [seconds]`.
//!
//! Click the icon, middle-click it, or open its menu (a right click) and choose an item; each
//! event is printed. "Shuffle" turns its tick on and off, which changes the menu; the tooltip
//! counts the events. It exits after the given seconds (default 60), or when "Quit" is chosen.

use std::time::{Duration, Instant};

use mdshared_text::image::Image;
use mdshared_ui::menu::Item;
use mdshared_ui::tray::{Event, Identity, Tray};

const PLAY: u32 = 1;
const SHUFFLE: u32 = 2;
const QUIT: u32 = 3;

/// A round note-blue icon, 32 px.
fn icon() -> Image {
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let inside = (dx * dx + dy * dy).sqrt() <= 15.0;
            rgba.extend_from_slice(if inside {
                &[30, 120, 220, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    Image::new(size, size, rgba).expect("the icon's size matches its pixels")
}

fn menu(shuffle: bool) -> Vec<Item<u32>> {
    vec![
        Item::new(PLAY, "&Play / Pause").hint("Space"),
        Item::new(SHUFFLE, "&Shuffle").checked(shuffle),
        Item {
            separator_before: true,
            ..Item::new(QUIT, "&Quit")
        },
    ]
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let identity = Identity {
        name: "mdshared example",
        id: "mdshared_example",
    };
    let mut tray = match Tray::new(&identity, &icon(), || {}) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("no tray icon: {e}");
            return;
        }
    };
    let mut shuffle = false;
    if let Err(e) = tray.set_menu(&menu(shuffle)) {
        eprintln!("menu: {e}");
    }
    _ = tray.set_tooltip("mdshared example: no events yet");
    println!("the icon is in the tray");
    let (end, mut count) = (Instant::now() + Duration::from_secs(seconds), 0);
    while Instant::now() < end {
        for event in tray.events() {
            println!("{event:?}");
            count += 1;
            _ = tray.set_tooltip(&format!("mdshared example: {count} events"));
            match event {
                Event::Chosen(QUIT) => return,
                Event::Chosen(SHUFFLE) => {
                    shuffle = !shuffle;
                    _ = tray.set_menu(&menu(shuffle));
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
