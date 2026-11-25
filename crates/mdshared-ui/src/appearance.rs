//! The look as a user setting: plain data an application keeps in one setting, shows in its
//! preferences and applies to what it draws. A scheme of colours ([`Scheme`]: dark, light, warm,
//! cool), where the accent comes from ([`AccentFrom`]: the scheme's own, the system's, the cover
//! art's, or a colour the user picked), the corners' radius, the shadows' strength, the height of
//! a menu's rows, whether menus are joined to the control that opens them, the speed of motion
//! ([`Speed`]) and the submenu's hover delay, together in [`Appearance`].
//!
//! It is opt-in, and it draws nothing itself. [`Appearance::DEFAULT`] is the toolkit's own look:
//! [`DARK`], its own accent, [`Look::ROUNDED`] menus and the Fluent timings, so an
//! application that opts in and changes nothing draws as before. The application owns:
//!
//! - the palette it starts from: [`Scheme::palette`] gives the toolkit's, and an application
//!   with palettes of its own passes its own instead (mdplay keeps its own dark) to
//!   [`Appearance::palette`], which only puts the chosen accent into it;
//! - the colours the accent may come from: the system's accent
//!   ([`platform::system_accent`](crate::platform::system_accent), asked again when the system
//!   says its colours changed) and the cover's
//!   ([`cover::colors`](crate::cover::colors)`(..).accent`, once per picture);
//! - giving the look to the widgets: [`Appearance::menu_look`] to every
//!   [`Menu::with_look`](crate::menu::Menu::with_look) and
//!   [`Cascade::with_look`](crate::submenu::Cascade::with_look),
//!   [`Appearance::submenu_delay`] to [`Cascade::with_delay`](crate::submenu::Cascade::with_delay),
//!   [`Speed::scale`] to the durations of its motions, [`Speed::policy`] to its animator, and
//!   `joined` to whether it opens its menus flush under their control and draws them with
//!   [`menu::join_under`](crate::menu::join_under) and
//!   [`menu::join_over`](crate::menu::join_over);
//! - saving: [`Appearance::write`] and [`Appearance::read`] carry it as `key value` pairs, and
//!   [`to_text`] and [`parse`] put those pairs (with any keys of the application's own) into one
//!   text with a header line, for a single setting string.
//!
//! Deliberately not here: fonts and text sizes ([`crate::tokens::Sizes`] names those), the
//! window's backdrop (`mdshared_app::backdrop`), high contrast (the system's choice, which
//! [`crate::tokens::Tokens::from_system`] honours; an application in high contrast does not
//! offer these schemes), a dark warm or cool scheme, a settings page (the application builds
//! one with [`crate::settings::Form`] from the `ALL` lists and `label`s), and reading or
//! writing a file: the application keeps the text where it keeps its settings.

use crate::anim::Policy;
use crate::gfx::{Rgb, rgb};
use crate::menu::Look;
use crate::theme::{DARK, LIGHT, Palette};
use crate::tokens::accented;
use std::time::Duration;

/// A light palette with warm greys: paper-coloured pages, brown-black text. Its accent and the
/// colours not named here are [`LIGHT`]'s; the selected row and the text selection are made
/// from that accent over these pages as [`accented`] makes them.
pub const WARM: Palette = Palette {
    page: rgb(0xfbf8f3),
    title: rgb(0xf1ebe2),
    bar: rgb(0xf1ebe2),
    pane: rgb(0xf1ebe2),
    text: rgb(0x2a241f),
    muted: rgb(0x6f6459),
    faint: rgb(0xaca193),
    hover: rgb(0xe6dfd6),
    press: rgb(0xdfd7cd),
    on: rgb(0xcad5db),
    line: rgb(0xe5ddd0),
    field: rgb(0xfffdf9),
    popup: rgb(0xfdfbf7),
    popup_line: rgb(0xe0d6c7),
    selection: rgb(0x3f85c7),
    ..LIGHT
};

