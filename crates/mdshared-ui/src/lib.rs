//! Shared UI pieces for the Markdown viewer and editor: a software painter, palettes, menu, text
//! field, file tree, key parsing, OS glue, and the generic drawing of the shell (title row and
//! tabs, panes, scrollbar, tooltips, toasts, dialog frames). No window loop; the application owns that.

pub mod chrome;
pub mod desktop_entry;
pub mod gfx;
pub mod glyph;
pub mod keys;
pub mod menu;
pub mod notice;
pub mod overlay;
pub mod palette;
pub mod pane;
pub mod platform;
pub mod scroll;
pub mod settings;
pub mod textfield;
pub mod theme;
pub mod tree;
pub mod window;

#[cfg(test)]
mod specimen;
