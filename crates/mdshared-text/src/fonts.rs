use parley::FontContext;

thread_local! {
    /// System fonts are enumerated once per thread; every renderer starts from a clone of this. A
    /// window makes several renderers (one per tab and a spare for `mem::take`), and enumerating
    /// fonts for each cost about 35 ms apiece at launch.
    static FONTS: std::cell::RefCell<Option<FontContext>> = const { std::cell::RefCell::new(None) };
}

/// The per-thread font context every renderer clones. A shell that draws its own text takes a
/// clone too, so system fonts are still enumerated once.
///
/// Off Windows the context already has [`add_fallbacks`] applied. On Windows it is the system's
/// own, as before; an application that wants the fallbacks there calls [`add_fallbacks`] on its
/// copy.
pub fn shared_fonts() -> FontContext {
    FONTS.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                #[cfg_attr(windows, allow(unused_mut))]
                let mut fonts = FontContext::new();
                #[cfg(not(windows))]
                add_fallbacks(&mut fonts);
                fonts
            })
            .clone()
    })
}

/// Families tried after the system's fallback for a script, for characters it lacks: the
/// system's Han fallback has no glyph for some simplified characters (U+4F24) and the common
/// script's none for musical symbols (U+1D11E), which then drew as boxes. A line of only
/// common-script characters found its fallback under Latin, not Common, so Latin gets them too.
///
/// The Windows names come first and in their original order; the families of Linux distributions
/// follow, and a name that is not installed is skipped. On Windows only the Windows names are used.
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

#[cfg(test)]
mod tests {
    use super::*;
    use parley::fontique::{FallbackKey, Script};

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
}