/// A light palette with cool, blue-tinted greys and blue-black text. Its accent and the colours
/// not named here are [`LIGHT`]'s; the selected row and the text selection are made from that
/// accent over these pages as [`accented`] makes them.
pub const COOL: Palette = Palette {
    page: rgb(0xf9fbfd),
    title: rgb(0xeef2f7),
    bar: rgb(0xeef2f7),
    pane: rgb(0xeef2f7),
    text: rgb(0x17202b),
    muted: rgb(0x5a6573),
    faint: rgb(0x9aa5b2),
    hover: rgb(0xe2e7ed),
    press: rgb(0xd8dee5),
    on: rgb(0xc8daed),
    line: rgb(0xdce3eb),
    field: rgb(0xffffff),
    popup: rgb(0xfbfcfe),
    popup_line: rgb(0xd8e0e9),
    selection: rgb(0x3e86c9),
    ..LIGHT
};

/// The colours an application's chrome is drawn in, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// [`DARK`].
    Dark,
    /// [`LIGHT`].
    Light,
    /// [`WARM`], light.
    Warm,
    /// [`COOL`], light.
    Cool,
}

impl Scheme {
    /// Every scheme, in the order a preferences page lists them.
    pub const ALL: [Scheme; 4] = [Scheme::Dark, Scheme::Light, Scheme::Warm, Scheme::Cool];

    /// The word saved for it: "dark", "light", "warm", "cool".
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Dark => "dark",
            Scheme::Light => "light",
            Scheme::Warm => "warm",
            Scheme::Cool => "cool",
        }
    }

    /// What a preferences page calls it: "Dark", "Light", "Warm", "Cool".
    pub fn label(self) -> &'static str {
        match self {
            Scheme::Dark => "Dark",
            Scheme::Light => "Light",
            Scheme::Warm => "Warm",
            Scheme::Cool => "Cool",
        }
    }

    /// The scheme `name` names, in any case and with white space round it; `None` for any
    /// other text.
    pub fn from_name(name: &str) -> Option<Scheme> {
        let name = name.trim();
        Scheme::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
    }

    /// Whether its pages are dark: only [`Scheme::Dark`].
    pub fn is_dark(self) -> bool {
        self == Scheme::Dark
    }

    /// The toolkit's palette for it: [`DARK`], [`LIGHT`], [`WARM`] or [`COOL`].
    pub fn palette(self) -> Palette {
        match self {
            Scheme::Dark => DARK,
            Scheme::Light => LIGHT,
            Scheme::Warm => WARM,
            Scheme::Cool => COOL,
        }
    }
}

/// Where the accent comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccentFrom {
    /// The palette's own accent.
    Theme,
    /// The system's accent colour, when it gives one.
    System,
    /// The accent of the cover art showing, when there is one.
    Cover,
    /// A colour the user picked.
    Custom(Rgb),
}

impl AccentFrom {
    /// The three that are not a colour, in the order a preferences page lists them (a page that
    /// offers a colour adds [`AccentFrom::Custom`] with the colour picked).
    pub const SOURCES: [AccentFrom; 3] = [AccentFrom::Theme, AccentFrom::System, AccentFrom::Cover];

