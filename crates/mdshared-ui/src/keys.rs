//! Shortcut strings such as "Ctrl+Shift+F", "F5" or "Ctrl++" parsed into [`Chord`]s and compared
//! with winit key events reduced to a [`KeyName`]. The table of commands and their shortcuts is
//! the application's own.
//!
//! "Ctrl" is the primary modifier: the Control key on Windows and Linux, Command on macOS, where
//! [`Mods::from_winit`] puts Command in [`Mods::ctrl`] and the Control key in [`Mods::control`].
//! A table is written once against it; [`for_platform`] moves the few shortcuts that the Mac
//! keeps for itself, and [`label`] prints a shortcut the way the platform spells it.

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// A key without modifiers, as the shortcut strings name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyName {
    /// A printable key, lowercased.
    Char(char),
    F(u8),
    Tab,
    Escape,
    Enter,
    Space,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// The primary modifier: Control on Windows and Linux, Command on macOS.
    pub ctrl: bool,
    pub shift: bool,
    /// Alt; Option on macOS.
    pub alt: bool,
    /// macOS only: the Control key, which is not the primary modifier there. Always false
    /// elsewhere.
    pub control: bool,
}

/// Whether the platform is a Mac, where Command is the primary modifier.
pub const MAC: bool = cfg!(target_os = "macos");

impl Mods {
    pub fn from_winit(m: ModifiersState) -> Mods {
        Mods {
            ctrl: if MAC { m.super_key() } else { m.control_key() },
            shift: m.shift_key(),
            alt: m.alt_key(),
            control: MAC && m.control_key(),
        }
    }

    /// No modifier at all.
    pub fn none(self) -> bool {
        self == Mods::default()
    }
}

/// One parsed shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: KeyName,
}

/// Parses "Ctrl+Shift+F", "F5", "Ctrl+Tab", "Ctrl++" and the like. `None` for anything unknown.
/// "Ctrl+" (or "Cmd+") is the primary modifier; "Control+" is the Control key on macOS and the
/// same as "Ctrl+" elsewhere.
pub fn parse(s: &str) -> Option<Chord> {
    parse_with(s, MAC)
}

/// [`parse`] as on a Mac (`mac`) or not, so the labels can be tested anywhere.
fn parse_with(s: &str, mac: bool) -> Option<Chord> {
    let mut mods = Mods::default();
    let mut rest = s.trim();
    loop {
        let lower = rest.to_ascii_lowercase();
        let take = |p: &str| lower.starts_with(p) && rest.len() > p.len();
        if take("control+") {
            if mac {
                mods.control = true;
            } else {
                mods.ctrl = true;
            }
            rest = &rest[8..];
        } else if take("ctrl+") {
            mods.ctrl = true;
            rest = &rest[5..];
        } else if take("cmd+") {
            mods.ctrl = true;
            rest = &rest[4..];
        } else if take("shift+") {
            mods.shift = true;
            rest = &rest[6..];
        } else if take("alt+") {
            mods.alt = true;
            rest = &rest[4..];
        } else {
            break;
        }
    }
    let key = match rest.to_ascii_lowercase().as_str() {
        "tab" => KeyName::Tab,
        "escape" | "esc" => KeyName::Escape,
        "enter" | "return" => KeyName::Enter,
        "space" => KeyName::Space,
        "backspace" => KeyName::Backspace,
        "delete" | "del" => KeyName::Delete,
        "home" => KeyName::Home,
        "end" => KeyName::End,
        "pageup" | "pgup" => KeyName::PageUp,
        "pagedown" | "pgdown" => KeyName::PageDown,
        "up" => KeyName::Up,
        "down" => KeyName::Down,
        "left" => KeyName::Left,
        "right" => KeyName::Right,
        k if k.len() > 1 && k.starts_with('f') => {
            KeyName::F(k[1..].parse().ok().filter(|n| (1..=24).contains(n))?)
        }
        k => {
            let mut chars = k.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyName::Char(c)
        }
    };
    Some(Chord { mods, key })
}

