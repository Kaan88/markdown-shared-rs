//! Shared UI pieces for the Markdown viewer and editor, and the toolkit of the applications
//! after them: a software painter, palettes, menu, text field, file tree, key parsing and a
//! command table, OS glue, the generic drawing of the shell (title row and tabs, panes,
//! scrollbar, tooltips, toasts, dialog frames), info bars, sliders, knobs and faders, drop-down
//! lists and toggle switches ([`dropdown`], [`toggle`]), expanders, anchored flyouts, table
//! columns and splitters, link buttons, the cursors the widgets ask for, the models
//! of lists, grids and selections, the keyboard focus over the widgets a frame declares
//! ([`focus`]), animation ([`anim`]), the look as a user setting ([`appearance`]), and with the
//! feature `a11y` an accessibility tree built from the same models (`a11y`). No window loop; the
//! application owns that, or hands it for one window to `host` (feature `host`).
//! `docs/CATALOGUE.md` lists every module with its
//! entry points.

#[cfg(feature = "a11y")]
pub mod a11y;
pub mod anim;
pub mod appearance;
pub mod blur;
pub mod cf_html;
pub mod check;
pub mod chrome;
pub mod commands;
pub mod corners;
pub mod cover;
pub mod cursor;
pub mod desktop_entry;
pub mod dropdown;
pub mod expander;
pub mod flyout;
pub mod focus;
pub mod frost;
pub mod gfx;
pub mod glyph;
pub mod grid;
#[cfg(feature = "host")]
pub mod host;
pub mod infobar;
pub mod keys;
pub mod knob;
pub mod link;
pub mod media_session;
pub mod menu;
pub mod menu_bar;
pub mod meter;
pub mod notice;
pub mod notify;
pub mod open_documents;
pub mod overlay;
pub mod palette;
pub mod pane;
pub mod pins;
pub mod place;
pub mod plan;
pub mod platform;
pub mod rows;
pub mod scroll;
pub mod sections;
pub mod seek;
pub mod segments;
pub mod select;
pub mod seltext;
pub mod settings;
pub mod shadow;
pub mod slider;
pub mod snap;
pub mod spectrum;
pub mod splitter;
pub mod stale;
pub mod submenu;
pub mod table;
pub mod textfield;
pub mod theme;
pub mod thumbs;
pub mod toggle;
pub mod tokens;
pub mod tooltip;
pub mod tray;
pub mod tree;
pub mod viewport;
pub mod waveform;
pub mod window;
pub mod window_state;

#[cfg(test)]
mod specimen;
