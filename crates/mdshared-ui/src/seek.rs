//! A seek bar for a player: a [`slider::Slider`] that stands for a position in a track of a
//! known length, with the start of each chapter marked by a gap through the track, and the
//! time (and the chapter's title) under the pointer as a label.
//!
//! The model ([`SeekBar`]) holds the length, the chapters and a [`Slider`] built from 0 to the
//! length; it answers questions of geometry and time. The caller owns the events and drives the
//! slider through [`SeekBar::slider_mut`] exactly as it would drive a slider, owns the hover x
//! it passes in, and draws the tooltip that [`hover_tip`] places with
//! [`overlay::Bubble::draw`](crate::overlay::Bubble::draw) or [`draw_presence`](crate::overlay::Bubble::draw_presence)
//! where it wants it. Nothing remembers the rectangle: each call is given it.
//!
//! Deliberately not here: decoding, buffering state beyond [`slider::Look::buffered`], and
//! thumbnails of the video frame.

use crate::anim::{Animator, AsId, Id};
use crate::gfx::{Painter, Rect, Rgb};
use crate::overlay;
use crate::slider::{self, Slider};
use crate::theme::Palette;

/// How far into a chapter the pointer must be before [`SeekBar::previous_chapter`] says the
/// chapter itself rather than the one before it, in seconds.
pub const CHAPTER_BACK: f64 = 3.0;

/// Where a chapter begins, and what it is called.
///
/// `at` is the start in seconds from the beginning of the track.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub at: f64,
    pub title: String,
}

/// A position in a track of a known length, and the chapters along it.
///
/// The slider inside runs from 0 to the length in seconds; the caller drives it with
/// [`slider_mut`](Self::slider_mut) and reads [`position`](Self::position). A bar whose length
/// is not a positive number is empty: it has no chapters, every question about time answers
/// nothing, and it draws the plain slider at 0.
#[derive(Clone, Debug, PartialEq)]
pub struct SeekBar {
    slider: Slider,
    length: f64,
    chapters: Vec<Chapter>,
}

impl SeekBar {
    /// A bar for a track `length_secs` long, at its beginning, with no chapters. A length that
    /// is not a number, is infinite or is not positive makes an empty bar.
    pub fn new(length_secs: f64) -> SeekBar {
        let length = if length_secs.is_finite() && length_secs > 0.0 {
            length_secs.min(f32::MAX as f64)
        } else {
            0.0
        };
        SeekBar {
            slider: Slider::new(0.0, length as f32, 0.0),
            length,
            chapters: Vec::new(),
        }
    }

    /// The same bar with its chapters. A chapter whose start is not a number, is negative or
    /// lies past the end of the track is dropped; the rest are put in order by their start,
    /// and of several chapters that start at the same moment the first given is kept. On an
    /// empty bar every chapter is dropped.
    pub fn chapters(mut self, chapters: Vec<Chapter>) -> SeekBar {
        if self.length > 0.0 {
            let mut kept: Vec<Chapter> = chapters
                .into_iter()
                .filter(|c| c.at.is_finite() && c.at >= 0.0 && c.at <= self.length)
                .collect();
            // A stable sort keeps the input order among chapters that start together, so the
            // first one given is the one that survives the dedup below.
            kept.sort_by(|a, b| a.at.total_cmp(&b.at));
            kept.dedup_by(|a, b| a.at == b.at);
            self.chapters = kept;
        }
        self
    }

    /// The length of the track in seconds: 0 for an empty bar.
    pub fn length(&self) -> f64 {
        self.length
    }

    /// The chapters that were kept, in order by their start: empty for an empty bar.
    pub fn chapter_list(&self) -> &[Chapter] {
        &self.chapters
    }

    /// The slider the caller drives: from 0 to the length, its value the position.
    pub fn slider(&self) -> &Slider {
        &self.slider
    }

    /// The slider to pass a press, a drag, a release, a key or the wheel to, or to set. The
    /// caller keeps its value between 0 and the length, which the slider does by itself.
    pub fn slider_mut(&mut self) -> &mut Slider {
        &mut self.slider
    }

    /// Where the position is, in seconds.
    pub fn position(&self) -> f64 {
        self.slider.value() as f64
    }