/// Keys that produce the same character with or without Shift depending on the layout ("+" is
/// Shift+"=" on US keyboards and a plain key on many others), so Shift is not compared for them.
fn shift_free(c: char) -> bool {
    !c.is_alphanumeric()
}

impl Chord {
    pub fn matches(&self, key: &KeyName, mods: Mods) -> bool {
        if self.mods.ctrl != mods.ctrl
            || self.mods.alt != mods.alt
            || self.mods.control != mods.control
        {
            return false;
        }
        match (&self.key, key) {
            (KeyName::Char(a), KeyName::Char(b)) => {
                // "Ctrl++" is typed as Ctrl+= on US layouts; accept both.
                let same = a == b || (*a == '+' && *b == '=');
                same && (shift_free(*a) || self.mods.shift == mods.shift)
            }
            (a, b) => a == b && self.mods.shift == mods.shift,
        }
    }
}

/// The shortcut a table writes as `s`, on this platform. On macOS the keys the system keeps for
/// itself move: Command+Tab switches applications (tabs take Control+Tab, as in Safari), Command+H
/// hides the application (Replace takes Option+Command+F), Command+Space is Spotlight (completion
/// takes Option+Escape), and Option+arrows move by word (Back and Forward take Command+[ and ]).
/// Everything else is unchanged.
pub fn for_platform(s: &'static str) -> &'static str {
    if !MAC {
        return s;
    }
    match s {
        "Ctrl+Tab" => "Control+Tab",
        "Ctrl+Shift+Tab" => "Control+Shift+Tab",
        "Ctrl+H" => "Ctrl+Alt+F",
        "Ctrl+Space" => "Alt+Escape",
        "Alt+Left" => "Ctrl+[",
        "Alt+Right" => "Ctrl+]",
        other => other,
    }
}

/// Moves a text-editing key to what it means on this platform, before a text box or the editor
/// reads it with the Windows and Linux meanings. On macOS: Command+Left and Right are the line's
/// start and end (Home, End), Command+Up and Down the document's (Ctrl+Home, Ctrl+End),
/// Option+Left, Right, Backspace and Delete go by word (Ctrl+...), and Control+A and Control+E
/// are the line's start and end. Shift is kept. Elsewhere, and for any other key, unchanged.
pub fn text_key(key: KeyName, mods: Mods) -> (KeyName, Mods) {
    if !MAC {
        return (key, mods);
    }
    let shift = mods.shift;
    let plain = Mods {
        shift,
        ..Mods::default()
    };
    let primary = Mods {
        ctrl: true,
        ..plain
    };
    let only = |m: Mods| Mods { shift, ..m } == mods;
    match key {
        KeyName::Left if only(primary) => (KeyName::Home, plain),
        KeyName::Right if only(primary) => (KeyName::End, plain),
        KeyName::Up if only(primary) => (KeyName::Home, primary),
        KeyName::Down if only(primary) => (KeyName::End, primary),
        KeyName::Left | KeyName::Right | KeyName::Backspace | KeyName::Delete
            if only(Mods { alt: true, ..plain }) =>
        {
            (key, primary)
        }
        KeyName::Char('a')
            if only(Mods {
                control: true,
                ..plain
            }) =>
        {
            (KeyName::Home, plain)
        }
        KeyName::Char('e')
            if only(Mods {
                control: true,
                ..plain
            }) =>
        {
            (KeyName::End, plain)
        }
        _ => (key, mods),
    }
}

/// Whether a key pressed with `mods` types the text it produced. Off the Mac, Ctrl alone makes
/// a shortcut and Ctrl+Alt is AltGr on Windows, which types. On a Mac, Option types (accents and
/// symbols, Option+E then E); Command or Control held makes a shortcut, with Option or not.
pub fn types_text(mods: Mods) -> bool {
    if MAC {
        !mods.ctrl && !mods.control
    } else {
        !(mods.ctrl && !mods.alt)
    }
}

/// A shortcut as the platform prints it: unchanged off macOS; on macOS in the Mac's symbols, in
/// its order (Control, Option, Shift, Command, then the key): "Ctrl+Shift+F" is "⇧⌘F". `s` may
/// hold several shortcuts ("Ctrl+W, Ctrl+F4", "Ctrl+K Ctrl+0"); a part that is not a shortcut
/// (a path, a heading level) is kept as it is. The result lives for the program, as menu hints do.
pub fn label(s: &str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some(done) = map.get(s) {
        return done;
    }
    let made: &'static str = Box::leak(label_for(s, MAC).into_boxed_str());
    // A hint that is not a shortcut can be anything (a file's path in the palette): only a
    // bounded number are kept, the rest are made again.
    if map.len() < 512 {
        map.insert(s.to_string(), made);
    }
    made
}

/// [`label`] for a given platform, without the cache.
fn label_for(s: &str, mac: bool) -> String {
    if !mac {
        return s.to_string();
    }
    let mut out = String::new();
    // Separators kept as written: ", " between alternatives, " " between the keys of a sequence.
    let mut rest = s;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .find(|&(i, c)| c == ' ' || (c == ',' && rest[i + 1..].starts_with(' ')))
            .map_or(rest.len(), |(i, _)| i);
        // A part ending in '+' is a chord whose key is '+' only when something precedes it.
        let part = &rest[..end];
        out.push_str(&mac_chord(part).unwrap_or_else(|| part.to_string()));
        let sep_len = rest[end..]
            .chars()
            .next()
            .map_or(0, |c| if c == ',' { 2 } else { 1 });
        out.push_str(&rest[end..end + sep_len]);
        rest = &rest[end + sep_len..];
    }
    out
}

