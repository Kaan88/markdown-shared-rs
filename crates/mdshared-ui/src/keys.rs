//! Shortcut strings such as "Ctrl+Shift+F", "F5" or "Ctrl++" parsed into [`Chord`]s and compared
//! with winit key events reduced to a [`KeyName`]. The table of commands and their shortcuts is
//! the application's own.

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
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub fn from_winit(m: ModifiersState) -> Mods {
        Mods {
            ctrl: m.control_key(),
            shift: m.shift_key(),
            alt: m.alt_key(),
        }
    }
}

/// One parsed shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: KeyName,
}

/// Parses "Ctrl+Shift+F", "F5", "Ctrl+Tab", "Ctrl++" and the like. `None` for anything unknown.
pub fn parse(s: &str) -> Option<Chord> {
    let mut mods = Mods::default();
    let mut rest = s.trim();
    loop {
        let lower = rest.to_ascii_lowercase();
        let take = |p: &str| lower.starts_with(p) && rest.len() > p.len();
        if take("ctrl+") {
            mods.ctrl = true;
            rest = &rest[5..];
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
        if self.mods.ctrl != mods.ctrl || self.mods.alt != mods.alt {
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
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
    const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
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
}
