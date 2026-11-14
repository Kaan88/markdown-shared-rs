//! Bitmap, text shaping and rasterisation for applications that draw their own UI. No window system.

pub mod bitmap;
mod fonts;
pub mod text;

pub use bitmap::{Bitmap, decode_png_rgba};
pub use fonts::{
    EMOJI_FAMILIES, add_fallbacks, emoji_presentation_runs, push_emoji_runs, shared_fonts,
};
pub use text::{CacheStats, Subpixel, TextEngine};