    /// The word saved for it: "theme", "system", "cover", or the colour as `#rrggbb` in lower
    /// case.
    pub fn name(self) -> String {
        match self {
            AccentFrom::Theme => "theme".into(),
            AccentFrom::System => "system".into(),
            AccentFrom::Cover => "cover".into(),
            AccentFrom::Custom([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }

    /// What a preferences page calls it: "Theme", "System", "Cover art", "Custom".
    pub fn label(self) -> &'static str {
        match self {
            AccentFrom::Theme => "Theme",
            AccentFrom::System => "System",
            AccentFrom::Cover => "Cover art",
            AccentFrom::Custom(_) => "Custom",
        }
    }

    /// The source `name` names: one of the words in any case, or `#` and six hexadecimal digits
    /// in any case; white space round it is ignored. `None` for any other text (`#abc`,
    /// `#1234567`, `123456`, a word misspelt).
    pub fn from_name(name: &str) -> Option<AccentFrom> {
        let name = name.trim();
        if let Some(found) = AccentFrom::SOURCES
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
        {
            return Some(found);
        }
        let hex = name.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(AccentFrom::Custom(rgb(v)))
    }

    /// `base` with this accent in it: `base` itself, unchanged in every field, for
    /// [`AccentFrom::Theme`] and for a source that has no colour now (`system` or `cover` is
    /// `None`: the system gives none, no picture is showing); otherwise
    /// [`accented`]`(base, colour)`, which makes the colour legible on `base`'s page and the
    /// selected row, the selection and the text on the accent from it.
    pub fn apply(self, base: &Palette, system: Option<Rgb>, cover: Option<Rgb>) -> Palette {
        let colour = match self {
            AccentFrom::Theme => None,
            AccentFrom::System => system,
            AccentFrom::Cover => cover,
            AccentFrom::Custom(c) => Some(c),
        };
        match colour {
            Some(c) => accented(base, c),
            None => *base,
        }
    }
}

/// How fast things move: the [Fluent](crate::anim::fluent) timings times a factor, or nothing
/// moving at all. Named `Speed`, not `Motion`, so that it is not mistaken for
/// [`anim::Motion`](crate::anim::Motion), which is how one value moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Speed {
    /// Nothing moves: every value is its target at once.
    Off,
    /// Two thirds of the Fluent timings (0.66).
    Snappy,
    /// The Fluent timings as they are.
    Fluent,
    /// Half as long again (1.5).
    Soft,
}

impl Speed {
    /// Every speed, slowest last, in the order a preferences page lists them.
    pub const ALL: [Speed; 4] = [Speed::Off, Speed::Snappy, Speed::Fluent, Speed::Soft];

    /// The word saved for it: "off", "snappy", "fluent", "soft".
    pub fn name(self) -> &'static str {
        match self {
            Speed::Off => "off",
            Speed::Snappy => "snappy",
            Speed::Fluent => "fluent",
            Speed::Soft => "soft",
        }
    }

    /// What a preferences page calls it: "Off", "Snappy", "Fluent", "Soft".
    pub fn label(self) -> &'static str {
        match self {
            Speed::Off => "Off",
            Speed::Snappy => "Snappy",
            Speed::Fluent => "Fluent",
            Speed::Soft => "Soft",
        }
    }

    /// The speed `name` names, in any case and with white space round it; `None` for any other
    /// text.
    pub fn from_name(name: &str) -> Option<Speed> {
        let name = name.trim();
        Speed::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
    }

    /// The factor a duration is multiplied by: 0, 0.66, 1 or 1.5.
    pub fn factor(self) -> f32 {
        let (n, d) = self.ratio();
        n as f32 / d as f32
    }

    fn ratio(self) -> (u128, u128) {
        match self {
            Speed::Off => (0, 1),
            Speed::Snappy => (66, 100),
            Speed::Fluent => (1, 1),
            Speed::Soft => (3, 2),
        }
    }

    /// `d` at this speed, to the nanosecond: [`Speed::Fluent`] gives `d` itself, `Off` zero; a
    /// duration too long for the result is `Duration::MAX`. For a motion built from the Fluent
    /// timings: `Motion::tween(speed.scale(fluent::FAST), fluent::DECELERATE)`.
    pub fn scale(self, d: Duration) -> Duration {
        let (n, den) = self.ratio();
        let nanos = d.as_nanos().saturating_mul(n) / den;
        let secs = nanos / 1_000_000_000;
        match u64::try_from(secs) {
            Ok(secs) => Duration::new(secs, (nanos % 1_000_000_000) as u32),
            Err(_) => Duration::MAX,
        }
    }

    /// `ms` milliseconds at this speed, rounded down to a whole millisecond; never overflows.
    pub fn scale_ms(self, ms: u64) -> u64 {
        let (n, den) = self.ratio();
        u64::try_from(u128::from(ms) * n / den).unwrap_or(u64::MAX)
    }

    /// What the application's animator is told ([`Animator::set_policy`](
    /// crate::anim::Animator::set_policy)): `Off` is [`Policy::Never`], the others
    /// [`Policy::System`], so the system's wish for less motion still holds.
    pub fn policy(self) -> Policy {
        match self {
            Speed::Off => Policy::Never,
            _ => Policy::System,
        }
    }
}