    /// Moves the position to `secs`, kept between the beginning and the length. A value that
    /// is not a number changes nothing. Returns whether the position changed.
    pub fn set_position(&mut self, secs: f64) -> bool {
        if secs.is_nan() {
            return false;
        }
        let secs = secs.clamp(0.0, self.length);
        self.slider.set(secs as f32)
    }

    /// The time in seconds under the pointer at `x` in the bar's rectangle `r`, by the slider's
    /// own map of the track (so the thumb's travel, not the rectangle's edges, is the track).
    /// An empty bar, and an `x` that is not a number, answer nothing.
    pub fn time_at(&self, r: Rect, x: f32) -> Option<f64> {
        if self.length <= 0.0 || !x.is_finite() {
            return None;
        }
        let fraction = self.slider.fraction_at(r, x, r.cy());
        Some(fraction as f64 * self.length)
    }

    /// The index in [`chapter_list`](Self::chapter_list) of the last chapter that has begun by
    /// `secs`: none before the first chapter, with no chapters, or for a time that is not a
    /// number.
    pub fn chapter_at(&self, secs: f64) -> Option<usize> {
        if secs.is_nan() {
            return None;
        }
        let mut found = None;
        for (i, c) in self.chapters.iter().enumerate() {
            if c.at <= secs {
                found = Some(i);
            } else {
                break;
            }
        }
        found
    }

    /// The start in seconds of the first chapter that begins after `secs`, or nothing when no
    /// chapter does or the time is not a number.
    pub fn next_chapter(&self, secs: f64) -> Option<f64> {
        if secs.is_nan() {
            return None;
        }
        self.chapters.iter().find(|c| c.at > secs).map(|c| c.at)
    }

    /// Where the "previous chapter" button takes the position from `secs`, as a player does:
    /// more than [`CHAPTER_BACK`] seconds into a chapter is that chapter's own start, and less
    /// is the start of the one before; within the first chapter, or before it, it is 0.0.
    /// Nothing for a time that is not a number or a bar with no chapters.
    pub fn previous_chapter(&self, secs: f64) -> Option<f64> {
        if secs.is_nan() || self.chapters.is_empty() {
            return None;
        }
        match self.chapter_at(secs) {
            None => Some(0.0),
            Some(i) => {
                let start = self.chapters[i].at;
                if secs - start > CHAPTER_BACK {
                    Some(start)
                } else if i == 0 {
                    Some(0.0)
                } else {
                    Some(self.chapters[i - 1].at)
                }
            }
        }
    }

    /// The label for the pointer at `x` in `r`: the time under it as
    /// [`mdshared_app::fmt::duration`] shows whole seconds, followed by " · " and the title of
    /// the chapter that time falls in ([`chapter_at`](Self::chapter_at)), as a player's seek bar
    /// names the part the pointer is over. Before the first chapter, or with none, the time
    /// alone. Nothing when [`time_at`](Self::time_at) has nothing, which is an empty bar or an
    /// `x` that is not a number.
    pub fn hover_label(&self, r: Rect, x: f32) -> Option<String> {
        let secs = self.time_at(r, x)?;
        let label = mdshared_app::fmt::duration(secs.floor().max(0.0) as u64);
        match self.chapter_at(secs) {
            Some(i) => Some(format!("{label} · {}", self.chapters[i].title)),
            None => Some(label),
        }
    }

    /// The x of a chapter's mark in `r`: where the thumb's middle would be at that time.
    fn mark_x(&self, r: Rect, at: f64) -> f32 {
        let mut probe = self.slider;
        probe.set(at as f32);
        probe.thumb_center(r).0
    }

    /// The x of each chapter's mark along the track in `r`, in order. A chapter at 0 has no
    /// mark, since there is no track before it to mark; an empty bar has none at all.
    pub fn marks(&self, r: Rect) -> Vec<f32> {
        if self.length <= 0.0 {
            return Vec::new();
        }
        self.chapters
            .iter()
            .filter(|c| c.at > 0.0)
            .map(|c| self.mark_x(r, c.at))
            .collect()
    }
}

/// Cuts the gap for each mark through the track: two logical pixels wide, [`slider::TRACK`]
/// tall, in `ground`, the colour behind the bar. A mark under the thumb (drawn at `at`, 0 to 1
/// along the track) is left out, since the gap would cut the thumb the slider has already put
/// there.
fn cut_marks(p: &mut Painter, ground: Rgb, r: Rect, bar: &SeekBar, at: f32) {
    let mut probe = *bar.slider();
    probe.set(probe.value_of(at));
    let thumb = probe.thumb_rect(r);
    for x in bar.marks(r) {
        if thumb.contains(x, r.cy()) {
            continue;
        }
        let gap = Rect::new(x - 1.0, r.cy() - slider::TRACK / 2.0, 2.0, slider::TRACK);
        p.fill(gap, ground);
    }
}

