//! The form's [`Style`]: the classic one is what every form draws, and each opt-in changes
//! only what it says.

use super::{Form, Row, Style, Target};
use crate::gfx::{Gfx, Painter, Rect, Rgb, Shade, mix, soft_bounds};
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_text::Bitmap;

fn rows() -> Vec<Row> {
    vec![
        Row::header("Colour"),
        Row::choice(1, "Theme", &["Dark", "Light", "Warm", "Cool"], 0),
        Row::toggle(2, "Joined", true),
        Row::header("Shape"),
        Row::number(3, "Corners", 6, 0, 10).unit("px"),
        Row::choice(4, "Rows", &["Tint", "Bar"], 1),
    ]
}

/// The form in `style` drawn over a `w` x `h` window of one grey, and the form (laid out).
fn drawn(pal: &Palette, style: Option<Style>, ground: u8, scale: f32) -> (Bitmap, Form) {
    let (w, h) = ((900.0 * scale) as u32, (700.0 * scale) as u32);
    let mut bmp = Bitmap::new(w, h);
    bmp.clear([ground; 3]);
    let mut gfx = Gfx::new();
    let mut f = Form::new("Look", rows()).single_page();
    if let Some(s) = style {
        f = f.style(s);
    }
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        f.draw(&mut p, pal, Rect::new(0.0, 0.0, 900.0, 700.0), false);
    }
    (bmp, f)
}

fn pixel(b: &Bitmap, x: f32, y: f32, scale: f32) -> Rgb {
    let (x, y) = ((x * scale) as usize, (y * scale) as usize);
    let i = (y * b.width as usize + x) * 4;
    [b.pixels[i + 2], b.pixels[i + 1], b.pixels[i]]
}

/// The same rows in the frozen copy of the form as it stood before styles
/// ([`super::frozen`]), drawn as [`drawn`] draws: the pixels the classic style must keep.
fn drawn_frozen(pal: &Palette, ground: u8, scale: f32) -> Bitmap {
    use super::frozen::{Form as OldForm, Row as OldRow};
    let rows = vec![
        OldRow::header("Colour"),
        OldRow::choice(1, "Theme", &["Dark", "Light", "Warm", "Cool"], 0),
        OldRow::toggle(2, "Joined", true),
        OldRow::header("Shape"),
        OldRow::number(3, "Corners", 6, 0, 10).unit("px"),
        OldRow::choice(4, "Rows", &["Tint", "Bar"], 1),
    ];
    let (w, h) = ((900.0 * scale) as u32, (700.0 * scale) as u32);
    let mut bmp = Bitmap::new(w, h);
    bmp.clear([ground; 3]);
    let mut gfx = Gfx::new();
    let mut f = OldForm::new("Look", rows).single_page();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        f.draw(&mut p, pal, Rect::new(0.0, 0.0, 900.0, 700.0), false);
    }
    bmp
}

#[test]
fn the_classic_style_is_what_a_form_given_none_draws() {
    assert_eq!(Style::default(), Style::CLASSIC);
    let c = Style::CLASSIC;
    assert_eq!(
        (c.radius, c.soft_shadow, c.neutral_segments, c.caps_headings),
        (10.0, None, false, false)
    );
    // Its headings, segmented choices, card and shadow are the form's before styles, pixel for
    // pixel, given no style or CLASSIC, light and dark, at 1 and 1.5.
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            let was = drawn_frozen(pal, 120, scale);
            for style in [None, Some(Style::CLASSIC)] {
                let (now, _) = drawn(pal, style, 120, scale);
                assert!(
                    now.pixels == was.pixels,
                    "{style:?}, dark {} at {scale}",
                    pal.dark
                );
            }
        }
    }
}

#[test]
fn a_soft_card_casts_the_popup_shadow_below_it_instead_of_the_toolkits() {
    // Over a white window (dimmed by the backdrop), the window under the card's foot is
    // darkened by the popup recipe at the given strength: about a third at the foot.
    let mut soft = Style::CLASSIC;
    soft.soft_shadow = Some(0.6);
    soft.radius = 8.0;
    for scale in [1.0, 1.5] {
        let (bmp, f) = drawn(&DARK, Some(soft), 255, scale);
        let card = f.rect();
        let x = card.cx();
        let far = pixel(&bmp, x, card.bottom() + 60.0, scale)[1] as f32;
        let at = |d: f32| 1.0 - pixel(&bmp, x, card.bottom() + d, scale)[1] as f32 / far;
        assert!((at(0.0) - 0.35).abs() < 0.03, "at {scale}: {}", at(0.0));
        assert!((at(8.0) - 0.18).abs() < 0.03, "at {scale}: {}", at(8.0));
        assert!(at(46.0).abs() < 0.02, "at {scale}: {}", at(46.0));
        // The classic shadow is twice the toolkit's on a dark palette: black at the foot.
        let (old, _) = drawn(&DARK, None, 255, scale);
        let old_at = 1.0 - pixel(&old, x, card.bottom(), scale)[1] as f32 / far;
        assert!(
            old_at > at(0.0) + 0.05,
            "{old_at} classic, {} soft",
            at(0.0)
        );
        // Nothing past the soft shadow's reach, and nothing at strength 0.
        let b = soft_bounds(card, &Shade::POPUP);
        assert_eq!(pixel(&bmp, x, b.bottom() + 2.0, scale)[1] as f32, far);
        let mut none = soft;
        none.soft_shadow = Some(0.0);
        let (bare, _) = drawn(&DARK, Some(none), 255, scale);
        assert_eq!(pixel(&bare, x, card.bottom() + 1.0, scale)[1] as f32, far);
    }
}