/// The largest corner radius [`Appearance::sane`] keeps, in logical px.
pub const MAX_RADIUS: f32 = 16.0;
/// The strongest shadow [`Appearance::sane`] keeps: twice the toolkit's own.
pub const MAX_SHADOW: f32 = 2.0;
/// The least a menu's row may be, in logical px.
pub const MIN_MENU_ROW: f32 = 20.0;
/// The most a menu's row may be, in logical px.
pub const MAX_MENU_ROW: f32 = 48.0;
/// The longest submenu delay [`Appearance::sane`] keeps, in milliseconds.
pub const MAX_SUBMENU_DELAY_MS: u32 = 2000;

/// The keys [`Appearance::write`] writes and [`Appearance::read`] reads, in that order, with
/// their values:
///
/// | Key | Value |
/// |---|---|
/// | `scheme` | `dark`, `light`, `warm`, `cool` ([`Scheme::name`]) |
/// | `accent` | `theme`, `system`, `cover` or `#rrggbb` ([`AccentFrom::name`]) |
/// | `radius` | the corners' radius in logical px, 0 to [`MAX_RADIUS`] |
/// | `shadow` | the shadows' strength, 0 (none) to [`MAX_SHADOW`]; 1 is the toolkit's |
/// | `menu_row` | a menu row's height in logical px, [`MIN_MENU_ROW`] to [`MAX_MENU_ROW`] |
/// | `joined` | `on` or `off` (also read: `true`, `false`, `1`, `0`) |
/// | `motion` | `off`, `snappy`, `fluent`, `soft` ([`Speed::name`]) |
/// | `submenu_delay` | the submenu's hover delay in whole milliseconds, 0 to [`MAX_SUBMENU_DELAY_MS`] |
pub const KEYS: [&str; 8] = [
    "scheme",
    "accent",
    "radius",
    "shadow",
    "menu_row",
    "joined",
    "motion",
    "submenu_delay",
];

/// The look an application draws with, as the user chose it. See the [module](self).
///
/// The fields are public and the struct is `non_exhaustive`: an application starts from
/// [`Appearance::DEFAULT`] (or `Appearance::default()`) and changes fields, and a field added
/// later breaks none. Sizes out of reason are put right by [`sane`](Appearance::sane), which
/// [`menu_look`](Appearance::menu_look) and [`write`](Appearance::write) apply.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Appearance {
    pub scheme: Scheme,
    pub accent: AccentFrom,
    /// The corners' radius of menus, and of whatever else the application rounds by it, in
    /// logical px.
    pub radius: f32,
    /// The shadows' strength: 1 is the toolkit's, 0 none.
    pub shadow: f32,
    /// A menu row's height, in logical px.
    pub menu_row: f32,
    /// Menus open flush under the control that opens them and are drawn joined to it.
    pub joined: bool,
    pub motion: Speed,
    /// How long the pointer rests on a row before its submenu opens, in milliseconds.
    pub submenu_delay_ms: u32,
}

impl Appearance {
    /// The toolkit's own look: [`Scheme::Dark`], the palette's accent, the corners, row and
    /// shadow of [`Look::ROUNDED`], menus apart from their controls, the Fluent timings and
    /// [`submenu::DELAY`](crate::submenu::DELAY).
    pub const DEFAULT: Appearance = Appearance {
        scheme: Scheme::Dark,
        accent: AccentFrom::Theme,
        radius: Look::ROUNDED.radius,
        shadow: Look::ROUNDED.shadow_strength,
        menu_row: Look::ROUNDED.row,
        joined: false,
        motion: Speed::Fluent,
        submenu_delay_ms: crate::submenu::DELAY.as_millis() as u32,
    };

    /// This appearance with every number held to its range ([`KEYS`] lists them); a number that
    /// is not finite is the default's.
    pub fn sane(self) -> Appearance {
        let d = Appearance::DEFAULT;
        let held = |v: f32, lo: f32, hi: f32, d: f32| {
            if v.is_finite() { v.clamp(lo, hi) } else { d }
        };
        Appearance {
            radius: held(self.radius, 0.0, MAX_RADIUS, d.radius),
            shadow: held(self.shadow, 0.0, MAX_SHADOW, d.shadow),
            menu_row: held(self.menu_row, MIN_MENU_ROW, MAX_MENU_ROW, d.menu_row),
            submenu_delay_ms: self.submenu_delay_ms.min(MAX_SUBMENU_DELAY_MS),
            ..self
        }
    }