/// Draws the bar in `r`: exactly what [`slider::draw`] draws for the slider inside, and then a
/// two pixel gap through the track at each chapter's mark, filled with `ground`, the colour the
/// bar is drawn on (`pal.page`, `pal.bar`, ...), so the background shows through; a mark under
/// the thumb is not drawn. `look` is passed to the slider unchanged.
pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    bar: &SeekBar,
    look: slider::Look,
    ground: Rgb,
) {
    slider::draw(p, pal, r, bar.slider(), look);
    cut_marks(p, ground, r, bar, bar.slider().fraction());
}

/// [`draw`] with the motion of [`slider::draw_animated`]: the thumb glides and its dot grows
/// and shrinks as there, and no mark is cut through the thumb where it is drawn on this frame,
/// on its way included. `id` names the slider in `anim`. At rest, and on every frame when the
/// animator does not animate, it draws exactly what [`draw`] draws.
#[allow(clippy::too_many_arguments)]
pub fn draw_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    bar: &SeekBar,
    look: slider::Look,
    ground: Rgb,
) {
    let id = Id::new(id);
    slider::draw_animated(p, pal, anim, id, r, bar.slider(), look);
    // Where the slider drew its thumb this frame: it keeps that under `id.with("at")`.
    let at = anim
        .get::<f32>(id.with("at"))
        .unwrap_or_else(|| bar.slider().fraction());
    cut_marks(p, ground, r, bar, at);
}

