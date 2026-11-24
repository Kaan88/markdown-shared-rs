//! Shared UI pieces for the Markdown viewer and editor, and the toolkit of the applications
//! after them: a software painter, palettes, menu, text field, file tree, key parsing and a
//! command table, OS glue, the generic drawing of the shell (title row and tabs, panes,
//! scrollbar, tooltips, toasts, dialog frames), sliders, knobs and faders, table columns and
//! splitters, the models
//! of lists, grids and selections, and animation ([`anim`]). No window loop; the application
//! owns that. `docs/CATALOGUE.md` lists every module with its entry points.

pub mod anim;
pub mod blur;
pub mod cf_html;
pub mod check;
pub mod chrome;
pub mod commands;
pub mod corners;
pub mod cover;
pub mod desktop_entry;
pub mod frost;
pub mod gfx;
pub mod glyph;
pub mod grid;
pub mod keys;
pub mod knob;
pub mod media_session;
pub mod menu;
pub mod menu_bar;
pub mod meter;
pub mod notice;
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
pub mod settings;
pub mod shadow;
pub mod slider;
pub mod snap;
pub mod splitter;
pub mod stale;
pub mod table;
pub mod textfield;
pub mod theme;
pub mod thumbs;
pub mod tree;
pub mod waveform;
pub mod window;

#[cfg(test)]
mod specimen;