    /// The look of a menu drawn in this appearance, its numbers made [`sane`](Self::sane):
    /// [`Look::ROUNDED`] with this radius, row height and shadow strength (no shadow at 0). The
    /// highlight's corners follow the menu's, two px less and at most the rounded look's 5, so
    /// [`DEFAULT`](Self::DEFAULT) gives `Look::ROUNDED` itself; at radius 0 the highlight is
    /// [`Look::FLAT`]'s, square and across the whole row inside the menu's line. The padding,
    /// the separator's gap and the submenu's gap stay the rounded look's.
    pub fn menu_look(&self) -> Look {
        let a = self.sane();
        let mut look = Look::ROUNDED;
        look.radius = a.radius;
        look.row = a.menu_row;
        look.shadow = a.shadow > 0.0;
        look.shadow_strength = a.shadow;
        if a.radius == 0.0 {
            look.inset = Look::FLAT.inset;
            look.hover_radius = Look::FLAT.hover_radius;
        } else {
            look.hover_radius = (a.radius - 2.0).clamp(0.0, Look::ROUNDED.hover_radius);
        }
        look.sane()
    }

    /// `base` with this appearance's accent in it: [`AccentFrom::apply`]. `base` is usually
    /// `self.scheme.palette()`, or the application's own palette for a scheme it draws
    /// differently; `system` and `cover` are the colours the accent may come from now.
    pub fn palette(&self, base: &Palette, system: Option<Rgb>, cover: Option<Rgb>) -> Palette {
        self.accent.apply(base, system, cover)
    }

    /// The submenu's hover delay, made [`sane`](Self::sane), for
    /// [`Cascade::with_delay`](crate::submenu::Cascade::with_delay).
    pub fn submenu_delay(&self) -> Duration {
        Duration::from_millis(u64::from(self.sane().submenu_delay_ms))
    }

    /// The appearance as `(key, value)` pairs, its numbers made [`sane`](Self::sane), in the
    /// order of [`KEYS`]. Numbers are written as Rust writes an `f32` (`8`, `0.6`), which reads
    /// back to the same number.
    pub fn write(&self) -> Vec<(String, String)> {
        let a = self.sane();
        let values = [
            a.scheme.name().to_string(),
            a.accent.name(),
            a.radius.to_string(),
            a.shadow.to_string(),
            a.menu_row.to_string(),
            if a.joined { "on" } else { "off" }.to_string(),
            a.motion.name().to_string(),
            a.submenu_delay_ms.to_string(),
        ];
        KEYS.iter()
            .zip(values)
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    /// Reads one pair: true when `key` is one of [`KEYS`], whatever its value, so a caller that
    /// passes every pair of a text knows the others are its own; false for any other key, and
    /// nothing changes. A value that does not read (a word misspelt, a number that is not one
    /// or is not finite) leaves the field as it was; a number out of its range is held to it.
    /// White space round the value is ignored.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let value = value.trim();
        let number = || value.parse::<f32>().ok().filter(|v| v.is_finite());
        match key {
            "scheme" => {
                if let Some(s) = Scheme::from_name(value) {
                    self.scheme = s;
                }
            }
            "accent" => {
                if let Some(a) = AccentFrom::from_name(value) {
                    self.accent = a;
                }
            }
            "radius" => {
                if let Some(v) = number() {
                    self.radius = v.clamp(0.0, MAX_RADIUS);
                }
            }
            "shadow" => {
                if let Some(v) = number() {
                    self.shadow = v.clamp(0.0, MAX_SHADOW);
                }
            }
            "menu_row" => {
                if let Some(v) = number() {
                    self.menu_row = v.clamp(MIN_MENU_ROW, MAX_MENU_ROW);
                }
            }
            "joined" => match value.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" => self.joined = true,
                "off" | "false" | "0" => self.joined = false,
                _ => {}
            },
            "motion" => {
                if let Some(s) = Speed::from_name(value) {
                    self.motion = s;
                }
            }
            "submenu_delay" => {
                if let Ok(ms) = value.parse::<u64>() {
                    self.submenu_delay_ms = ms.min(u64::from(MAX_SUBMENU_DELAY_MS)) as u32;
                }
            }
            _ => return false,
        }
        true
    }

    /// Reads every pair that is one of [`KEYS`] ([`read`](Self::read)), in order, so a later
    /// pair wins over an earlier one, and gives back the others, in their order: the
    /// application's own keys, kept in the same text.
    pub fn read_all(
        &mut self,
        pairs: impl IntoIterator<Item = (String, String)>,
    ) -> Vec<(String, String)> {
        pairs
            .into_iter()
            .filter(|(k, v)| !self.read(k, v))
            .collect()
    }
}