/// The tooltip for the pointer at `x` in `r`, or nothing where [`SeekBar::hover_label`] has
/// nothing. Its anchor is a one pixel wide column at `x` as tall as `r`, so
/// [`overlay::Bubble::tip`] puts the bubble below the bar, and above it when a `win_w` by
/// `win_h` window has no room below. Drawing it is the caller's.
pub fn hover_tip(
    p: &mut Painter,
    r: Rect,
    bar: &SeekBar,
    x: f32,
    win_w: f32,
    win_h: f32,
) -> Option<overlay::Bubble> {
    let label = bar.hover_label(r, x)?;
    let anchor = Rect::new(x, r.y, 1.0, r.h);
    Some(overlay::Bubble::tip(p, &label, anchor, win_w, win_h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock};
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    /// The bar's rectangle: the thumb's middle travels from x 29 to 211.
    const R: Rect = Rect::new(20.0, 40.0, 200.0, 20.0);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-3 * b.abs().max(1.0)
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    fn shot(pal: &Palette, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(240, 100);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 240.0, 100.0), pal.pane);
            draw(&mut p);
        }
        bmp
    }

    fn chapter(at: f64, title: &str) -> Chapter {
        Chapter {
            at,
            title: title.to_string(),
        }
    }

    #[test]
    fn a_bar_with_no_chapters_draws_the_slider_it_is_built_on_pixel_for_pixel() {
        let bar = SeekBar::new(100.0);
        assert!(bar.chapter_list().is_empty());
        for pal in [&LIGHT, &DARK] {
            for value in [0.0, 37.0] {
                let mut bar = bar.clone();
                bar.set_position(value);
                let plain = shot(pal, |p| {
                    slider::draw(p, pal, R, bar.slider(), slider::Look::default())
                });
                let seek = shot(pal, |p| {
                    draw(p, pal, R, &bar, slider::Look::default(), pal.page)
                });
                assert!(plain.pixels == seek.pixels, "{value} s");
            }
        }
    }

    #[test]
    fn a_mark_cuts_a_two_pixel_gap_through_the_track_to_the_page() {
        let bar = SeekBar::new(100.0).chapters(vec![chapter(25.0, "Intro")]);
        let marks = bar.marks(R);
        assert_eq!(marks.len(), 1);
        assert!(close(marks[0], 74.5), "{}", marks[0]);
        for pal in [&LIGHT, &DARK] {
            let bmp = shot(pal, |p| {
                draw(p, pal, R, &bar, slider::Look::default(), pal.page)
            });
            let plain = shot(pal, |p| {
                slider::draw(p, pal, R, bar.slider(), slider::Look::default())
            });
            // The two pixels nearest the mark are the page; its neighbours are not.
            assert_eq!(at(&bmp, 74.0, 50.0), pal.page);
            assert_eq!(at(&bmp, 75.0, 50.0), pal.page);
            assert_ne!(at(&bmp, 73.0, 50.0), pal.page);
            assert_ne!(at(&bmp, 76.0, 50.0), pal.page);
            assert_eq!(at(&plain, 74.0, 50.0), at(&plain, 75.0, 50.0));
            assert_ne!(at(&plain, 74.0, 50.0), pal.page);
            // The gap is the track's height and no more: the rows around it are untouched.
            assert_eq!(at(&bmp, 74.0, 47.0), pal.pane);
            assert_eq!(at(&bmp, 74.0, 52.0), pal.pane);
            assert_eq!(at(&bmp, 74.0, 48.0), pal.page);
            assert_eq!(at(&bmp, 74.0, 51.0), pal.page);
        }
    }

    #[test]
    fn the_gap_shows_the_colour_the_bar_is_drawn_on() {
        let bar = SeekBar::new(100.0).chapters(vec![chapter(25.0, "Intro")]);
        for pal in [&LIGHT, &DARK] {
            let bmp = shot(pal, |p| {
                draw(p, pal, R, &bar, slider::Look::default(), pal.pane)
            });
            assert_eq!(at(&bmp, 74.0, 50.0), pal.pane);
            assert_eq!(at(&bmp, 75.0, 50.0), pal.pane);
        }
    }

    #[test]
    fn no_mark_is_cut_through_a_thumb_on_its_way() {
        let bar = SeekBar::new(100.0).chapters(vec![chapter(25.0, "Intro")]);
        let mark = bar.marks(R)[0];
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.begin_frame(clock.now());
        let first = shot(&LIGHT, |p| {
            draw_animated(
                p,
                &LIGHT,
                &mut anim,
                "seek",
                R,
                &bar,
                slider::Look::default(),
                LIGHT.page,
            )
        });
        anim.end_frame();
        assert_eq!(at(&first, mark, 50.0), LIGHT.page);
        // A jump to 60 s: the thumb glides from 0 past the mark at 25 s.
        let mut moved = bar.clone();
        moved.set_position(60.0);
        let mut crossed = false;
        for _ in 0..40 {
            clock.advance(std::time::Duration::from_millis(8));
            anim.begin_frame(clock.now());
            let bmp = shot(&LIGHT, |p| {
                draw_animated(
                    p,
                    &LIGHT,
                    &mut anim,
                    "seek",
                    R,
                    &moved,
                    slider::Look::default(),
                    LIGHT.page,
                )
            });
            let at_now = anim.get::<f32>(Id::new("seek").with("at")).unwrap();
            let mut probe = *moved.slider();
            probe.set(probe.value_of(at_now));
            if probe.thumb_rect(R).contains(mark, R.cy()) {
                crossed = true;
                assert_ne!(
                    at(&bmp, mark, 50.0),
                    LIGHT.page,
                    "a gap cut the moving thumb"
                );
            }
            anim.end_frame();
        }
        assert!(crossed, "the thumb never passed over the mark");
    }

    #[test]
    fn a_mark_under_the_thumb_is_not_drawn() {
        let bar = SeekBar::new(100.0).chapters(vec![chapter(25.0, "Intro")]);
        let mut held = bar.clone();
        held.set_position(25.0);
        let thumb = held.slider().thumb_rect(R);
        assert!(thumb.contains(bar.marks(R)[0], R.cy()));
        for pal in [&LIGHT, &DARK] {
            let plain = shot(pal, |p| {
                slider::draw(p, pal, R, held.slider(), slider::Look::default())
            });
            let seek = shot(pal, |p| {
                draw(p, pal, R, &held, slider::Look::default(), pal.page)
            });
            assert!(plain.pixels == seek.pixels);
        }
    }

    #[test]
    fn time_at_reads_the_left_end_the_middle_and_the_right_end() {
        let bar = SeekBar::new(100.0);
        assert_eq!(bar.time_at(R, 29.0), Some(0.0));
        assert!(close(bar.time_at(R, 120.0).unwrap() as f32, 50.0));
        assert!(close(bar.time_at(R, 211.0).unwrap() as f32, 100.0));
        // Past the ends it holds at them, as the slider does.
        assert_eq!(bar.time_at(R, 0.0), Some(0.0));
        assert!(close(bar.time_at(R, 900.0).unwrap() as f32, 100.0));
        assert_eq!(bar.time_at(R, f32::NAN), None);
        assert_eq!(bar.time_at(R, f32::INFINITY), None);
        assert_eq!(SeekBar::new(0.0).time_at(R, 120.0), None);
        assert_eq!(SeekBar::new(f64::NAN).time_at(R, 120.0), None);
    }

    #[test]
    fn chapter_at_finds_the_last_chapter_that_has_begun() {
        let bar = SeekBar::new(100.0).chapters(vec![
            chapter(20.0, "Two"),
            chapter(0.0, "Zero"),
            chapter(10.0, "One"),
        ]);
        assert_eq!(bar.chapter_at(0.0), Some(0));
        assert_eq!(bar.chapter_at(5.0), Some(0));
        assert_eq!(bar.chapter_at(10.0), Some(1));
        assert_eq!(bar.chapter_at(19.9), Some(1));
        assert_eq!(bar.chapter_at(20.0), Some(2));
        assert_eq!(bar.chapter_at(99.0), Some(2));
        assert_eq!(bar.chapter_at(-1.0), None);
        assert_eq!(bar.chapter_at(f64::NAN), None);
        assert_eq!(SeekBar::new(100.0).chapter_at(5.0), None);
    }

    #[test]
    fn moving_between_chapters_follows_the_three_second_rule() {
        let bar = SeekBar::new(120.0).chapters(vec![
            chapter(30.0, "One"),
            chapter(60.0, "Two"),
            chapter(90.0, "Three"),
        ]);
        assert_eq!(bar.next_chapter(0.0), Some(30.0));
        assert_eq!(bar.next_chapter(29.0), Some(30.0));
        assert_eq!(bar.next_chapter(30.0), Some(60.0));
        assert_eq!(bar.next_chapter(55.0), Some(60.0));
        assert_eq!(bar.next_chapter(95.0), None);
        assert_eq!(bar.next_chapter(f64::NAN), None);
        // Before the first chapter, and within it, the way back is the start.
        assert_eq!(bar.previous_chapter(0.0), Some(0.0));
        assert_eq!(bar.previous_chapter(30.0), Some(0.0));
        assert_eq!(bar.previous_chapter(33.0), Some(0.0));
        // More than three seconds in: the chapter's own start.
        assert_eq!(bar.previous_chapter(33.1), Some(30.0));
        assert_eq!(bar.previous_chapter(40.0), Some(30.0));
        assert_eq!(bar.previous_chapter(60.0), Some(30.0));
        assert_eq!(bar.previous_chapter(63.5), Some(60.0));
        assert_eq!(bar.previous_chapter(119.0), Some(90.0));
        assert_eq!(bar.previous_chapter(f64::NAN), None);
        assert_eq!(SeekBar::new(120.0).previous_chapter(10.0), None);
    }

    #[test]
    fn hover_label_shows_the_time_and_the_chapter_the_pointer_is_on() {
        // A length of 182 s makes a second one pixel along the travel of the track.
        let bar = SeekBar::new(182.0).chapters(vec![chapter(65.0, "Intro")]);
        let mark = bar.marks(R)[0];
        assert!(close(mark, 94.0));
        assert_eq!(
            bar.hover_label(R, mark).as_deref(),
            Some("1:05 · Intro"),
            "the time and the title"
        );
        assert_eq!(
            bar.hover_label(R, mark + 1.0).as_deref(),
            Some("1:06 · Intro"),
            "past the mark, still in its chapter"
        );
        assert_eq!(
            bar.hover_label(R, 120.0).as_deref(),
            Some("1:31 · Intro"),
            "anywhere in the chapter names it"
        );
        assert_eq!(
            bar.hover_label(R, mark - 2.0).as_deref(),
            Some("1:03"),
            "before the first chapter, the time alone"
        );
        assert_eq!(bar.hover_label(R, 29.0).as_deref(), Some("0:00"));
        assert_eq!(bar.hover_label(R, f32::NAN), None);
        assert_eq!(
            SeekBar::new(182.0).hover_label(R, 120.0).as_deref(),
            Some("1:31")
        );
        assert_eq!(SeekBar::new(0.0).hover_label(R, 120.0), None);
    }

    #[test]
    fn the_hover_tip_holds_the_hover_label() {
        let bar = SeekBar::new(182.0).chapters(vec![chapter(65.0, "Intro")]);
        let mark = bar.marks(R)[0];
        let mut bmp = Bitmap::new(400, 200);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let bubble = hover_tip(&mut p, R, &bar, mark, 400.0, 200.0).unwrap();
        assert_eq!(bubble.text, "1:05 · Intro");
        // Anchored at the pointer, so the bubble straddles its x, and below the bar.
        assert!(bubble.rect.x <= mark && bubble.rect.right() >= mark);
        assert!(bubble.rect.y > R.bottom());
        assert!(hover_tip(&mut p, R, &bar, f32::NAN, 400.0, 200.0).is_none());
        assert!(hover_tip(&mut p, R, &SeekBar::new(0.0), 20.0, 400.0, 200.0).is_none());
        // With no room below, the bubble goes above the bar.
        let above = hover_tip(&mut p, R, &bar, mark, 400.0, 80.0).unwrap();
        assert!(bubble.rect != above.rect);
        assert!(above.rect.bottom() < R.y);
    }

    #[test]
    fn the_chapters_kept_are_the_real_ones_in_order_and_the_first_of_a_duplicate() {
        let bar = SeekBar::new(100.0).chapters(vec![
            chapter(70.0, "Later"),
            chapter(10.0, "Second"),
            chapter(f64::NAN, "Not a time"),
            chapter(-5.0, "Before"),
            chapter(101.0, "Past"),
            chapter(10.0, "Dropped duplicate"),
            chapter(0.0, "First"),
            chapter(f64::INFINITY, "Never"),
        ]);
        let kept: Vec<(f64, &str)> = bar
            .chapter_list()
            .iter()
            .map(|c| (c.at, c.title.as_str()))
            .collect();
        assert_eq!(kept, [(0.0, "First"), (10.0, "Second"), (70.0, "Later")]);
        // The ends of the track itself are kept.
        let edge = SeekBar::new(100.0).chapters(vec![chapter(0.0, "A"), chapter(100.0, "B")]);
        assert_eq!(edge.chapter_list().len(), 2);
        // The first chapter has no mark; the others stand at their times.
        assert_eq!(bar.marks(R).len(), 2);
        assert!(close(bar.marks(R)[0], 29.0 + 182.0 * 0.1));
        // A second builder call replaces the first list.
        let again = bar.chapters(vec![chapter(50.0, "Only")]);
        assert_eq!(again.chapter_list().len(), 1);
    }

    #[test]
    fn a_length_that_is_not_a_positive_number_makes_an_empty_bar() {
        for length in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -5.0] {
            let mut bar = SeekBar::new(length).chapters(vec![chapter(0.0, "Intro")]);
            assert_eq!(bar.length(), 0.0, "{length}");
            assert!(bar.chapter_list().is_empty(), "{length}");
            assert_eq!(bar.position(), 0.0);
            assert_eq!(bar.time_at(R, 120.0), None);
            assert_eq!(bar.chapter_at(0.0), None);
            assert_eq!(bar.next_chapter(0.0), None);
            assert_eq!(bar.previous_chapter(0.0), None);
            assert_eq!(bar.hover_label(R, 120.0), None);
            assert!(bar.marks(R).is_empty());
            assert!(!bar.set_position(10.0), "an empty bar stays at 0");
            let empty = Slider::new(0.0, 0.0, 0.0);
            for pal in [&LIGHT, &DARK] {
                let plain = shot(pal, |p| {
                    slider::draw(p, pal, R, &empty, slider::Look::default())
                });
                let drawn = shot(pal, |p| {
                    draw(p, pal, R, &bar, slider::Look::default(), pal.page)
                });
                assert!(plain.pixels == drawn.pixels, "{length}");
            }
        }
    }

    #[test]
    fn set_position_keeps_the_position_between_the_ends_and_ignores_a_nan() {
        let mut bar = SeekBar::new(100.0);
        assert!(!bar.set_position(0.0), "already there");
        assert!(bar.set_position(37.5) && bar.position() == 37.5);
        assert!(!bar.set_position(37.5), "the same position");
        assert!(bar.set_position(f64::INFINITY) && bar.position() == 100.0);
        assert!(bar.set_position(-20.0) && bar.position() == 0.0);
        assert!(!bar.set_position(f64::NAN) && bar.position() == 0.0);
        assert!(bar.slider().max() == 100.0 && bar.slider().min() == 0.0);
    }

    #[test]
    fn the_slider_inside_is_driven_exactly_as_a_slider() {
        let mut bar = SeekBar::new(100.0);
        assert!(bar.slider_mut().press(R, 120.0, 50.0));
        assert!(bar.slider().dragging());
        assert!(close(bar.position() as f32, 50.0));
        assert!(bar.slider_mut().drag(R, 165.5, 50.0));
        assert!(close(bar.position() as f32, 75.0));
        assert!(bar.slider_mut().release());
        assert!(!bar.slider_mut().release());
    }

    #[test]
    fn the_animated_bar_at_rest_draws_the_plain_bar() {
        let bar = SeekBar::new(100.0).chapters(vec![chapter(25.0, "Intro")]);
        let mut moved = bar.clone();
        moved.set_position(60.0);
        for pal in [&LIGHT, &DARK] {
            for look in [
                slider::Look::default(),
                slider::Look {
                    hot: true,
                    ..slider::Look::default()
                },
            ] {
                let clock = ManualClock::new();
                let mut anim = Animator::new();
                anim.begin_frame(clock.now());
                let animated = shot(pal, |p| {
                    draw_animated(p, pal, &mut anim, "seek", R, &moved, look, pal.page)
                });
                assert_eq!(anim.end_frame(), None, "at rest, no frame asked for");
                let plain = shot(pal, |p| draw(p, pal, R, &moved, look, pal.page));
                assert!(animated.pixels == plain.pixels);
            }
        }
        // With reduced motion every frame is the plain one.
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.set_policy(crate::anim::Policy::Never);
        anim.begin_frame(clock.now());
        let still = shot(&LIGHT, |p| {
            draw_animated(
                p,
                &LIGHT,
                &mut anim,
                "seek",
                R,
                &bar,
                slider::Look::default(),
                LIGHT.page,
            )
        });
        assert_eq!(anim.end_frame(), None);
        assert!(
            still.pixels
                == shot(&LIGHT, |p| draw(
                    p,
                    &LIGHT,
                    R,
                    &bar,
                    slider::Look::default(),
                    LIGHT.page
                ))
                .pixels
        );
    }

    /// A sheet for a person to judge: `cargo test -p mdshared-ui seek_sheet -- --ignored`
    /// writes `target/specimen/seek.png` and `seek-150.png` (light above, dark below): a bar
    /// with four chapters, what is loaded, the hover look with its tip, and a held thumb.
    #[test]
    #[ignore]
    fn seek_sheet() {
        for (scale, file) in [(1.0, "seek.png"), (1.5, "seek-150.png")] {
            let mut bmp = Bitmap::new((520.0 * scale) as u32, (300.0 * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
                let oy = half as f32 * 150.0;
                p.fill(Rect::new(0.0, oy, 520.0, 150.0), pal.pane);
                let o = p.offset(0.0, oy);
                let mut bar = SeekBar::new(200.0).chapters(vec![
                    chapter(0.0, "Opening"),
                    chapter(40.0, "Verse"),
                    chapter(95.0, "Chorus"),
                    chapter(150.0, "Outro"),
                ]);
                bar.set_position(70.0);
                let loaded = slider::Look {
                    buffered: Some((0.0, 120.0)),
                    ..slider::Look::default()
                };
                let row = |k: usize| Rect::new(20.0, 12.0 + 34.0 * k as f32, 480.0, 20.0);
                draw(&mut p, pal, row(0), &bar, loaded, pal.pane);
                let hot = slider::Look {
                    hot: true,
                    ..loaded
                };
                draw(&mut p, pal, row(1), &bar, hot, pal.pane);
                if let Some(tip) = hover_tip(&mut p, row(1), &bar, 330.0, 520.0, 150.0) {
                    tip.draw(&mut p, pal);
                }
                let held = slider::Look {
                    pressed: true,
                    ..hot
                };
                let mut at_mark = bar.clone();
                at_mark.set_position(95.0);
                draw(&mut p, pal, row(3), &at_mark, held, pal.pane);
                p.restore_offset(o);
            }
            let out =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
            std::fs::create_dir_all(&out).unwrap();
            bmp.write_png(&out.join(file)).unwrap();
        }
    }
}
