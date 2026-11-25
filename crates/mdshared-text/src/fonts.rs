use parley::FontContext;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

thread_local! {
    /// System fonts are enumerated once per thread; every renderer starts from a clone of this. A
    /// window makes several renderers (one per tab and a spare for `mem::take`), and enumerating
    /// fonts for each cost about 35 ms apiece at launch (measured again on 2026-11-25 on
    /// Windows: about 1 ms in release, a few ms with the first layout; docs/FEATURES.md).
    static FONTS: std::cell::RefCell<Option<FontContext>> = const { std::cell::RefCell::new(None) };
}

/// The per-thread font context every renderer clones. A shell that draws its own text takes a
/// clone too, so system fonts are still enumerated once.
///
/// The context has [`add_fallbacks`] applied on every system, Windows included (the viewer drew
/// U+2714 and U+2764 as boxes on Windows without it, where the editor already applied it).
/// Applying it again on a clone changes nothing.
///
/// After [`warm_fonts`], the first call on a thread that has none yet takes the context built in
/// the background (waiting for it if it is still being built) instead of enumerating again.
pub fn shared_fonts() -> FontContext {
    FONTS.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| WARM.take().unwrap_or_else(build_fonts))
            .clone()
    })
}

/// Starts building the font context on a background thread, for a window that wants its first
/// paint not to wait for it: call it when the window is created, before the first
/// [`shared_fonts`]. Building the context is cheap (fontique is lazy, about 1 ms); the first
/// text laid out in a family is what costs (18-21 ms for the UI face, mdplay measured), so a
/// caller that knows its families uses [`warm_fonts_with`], which lays them out there too.
/// `mdshared_ui::gfx::Gfx::warm_fonts` does that for the toolkit's faces. The next `shared_fonts` on a thread that has no context yet takes the one
/// built there, waiting for it if it is still being built. Only that one thread takes it; any
/// other thread enumerates for itself as before.
///
/// At most once per process: a second call does nothing. If the thread cannot be started or
/// panics, `shared_fonts` builds on the calling thread as it does without this; the panic stays
/// on that thread and never unwinds into the caller. Without a call nothing changes.
pub fn warm_fonts() {
    WARM.start(build_fonts, |_| {});
}

/// [`warm_fonts`], then `prime` on the built context on the same thread: lay out a sample in
/// each family the first paint uses, so the families are resolved and their fonts loaded before
/// the context is handed over. What the priming loads stays with the context and its clones.
/// A panic in `prime` is caught there like one in the build, and `shared_fonts` then builds on
/// the calling thread. Only the first of `warm_fonts` and `warm_fonts_with` in a process starts.
pub fn warm_fonts_with(prime: fn(&mut FontContext)) {
    WARM.start(build_fonts, prime);
}

/// True while a context started by [`warm_fonts`] waits to be taken, built or not: a caller
/// that makes its renderer early (`Gfx::new` in the window's creation) can then take the
/// context at its first text instead, so the build and priming run while the rest of the
/// window is made.
pub fn fonts_warming() -> bool {
    WARM.waiting()
}

/// The context [`shared_fonts`] keeps: the system's fonts with [`add_fallbacks`] applied.
fn build_fonts() -> FontContext {
    let mut fonts = FontContext::new();
    add_fallbacks(&mut fonts);
    fonts
}

static WARM: Warm = Warm::new();

/// A font context built on a background thread, taken once ([`warm_fonts`]).
struct Warm {
    started: AtomicBool,
    built: Mutex<Option<Receiver<FontContext>>>,
}

impl Warm {
    const fn new() -> Warm {
        Warm {
            started: AtomicBool::new(false),
            built: Mutex::new(None),
        }
    }

    /// Starts `build`, then `prime` on what it built, on a thread, the first time only.
    fn start(&self, build: fn() -> FontContext, prime: fn(&mut FontContext)) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let (send, recv) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("mdshared-fonts".into())
            .spawn(move || {
                // A panic drops `send`, and the taker builds for itself.
                let built = std::panic::catch_unwind(|| {
                    let mut fonts = build();
                    prime(&mut fonts);
                    fonts
                });
                if let Ok(fonts) = built {
                    let _ = send.send(fonts);
                }
            });
        if spawned.is_ok() {
            *self.built.lock().unwrap_or_else(|e| e.into_inner()) = Some(recv);
        }
    }

    /// Whether a started context waits to be taken.
    fn waiting(&self) -> bool {
        self.built
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// The context built by [`start`](Self::start), waiting for it; `None` when it was never
    /// started, was taken already, or failed.
    fn take(&self) -> Option<FontContext> {
        // Out of the lock before waiting, so another thread finds it taken at once.
        let recv = self
            .built
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        recv.recv().ok()
    }
}

