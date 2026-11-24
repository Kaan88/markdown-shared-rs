//! Shared UI pieces for the Markdown viewer and editor: a software painter, palettes, menu, text
//! field, file tree, key parsing, OS glue, the generic drawing of the shell (title row and tabs,
//! panes, scrollbar, tooltips, toasts, dialog frames), and animation ([`anim`]). No window loop;
//! the application owns that.

pub mod anim;
pub mod blur;
pub mod cf_html;
pub mod check;
pub mod chrome;
pub mod corners;
pub mod desktop_entry;
pub mod frost;
pub mod gfx;
pub mod glyph;
pub mod keys;
pub mod menu;
pub mod menu_bar;
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
pub mod segments;
pub mod settings;
pub mod shadow;
pub mod snap;
pub mod stale;
pub mod textfield;
pub mod theme;
pub mod tree;
pub mod window;

#[cfg(test)]
mod specimen;
