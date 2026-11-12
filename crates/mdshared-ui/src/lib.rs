//! Shared UI pieces for the Markdown viewer and editor: a software painter, palettes, menu, text
//! field, file tree, key parsing and OS glue. No window loop; the application owns that.

pub mod gfx;
pub mod keys;
pub mod menu;
pub mod platform;
pub mod textfield;
pub mod theme;
pub mod tree;