/// Families tried after the system's fallback for a script, for characters it lacks: the
/// system's Han fallback has no glyph for some simplified characters (U+4F24) and the common
/// script's none for musical symbols (U+1D11E), which then drew as boxes. A line of only
/// common-script characters found its fallback under Latin, not Common, so Latin gets them too.
///
/// The Windows names come first and in their original order; the families of Linux distributions
/// follow, then those of macOS, and a name that is not installed is skipped. On Windows only the Windows names are used.
const EXTRA_FALLBACKS: &[([u8; 4], &[&str])] = &[
    (
        *b"Hani",
        &[
            "Microsoft YaHei",
            "Microsoft JhengHei",
            "SimSun-ExtB",
            #[cfg(not(windows))]
            "Noto Sans CJK SC",
            #[cfg(not(windows))]
            "Noto Sans CJK JP",
            #[cfg(not(windows))]
            "Noto Sans CJK TC",
            #[cfg(not(windows))]
            "Noto Sans CJK KR",
            #[cfg(not(windows))]
            "WenQuanYi Micro Hei",
            #[cfg(not(windows))]
            "Source Han Sans SC",
            #[cfg(target_os = "macos")]
            "PingFang SC",
            #[cfg(target_os = "macos")]
            "PingFang TC",
            #[cfg(target_os = "macos")]
            "Hiragino Sans",
            #[cfg(target_os = "macos")]
            "Apple SD Gothic Neo",
        ],
    ),
    (
        *b"Zyyy",
        &[
            "Segoe UI Symbol",
            "Segoe UI Emoji",
            #[cfg(not(windows))]
            "Noto Color Emoji",
            #[cfg(not(windows))]
            "Twemoji",
            #[cfg(not(windows))]
            "JoyPixels",
            #[cfg(not(windows))]
            "Symbola",
            #[cfg(not(windows))]
            "Noto Sans Symbols2",
            #[cfg(not(windows))]
            "Noto Sans Symbols",
            #[cfg(not(windows))]
            "Noto Sans Math",
            #[cfg(not(windows))]
            "DejaVu Sans",
            #[cfg(target_os = "macos")]
            "Apple Color Emoji",
            #[cfg(target_os = "macos")]
            "Apple Symbols",
            #[cfg(target_os = "macos")]
            "STIX Two Math",
        ],
    ),
    (
        *b"Zinh",
        &[
            "Segoe UI Symbol",
            #[cfg(not(windows))]
            "Noto Sans Symbols2",
            #[cfg(not(windows))]
            "Noto Sans Symbols",
            #[cfg(not(windows))]
            "Noto Sans Math",
            #[cfg(not(windows))]
            "DejaVu Sans",
            #[cfg(target_os = "macos")]
            "Apple Symbols",
        ],
    ),
    (
        *b"Latn",
        &[
            "Segoe UI Symbol",
            "Segoe UI Emoji",
            #[cfg(not(windows))]
            "Noto Color Emoji",
            #[cfg(not(windows))]
            "Twemoji",
            #[cfg(not(windows))]
            "JoyPixels",
            #[cfg(not(windows))]
            "Symbola",
            #[cfg(not(windows))]
            "Noto Sans Symbols2",
            #[cfg(not(windows))]
            "Noto Sans Symbols",
            #[cfg(not(windows))]
            "Noto Sans Math",
            #[cfg(not(windows))]
            "DejaVu Sans",
            #[cfg(target_os = "macos")]
            "Apple Color Emoji",
            #[cfg(target_os = "macos")]
            "Apple Symbols",
            #[cfg(target_os = "macos")]
            "STIX Two Math",
        ],
    ),
];

