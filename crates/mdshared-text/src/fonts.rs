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
