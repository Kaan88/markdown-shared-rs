//! The model of a terminal emulator, with no window, no font and no process: bytes from a
//! program go in, a grid of cells comes out, and keys and mouse events are turned into the bytes
//! the program expects. An application puts a pseudo-terminal on one side and a painter on the
//! other; neither is here, and this crate depends on nothing.
//!
//! - [`parse`]: the byte-stream parser. UTF-8 text, control bytes and escape sequences, told
//!   apart and handed to a [`Perform`].
//! - [`screen`]: the [`Screen`], which is what the parser's calls mean: cells with colours and
//!   attributes, the cursor, the scrollback, the alternate screen, scroll regions, modes, the
//!   rows that changed, and the replies owed to the program.
//! - [`select`]: a [`Selection`] of cells over the scrollback and the screen, by cell, word or
//!   line, as a stream or a block, its text, and whether the cells under it still hold that text
//!   after the program wrote to them.
//! - [`input`]: keys, pastes, mouse events and focus changes as bytes for the program.
//! - [`width`]: how many cells a character takes.
//!
//! The loop an application runs:
//!
//! ```
//! use mdshared_term::{Key, Mods, Screen, input};
//!
//! let mut screen = Screen::new(80, 24, 10_000);
//! // Bytes read from the program:
//! screen.feed(b"\x1b[1;32muser@host\x1b[0m:~$ \x1b[6n");
//! assert_eq!(screen.row_text(0), "user@host:~$");
//! // What the program asked for goes back to it:
//! assert_eq!(screen.take_reply(), b"\x1b[1;14R");
//! // Rows to repaint, and where the cursor is:
//! assert_eq!(screen.take_dirty(), [0..24]);
//! assert_eq!((screen.cursor().row, screen.cursor().col), (0, 13));
//! // A key, as bytes to write to the program:
//! assert_eq!(input::encode_key(Key::Up, Mods::NONE, &screen.modes()), b"\x1b[A");
//! ```
//!
//! # What is not here
//!
//! The solid core of a VT220-style terminal as xterm does it is here ([`Screen::feed`] lists
//! it). These are known to be missing, in rough order of how soon a user meets them:
//!
//! - **Combining marks and grapheme clusters.** A character of no width (an accent that
//!   combines, a variation selector, a zero-width joiner) is dropped, so decomposed text loses
//!   its accents and an emoji sequence shows as its separate emoji. Widths come from a small
//!   hand-written table ([`width`]), not from the Unicode data.
//! - **Hyperlinks (OSC 8), the clipboard (OSC 52), colour queries and palette changes
//!   (OSC 4, 10, 11, 104).** Only the title (OSC 0 and 2) is read.
//! - **Underline styles and colours** (`4:3`, SGR 58): any underline is the one underline.
//! - **Synchronized output (mode 2026)**, **left and right margins (DECLRMM)**, **rectangle
//!   operations (DECCRA and the like)**, **selective erase (DECSCA)**, **132 columns
//!   (DECCOLM)**, **double-width and double-height lines**.
//! - **Character sets** other than ASCII and DEC special graphics, and the single shifts.
//! - **The kitty keyboard protocol and xterm's `modifyOtherKeys`**: keys are sent the classic
//!   way, so `Ctrl+I` is Tab and `Ctrl+Shift+A` is `Ctrl+A`. The application keypad mode is
//!   recorded ([`Modes::app_keypad`]) but [`input`] has no keypad keys.
//! - **Mouse**: the SGR and the old X10 forms; not the UTF-8 (1005) and urxvt (1015) forms, not
//!   pixel positions (1016), not highlight tracking.
//! - **Images**: sixel, the kitty and iTerm2 protocols. Device control strings are swallowed.
//! - **Right-to-left text.**
//!
//! Nor is anything here that needs the system: a pseudo-terminal (ConPTY, `openpty`), a child
//! process, drawing a grid of glyphs, the bell's sound.

mod line;

pub mod input;
pub mod parse;
pub mod screen;
pub mod select;
pub mod width;

mod reflow;

#[cfg(test)]
mod reflow_tests;
#[cfg(test)]
mod screen_tests;

pub use input::{Key, Mods, MouseButton, MouseEvent, MouseKind};
pub use parse::{Params, Parser, Perform};
pub use screen::{Attrs, Cell, Color, Cursor, CursorShape, Line, Modes, MouseMode, Screen};
pub use select::{Point, Selection, SelectionKind};
pub use width::{char_width, str_width};
