//! What the terminal sends to the program: keys, pasted text, mouse events and focus changes as
//! the bytes an xterm would send for them, given the [`Modes`] the program has set.
//!
//! These are pure functions from an event to bytes; writing the bytes to the program is the
//! caller's. Text that was typed (what the window system delivers as text, an IME's commit
//! included) is not a key: it is sent as it is, in UTF-8, and needs no function. [`encode_key`]
//! is for the keys that are not text, and for letters held with Control or Alt.
//!
//! [`Key`] and [`Mods`] mirror `KeyName` and `Mods` of `mdshared_ui::keys`, variant for variant
//! (plus `Insert`). They are defined again here because that crate brings a window system with
//! it and this one must not; an application maps one to the other in a `match`.

use crate::screen::{Modes, MouseMode};

/// A key, as `mdshared_ui::keys::KeyName` names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Key {
    /// A printable key, lower case: for a letter held with Control or Alt. Plain typed text
    /// does not come through here.
    Char(char),
    /// A function key, from 1.
    F(u8),
    Tab,
    Escape,
    Enter,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

/// The modifiers held with a key or a mouse event. `ctrl` is the Control key (on a Mac, the key
/// marked Control, which is `Mods::control` of `mdshared_ui::keys` there, not Command).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };

    fn any(self) -> bool {
        self.ctrl || self.shift || self.alt
    }

    /// The modifier parameter of xterm's sequences: one, plus 1 for Shift, 2 for Alt and 4 for
    /// Control.
    fn parameter(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// The byte Control makes of a character, as on a VT keyboard: `Ctrl+A` is 1 and so on to
/// `Ctrl+Z`, and a few symbols and digits give the remaining control bytes. `None` for a
/// character Control does nothing to.
fn control_byte(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' | '`' => 0x00,
        '[' | '3' => 0x1B,
        '\\' | '4' => 0x1C,
        ']' | '5' => 0x1D,
        '^' | '6' | '~' => 0x1E,
        '_' | '7' | '/' | '-' => 0x1F,
        '8' | '?' => 0x7F,
        _ => return None,
    })
}

/// `CSI 1 ; m X` with modifiers, or the short form without: for the keys that end in a letter.
fn letter_key(letter: u8, mods: Mods, ss3: bool) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{}", mods.parameter(), letter as char).into_bytes()
    } else if ss3 {
        vec![0x1B, b'O', letter]
    } else {
        vec![0x1B, b'[', letter]
    }
}

/// `CSI n ~`, or `CSI n ; m ~` with modifiers: for the keys that are a number.
fn tilde_key(number: u8, mods: Mods) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{number};{}~", mods.parameter()).into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// The bytes a key sends, by xterm's conventions:
///
/// - arrows, Home and End: `ESC [ A` and so on, or `ESC O A` while the program has asked for
///   application cursor keys ([`Modes::app_cursor`]); with modifiers `ESC [ 1 ; m A`;
/// - Insert, Delete, Page Up and Page Down: `ESC [ n ~`, with modifiers `ESC [ n ; m ~`;
/// - F1 to F4: `ESC O P` to `ESC O S` (with modifiers `ESC [ 1 ; m P`); F5 to F20: `ESC [ n ~`
///   with xterm's numbers; a function key past F20 sends nothing;
/// - Enter: carriage return, with a line feed after it in new-line mode; Tab: a tab, or
///   `ESC [ Z` with Shift; Backspace: DEL (0x7F), or backspace (0x08) with Control; Escape and
///   Space: themselves, Space with Control being NUL;
/// - a character: itself in UTF-8, upper case with Shift if it is an ASCII letter; with Control
///   the control byte of a VT keyboard (`Ctrl+C` is 3, `Ctrl+[` is escape), or itself where
///   there is none;
/// - Alt puts an `ESC` before the bytes of the keys of the last two lines.
///
/// `m` is 1, plus 1 for Shift, 2 for Alt, 4 for Control.
pub fn encode_key(key: Key, mods: Mods, modes: &Modes) -> Vec<u8> {
    // The keys that are bytes of their own take Alt as a prefix.
    let prefixed = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if mods.alt {
            out.push(0x1B);
        }
        out.extend_from_slice(bytes);
        out
    };
    match key {
        Key::Up => letter_key(b'A', mods, modes.app_cursor),
        Key::Down => letter_key(b'B', mods, modes.app_cursor),
        Key::Right => letter_key(b'C', mods, modes.app_cursor),
        Key::Left => letter_key(b'D', mods, modes.app_cursor),
        Key::Home => letter_key(b'H', mods, modes.app_cursor),
        Key::End => letter_key(b'F', mods, modes.app_cursor),
        Key::Insert => tilde_key(2, mods),
        Key::Delete => tilde_key(3, mods),
        Key::PageUp => tilde_key(5, mods),
        Key::PageDown => tilde_key(6, mods),
        Key::F(n @ 1..=4) => letter_key(b'P' + n - 1, mods, true),
        Key::F(n @ 5..=20) => {
            const NUMBERS: [u8; 16] = [
                15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34,
            ];
            tilde_key(NUMBERS[n as usize - 5], mods)
        }
        Key::F(_) => Vec::new(),
        Key::Enter => prefixed(if modes.newline { b"\r\n" } else { b"\r" }),
        Key::Tab if mods.shift => prefixed(b"\x1b[Z"),
        Key::Tab => prefixed(b"\t"),
        Key::Escape => prefixed(b"\x1b"),
        Key::Backspace => prefixed(if mods.ctrl { b"\x08" } else { b"\x7f" }),
        Key::Space => prefixed(if mods.ctrl { b"\x00" } else { b" " }),
        Key::Char(c) => {
            if mods.ctrl
                && let Some(b) = control_byte(c)
            {
                return prefixed(&[b]);
            }
            let c = if mods.shift {
                c.to_ascii_uppercase()
            } else {
                c
            };
            prefixed(c.encode_utf8(&mut [0; 4]).as_bytes())
        }
    }
}

/// The bytes of a paste. Line ends become carriage returns, as if the lines had been typed.
/// With `bracketed` ([`Modes::bracketed_paste`]) the text goes between `ESC [ 200 ~` and
/// `ESC [ 201 ~`, so the program can tell a paste from typing; the end marker is taken out of
/// the text itself first, or pasted text could end the paste and type what follows it.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed {
        return text.into_bytes();
    }
    let mut body = text;
    // Removing one marker can put the two halves of another together.
    while body.contains("\x1b[201~") {
        body = body.replace("\x1b[201~", "");
    }
    let mut out = b"\x1b[200~".to_vec();
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// A mouse button, the wheel's two directions among them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// One notch of the wheel away from the user. Sent as a press; it has no release.
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum MouseKind {
    Press,
    Release,
    /// The pointer moved to another cell.
    Move,
}

/// A mouse event on the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct MouseEvent {
    pub kind: MouseKind,
    /// The button pressed or released; for a move, the button held, if any.
    pub button: Option<MouseButton>,
    /// The cell, from 0.
    pub col: usize,
    pub row: usize,
    pub mods: Mods,
}

/// The bytes of a mouse event, or `None` when the program has not asked for it: nothing while
/// [`Modes::mouse`] is off (the mouse is then the terminal's own, for selecting), moves only
/// with a button held in [`MouseMode::Drag`] and always in [`MouseMode::Motion`].
///
/// With [`Modes::mouse_sgr`] the report is `ESC [ < b ; col ; row M` (`m` for a release), cells
/// from 1, with no limit. Without it, the old form: `ESC [ M` and three bytes, which cannot
/// say a column or row past 223 (such an event is not sent) nor which button was released.
/// `b` is 0, 1, 2 for left, middle, right, 64 and 65 for the wheel, plus 32 for a move, 4 for
/// Shift, 8 for Alt and 16 for Control.
pub fn encode_mouse(event: MouseEvent, modes: &Modes) -> Option<Vec<u8>> {
    let wanted = match (modes.mouse, event.kind) {
        (MouseMode::Off, _) => false,
        (_, MouseKind::Press | MouseKind::Release) => true,
        (MouseMode::Press, MouseKind::Move) => false,
        (MouseMode::Drag, MouseKind::Move) => event.button.is_some(),
        (MouseMode::Motion, MouseKind::Move) => true,
    };
    let wheel = matches!(
        event.button,
        Some(MouseButton::WheelUp | MouseButton::WheelDown)
    );
    // A press or a release needs its button, and the wheel is never released or dragged.
    if !wanted
        || (event.kind != MouseKind::Move && event.button.is_none())
        || (event.kind != MouseKind::Press && wheel)
    {
        return None;
    }
    let mut b: u32 = match event.button {
        Some(MouseButton::Left) => 0,
        Some(MouseButton::Middle) => 1,
        Some(MouseButton::Right) => 2,
        Some(MouseButton::WheelUp) => 64,
        Some(MouseButton::WheelDown) => 65,
        None => 3,
    };
    if event.kind == MouseKind::Move {
        b += 32;
    }
    b += 4 * u32::from(event.mods.shift)
        + 8 * u32::from(event.mods.alt)
        + 16 * u32::from(event.mods.ctrl);
    let (x, y) = (event.col.saturating_add(1), event.row.saturating_add(1));
    if modes.mouse_sgr {
        let end = if event.kind == MouseKind::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{b};{x};{y}{end}").into_bytes());
    }
    if x > 223 || y > 223 {
        return None;
    }
    if event.kind == MouseKind::Release {
        // The old form has one code for every release.
        b = (b & !3) | 3;
    }
    Some(vec![
        0x1B,
        b'[',
        b'M',
        (b + 32) as u8,
        (x + 32) as u8,
        (y + 32) as u8,
    ])
}

/// The bytes that tell the program the window gained (`ESC [ I`) or lost (`ESC [ O`) the focus,
/// or `None` when it has not asked ([`Modes::focus`]).
pub fn encode_focus(focused: bool, modes: &Modes) -> Option<Vec<u8>> {
    let bytes: &[u8] = if focused { b"\x1b[I" } else { b"\x1b[O" };
    modes.focus.then(|| bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Mods = Mods::NONE;
    const SHIFT: Mods = Mods {
        shift: true,
        ..Mods::NONE
    };
    const ALT: Mods = Mods {
        alt: true,
        ..Mods::NONE
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        ..Mods::NONE
    };

    fn key(key: Key, mods: Mods) -> String {
        String::from_utf8(encode_key(key, mods, &Modes::default())).unwrap()
    }

    #[test]
    fn cursor_keys_follow_the_application_mode_and_take_a_modifier_parameter() {
        let app = Modes {
            app_cursor: true,
            ..Modes::default()
        };
        for (k, letter) in [
            (Key::Up, 'A'),
            (Key::Down, 'B'),
            (Key::Right, 'C'),
            (Key::Left, 'D'),
            (Key::Home, 'H'),
            (Key::End, 'F'),
        ] {
            assert_eq!(key(k, PLAIN), format!("\x1b[{letter}"));
            assert_eq!(
                encode_key(k, PLAIN, &app),
                format!("\x1bO{letter}").into_bytes()
            );
            assert_eq!(key(k, SHIFT), format!("\x1b[1;2{letter}"));
            assert_eq!(key(k, ALT), format!("\x1b[1;3{letter}"));
            assert_eq!(key(k, CTRL), format!("\x1b[1;5{letter}"));
            // With a modifier the application mode makes no difference.
            assert_eq!(
                encode_key(k, CTRL, &app),
                format!("\x1b[1;5{letter}").into_bytes()
            );
        }
        let all = Mods {
            ctrl: true,
            shift: true,
            alt: true,
        };
        assert_eq!(key(Key::Up, all), "\x1b[1;8A");
    }

    #[test]
    fn editing_and_function_keys_send_xterms_numbers() {
        assert_eq!(key(Key::Insert, PLAIN), "\x1b[2~");
        assert_eq!(key(Key::Delete, PLAIN), "\x1b[3~");
        assert_eq!(key(Key::PageUp, PLAIN), "\x1b[5~");
        assert_eq!(key(Key::PageDown, PLAIN), "\x1b[6~");
        assert_eq!(key(Key::Delete, CTRL), "\x1b[3;5~");
        assert_eq!(key(Key::PageUp, SHIFT), "\x1b[5;2~");
        let plain: Vec<String> = (1..=12).map(|n| key(Key::F(n), PLAIN)).collect();
        assert_eq!(
            plain,
            [
                "\x1bOP", "\x1bOQ", "\x1bOR", "\x1bOS", "\x1b[15~", "\x1b[17~", "\x1b[18~",
                "\x1b[19~", "\x1b[20~", "\x1b[21~", "\x1b[23~", "\x1b[24~"
            ]
        );
        assert_eq!(key(Key::F(1), SHIFT), "\x1b[1;2P");
        assert_eq!(key(Key::F(4), CTRL), "\x1b[1;5S");
        assert_eq!(key(Key::F(5), ALT), "\x1b[15;3~");
        assert_eq!(key(Key::F(13), PLAIN), "\x1b[25~");
        assert_eq!(key(Key::F(20), PLAIN), "\x1b[34~");
        assert_eq!(key(Key::F(21), PLAIN), "");
        assert_eq!(key(Key::F(0), PLAIN), "");
    }

    #[test]
    fn the_keys_that_are_bytes_and_what_control_shift_and_alt_do_to_them() {
        assert_eq!(key(Key::Enter, PLAIN), "\r");
        assert_eq!(key(Key::Enter, ALT), "\x1b\r");
        let newline = Modes {
            newline: true,
            ..Modes::default()
        };
        assert_eq!(encode_key(Key::Enter, PLAIN, &newline), b"\r\n");
        assert_eq!(key(Key::Tab, PLAIN), "\t");
        assert_eq!(key(Key::Tab, SHIFT), "\x1b[Z");
        assert_eq!(key(Key::Tab, CTRL), "\t");
        assert_eq!(key(Key::Escape, PLAIN), "\x1b");
        assert_eq!(key(Key::Escape, ALT), "\x1b\x1b");
        assert_eq!(key(Key::Backspace, PLAIN), "\x7f");
        assert_eq!(key(Key::Backspace, CTRL), "\x08");
        assert_eq!(key(Key::Backspace, ALT), "\x1b\x7f");
        assert_eq!(key(Key::Space, PLAIN), " ");
        assert_eq!(key(Key::Space, CTRL), "\0");

        assert_eq!(key(Key::Char('a'), PLAIN), "a");
        assert_eq!(key(Key::Char('a'), SHIFT), "A");
        assert_eq!(key(Key::Char('a'), ALT), "\x1ba");
        assert_eq!(key(Key::Char('a'), CTRL), "\x01");
        assert_eq!(key(Key::Char('c'), CTRL), "\x03");
        assert_eq!(key(Key::Char('z'), CTRL), "\x1a");
        assert_eq!(key(Key::Char('C'), CTRL), "\x03");
        assert_eq!(key(Key::Char('['), CTRL), "\x1b");
        assert_eq!(key(Key::Char('\\'), CTRL), "\x1c");
        assert_eq!(key(Key::Char(']'), CTRL), "\x1d");
        assert_eq!(key(Key::Char('/'), CTRL), "\x1f");
        assert_eq!(key(Key::Char('2'), CTRL), "\0");
        assert_eq!(key(Key::Char('8'), CTRL), "\x7f");
        // Control does nothing to these; Alt and Control together still prefix.
        assert_eq!(key(Key::Char('1'), CTRL), "1");
        assert_eq!(key(Key::Char(','), CTRL), ",");
        let both = Mods {
            ctrl: true,
            alt: true,
            shift: false,
        };
        assert_eq!(key(Key::Char('x'), both), "\x1b\x18");
        // Not ASCII: itself in UTF-8, and Shift leaves it alone.
        assert_eq!(key(Key::Char('é'), PLAIN), "é");
        assert_eq!(key(Key::Char('é'), SHIFT), "é");
        assert_eq!(key(Key::Char('ü'), ALT), "\x1bü");
    }

    #[test]
    fn a_paste_is_bracketed_when_asked_and_cannot_end_itself() {
        assert_eq!(encode_paste("one\ntwo\r\nthree", false), b"one\rtwo\rthree");
        assert_eq!(encode_paste("ls -l", true), b"\x1b[200~ls -l\x1b[201~");
        assert_eq!(encode_paste("a\nb", true), b"\x1b[200~a\rb\x1b[201~");
        // The end marker inside the text, also one put together by removing another.
        assert_eq!(
            encode_paste("safe\x1b[201~rm -rf", true),
            b"\x1b[200~saferm -rf\x1b[201~"
        );
        assert_eq!(
            encode_paste("a\x1b[20\x1b[201~1~b", true),
            b"\x1b[200~ab\x1b[201~"
        );
        assert_eq!(encode_paste("", true), b"\x1b[200~\x1b[201~");
        assert_eq!(encode_paste("", false), b"");
    }

    fn mouse(kind: MouseKind, button: Option<MouseButton>, col: usize, row: usize) -> MouseEvent {
        MouseEvent {
            kind,
            button,
            col,
            row,
            mods: PLAIN,
        }
    }

    fn modes(mouse: MouseMode, sgr: bool) -> Modes {
        Modes {
            mouse,
            mouse_sgr: sgr,
            ..Modes::default()
        }
    }

    #[test]
    fn mouse_reports_in_the_sgr_form() {
        let m = modes(MouseMode::Press, true);
        let left = Some(MouseButton::Left);
        let s = |e| encode_mouse(e, &m).map(|b| String::from_utf8(b).unwrap());
        assert_eq!(
            s(mouse(MouseKind::Press, left, 0, 0)).unwrap(),
            "\x1b[<0;1;1M"
        );
        assert_eq!(
            s(mouse(MouseKind::Release, left, 9, 4)).unwrap(),
            "\x1b[<0;10;5m"
        );
        assert_eq!(
            s(mouse(MouseKind::Press, Some(MouseButton::Middle), 2, 3)).unwrap(),
            "\x1b[<1;3;4M"
        );
        assert_eq!(
            s(mouse(MouseKind::Press, Some(MouseButton::Right), 2, 3)).unwrap(),
            "\x1b[<2;3;4M"
        );
        assert_eq!(
            s(mouse(MouseKind::Press, Some(MouseButton::WheelUp), 2, 3)).unwrap(),
            "\x1b[<64;3;4M"
        );
        assert_eq!(
            s(mouse(MouseKind::Press, Some(MouseButton::WheelDown), 2, 3)).unwrap(),
            "\x1b[<65;3;4M"
        );
        // No limit on the cell.
        assert_eq!(
            s(mouse(MouseKind::Press, left, 999, 499)).unwrap(),
            "\x1b[<0;1000;500M"
        );
        // Modifiers add to the button.
        let held = MouseEvent {
            mods: Mods {
                ctrl: true,
                shift: true,
                alt: true,
            },
            ..mouse(MouseKind::Press, left, 0, 0)
        };
        assert_eq!(s(held).unwrap(), "\x1b[<28;1;1M");
        // A press with no button, and a wheel that is released or dragged, are nothing.
        assert_eq!(s(mouse(MouseKind::Press, None, 0, 0)), None);
        assert_eq!(
            s(mouse(MouseKind::Release, Some(MouseButton::WheelUp), 0, 0)),
            None
        );
        assert_eq!(
            s(mouse(MouseKind::Move, Some(MouseButton::WheelDown), 0, 0)),
            None
        );
    }

    #[test]
    fn moves_are_sent_only_in_the_modes_that_ask_for_them() {
        let left = Some(MouseButton::Left);
        let drag = mouse(MouseKind::Move, left, 4, 2);
        let hover = mouse(MouseKind::Move, None, 4, 2);
        let press = mouse(MouseKind::Press, left, 4, 2);
        let s =
            |e, mode| encode_mouse(e, &modes(mode, true)).map(|b| String::from_utf8(b).unwrap());
        for e in [drag, hover, press] {
            assert_eq!(s(e, MouseMode::Off), None);
        }
        assert_eq!(s(drag, MouseMode::Press), None);
        assert_eq!(s(hover, MouseMode::Press), None);
        assert!(s(press, MouseMode::Press).is_some());
        assert_eq!(s(drag, MouseMode::Drag).unwrap(), "\x1b[<32;5;3M");
        assert_eq!(s(hover, MouseMode::Drag), None);
        assert_eq!(s(drag, MouseMode::Motion).unwrap(), "\x1b[<32;5;3M");
        assert_eq!(s(hover, MouseMode::Motion).unwrap(), "\x1b[<35;5;3M");
    }

    #[test]
    fn the_old_mouse_form_has_one_release_and_a_limit() {
        let m = modes(MouseMode::Motion, false);
        let left = Some(MouseButton::Left);
        let right = Some(MouseButton::Right);
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, left, 0, 0), &m).unwrap(),
            b"\x1b[M !!"
        );
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, right, 9, 4), &m).unwrap(),
            b"\x1b[M\"*%"
        );
        // Every release is button 3.
        assert_eq!(
            encode_mouse(mouse(MouseKind::Release, right, 0, 0), &m).unwrap(),
            b"\x1b[M#!!"
        );
        assert_eq!(
            encode_mouse(
                mouse(MouseKind::Press, Some(MouseButton::WheelUp), 0, 0),
                &m
            )
            .unwrap(),
            b"\x1b[M`!!"
        );
        assert_eq!(
            encode_mouse(mouse(MouseKind::Move, None, 0, 0), &m).unwrap(),
            b"\x1b[MC!!"
        );
        // Cell 223 is the last it can say.
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, left, 222, 222), &m).unwrap(),
            [0x1b, b'[', b'M', 32, 255, 255]
        );
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, left, 223, 0), &m),
            None
        );
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, left, 0, 223), &m),
            None
        );
        assert_eq!(
            encode_mouse(mouse(MouseKind::Press, left, usize::MAX, 0), &m),
            None
        );
    }

    #[test]
    fn focus_is_reported_only_when_asked() {
        let off = Modes::default();
        let on = Modes {
            focus: true,
            ..Modes::default()
        };
        assert_eq!(encode_focus(true, &off), None);
        assert_eq!(encode_focus(true, &on).unwrap(), b"\x1b[I");
        assert_eq!(encode_focus(false, &on).unwrap(), b"\x1b[O");
    }
}