impl Default for Appearance {
    /// [`Appearance::DEFAULT`].
    fn default() -> Appearance {
        Appearance::DEFAULT
    }
}

/// The longest text [`parse`] reads, in bytes; a longer one is refused whole.
pub const MAX_TEXT: usize = 64 * 1024;
/// The most pairs [`parse`] reads and [`to_text`] writes; the rest are left out.
pub const MAX_PAIRS: usize = 256;
/// The longest key, in bytes.
pub const MAX_KEY: usize = 64;
/// The longest value as written (escaped), in bytes.
pub const MAX_VALUE: usize = 4096;

fn good_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY
        && !key.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The text of one setting: a header line `name version`, then a line `key value` for each
/// pair, each line ended by `\n`. The value is escaped as the settings file escapes values
/// ([`mdshared_app::settings::escape`]: `%`, line feeds and carriage returns as `%25`, `%0A`,
/// `%0D`), so a value of any text stays on its line; spaces in it are kept as they are. A pair
/// whose key is empty, longer than [`MAX_KEY`] or has white space or a control character in it,
/// or whose escaped value is longer than [`MAX_VALUE`], is left out, as are the pairs after the
/// first [`MAX_PAIRS`], so what is written always [`parse`]s back to the same pairs.
///
/// `name` is one word that says whose text it is (`mdplay-look`); `version` lets a later
/// application read an older text differently. The text has line breaks: an application that
/// keeps it as one value of a `key=value` settings file escapes it again with
/// [`mdshared_app::settings::escape`].
pub fn to_text(name: &str, version: u32, pairs: &[(String, String)]) -> String {
    let mut out = format!("{name} {version}\n");
    let mut written = 0;
    for (key, value) in pairs {
        if written == MAX_PAIRS {
            break;
        }
        let value = mdshared_app::settings::escape(value);
        if !good_key(key) || value.len() > MAX_VALUE {
            continue;
        }
        out.push_str(key);
        if !value.is_empty() {
            out.push(' ');
            out.push_str(&value);
        }
        out.push('\n');
        written += 1;
    }
    out
}

/// Reads a text [`to_text`] wrote: its version and its pairs, in order, with every key kept,
/// known or not (the application's own keys and keys of a later version pass through), values
/// unescaped. `None` when the text is longer than [`MAX_TEXT`] or its first line that is not
/// blank is not `name` and a version number (another application's text, or none).
///
/// It takes any text without panicking and reads what it can: a line ending `\r\n` is a line,
/// a leading byte-order mark is skipped, blank lines are skipped, a line with no space is a key
/// with an empty value, a line whose key or value would not have been written (see
/// [`to_text`]) is skipped, and pairs after the first [`MAX_PAIRS`] are left out. A text cut
/// short gives the pairs before the cut, the last one perhaps with its value cut, which
/// [`Appearance::read`] then refuses or holds to its range.
pub fn parse(text: &str, name: &str) -> Option<(u32, Vec<(String, String)>)> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let (word, version) = lines.next()?.trim().split_once(' ')?;
    if word != name || name.is_empty() {
        return None;
    }
    let version = version.trim().parse::<u32>().ok()?;
    let mut pairs = Vec::new();
    for line in lines {
        if pairs.len() == MAX_PAIRS {
            break;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if !good_key(key) || value.len() > MAX_VALUE {
            continue;
        }
        pairs.push((key.to_string(), mdshared_app::settings::unescape(value)));
    }
    Some((version, pairs))
}

#[cfg(test)]
#[path = "appearance_tests.rs"]
mod tests;
