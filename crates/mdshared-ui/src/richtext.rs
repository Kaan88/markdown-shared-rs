//! Rich text a caller shapes itself: where each run of a parley layout went, and the `parley`
//! this toolkit draws with, re-exported so an application names one version of it.
//!
//! A caller builds a layout with `LayoutContext<Brush>` (see [`Brush`]: a colour per run plus a
//! baseline shift byte), shapes it at the device size the painter will draw it at, that is its
//! own `scale` times the size in logical px, and draws it with
//! [`Painter::rich_text`](crate::gfx::Painter::rich_text). [`runs`] says where each run went, so
//! a caller can put a [`wavy_underline`](crate::gfx::Painter::wavy_underline) under a misspelled
//! word, a highlight behind a run, or a caret between two runs, without walking the layout
//! again.
//!
//! Deliberately not here: shaping, wrapping and alignment (parley does those), the font
//! contexts (the caller keeps them, as it keeps the layout), hit testing a point to a byte, the
//! selection, and the colours a run should be given (the application's own rules).

pub use crate::gfx::Brush;
pub use parley;

use crate::gfx::Rect;
use parley::{Layout, PositionedLayoutItem};
use std::ops::Range;

/// One glyph run of a layout: the text it covers and the box it was placed in.
///
/// The numbers are in the layout's own units, the device pixels the caller shaped it at: a
/// caller that shaped at `size * scale` divides by its scale for logical px, or draws with the
/// same scale the painter is given. A wrapped layout gives one entry per run on each line, in
/// reading order, so the runs of one line follow each other and a later line's runs come after.
///
/// `#[non_exhaustive]`: a field may be added without breaking an application.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct RunBox {
    /// The run's byte range in the text the layout was built from.
    pub range: Range<usize>,
    /// The run's advance across its line, and its line's content height: the left edge at the
    /// first glyph, the top of the line's content, the advance wide and as tall as the content
    /// box. A glyph may reach a little past it (a tall accent, a swash).
    pub rect: Rect,
    /// The baseline of the run's line, from the top of the layout, in the same units.
    pub baseline: f32,
}

/// One atom of a run: a shaped cluster (one grapheme, or a whole ligature), the style its first
/// character asks for, and whether it has a glyph of its own.
struct Atom {
    range: Range<usize>,
    style: u16,
    glyphs: bool,
}

