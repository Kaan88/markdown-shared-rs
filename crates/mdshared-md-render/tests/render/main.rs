//! Pixel-level smoke tests for the headless renderer: a crash, a blank page, a lost theme, a lost
//! clamp, and the subpixel channel order. `golden.rs` compares whole pages against `tests/golden/`.

#[path = "../common/mod.rs"]
mod common;
use common::engine_available;

use mdshared_md_render::{DARK, DocumentRenderer, LIGHT, Options, render, render_slice};
use std::path::PathBuf;

fn fixture(name: &str) -> mdshared_md_core::Document {
    // A fixture can contain formulas or diagrams; decide on the engine before any render.
    engine_available();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/md")
        .join(name);
    mdshared_md_core::parse_bytes(&std::fs::read(path).unwrap())
}

fn background(r: &mdshared_md_render::Rendered, _theme: mdshared_md_render::Theme) -> (u8, u8, u8) {
    let p = &r.bitmap.pixels[0..4];
    (p[0], p[1], p[2])
}

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("mdshared-md-render-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn narrow(width: f32) -> Options {
    Options {
        width,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    }
}

fn rects_of(
    renderer: &mut DocumentRenderer,
    doc: &mdshared_md_core::Document,
    options: &Options,
    word: &str,
) -> Vec<mdshared_md_render::Rect> {
    let at = doc.text.find(word).expect("the word is in the document") as u32;
    renderer.selection_rects(doc, options, at, at + word.len() as u32)
}

/// The page for `markdown` in the light theme, as BGRA rows.
fn page(markdown: &str) -> mdshared_md_render::Rendered {
    engine_available();
    render(&mdshared_md_core::parse(markdown), &Options::light())
}

/// Columns holding a pixel darker than the page, as `(first, last)`.
fn ink_columns(r: &mdshared_md_render::Rendered) -> (usize, usize) {
    let width = r.bitmap.width as usize;
    let mut span = (usize::MAX, 0);
    for (i, p) in r.bitmap.pixels.as_chunks::<4>().0.iter().enumerate() {
        if p[1] < 0x80 {
            span = (span.0.min(i % width), span.1.max(i % width));
        }
    }
    span
}

fn count(r: &mdshared_md_render::Rendered, bgr: [u8; 3]) -> usize {
    r.bitmap
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3] == bgr)
        .count()
}

mod hits_and_selection;
mod images_and_frames;
mod inline_content;
mod table_and_flow;