/// One shortcut in the Mac's symbols, if `s` is one.
fn mac_chord(s: &str) -> Option<String> {
    let chord = parse_with(s, true)?;
    let mut out = String::new();
    let m = chord.mods;
    for (on, sym) in [
        (m.control, '\u{2303}'),
        (m.alt, '\u{2325}'),
        (m.shift, '\u{21e7}'),
        (m.ctrl, '\u{2318}'),
    ] {
        if on {
            out.push(sym);
        }
    }
    let key = match chord.key {
        KeyName::Char(c) => c.to_uppercase().collect(),
        KeyName::F(n) => format!("F{n}"),
        KeyName::Tab => "\u{21e5}".into(),
        KeyName::Escape => "\u{238b}".into(),
        KeyName::Enter => "\u{21a9}".into(),
        KeyName::Space => "Space".into(),
        KeyName::Backspace => "\u{232b}".into(),
        KeyName::Delete => "\u{2326}".into(),
        KeyName::Home => "\u{2196}".into(),
        KeyName::End => "\u{2198}".into(),
        KeyName::PageUp => "\u{21de}".into(),
        KeyName::PageDown => "\u{21df}".into(),
        KeyName::Up => "\u{2191}".into(),
        KeyName::Down => "\u{2193}".into(),
        KeyName::Left => "\u{2190}".into(),
        KeyName::Right => "\u{2192}".into(),
    };
    out.push_str(&key);
    Some(out)
}

