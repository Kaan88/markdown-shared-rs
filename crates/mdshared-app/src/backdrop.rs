//! The "Window backdrop" setting: whether the window's chrome lets a system backdrop show through
//! it (Windows 11's Mica or Acrylic). The type, its place in a settings file and the words of its
//! settings row; the drawing is `mdshared_ui::frost`, the system calls `mdshared_ui::platform`.

/// What shows behind the chrome. `Off` is the flat colours, on every system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Backdrop {
    #[default]
    Off,
    /// The desktop wallpaper, heavily blurred and tinted by the system: close to a flat colour.
    Mica,
    /// What is behind the window, blurred. The chrome keeps a veil of its own colour over it.
    Acrylic,
}

impl Backdrop {
    /// In the order of [`Backdrop::LABELS`].
    pub const ALL: [Backdrop; 3] = [Backdrop::Off, Backdrop::Mica, Backdrop::Acrylic];
    /// The choices of the settings row.
    pub const LABELS: [&'static str; 3] = ["Off", "Mica", "Acrylic"];
    /// The label of the settings row.
    pub const TITLE: &'static str = "Window backdrop";
    /// The help line of the settings row.
    pub const HELP: &'static str =
        "Windows 11: the desktop shows softly through the title row, bars and side pane.";

    /// The value in a settings file.
    pub fn key(self) -> &'static str {
        match self {
            Backdrop::Off => "off",
            Backdrop::Mica => "mica",
            Backdrop::Acrylic => "acrylic",
        }
    }

    /// Reads [`Backdrop::key`]; anything else is `None` (the caller's default).
    pub fn parse(value: &str) -> Option<Backdrop> {
        let v = value.trim();
        Backdrop::ALL
            .into_iter()
            .find(|b| b.key().eq_ignore_ascii_case(v))
    }

    /// The place in [`Backdrop::LABELS`].
    pub fn index(self) -> usize {
        Backdrop::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }

    /// The choice at `index` of [`Backdrop::LABELS`]; out of range is `Off`.
    pub fn from_index(index: usize) -> Backdrop {
        Backdrop::ALL.get(index).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_unknown_is_none() {
        for b in Backdrop::ALL {
            assert_eq!(Backdrop::parse(b.key()), Some(b));
            assert_eq!(Backdrop::from_index(b.index()), b);
        }
        assert_eq!(Backdrop::parse(" MICA "), Some(Backdrop::Mica));
        assert_eq!(Backdrop::parse("glass"), None);
        assert_eq!(Backdrop::parse(""), None);
        assert_eq!(Backdrop::from_index(9), Backdrop::Off);
        assert_eq!(Backdrop::default(), Backdrop::Off);
        assert_eq!(Backdrop::LABELS.len(), Backdrop::ALL.len());
    }
}