/// The runs of `layout`, line by line and in reading order, with their byte ranges and boxes.
///
/// A run is a stretch of text parley placed in one style: a change of colour, of family, of size
/// or of weight begins another, and a run that wraps continues on the next line as another entry
/// with the rest of its range. Two things do not begin one, because parley shapes them as one
/// piece: the second half of a ligature, whatever style it asks for (a brush change does not
/// split shaping, so `fi` may stay one glyph with the `i` in another colour), and a cluster with
/// no glyph of its own (a soft hyphen), whose bytes join the run next to it. The work is linear
/// in the layout's length.
pub fn runs(layout: &Layout<Brush>) -> Vec<RunBox> {
    let mut out = Vec::new();
    for line in layout.lines() {
        let metrics = line.metrics();
        let top = metrics.content_block_min_coord;
        let height = (metrics.content_block_max_coord - top).max(0.0);
        let mut ranges: Vec<(usize, Vec<Range<usize>>)> = Vec::new();
        for run in line.runs() {
            // Parley groups its glyph runs by shaped cluster (an atom: one grapheme, or a whole
            // ligature) and by style, and places them in visual order. The clusters come in
            // logical order, so the atoms are built the same way and then turned round for a
            // right-to-left run; grouping them by style gives the runs the glyph runs were made
            // from, in their order.
            let mut atoms: Vec<Atom> = Vec::new();
            for cluster in run.clusters() {
                let range = cluster.text_range();
                let glyphs = cluster.glyphs().next().is_some();
                match atoms.last_mut() {
                    Some(atom) if cluster.is_ligature_continuation() => {
                        atom.range.start = atom.range.start.min(range.start);
                        atom.range.end = atom.range.end.max(range.end);
                        atom.glyphs |= glyphs;
                    }
                    _ => atoms.push(Atom {
                        range,
                        style: cluster.style_index(),
                        glyphs,
                    }),
                }
            }
            if run.is_rtl() {
                atoms.reverse();
            }
            let mut groups: Vec<Range<usize>> = Vec::new();
            let mut style: Option<u16> = None;
            let mut waiting: Option<Range<usize>> = None;
            for atom in atoms {
                let mut range = atom.range;
                if let Some(before) = waiting.take() {
                    range.start = range.start.min(before.start);
                    range.end = range.end.max(before.end);
                }
                if !atom.glyphs {
                    // An atom without a glyph of its own has no glyph run: its bytes join the
                    // group before it, or wait for the one after it, so no byte is left out.
                    match groups.last_mut() {
                        Some(last) => {
                            last.start = last.start.min(range.start);
                            last.end = last.end.max(range.end);
                        }
                        None => waiting = Some(range),
                    }
                    continue;
                }
                match groups.last_mut() {
                    Some(last) if style == Some(atom.style) => {
                        last.start = last.start.min(range.start);
                        last.end = last.end.max(range.end);
                    }
                    _ => {
                        style = Some(atom.style);
                        groups.push(range);
                    }
                }
            }
            ranges.push((run.index(), groups));
        }
        let mut used = vec![0usize; ranges.len()];
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let index = glyph_run.run().index();
            let mut range = glyph_run.run().text_range();
            if let Some(slot) = ranges.iter().position(|(i, _)| *i == index) {
                if let Some(found) = ranges[slot].1.get(used[slot]) {
                    range = found.clone();
                }
                used[slot] += 1;
            }
            out.push(RunBox {
                range,
                rect: Rect::new(glyph_run.offset(), top, glyph_run.advance(), height),
                baseline: metrics.baseline,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::{Alignment, AlignmentOptions, FontFamily, LayoutContext, StyleProperty};

    const RED: Brush = [200, 30, 30, 255];
    const BLUE: Brush = [20, 60, 220, 255];

    /// A layout of `text` at `size` device px in the interface family, every run in `base`
    /// except what `add` says otherwise; no wrapping until a test asks for it.
    fn layout(
        text: &str,
        size: f32,
        base: Brush,
        add: impl FnOnce(&mut parley::RangedBuilder<'_, Brush>),
    ) -> Layout<Brush> {
        layout_in(
            "Segoe UI, Noto Sans, DejaVu Sans, sans-serif",
            text,
            size,
            base,
            add,
        )
    }

    /// [`layout`] in the family list `family`.
    fn layout_in(
        family: &str,
        text: &str,
        size: f32,
        base: Brush,
        add: impl FnOnce(&mut parley::RangedBuilder<'_, Brush>),
    ) -> Layout<Brush> {
        let mut fonts = mdshared_text::shared_fonts();
        let mut layouts = LayoutContext::<Brush>::new();
        let mut b = layouts.ranged_builder(&mut fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(family)));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::Brush(base));
        add(&mut b);
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    /// Whether any cluster of `layout` is the second half of a ligature, i.e. whether the font
    /// the text was shaped in has the ligature a test needs.
    fn ligates(layout: &Layout<Brush>) -> bool {
        layout
            .lines()
            .flat_map(|line| line.runs())
            .any(|run| run.clusters().any(|c| c.is_ligature_continuation()))
    }

    #[test]
    fn each_run_gives_its_text_and_the_box_it_was_placed_in() {
        let layout = layout("wrong word", 26.0, RED, |b| {
            b.push(StyleProperty::Brush(BLUE), 6..)
        });
        let boxes = runs(&layout);
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert_eq!(boxes[0].range, 0..6);
        assert_eq!(boxes[1].range, 6..10);
        // The second run starts where the first ends, on the same baseline.
        let (a, b) = (boxes[0].rect, boxes[1].rect);
        assert!((a.right() - b.x).abs() < 0.5, "{a:?} then {b:?}");
        assert_eq!(boxes[0].baseline, boxes[1].baseline);
        assert_eq!(a.y, b.y);
        // Both words are there, and the boxes stay inside the layout's width.
        assert!(a.w > 10.0 && b.w > 10.0);
        assert!(b.right() <= layout.width() + 1.0);
    }

    #[test]
    fn a_wrapped_layout_reports_its_runs_line_by_line() {
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let marked = text.find("five").expect("the marked word");
        let mut layout = layout(text, 18.0, RED, |b| {
            b.push(StyleProperty::Brush(BLUE), marked..marked + 4)
        });
        layout.break_all_lines(Some(90.0));
        let boxes = runs(&layout);
        assert!(boxes.len() > 2, "{boxes:?}");
        // Every byte of the text is in exactly one run, in order, and the lines go down.
        let mut at = 0;
        let mut last_y = f32::MIN;
        for b in &boxes {
            assert_eq!(b.range.start, at, "{boxes:?}");
            at = b.range.end;
            assert!(b.rect.y >= last_y);
            last_y = b.rect.y;
            assert!(b.rect.h > 0.0);
        }
        assert_eq!(at, text.len());
        // The runs split where the style does, not only where the line does.
        assert!(
            boxes.iter().any(|b| b.range.start == marked),
            "no run begins at the marked word: {boxes:?}"
        );
        assert!(
            boxes.iter().any(|b| b.range.start == marked + 4),
            "no run ends at the marked word: {boxes:?}"
        );
        // The second line's baseline is below the first's.
        let first = boxes[0].rect.y;
        let below = boxes
            .iter()
            .find(|b| b.rect.y > first)
            .expect("one more line");
        assert!(below.baseline > boxes[0].baseline);
    }

    #[test]
    fn a_run_that_changes_colour_back_again_is_three_runs() {
        let text = "aaa bbb aaa";
        let layout = layout(text, 22.0, RED, |b| {
            b.push(StyleProperty::Brush(BLUE), 4..7);
        });
        let boxes = runs(&layout);
        let ranges: Vec<_> = boxes.iter().map(|b| b.range.clone()).collect();
        assert_eq!(ranges, vec![0..4, 4..7, 7..11], "{boxes:?}");
        // The three boxes follow each other across the line.
        assert!((boxes[0].rect.right() - boxes[1].rect.x).abs() < 0.5);
        assert!((boxes[1].rect.right() - boxes[2].rect.x).abs() < 0.5);
    }

    #[test]
    fn a_right_to_left_line_keeps_each_range_with_its_own_box() {
        // The logical first run sits on the right, so pairing a range with the wrong box would
        // show here: the long run is three letters, the short one a single letter.
        let text = "א אאא";
        let layout = layout(text, 22.0, RED, |b| b.push(StyleProperty::Brush(BLUE), 3..));
        let boxes = runs(&layout);
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        let short = boxes
            .iter()
            .find(|b| b.range.start == 0)
            .unwrap_or_else(|| panic!("the first: {boxes:?}"));
        let long = boxes
            .iter()
            .find(|b| b.range.end == text.len())
            .unwrap_or_else(|| panic!("the second: {boxes:?}"));
        assert_eq!((short.range.clone(), long.range.clone()), (0..3, 3..9));
        assert!(
            long.rect.w > 1.5 * short.rect.w,
            "the long run is {} wide and the short one {}",
            long.rect.w,
            short.rect.w
        );
        // Visual order: the logically first run is placed after the logically last one.
        assert!(short.rect.x > long.rect.x, "{boxes:?}");
        assert_eq!(short.baseline, long.baseline);
    }

    /// A layout of `text` in the first family list on this machine that shapes it with a
    /// ligature, or `None` when no font here forms one. `add` says where the style changes.
    fn ligating(
        text: &str,
        add: impl Fn(&mut parley::RangedBuilder<'_, Brush>) + Copy,
    ) -> Option<Layout<Brush>> {
        const FAMILIES: [&str; 5] = [
            "Cambria, Georgia, serif",
            "Times New Roman, Liberation Serif, serif",
            "Calibri, Carlito, sans-serif",
            "DejaVu Sans, Noto Naskh Arabic, FreeSerif, serif",
            "Consolas, Courier New, monospace",
        ];
        for family in FAMILIES {
            let layout = layout_in(family, text, 26.0, RED, add);
            if ligates(&layout) {
                return Some(layout);
            }
        }
        None
    }

    #[test]
    fn a_ligature_in_a_right_to_left_line_keeps_its_bytes() {
        // The lam-alef is one shaped cluster, and a right-to-left run walks its clusters
        // backwards: the alef, the continuation, comes before the lam that starts the glyph. A
        // machine without a font that ligates it skips this, with a note.
        let Some(layout) = ligating("لا", |b| b.push(StyleProperty::Brush(BLUE), 2..)) else {
            eprintln!("note: no Arabic font with the lam-alef ligature here; skipped one test");
            return;
        };
        let whole = ligating("لا", |_| ()).expect("the same font");
        let boxes = runs(&layout);
        let plain = runs(&whole);
        assert_eq!(plain.len(), 1, "{plain:?}");
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        assert_eq!(boxes[0].range, 0..4, "the lam's bytes were lost: {boxes:?}");
        assert!(
            (boxes[0].rect.w - plain[0].rect.w).abs() < 0.5,
            "the box is {} wide of the word's {}",
            boxes[0].rect.w,
            plain[0].rect.w
        );
    }

    #[test]
    fn a_brush_on_a_continuation_in_a_right_to_left_word_keeps_the_word_whole() {
        // Blue on the alef alone, inside the lam-alef: the run is the whole word, so its box
        // must be as wide as the word, not as wide as the alef.
        let Some(layout) = ligating("بلا", |b| b.push(StyleProperty::Brush(BLUE), 4..)) else {
            eprintln!("note: no Arabic font with the lam-alef ligature here; skipped one test");
            return;
        };
        let whole = ligating("بلا", |_| ()).expect("the same font");
        let boxes = runs(&layout);
        let plain = runs(&whole);
        assert_eq!(plain.len(), 1, "{plain:?}");
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        assert_eq!(boxes[0].range, 0..6, "{boxes:?}");
        assert!(
            (boxes[0].rect.w - plain[0].rect.w).abs() < 0.5,
            "the box is {} wide of the word's {}",
            boxes[0].rect.w,
            plain[0].rect.w
        );
    }

    #[test]
    fn nothing_comes_of_nothing() {
        assert!(runs(&layout("", 18.0, RED, |_| ())).is_empty());
    }
}
