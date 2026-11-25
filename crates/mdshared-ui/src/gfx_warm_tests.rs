//! [`Gfx::warm_fonts`]: the primed context lays out as the cold one does.

use super::*;

const TEXTS: &[&str] = &[
    "Playback stopped.",
    "\u{2630} Menu",
    "Artist \u{2014} Album 3:45",
    "W",
];

fn widths(gfx: &mut Gfx) -> Vec<f32> {
    let mut out = Vec::new();
    for face in [Face::Ui, Face::Semibold, Face::Mono] {
        for size in [13.0, 19.5] {
            for t in TEXTS {
                out.push(gfx.layout(face, size, [0, 0, 0], 0.0, t).width());
            }
        }
    }
    out
}

/// A context primed as the warm thread primes it measures every face as a cold one does.
#[test]
fn a_primed_context_gives_the_same_widths_as_a_cold_one() {
    let cold = widths(&mut Gfx::new());
    let mut fonts = shared_fonts();
    prime_faces(&mut fonts);
    let mut gfx = Gfx::new();
    gfx.fonts = Some(fonts);
    assert_eq!(widths(&mut gfx), cold);
}

/// Through the public call: a `Gfx` made on a fresh thread after `warm_fonts` lays out the same
/// widths, and a second call starts nothing (`mdshared_text`'s own tests count the builds).
#[test]
fn a_gfx_after_warm_fonts_measures_as_before() {
    let cold = std::thread::spawn(|| widths(&mut Gfx::new()))
        .join()
        .unwrap();
    Gfx::warm_fonts();
    Gfx::warm_fonts();
    let warmed = std::thread::spawn(|| widths(&mut Gfx::new()))
        .join()
        .unwrap();
    assert_eq!(warmed, cold);
}

/// The first measure in the UI face on a fresh thread: cold, and after a finished warm. Run
/// alone, in release:
/// `cargo test -p mdshared-ui --release --lib -- --ignored --nocapture first_measure_timing`
#[test]
#[ignore]
fn first_measure_timing() {
    let first = || {
        std::thread::spawn(|| {
            let t = std::time::Instant::now();
            let mut gfx = Gfx::new();
            let made = t.elapsed();
            gfx.layout(Face::Ui, 13.0, [0, 0, 0], 0.0, "Playback stopped.");
            let measured = t.elapsed() - made;
            let t = std::time::Instant::now();
            gfx.layout(Face::Ui, 13.0, [0, 0, 0], 0.0, "\u{2630}");
            (made, measured, t.elapsed())
        })
        .join()
        .unwrap()
    };
    let cold = first();
    Gfx::warm_fonts();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let warm = first();
    println!(
        "cold: Gfx::new {:?}, first measure {:?}, first symbol {:?}",
        cold.0, cold.1, cold.2
    );
    println!(
        "warm: Gfx::new {:?}, first measure {:?}, first symbol {:?}",
        warm.0, warm.1, warm.2
    );
}