/// Reduces a winit key (pressed without modifiers, so Ctrl+Shift+1 arrives as "1") to a name.
pub fn key_name(key: &Key) -> Option<KeyName> {
    Some(match key {
        Key::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyName::Char(c.to_lowercase().next().unwrap_or(c))
        }
        Key::Named(n) => match n {
            NamedKey::Tab => KeyName::Tab,
            NamedKey::Escape => KeyName::Escape,
            NamedKey::Enter => KeyName::Enter,
            NamedKey::Space => KeyName::Space,
            NamedKey::Backspace => KeyName::Backspace,
            NamedKey::Delete => KeyName::Delete,
            NamedKey::Home => KeyName::Home,
            NamedKey::End => KeyName::End,
            NamedKey::PageUp => KeyName::PageUp,
            NamedKey::PageDown => KeyName::PageDown,
            NamedKey::ArrowUp => KeyName::Up,
            NamedKey::ArrowDown => KeyName::Down,
            NamedKey::ArrowLeft => KeyName::Left,
            NamedKey::ArrowRight => KeyName::Right,
            NamedKey::F1 => KeyName::F(1),
            NamedKey::F2 => KeyName::F(2),
            NamedKey::F3 => KeyName::F(3),
            NamedKey::F4 => KeyName::F(4),
            NamedKey::F5 => KeyName::F(5),
            NamedKey::F6 => KeyName::F(6),
            NamedKey::F7 => KeyName::F(7),
            NamedKey::F8 => KeyName::F(8),
            NamedKey::F9 => KeyName::F(9),
            NamedKey::F10 => KeyName::F(10),
            NamedKey::F11 => KeyName::F(11),
            NamedKey::F12 => KeyName::F(12),
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
        control: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
        control: false,
    };
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
        control: false,
    };
    const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
        control: false,
    };

    #[test]
    fn parses_strings() {
        assert_eq!(
            parse("Ctrl+Shift+F"),
            Some(Chord {
                mods: CTRL_SHIFT,
                key: KeyName::Char('f')
            })
        );
        assert_eq!(
            parse("F5"),
            Some(Chord {
                mods: NONE,
                key: KeyName::F(5)
            })
        );
        assert_eq!(
            parse("Shift+F3"),
            Some(Chord {
                mods: SHIFT,
                key: KeyName::F(3)
            })
        );
        assert_eq!(
            parse("Ctrl+Tab"),
            Some(Chord {
                mods: CTRL,
                key: KeyName::Tab
            })
        );
        assert_eq!(
            parse("Ctrl++"),
            Some(Chord {
                mods: CTRL,
                key: KeyName::Char('+')
            })
        );
        assert_eq!(
            parse("Ctrl+-"),
            Some(Chord {
                mods: CTRL,
                key: KeyName::Char('-')
            })
        );
        assert_eq!(
            parse("Ctrl+F4"),
            Some(Chord {
                mods: CTRL,
                key: KeyName::F(4)
            })
        );
        assert_eq!(
            parse("Escape"),
            Some(Chord {
                mods: NONE,
                key: KeyName::Escape
            })
        );
        assert_eq!(
            parse("Ctrl+Shift+1"),
            Some(Chord {
                mods: CTRL_SHIFT,
                key: KeyName::Char('1')
            })
        );
        assert_eq!(
            parse("F"),
            Some(Chord {
                mods: NONE,
                key: KeyName::Char('f')
            })
        );
        assert_eq!(parse("Ctrl+Bogus"), None);
        assert_eq!(parse("F99"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn matching_rules() {
        let f = parse("Ctrl+Shift+F").unwrap();
        assert!(f.matches(&KeyName::Char('f'), CTRL_SHIFT));
        assert!(!f.matches(&KeyName::Char('f'), CTRL));
        let plus = parse("Ctrl++").unwrap();
        assert!(plus.matches(&KeyName::Char('+'), CTRL));
        assert!(
            plus.matches(&KeyName::Char('='), CTRL_SHIFT),
            "US layout: Ctrl+Shift+="
        );
        assert!(plus.matches(&KeyName::Char('='), CTRL));
        assert!(!plus.matches(&KeyName::Char('+'), NONE));
        let tab = parse("Ctrl+Tab").unwrap();
        assert!(!tab.matches(&KeyName::Tab, CTRL_SHIFT));
    }

    #[test]
    fn mac_labels_use_the_symbols_in_the_mac_order() {
        let mac = |s: &str| label_for(s, true);
        assert_eq!(mac("Ctrl+Shift+F"), "\u{21e7}\u{2318}F");
        assert_eq!(mac("Ctrl+Alt+F"), "\u{2325}\u{2318}F");
        assert_eq!(mac("Control+Shift+Tab"), "\u{2303}\u{21e7}\u{21e5}");
        assert_eq!(mac("Ctrl++"), "\u{2318}+");
        assert_eq!(mac("Ctrl+,"), "\u{2318},");
        assert_eq!(mac("F5"), "F5");
        assert_eq!(mac("Escape"), "\u{238b}");
        // Several shortcuts, a sequence, and what is not a shortcut at all.
        assert_eq!(mac("Ctrl+W, Ctrl+F4"), "\u{2318}W, \u{2318}F4");
        assert_eq!(mac("Ctrl+K Ctrl+0"), "\u{2318}K \u{2318}0");
        assert_eq!(
            mac("Alt+Shift+Arrows, Alt+Drag"),
            "Alt+Shift+Arrows, Alt+Drag"
        );
        assert_eq!(mac("docs/read me.md"), "docs/read me.md");
        assert_eq!(mac(""), "");
        // Off the Mac a label is the table's own string.
        assert_eq!(label_for("Ctrl+Shift+F", false), "Ctrl+Shift+F");
        assert_eq!(label("Ctrl+Shift+F"), label("Ctrl+Shift+F"));
    }

    #[test]
    fn control_is_the_mac_control_key_and_ctrl_elsewhere() {
        let mac = parse_with("Control+Tab", true).unwrap();
        assert!(mac.mods.control && !mac.mods.ctrl);
        let other = parse_with("Control+Tab", false).unwrap();
        assert!(other.mods.ctrl && !other.mods.control);
        assert_eq!(parse("Cmd+S"), parse("Ctrl+S"));
        // The Control key held is not the primary modifier.
        let tab = parse_with("Control+Tab", true).unwrap();
        assert!(!tab.matches(&KeyName::Tab, CTRL));
        assert!(tab.matches(
            &KeyName::Tab,
            Mods {
                control: true,
                ..NONE
            }
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_the_system_shortcuts_move_and_text_keys_take_the_mac_meanings() {
        assert_eq!(for_platform("Ctrl+Tab"), "Control+Tab");
        assert_eq!(for_platform("Ctrl+H"), "Ctrl+Alt+F");
        assert_eq!(for_platform("Ctrl+S"), "Ctrl+S");
        let alt = Mods { alt: true, ..NONE };
        let alt_shift = Mods { shift: true, ..alt };
        assert_eq!(text_key(KeyName::Left, CTRL), (KeyName::Home, NONE));
        assert_eq!(text_key(KeyName::Right, CTRL_SHIFT), (KeyName::End, SHIFT));
        assert_eq!(text_key(KeyName::Down, CTRL), (KeyName::End, CTRL));
        assert_eq!(text_key(KeyName::Left, alt), (KeyName::Left, CTRL));
        assert_eq!(
            text_key(KeyName::Right, alt_shift),
            (KeyName::Right, CTRL_SHIFT)
        );
        assert_eq!(
            text_key(KeyName::Backspace, alt),
            (KeyName::Backspace, CTRL)
        );
        let control = Mods {
            control: true,
            ..NONE
        };
        assert_eq!(text_key(KeyName::Char('e'), control), (KeyName::End, NONE));
        assert_eq!(
            text_key(KeyName::Char('c'), CTRL),
            (KeyName::Char('c'), CTRL)
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn off_the_mac_tables_and_text_keys_are_unchanged() {
        assert_eq!(for_platform("Ctrl+Tab"), "Ctrl+Tab");
        assert_eq!(for_platform("Alt+Left"), "Alt+Left");
        assert_eq!(text_key(KeyName::Left, CTRL), (KeyName::Left, CTRL));
    }
}