#[test]
fn the_card_takes_the_styles_corners_and_holds_nonsense_to_the_classic_ones() {
    let corner = |radius: f32| {
        let mut s = Style::CLASSIC;
        s.radius = radius;
        let (bmp, f) = drawn(&DARK, Some(s), 120, 1.0);
        let r = f.rect();
        // Two px in from the corner along the diagonal: inside a square card, outside a
        // rounded one.
        pixel(&bmp, r.x + 2.0, r.y + 2.0, 1.0)
    };
    assert_eq!(corner(0.0), DARK.popup);
    assert_ne!(corner(8.0), DARK.popup);
    for bad in [f32::NAN, f32::INFINITY, -3.0, 500.0] {
        assert_eq!(corner(bad), corner(10.0), "{bad}");
    }
}

#[test]
fn neutral_segments_draw_the_chosen_one_as_a_grey_pill_not_the_accent() {
    let mut neutral = Style::CLASSIC;
    neutral.neutral_segments = true;
    for pal in [&DARK, &LIGHT] {
        let (bmp, f) = drawn(pal, Some(neutral), 120, 1.0);
        let (classic, _) = drawn(pal, None, 120, 1.0);
        // Row 1 is the theme, its first segment chosen. A point at the segment's left end,
        // clear of its label, and one in the track's gap past the last segment.
        let s = f.segment_rect(1, 0);
        let (x, y) = (s.x + 3.0, s.cy());
        let pill = if pal.dark {
            mix(pal.popup, pal.text, 0.2)
        } else {
            [255, 255, 255]
        };
        assert_eq!(pixel(&bmp, x, y, 1.0), pill, "dark {}", pal.dark);
        assert_eq!(pixel(&classic, x, y, 1.0), pal.accent, "dark {}", pal.dark);
        let other = f.segment_rect(1, 2);
        let track = mix(pal.popup, pal.text, 0.04);
        assert_eq!(pixel(&bmp, other.x + 3.0, other.cy(), 1.0), track);
        // Everything outside the segmented rows is as the classic style draws it.
        let c = f.control_rect(1);
        let d = f.control_rect(5);
        for (i, (a, b)) in bmp
            .pixels
            .chunks(4)
            .zip(classic.pixels.chunks(4))
            .enumerate()
        {
            let (px, py) = ((i % 900) as f32 + 0.5, (i / 900) as f32 + 0.5);
            if !c.inset(-4.0).contains(px, py) && !d.inset(-4.0).contains(px, py) {
                assert_eq!(a, b, "({px}, {py}) dark {}", pal.dark);
            }
        }
    }
}

#[test]
fn a_hovered_neutral_segment_is_a_shade_lighter() {
    let mut neutral = Style::CLASSIC;
    neutral.neutral_segments = true;
    let (_, mut f) = drawn(&DARK, Some(neutral), 120, 1.0);
    let s = f.segment_rect(1, 2);
    f.pointer_move(s.cx(), s.cy());
    assert_eq!(f.hover, Some(Target::Segment(1, 2)));
    let mut bmp = Bitmap::new(900, 700);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.draw(&mut p, &DARK, Rect::new(0.0, 0.0, 900.0, 700.0), false);
    }
    // The row's own hover lights the row behind; the segment is lighter than the track.
    let hovered = pixel(&bmp, s.x + 3.0, s.cy(), 1.0);
    assert_eq!(hovered, mix(DARK.popup, DARK.text, 0.1));
}

#[test]
fn caps_headings_are_small_capitals_in_the_muted_colour() {
    let mut caps = Style::CLASSIC;
    caps.caps_headings = true;
    let header = |style: Option<Style>, text: &str| {
        let mut rows = rows();
        rows[0] = Row::header(text);
        let mut bmp = Bitmap::new(900, 700);
        let mut gfx = Gfx::new();
        let mut f = Form::new("Look", rows).single_page();
        if let Some(s) = style {
            f = f.style(s);
        }
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            f.draw(&mut p, &DARK, Rect::new(0.0, 0.0, 900.0, 700.0), false);
        }
        let r = f.rect_of_row(0);
        // The heading's row, as pixels.
        let mut out = Vec::new();
        for y in r.y as usize..r.bottom() as usize {
            for x in r.x as usize..r.right() as usize {
                let i = (y * 900 + x) * 4;
                out.push([bmp.pixels[i + 2], bmp.pixels[i + 1], bmp.pixels[i]]);
            }
        }
        out
    };
    // Upper case: "Colour" is drawn as "COLOUR" is.
    assert_eq!(header(Some(caps), "Colour"), header(Some(caps), "COLOUR"));
    assert_ne!(header(Some(caps), "Colour"), header(None, "Colour"));
    // Muted: no pixel of the heading is brighter than the muted colour, where the classic
    // heading reaches the text colour.
    let brightest = |px: &[Rgb]| px.iter().map(|c| c[1]).max().unwrap_or(0);
    assert!(brightest(&header(Some(caps), "Colour")) <= DARK.muted[1]);
    assert!(brightest(&header(None, "Colour")) > DARK.muted[1]);
}

#[test]
fn a_style_changed_while_the_form_is_open_shows_in_the_next_frame() {
    let (classic, mut f) = drawn(&DARK, None, 120, 1.0);
    let mut s = Style::CLASSIC;
    s.neutral_segments = true;
    assert!(f.set_style(s));
    assert!(!f.set_style(s));
    let mut bmp = Bitmap::new(900, 700);
    bmp.clear([120; 3]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.draw(&mut p, &DARK, Rect::new(0.0, 0.0, 900.0, 700.0), false);
    }
    assert!(bmp.pixels != classic.pixels);
    assert!(f.set_style(Style::CLASSIC));
}
