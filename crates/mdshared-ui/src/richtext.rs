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
/// no glyph of its own (a soft hyphen), whose bytes join the run next to it. A run of nothing
/// but such clusters has no entry of its own: its bytes join the last run of the line before it
/// that has glyphs, and a line with no glyph run at all keeps none of its bytes. The work is
/// linear in the layout's length.
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
            if let Some(left) = waiting {
                // A run of nothing but atoms without glyphs has no glyph run of its own: its
                // bytes belong to the last run of the line before it that has one.
                if let Some(last) = ranges
                    .iter_mut()
                    .rev()
                    .find_map(|(_, groups)| groups.last_mut())
                {
                    last.start = last.start.min(left.start);
                    last.end = last.end.max(left.end);
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

    /// What every set of runs satisfies, whatever fonts the machine resolved (a font fallback
    /// may split a run by itself, so the count is not fixed): the ranges tile the text in
    /// reading order, every brush boundary the caller made is a run boundary, and the boxes of
    /// one line touch in visual order.
    fn checked(boxes: &[RunBox], text: &str, boundaries: &[usize]) {
        let mut ranges: Vec<_> = boxes.iter().map(|b| b.range.clone()).collect();
        ranges.sort_by_key(|r| r.start);
        let mut at = 0;
        for r in &ranges {
            assert_eq!(r.start, at, "{boxes:?}");
            assert!(r.end > r.start, "an empty run: {boxes:?}");
            at = r.end;
        }
        assert_eq!(at, text.len(), "the runs do not cover the text: {boxes:?}");
        for &cut in boundaries {
            assert!(
                boxes.iter().any(|b| b.range.start == cut),
                "no run begins at the brush change {cut}: {boxes:?}"
            );
        }
        for pair in boxes.windows(2) {
            if (pair[0].rect.y - pair[1].rect.y).abs() < 0.01 {
                assert!(
                    (pair[0].rect.right() - pair[1].rect.x).abs() < 0.5,
                    "the boxes do not touch: {:?} then {:?}",
                    pair[0].rect,
                    pair[1].rect
                );
            }
        }
    }

    /// The boxes' widths added up: the same on any font, so a line drawn in one colour and the
    /// same line drawn in two add to the same length.
    fn width(boxes: &[RunBox]) -> f32 {
        boxes.iter().map(|b| b.rect.w).sum()
    }

    #[test]
    fn each_run_gives_its_text_and_the_box_it_was_placed_in() {
        let text = "wrong word";
        let layout = layout(text, 26.0, RED, |b| b.push(StyleProperty::Brush(BLUE), 6..));
        let boxes = runs(&layout);
        checked(&boxes, text, &[6]);
        let first = boxes
            .iter()
            .find(|b| b.range.start == 0)
            .expect("the first");
        let second = boxes
            .iter()
            .find(|b| b.range.start == 6)
            .expect("the second");
        // The second run begins where the first ends, on the same baseline.
        assert_eq!(first.baseline, second.baseline);
        assert_eq!(first.rect.y, second.rect.y);
        // Both words are there, and the boxes stay inside the layout's width.
        assert!(first.rect.w > 10.0 && second.rect.w > 10.0);
        assert!(second.rect.right() <= layout.width() + 1.0);
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
        checked(&boxes, text, &[marked, marked + 4]);
        // The lines go down.
        let mut last_y = f32::MIN;
        for b in &boxes {
            assert!(b.rect.y >= last_y, "{boxes:?}");
            last_y = b.rect.y;
            assert!(b.rect.h > 0.0);
        }
        // The second line's baseline is below the first's.
        let first = boxes[0].rect.y;
        let below = boxes
            .iter()
            .find(|b| b.rect.y > first)
            .expect("one more line");
        assert!(below.baseline > boxes[0].baseline);
    }

    #[test]
    fn a_colour_that_changes_back_makes_the_middle_word_a_run_of_its_own() {
        let text = "aaa bbb aaa";
        let layout = layout(text, 22.0, RED, |b| {
            b.push(StyleProperty::Brush(BLUE), 4..7);
        });
        let boxes = runs(&layout);
        // The colour changes at byte 4 and back at 7, so those are run boundaries on any font
        // and the middle word is one run of its own.
        checked(&boxes, text, &[4, 7]);
        let middle = boxes
            .iter()
            .find(|b| b.range.start == 4)
            .expect("the blue word");
        assert_eq!(middle.range.end, 7, "{boxes:?}");
    }

    #[test]
    fn a_right_to_left_line_keeps_each_range_with_its_own_box() {
        // The logical first run sits on the right, so a pairing that put a range with the wrong
        // box would show here: the long stretch is three letters, the short one a single
        // letter. Whatever fonts the machine resolved (a fallback may split the line by itself,
        // and the space between the words may come from another font), the runs must tile the
        // text, the brush change must be a run boundary, and the boxes must touch in visual
        // order.
        let text = "א אאא";
        let coloured = layout(text, 22.0, RED, |b| b.push(StyleProperty::Brush(BLUE), 3..));
        let boxes = runs(&coloured);
        checked(&boxes, text, &[3]);

        let short = boxes
            .iter()
            .find(|b| b.range.start == 0)
            .unwrap_or_else(|| panic!("the first run: {boxes:?}"));
        let long = boxes
            .iter()
            .find(|b| b.range.start == 3)
            .unwrap_or_else(|| panic!("the long run: {boxes:?}"));
        // Visual order: the logically first run is placed after the logically last one.
        assert!(short.rect.x > long.rect.x, "{boxes:?}");
        assert_eq!(short.baseline, long.baseline);
        // The range with three letters is the wide one: each range keeps its own box.
        assert!(
            long.rect.w > 1.5 * short.rect.w,
            "the long run is {} wide and the short one {}",
            long.rect.w,
            short.rect.w
        );
        // Each range's box is where that text was drawn: the widths add up to the same line
        // drawn in one colour.
        let plain = runs(&layout(text, 22.0, RED, |_| ()));
        assert!(
            (width(&boxes) - width(&plain)).abs() < 0.5,
            "{} of the one-colour line's {}",
            width(&boxes),
            width(&plain)
        );
    }

    #[test]
    fn a_right_to_left_line_split_between_two_fonts_reports_each_part() {
        // A font fallback can split one line into several runs by itself: here the space is
        // asked for in the mono family, which has no Hebrew, so the letters and the space come
        // from different fonts on any machine that has a mono family. The same properties hold,
        // and each part keeps its own box.
        let text = "א אאא";
        let mono = |b: &mut parley::RangedBuilder<'_, Brush>| {
            b.push(
                StyleProperty::FontFamily(FontFamily::from(
                    "Cascadia Mono, Consolas, DejaVu Sans Mono, monospace",
                )),
                2..3,
            );
        };
        let boxes = runs(&layout(text, 22.0, RED, mono));
        checked(&boxes, text, &[]);
        // The mono family has no Hebrew, so the space is a run of its own; a machine without a
        // mono family falls back to the letters' font and says so rather than failing.
        if !boxes.iter().any(|b| b.range.start == 2) {
            eprintln!("note: the space took the letters' font; the split was not tested");
            return;
        }
        // The logically first part is placed after the last one, and the parts add up to the
        // line drawn with the same fonts and no colour change.
        let first = boxes
            .iter()
            .find(|b| b.range.start == 0)
            .unwrap_or_else(|| panic!("the first run: {boxes:?}"));
        let last = boxes
            .iter()
            .find(|b| b.range.end == text.len())
            .unwrap_or_else(|| panic!("the last run: {boxes:?}"));
        assert!(first.rect.x > last.rect.x, "{boxes:?}");
        let plain = runs(&layout(text, 22.0, RED, mono));
        assert!(
            (width(&boxes) - width(&plain)).abs() < 0.5,
            "{} of the one-font line's {}",
            width(&boxes),
            width(&plain)
        );
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
        let text = "لا";
        let Some(layout) = ligating(text, |b| b.push(StyleProperty::Brush(BLUE), 2..)) else {
            eprintln!("note: no Arabic font with the lam-alef ligature here; skipped one test");
            return;
        };
        let boxes = runs(&layout);
        checked(&boxes, text, &[]);
        // The ligature is one glyph, so its two letters are one run even though the alef asks
        // for another brush.
        assert!(
            boxes
                .iter()
                .any(|b| b.range.start == 0 && b.range.end == text.len()),
            "the lam's bytes were lost: {boxes:?}"
        );
        // And the box is as wide as the text drawn in one colour.
        let plain = runs(&ligating(text, |_| ()).expect("the same font"));
        assert!(
            (width(&boxes) - width(&plain)).abs() < 0.5,
            "{} of the one-colour line's {}",
            width(&boxes),
            width(&plain)
        );
    }

    #[test]
    fn a_brush_on_a_continuation_in_a_right_to_left_word_keeps_the_word_whole() {
        // Blue on the alef alone, inside the lam-alef: the ligature's bytes stay in one run, so
        // their box is as wide as both letters, not as wide as the alef.
        let text = "بلا";
        let Some(layout) = ligating(text, |b| b.push(StyleProperty::Brush(BLUE), 4..)) else {
            eprintln!("note: no Arabic font with the lam-alef ligature here; skipped one test");
            return;
        };
        let boxes = runs(&layout);
        checked(&boxes, text, &[]);
        assert!(
            boxes.iter().any(|b| b.range.start <= 2 && b.range.end >= 6),
            "the lam-alef was split: {boxes:?}"
        );
        let plain = runs(&ligating(text, |_| ()).expect("the same font"));
        assert!(
            (width(&boxes) - width(&plain)).abs() < 0.5,
            "{} of the one-colour line's {}",
            width(&boxes),
            width(&plain)
        );
    }

    #[test]
    fn nothing_comes_of_nothing() {
        assert!(runs(&layout("", 18.0, RED, |_| ())).is_empty());
    }
}
