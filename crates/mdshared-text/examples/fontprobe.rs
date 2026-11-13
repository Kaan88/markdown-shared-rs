//! Lays out text with a family string, prints which font each glyph came from and whether its glyph
//! id is 0 (a missing-glyph box), and writes the rendering to a PNG.
//!
//! `cargo run -p mdshared-text --example fontprobe -- [out.png] [text...]`
//! The family string is the editor's by default; set `FAMILY` to change it.

use mdshared_text::{Bitmap, TextEngine, shared_fonts};
use parley::{FontFamily, LayoutContext, PositionedLayoutItem, StyleProperty};
use swash::{FontRef, StringId};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "fontprobe.png".into());
    let mut lines: Vec<String> = args.collect();
    if lines.is_empty() {
        lines = vec![
            "x \u{1F600} x".into(),
            "emoji: \u{1F642}. symbols: \u{2713} \u{1D11E} \u{4F24} \u{2603}".into(),
        ];
    }
    let family = std::env::var("FAMILY").unwrap_or_else(|_| {
        "Consolas, Cascadia Mono, Noto Sans Mono, DejaVu Sans Mono, monospace".into()
    });
    let mut fonts = shared_fonts();
    let mut lcx = LayoutContext::<[u8; 4]>::new();
    let mut engine = TextEngine::new(4 << 20);
    let size: f32 = std::env::var("SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(32.0);
    let row = size * 1.8;
    let mut fb = Bitmap::new(900, (row * lines.len() as f32) as u32 + 20);
    fb.clear([255, 255, 255]);
    for (n, text) in lines.iter().enumerate() {
        let mut b = lcx.ranged_builder(&mut fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(family.as_str())));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::Brush([0, 0, 0, 255]));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        println!("line: {text}");
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                let font = run.run().font();
                let name = FontRef::from_index(font.data.as_ref(), font.index as usize)
                    .and_then(|f| {
                        f.localized_strings()
                            .find_by_id(StringId::Family, None)
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                let range = run.run().text_range();
                let glyphs: Vec<u32> = run.glyphs().map(|g| g.id).collect();
                println!(
                    "  run {:?} {:?} font {name:?}: glyph ids {glyphs:?}{}",
                    range,
                    &text[range.clone()],
                    if glyphs.contains(&0) {
                        "  <-- .notdef"
                    } else {
                        ""
                    }
                );
            }
        }
        layout.align(
            parley::Alignment::Start,
            parley::AlignmentOptions::default(),
        );
        engine.draw_layout(&mut fb, &layout, 10.0, 10.0 + row * n as f32);
    }
    fb.write_png(std::path::Path::new(&out)).unwrap();
    println!("wrote {out}");
}
