//! A terminal's grid drawn with the painter: a [`mdshared_term::Screen`] put into a rectangle,
//! one character per cell on a fixed grid (roadmap T1). Feature `term`.
//!
//! The pieces:
//!
//! - [`CellFont`]: the font of the grid and its caches. It measures the cells
//!   ([`CellFont::metrics`]) and draws each character from a layout shaped once, in any colour.
//! - [`Metrics`]: the cell size in whole device pixels, and the geometry of the grid: how many
//!   columns and rows fit a rectangle ([`Metrics::grid_size`], for resizing the screen and the
//!   pseudo-terminal), the cell under a point ([`Metrics::cell_at`], for mouse reports), the
//!   rectangle of a cell and of a row.
//! - [`Scheme`]: the colours: the sixteen, the 256 and true colour, defaults, cursor and
//!   selection, made from the toolkit's [`Palette`](crate::theme::Palette) for light and dark.
//! - [`View`]: the application's state of the grid: the scroll offset into the scrollback,
//!   the focus, and the questions that need it (the line under a point for a selection, the
//!   cursor's row, the rows to redraw).
//! - [`draw_screen`] and [`draw_rows`] draw; [`draw_screen_animated`] and [`draw_rows_animated`]
//!   do the same with a blinking cursor ([`cursor_blinks_on`]).
//!
//! The application owns everything else: the screen and its bytes, the selection and the
//! pointer events that make it, the scroll offset, the focus, the bitmap and when it is
//! presented. Nothing here keeps a rectangle, reads a clock or changes the screen.
//!
//! # Redrawing only what changed
//!
//! A row is drawn on its own, clipped to its band, and everything it shows is drawn in that
//! band each time (its background first), so a row drawn by [`draw_rows`] is pixel for pixel the
//! row [`draw_screen`] draws. The cheapest correct way with this painter is therefore to keep the
//! last frame's bitmap and draw only the rows that changed over it, then present those bands
//! ([`Metrics::row_rect`]): no scrolling of pixels, no layers. After [`Screen::feed`]:
//!
//! ```ignore
//! let dirty = screen.take_dirty();
//! let rows = view.damage(&screen, &dirty);
//! draw_rows(&mut p, &mut font, &scheme, &metrics, area, &screen, selection.as_ref(), view, &rows);
//! ```
//!
//! and [`draw_screen`] after anything the screen cannot know of: a resize, a new scheme or font
//! size, the focus, the scroll offset, a changed selection (or just the rows of the old and the
//! new one). A whole 200 by 60 grid of text costs a few milliseconds (the speed test in the
//! tests says how many on the machine it runs on); the damaged rows of a shell's output, a
//! fraction of that.
//!
//! # A translucent background
//!
//! [`draw_screen_with_background`] and [`draw_rows_with_background`] produce premultiplied
//! pixels ready to present. Ordinary scrollbars (including animated and horizontal variants)
//! and [`Painter::text`](crate::gfx::Painter::text), for example for IME preedit or search labels,
//! assume an opaque destination. Put them in an opaque gutter or paint an opaque backing under
//! their full bounds, including antialiased edges, before drawing them. Do not mark a
//! translucent terminal area as frosted chrome.
//!
//! # A frame, and the pointer
//!
//! ```ignore
//! // Once: the font and its cells, again when the size or the scale changes.
//! let mut font = CellFont::new();
//! let metrics = font.metrics(14.0, scale);
//! // On a resize: as many cells as fit, for the screen and the pseudo-terminal.
//! let (cols, rows) = metrics.grid_size(area);
//! screen.resize(cols, rows);
//! // Each frame:
//! let scheme = Scheme::from_palette(&pal);
//! let view = View::new().scrolled(scroll).focused(window_focused);
//! draw_screen_animated(&mut p, &mut font, &scheme, &metrics, area, &screen, sel.as_ref(),
//!     view, &mut anim, "terminal-cursor", platform::caret_blink_time());
//! // A press: a mouse report in screen cells, or the start of a selection in lines.
//! if let Some((row, col)) = metrics.cell_at(area, x, y) { /* encode a report */ }
//! sel = Some(Selection::start(&screen, view.point_at(&screen, &metrics, area, x, y), kind, clicks));
//! ```
//!
//! # Deliberately not here
//!
//! - Reading the screen's bytes, the pseudo-terminal, keys, the clipboard: the application's,
//!   with `mdshared_term::input` for the bytes a key becomes.
//! - Named colour schemes and a user's own (roadmap T7): [`Scheme`] is a struct with public
//!   fields and room to grow, and nothing more.
//! - The blink attribute (SGR 5): text is drawn steadily; only the cursor blinks.
//! - Underline styles and colours, hyperlinks, images: the screen does not keep them.
//! - Ligatures and shaping across cells: each cell is shaped alone, which is what a grid needs
//!   (a terminal's text is not a paragraph), so programming ligatures do not join.
//! - Powerline and other private-use symbols drawn as shapes: they come from the font, or not.
//! - Smooth scrolling of the scrollback: the offset is whole lines.
//! - Text accessibility of the grid.
//!
//! # Tests
//!
//! `cargo test -p mdshared-ui --features term termview`. The sheet: `cargo test -p mdshared-ui
//! --features term termview_sheet -- --ignored`, then open `target/specimen/termview.png`,
//! `termview-125.png`, and `termview-150.png`. The speed of a full 200 by 60 frame: `cargo test --release -p
//! mdshared-ui --features term termview_speed -- --ignored --nocapture`.

mod backdrop;
mod boxes;
mod draw;
mod font;
mod metrics;
mod scheme;
mod view;

#[cfg(test)]
mod backdrop_tests;
#[cfg(test)]
mod tests;

pub use backdrop::BackgroundOpacity;
pub use boxes::is_box_char;
pub use draw::{
    cursor_blinks_on, draw_rows, draw_rows_animated, draw_rows_with_background, draw_screen,
    draw_screen_animated, draw_screen_with_background,
};
pub use font::{CellFont, MAX_SHAPED, MONO_FAMILY};
pub use metrics::Metrics;
pub use scheme::{CellColors, DARK_ANSI, LIGHT_ANSI, Scheme, xterm_color};
pub use view::View;

#[cfg(doc)]
use mdshared_term::Screen;