/// Puts the extra families behind the system's fallback family for the scripts that need them in
/// `fonts`. The system family stays first, so text it covers is drawn as before; applying this
/// twice changes nothing more.
///
/// Emoji need one more thing from the caller. A cluster flagged as emoji is looked up in the
/// style's own family list first and in the `emoji` generic only after it, and the first font
/// that has the character wins, so a `monospace` (or other generic) at the end of the list can
/// win with a monochrome emoji glyph, which draws as a tall box in a monospace face (Adwaita Mono
/// does). A family string that ends in a generic should name the colour emoji fonts before it
/// ([`EMOJI_FAMILIES`]).
pub fn add_fallbacks(fonts: &mut FontContext) {
    use parley::fontique::{FallbackKey, Script};
    for (script, names) in EXTRA_FALLBACKS {
        let key = FallbackKey::new(Script::from_bytes(*script), None);
        let mut ids: Vec<_> = fonts.collection.fallback_families(key).collect();
        for name in *names {
            if let Some(id) = fonts.collection.family_id(name)
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        fonts.collection.set_fallbacks(key, ids.into_iter());
    }
}

/// The colour emoji families of the systems, for a family string: put them right before a generic
/// at its end (`"Consolas, Noto Sans Mono, Noto Color Emoji, Segoe UI Emoji, monospace"`), see
/// [`add_fallbacks`]. Characters the earlier families have are still drawn by them; the emoji
/// fonts only take what is left, which is emoji.
pub const EMOJI_FAMILIES: &str = "Noto Color Emoji, Segoe UI Emoji, Apple Color Emoji";

/// Byte ranges of `text` that are emoji sequences needing the colour emoji font ahead of the
/// style's own families: a base character followed by U+FE0F (variation selector 16) or the
/// keycap U+20E3, an emoji modifier (U+1F3FB..U+1F3FF), or a joined (U+200D) sequence of
/// pictographs. Adjacent sequences are one range. Empty for ASCII (one fast scan) and on Windows.
///
/// Why: parley looks an emoji cluster up in the style's family list first and in the `emoji`
/// generic only after it, and the first font with the base character wins. U+2764 and U+26A0 are
/// text-default characters that DejaVu Sans, Noto Sans Symbols and others have, so with U+FE0F
/// they drew as a monochrome glyph (and a keycap as a plain digit) although the font that makes
/// them emoji was in the family string. Characters without U+FE0F (a bare U+2714, U+2192, U+00A9)
/// are not returned and keep the family list's own glyph. On Windows the system's fallback
/// already picks Segoe UI Emoji, so nothing changes there.
pub fn emoji_presentation_runs(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    if cfg!(windows) || text.is_ascii() {
        return out;
    }
    // The pictographic range of a sequence's base: what a ZWJ may join. Kept narrow so the ZWJ
    // of Indic conjuncts never splits a style run.
    fn pictographic(c: char) -> bool {
        matches!(c as u32, 0xA9 | 0xAE | 0x203C | 0x2049 | 0x2122 | 0x2139
            | 0x2190..=0x2BFF | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0x1F000..)
    }
    let (mut start, mut end) = (0, 0);
    let (mut ext, mut zwj, mut pict, mut joined) = (false, false, false, false);
    let mut flush = |start: usize, end: usize, marked: bool| {
        if marked {
            match out.last_mut() {
                Some(last) if last.end == start => last.end = end,
                _ => out.push(start..end),
            }
        }
    };
    for (i, c) in text.char_indices() {
        match c {
            '\u{FE0F}' | '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}' => ext = true,
            '\u{200D}' => {
                zwj = true;
                joined = true;
            }
            _ if joined => {
                pict |= pictographic(c);
                joined = false;
            }
            _ => {
                flush(start, end, ext || (zwj && pict));
                (start, ext, zwj, pict) = (i, false, false, pictographic(c));
            }
        }
        end = i + c.len_utf8();
    }
    flush(start, end, ext || (zwj && pict));
    out
}

/// Pushes the colour emoji families ([`EMOJI_FAMILIES`], then `base`, the style's own family
/// string, so a system without them draws what it drew before) for every range of
/// [`emoji_presentation_runs`]. Call it on a builder after the default and run styles are pushed,
/// since a later push wins.
pub fn push_emoji_runs<B: parley::Brush>(
    builder: &mut parley::RangedBuilder<'_, B>,
    text: &str,
    base: &str,
) {
    let runs = emoji_presentation_runs(text);
    if runs.is_empty() {
        return;
    }
    let family = format!("{EMOJI_FAMILIES}, {base}");
    for run in runs {
        builder.push(
            parley::StyleProperty::FontFamily(parley::FontFamily::from(family.as_str())),
            run,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::fontique::{FallbackKey, Script};
    use std::sync::atomic::AtomicUsize;

    const _: () = {
        const fn send<T: Send>() {}
        send::<FontContext>();
    };

    #[test]
    fn a_warmed_context_is_built_once_and_taken_once() {
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        fn counted() -> FontContext {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            build_fonts()
        }
        static PRIMED: AtomicUsize = AtomicUsize::new(0);
        fn prime(_: &mut FontContext) {
            PRIMED.fetch_add(1, Ordering::SeqCst);
        }
        let warm = Warm::new();
        assert!(!warm.waiting());
        warm.start(counted, prime);
        warm.start(counted, prime);
        assert!(warm.waiting(), "waits to be taken, built or not");
        let mut fonts = warm.take().expect("the built context");
        assert!(!warm.waiting());
        assert_eq!(
            PRIMED.load(Ordering::SeqCst),
            1,
            "primed once, before the hand-over"
        );
        assert!(
            fonts.collection.family_names().count() > 0,
            "it has the fonts"
        );
        assert!(
            warm.take().is_none(),
            "taken once; the next thread builds its own"
        );
        assert_eq!(
            BUILDS.load(Ordering::SeqCst),
            1,
            "a second start does nothing"
        );
    }

    #[test]
    fn without_a_start_or_after_a_panic_nothing_is_taken() {
        assert!(
            Warm::new().take().is_none(),
            "never started: built as before"
        );
        let warm = Warm::new();
        warm.start(|| panic!("no fonts"), |_| {});
        assert!(warm.take().is_none(), "the panic stays on its thread");
        let warm = Warm::new();
        warm.start(build_fonts, |_| panic!("priming failed"));
        assert!(warm.take().is_none(), "so does one in the priming");
    }

    /// The public pair: a thread with no context yet gets a working one after `warm_fonts`.
    #[test]
    fn shared_fonts_after_warm_fonts_works() {
        warm_fonts();
        warm_fonts();
        let mut fonts = std::thread::spawn(shared_fonts).join().unwrap();
        assert!(fonts.collection.family_names().count() > 0);
    }

    /// Times `shared_fonts` on a fresh thread: cold, and after a finished warm.
    /// `cargo test -p mdshared-text --release -- --ignored --nocapture warm_timing`
    #[test]
    #[ignore]
    fn warm_timing() {
        let cold = std::thread::spawn(|| {
            let t = std::time::Instant::now();
            shared_fonts();
            t.elapsed()
        })
        .join()
        .unwrap();
        warm_fonts();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let taken = std::thread::spawn(|| {
            let t = std::time::Instant::now();
            shared_fonts();
            t.elapsed()
        })
        .join()
        .unwrap();
        println!("cold shared_fonts {cold:?}; shared_fonts after a finished warm {taken:?}");
    }

    #[test]
    fn the_extra_families_that_are_installed_follow_the_system_ones_once() {
        let mut fonts = FontContext::new();
        let key = |s: &[u8; 4]| FallbackKey::new(Script::from_bytes(*s), None);
        let before: Vec<_> = fonts.collection.fallback_families(key(b"Zyyy")).collect();
        add_fallbacks(&mut fonts);
        let once: Vec<_> = fonts.collection.fallback_families(key(b"Zyyy")).collect();
        add_fallbacks(&mut fonts);
        let twice: Vec<_> = fonts.collection.fallback_families(key(b"Zyyy")).collect();
        assert_eq!(once, twice, "applying it again changes nothing");
        assert_eq!(
            &once[..before.len()],
            &before[..],
            "the system's come first"
        );
        for (script, names) in EXTRA_FALLBACKS {
            let ids: Vec<_> = fonts.collection.fallback_families(key(script)).collect();
            for name in *names {
                if let Some(id) = fonts.collection.family_id(name) {
                    assert!(ids.contains(&id), "{name} missing for {script:?}");
                }
            }
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn emoji_runs_cover_variation_keycap_modifier_and_joined_sequences_only() {
        fn runs(t: &str) -> Vec<&str> {
            emoji_presentation_runs(t)
                .into_iter()
                .map(|r| &t[r])
                .collect()
        }
        assert!(runs("plain ascii").is_empty(), "ascii is skipped");
        assert_eq!(runs("a \u{2764}\u{FE0F} b"), ["\u{2764}\u{FE0F}"]);
        assert_eq!(
            runs("\u{2764}\u{FE0F}\u{26A0}\u{FE0F}"),
            ["\u{2764}\u{FE0F}\u{26A0}\u{FE0F}"],
            "adjacent sequences are one range"
        );
        assert_eq!(runs("x 1\u{FE0F}\u{20E3} y"), ["1\u{FE0F}\u{20E3}"]);
        assert_eq!(runs("\u{1F44D}\u{1F3FD} ok"), ["\u{1F44D}\u{1F3FD}"]);
        let flag = "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}";
        assert_eq!(runs(&format!("a {flag} b")), [flag]);
        let tech = "\u{1F469}\u{200D}\u{1F4BB}";
        assert_eq!(runs(tech), [tech]);
        // Text-style characters and a text selector are left alone.
        assert!(runs("\u{2714} \u{2192} \u{A9} \u{2764}\u{FE0E}").is_empty());
        // The ZWJ of a conjunct is not an emoji sequence.
        assert!(runs("\u{0915}\u{094D}\u{200D}\u{0937}").is_empty());
    }
}
