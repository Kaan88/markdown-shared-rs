use super::*;
use crate::gfx::Gfx;
use mdshared_text::Bitmap;

/// `row_span` is what `place` lays out: every row's top and height, with separators.
#[test]
fn row_span_is_the_layout_the_menu_places() {
    let items = vec![
        Item::new(0u8, "One"),
        Item::new(1, "Two").sep(),
        Item::new(2, "Three"),
        Item::new(3, "Four").sep(),
        Item::new(4, "Five").sep(),
    ];
    let mut menu = Menu::new(items, 30.0, 40.0, false);
    let mut bmp = Bitmap::new(400, 400);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    menu.place(&mut p, 400.0, 400.0);
    let top = menu.rect().y;
    for i in 0..menu.items.len() {
        let (offset, height) = row_span(&menu.items, i);
        assert_eq!(top + offset, menu.rows[i].y, "row {i}");
        assert_eq!(height, menu.rows[i].h, "row {i}");
    }
    // The pinned numbers a viewer once copied: 5 px padding, 32 px rows, 9 px before a rule.
    assert_eq!(row_span(&menu.items, 0), (5.0, 32.0));
    assert_eq!(row_span(&menu.items, 2), (5.0 + 9.0 + 64.0, 32.0));
}

fn placed(items: Vec<Item<u8>>, anchor: (f32, f32), area: Rect) -> Menu<u8> {
    let mut menu = Menu::new(items, anchor.0, anchor.1, true);
    let mut bmp = Bitmap::new(100, 100);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    menu.place_in(&mut p, area);
    menu
}

fn many(n: u8) -> Vec<Item<u8>> {
    (0..n).map(|i| Item::new(i, format!("Item {i}"))).collect()
}

#[test]
fn a_menu_that_is_there_draws_as_before_and_one_opening_is_fainter_and_off_its_place() {
    use crate::theme::LIGHT;
    let area = Rect::new(0.0, 0.0, 300.0, 400.0);
    let draw = |t: Option<f32>, above: bool| {
        let mut menu = Menu::new(many(4), 60.0, 200.0, false);
        if above {
            menu = menu.above();
        }
        let mut bmp = Bitmap::new(300, 400);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(area, LIGHT.page);
        match t {
            Some(t) => menu.draw_in_presence(&mut p, &LIGHT, area, t),
            None => menu.draw_in(&mut p, &LIGHT, area),
        }
        (bmp, menu.rect())
    };
    let mut blank = Bitmap::new(300, 400);
    blank.clear(LIGHT.page);
    for above in [false, true] {
        let (plain, rect) = draw(None, above);
        let (there, _) = draw(Some(1.0), above);
        assert!(there.pixels == plain.pixels, "above {above}: at 1");
        // Gone: placed all the same, so the caller's geometry holds, and nothing drawn.
        let (gone, placed) = draw(Some(0.0), above);
        assert!(gone.pixels == blank.pixels && placed == rect);
        // Half way: the whole menu, shadow and text and all, at half strength, and half of
        // the drop away from its place: 3 px up when it opens downwards, 3 px down when it
        // opens upwards.
        let (half, _) = draw(Some(0.5), above);
        let shift: i32 = if above { -3 } else { 3 };
        for y in 0..400i32 {
            for x in 0..300usize {
                let from = (y + shift).clamp(0, 399) as usize;
                for c in 0..3 {
                    let moved = plain.pixels[(from * 300 + x) * 4 + c] as u32;
                    let under = blank.pixels[(y as usize * 300 + x) * 4 + c] as u32;
                    assert_eq!(
                        half.pixels[(y as usize * 300 + x) * 4 + c] as u32,
                        (moved + under).div_ceil(2),
                        "above {above} at {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_menu_that_fits_is_placed_as_before() {
    let area = Rect::new(0.0, 40.0, 800.0, 560.0);
    let menu = placed(many(5), (700.0, 50.0), area);
    assert_eq!(menu.rect().y, 50.0);
    assert!(!menu.scrolls());
    // Opened inside the title row, a menu that fits stays where it was opened.
    assert_eq!(placed(many(3), (700.0, 25.0), area).rect().y, 25.0);
    // Too low for its anchor: moved up, but not above the area.
    let menu = placed(many(10), (700.0, 400.0), area);
    assert_eq!(menu.rect().bottom(), 596.0);
    assert!(!menu.scrolls());
}

#[test]
fn a_tall_menu_stays_below_the_top_of_its_area_and_scrolls() {
    let area = Rect::new(0.0, 40.0, 800.0, 400.0);
    let mut menu = placed(many(20), (700.0, 4.0), area);
    // 20 rows of 32 px and 10 px of padding do not fit in 392 px.
    assert!(menu.scrolls());
    assert_eq!(menu.rect().y, 40.0);
    assert_eq!(menu.rect().bottom(), 436.0);
    // The same for a menu that stands on its anchor.
    let above = Menu::new(many(20), 700.0, 300.0, true).above();
    let mut above = above;
    let mut bmp = Bitmap::new(100, 100);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    above.place_in(&mut p, area);
    assert_eq!(above.rect().y, 40.0);
    // Scrolling moves the rows, clamps at both ends, and the last row ends the menu.
    assert!(menu.scroll_by(64.0));
    assert_eq!(menu.scroll(), 64.0);
    assert_eq!(menu.row_rect(0).y, 40.0 + PAD - 64.0);
    assert!(menu.scroll_by(1e6));
    let last = menu.row_rect(19);
    assert_eq!(last.bottom() + PAD, menu.rect().bottom());
    assert!(!menu.scroll_by(1.0));
    let mut ended = placed(many(20), (700.0, 4.0), area);
    ended.scroll_to_end();
    let mut bmp = Bitmap::new(100, 100);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    ended.place_in(&mut p, area);
    assert_eq!(ended.scroll(), menu.scroll());
    assert!(menu.scroll_by(-1e6));
    assert_eq!(menu.scroll(), 0.0);
    assert!(!menu.scroll_by(-1.0));
}

#[test]
fn hits_account_for_the_scroll_and_the_clip() {
    let area = Rect::new(0.0, 40.0, 800.0, 400.0);
    let mut menu = placed(many(20), (700.0, 4.0), area);
    menu.scroll_by(320.0);
    let r = menu.row_rect(10);
    assert!(menu.pointer(r.x + 20.0, r.cy()));
    assert_eq!(menu.hover, Some(10));
    // A row scrolled out of the menu is not under a point outside it.
    let gone = menu.row_rect(0);
    assert!(gone.bottom() < menu.rect().y);
    let r0 = menu.row_rect(19);
    assert!(!menu.pointer(r0.x + 20.0, menu.rect().bottom() + 3.0) || menu.hover.is_none());
    assert_eq!(menu.hover, None);
}

#[test]
fn a_menu_whose_items_became_fewer_chooses_nothing_stale() {
    let area = Rect::new(0.0, 0.0, 400.0, 600.0);
    let mut menu = placed(many(5), (10.0, 10.0), area);
    menu.hover = Some(4);
    assert_eq!(menu.chosen(), Some(4));
    // The items are listed again while the menu is open, fewer, its rows not placed again.
    let r4 = menu.row_rect(4);
    menu.items.truncate(2);
    assert_eq!(menu.chosen(), None);
    assert!(menu.pointer(r4.x + 20.0, r4.cy()) && menu.hover.is_none());
    menu.hover = Some(3);
    menu.step(true);
    assert_eq!(menu.hover, Some(0));
    assert_eq!(menu.chosen(), Some(0));
}

#[test]
fn labels_are_read_for_access_keys() {
    let read = |s: &str| {
        let a = parse_access(s);
        let key = a.key.clone().map(|k| a.text[k].to_string());
        (a.text, key)
    };
    assert_eq!(
        read("Open &files…"),
        ("Open files…".into(), Some("f".into()))
    );
    assert_eq!(read("&&"), ("&".into(), None));
    assert_eq!(
        read("Fish && &Chips"),
        ("Fish & Chips".into(), Some("C".into()))
    );
    // A `&` before a space or at the end is the character itself.
    assert_eq!(read("A & B"), ("A & B".into(), None));
    assert_eq!(read("Save &"), ("Save &".into(), None));
    assert_eq!(read("&"), ("&".into(), None));
    // The first mark wins; a later one loses its `&` and marks nothing.
    assert_eq!(
        read("&Alpha &beta"),
        ("Alpha beta".into(), Some("A".into()))
    );
    // Non-ASCII letters: the range is in bytes of the text drawn.
    let a = parse_access("Çı&kış &Ç");
    assert_eq!(a.text, "Çıkış Ç");
    assert_eq!(a.key, Some(4..5));
    assert_eq!(access_key_of("&Ünite"), Some('Ü'));
    assert_eq!(access_key_of("No mark"), None);
    assert_eq!(access_key_of("A && B"), None);
    assert_eq!(escape_access("Q&A & more"), "Q&&A && more");
    assert_eq!(parse_access(&escape_access("Q&A && &x")).text, "Q&A && &x");
    assert!(parse_access(&escape_access("Q&A & &x")).key.is_none());
    // Case-insensitive, by Unicode lower case.
    assert!(same_letter('f', 'F') && same_letter('F', 'f'));
    assert!(same_letter('Ç', 'ç') && same_letter('ç', 'Ç'));
    assert!(same_letter('Ö', 'ö'));
    assert!(!same_letter('f', 'g'));
    assert!(!same_letter('ç', 'c'));
}

fn menu_of(labels: &[&str], access: bool) -> Menu<u8> {
    let items = labels
        .iter()
        .enumerate()
        .map(|(i, l)| Item::new(i as u8, *l))
        .collect();
    Menu::new(items, 10.0, 10.0, false).access_keys(access)
}

fn placed_width(menu: &mut Menu<u8>) -> f32 {
    let mut bmp = Bitmap::new(600, 400);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    menu.place(&mut p, 600.0, 400.0);
    menu.rect().w
}

#[test]
fn the_ampersand_is_not_measured() {
    let long = "x".repeat(24);
    let plain = format!("Open {long} files…");
    let marked = format!("Open {long} &files…");
    let both = format!("Open {long} &&files…");
    let mut a = menu_of(&[&plain], true);
    let mut b = menu_of(&[&marked], true);
    assert_eq!(placed_width(&mut a), placed_width(&mut b));
    // `&&` draws one `&`, which has width.
    let mut c = menu_of(&[&both], true);
    assert!(placed_width(&mut c) > placed_width(&mut a));
    // A menu that does not read access keys draws the label as it is, `&` included.
    let mut d = menu_of(&[&marked], false);
    assert!(placed_width(&mut d) > placed_width(&mut a));
}

#[test]
fn a_letter_activates_the_first_enabled_item_with_that_key() {
    let mut menu = menu_of(
        &["&New", "&Open files…", "Sa&ve", "Çık&ış", "O&pen again"],
        true,
    );
    menu.items[1].enabled = false;
    // Case does not matter, and a disabled item is skipped for the next one with the key.
    assert_eq!(menu.access_key('n'), Some(0));
    assert_eq!(menu.hover, Some(0));
    assert_eq!(menu.access_key('N'), Some(0));
    assert_eq!(menu.access_key('o'), None);
    assert_eq!(menu.access_key('p'), Some(4));
    assert_eq!(menu.hover, Some(4));
    assert_eq!(menu.chosen(), Some(4));
    assert_eq!(menu.access_key('V'), Some(2));
    assert_eq!(menu.access_key('Ş'), None);
    assert_eq!(menu.access_key('x'), None);
    // The same key on two enabled items: the first.
    let mut two = menu_of(&["&Copy", "Co&py path", "C&ut"], true);
    assert_eq!(two.access_key('c'), Some(0));
    assert_eq!(two.access_key('y'), None);
    assert_eq!(two.access_key('u'), Some(2));
    // Non-ASCII, by Unicode lower case.
    let mut turkish = menu_of(&["&Çıkış", "&Ünite", "&İlk"], true);
    assert_eq!(turkish.access_key('ç'), Some(0));
    assert_eq!(turkish.access_key('Ç'), Some(0));
    assert_eq!(turkish.access_key('ü'), Some(1));
    // A menu that does not read access keys never answers.
    let mut off = menu_of(&["&New", "Sa&ve"], false);
    assert_eq!(off.access_key('n'), None);
    assert_eq!(off.hover, None);
    // A label without a mark has no key, in a menu that reads them.
    let mut none = menu_of(&["New", "Open"], true);
    assert_eq!(none.access_key('n'), None);
    assert_eq!(none.hover, None);
}

fn drawn(menu: &mut Menu<u8>) -> Vec<u8> {
    let mut bmp = Bitmap::new(400, 300);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    menu.draw(&mut p, &crate::theme::LIGHT, 400.0, 300.0);
    bmp.pixels
}

#[test]
fn marks_are_underlined_only_where_asked_and_unmarked_menus_draw_as_before() {
    let labels = ["&New", "Open", "Sa&ve"];
    // Labels without a `&` draw identically whether the menu reads access keys or not, and
    // whether the underlines are shown.
    let plain = ["New", "Open", "Save"];
    let reference = drawn(&mut menu_of(&plain, false));
    assert_eq!(reference, drawn(&mut menu_of(&plain, true)));
    let mut hidden = menu_of(&plain, true);
    hidden.set_access_visible(false);
    assert_eq!(reference, drawn(&mut hidden));
    // With marks: the text is that of the unmarked labels, plus an underline when shown.
    let mut shown = menu_of(&labels, true);
    let with_line = drawn(&mut shown);
    let mut quiet = menu_of(&labels, true);
    quiet.set_access_visible(false);
    let without_line = drawn(&mut quiet);
    assert_eq!(without_line, reference, "no mark is visible when hidden");
    // A Mac has no access keys: the marks are read but never drawn.
    if crate::keys::MAC {
        assert_eq!(with_line, reference, "a Mac draws no underline");
        return;
    }
    assert_ne!(with_line, reference, "the marks are underlined");
    // Menu width and row positions are the same, so the underline is all that differs, and
    // it is on the marked letters' rows only (rows 0 and 2).
    let rows = shown.rect();
    let (r0, r1, r2) = (shown.row_rect(0), shown.row_rect(1), shown.row_rect(2));
    let differs = |r: Rect| {
        let (y0, y1) = (r.y as usize, r.bottom() as usize);
        (y0..y1).any(|y| {
            let row = |b: &[u8]| {
                let start = (y * 400 + rows.x as usize) * 4;
                b[start..start + rows.w as usize * 4].to_vec()
            };
            row(&with_line) != row(&reference)
        })
    };
    assert!(differs(r0) && differs(r2));
    assert!(!differs(r1));
}
#[test]
fn stepping_keeps_the_row_in_view_and_wraps() {
    let area = Rect::new(0.0, 40.0, 800.0, 400.0);
    let mut menu = placed(many(20), (700.0, 4.0), area);
    let mut bmp = Bitmap::new(100, 100);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for k in 0..20 {
        menu.step(true);
        menu.place_in(&mut p, area);
        let r = menu.row_rect(menu.hover.unwrap());
        assert_eq!(menu.hover, Some(k));
        assert!(
            r.y >= menu.rect().y && r.bottom() <= menu.rect().bottom(),
            "row {k}"
        );
    }
    // The last row is in view with the menu scrolled to its end.
    assert_eq!(menu.row_rect(19).bottom() + PAD, menu.rect().bottom());
    // Down from the last row wraps to the first, and the menu scrolls back.
    menu.step(true);
    menu.place_in(&mut p, area);
    assert_eq!(menu.hover, Some(0));
    assert_eq!(menu.scroll(), 0.0);
    // Up from the first wraps to the last.
    menu.step(false);
    menu.place_in(&mut p, area);
    assert_eq!(menu.hover, Some(19));
    assert!(menu.scroll() > 0.0);
}

fn look_drawn(look: Option<Look>, hover: Option<usize>) -> (Bitmap, Menu<u8>) {
    use crate::theme::LIGHT;
    let items = vec![
        Item::new(0u8, "One"),
        Item::new(1, "Two").sep(),
        Item::new(2, "Three").hint("Ctrl+T"),
    ];
    let mut menu = Menu::new(items, 40.0, 30.0, false);
    if let Some(look) = look {
        menu = menu.with_look(look);
    }
    menu.hover = hover;
    let mut bmp = Bitmap::new(320, 240);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.fill(Rect::new(0.0, 0.0, 320.0, 240.0), LIGHT.page);
    menu.draw(&mut p, &LIGHT, 320.0, 240.0);
    (bmp, menu)
}

fn pixel(b: &Bitmap, x: f32, y: f32) -> [u8; 3] {
    let i = (y as usize * b.width as usize + x as usize) * 4;
    [b.pixels[i + 2], b.pixels[i + 1], b.pixels[i]]
}

#[test]
fn the_rounded_look_is_the_old_constants_and_what_a_menu_given_no_look_draws() {
    assert_eq!(Look::default(), Look::ROUNDED);
    let r = Look::ROUNDED;
    assert_eq!(
        (r.radius, r.shadow, r.row, r.pad, r.inset),
        (8.0, true, 32.0, 5.0, 5.0)
    );
    assert_eq!(
        (r.hover_radius, r.separator, r.gap),
        (5.0, 9.0, crate::submenu::GAP)
    );
    for hover in [None, Some(0), Some(2)] {
        let (plain, a) = look_drawn(None, hover);
        let (given, b) = look_drawn(Some(Look::ROUNDED), hover);
        assert!(plain.pixels == given.pixels, "hover {hover:?}");
        assert_eq!(a.rect(), b.rect());
    }
}

#[test]
fn a_flat_menu_is_square_has_no_shadow_and_lights_the_whole_row() {
    use crate::theme::LIGHT;
    let (rounded, rm) = look_drawn(None, Some(2));
    let (flat, fm) = look_drawn(Some(Look::FLAT), Some(2));
    let (r, f) = (rm.rect(), fm.rect());
    // The rounded corner leaves the page (or its shadow) at the corner pixel; the square one
    // is the menu's line.
    assert_ne!(pixel(&rounded, r.x, r.y), LIGHT.popup_line);
    assert_eq!(pixel(&flat, f.x, f.y), LIGHT.popup_line);
    assert_eq!(
        pixel(&flat, f.right() - 1.0, f.bottom() - 1.0),
        LIGHT.popup_line
    );
    // No shadow: the page just outside the flat menu is untouched, below and to the right.
    for (x, y) in [
        (f.cx(), f.bottom() + 2.0),
        (f.right() + 2.0, f.cy()),
        (f.x - 2.0, f.cy()),
    ] {
        assert_eq!(pixel(&flat, x, y), LIGHT.page, "at {x},{y}");
    }
    assert_ne!(pixel(&rounded, r.cx(), r.bottom() + 2.0), LIGHT.page);
    // The highlight runs from the menu's line to its line, square at its corners.
    let row = fm.row_rect(2);
    assert_eq!(row.x, f.x + 1.0);
    assert_eq!(row.right(), f.right() - 1.0);
    assert_eq!(pixel(&flat, row.x, row.y), LIGHT.hover);
    assert_eq!(
        pixel(&flat, row.right() - 1.0, row.bottom() - 1.0),
        LIGHT.hover
    );
    // The rounded highlight is inset and its corner is not lit.
    let rrow = rm.row_rect(2);
    assert_ne!(pixel(&rounded, rrow.x, rrow.y), LIGHT.hover);
    // The separator's line is still drawn, across the middle of its gap.
    let line_y = fm.row_rect(1).y - Look::FLAT.separator / 2.0 - 0.5;
    assert_eq!(pixel(&flat, f.cx(), line_y), LIGHT.popup_line);
}

#[test]
fn a_look_lays_out_its_rows_and_row_span_with_says_where() {
    let look = Look::FLAT;
    let (_, menu) = look_drawn(Some(look), None);
    let top = menu.rect().y;
    for i in 0..3 {
        let (offset, height) = row_span_with(&menu.items, i, &look);
        assert_eq!(top + offset, menu.row_rect(i).y, "row {i}");
        assert_eq!(height, look.row);
    }
    // Two pads, three rows and one separator.
    assert_eq!(
        menu.rect().h,
        2.0 * look.pad + 3.0 * look.row + look.separator
    );
    assert_eq!(
        row_span_with(&menu.items, 0, &Look::ROUNDED),
        row_span(&menu.items, 0)
    );
    assert_eq!(
        row_span_with(&menu.items, 2, &Look::ROUNDED),
        row_span(&menu.items, 2)
    );
}

#[test]
fn sizes_out_of_reason_are_the_rounded_looks_own() {
    let mut odd = Look::FLAT;
    odd.radius = f32::NAN;
    odd.row = 0.0;
    odd.pad = -3.0;
    odd.inset = f32::INFINITY;
    odd.hover_radius = 1e9;
    odd.separator = f32::NEG_INFINITY;
    odd.gap = -1.0;
    let sane = odd.sane();
    let r = Look::ROUNDED;
    assert_eq!(
        (sane.radius, sane.row, sane.pad, sane.inset),
        (r.radius, r.row, r.pad, r.inset)
    );
    assert_eq!(
        (sane.hover_radius, sane.separator, sane.gap),
        (r.hover_radius, r.separator, r.gap)
    );
    // What is in reason is kept, and so is the shadow's choice.
    assert!(!sane.shadow);
    assert_eq!(Look::FLAT.sane(), Look::FLAT);
    // A menu given it draws, and is laid out as the sane look says.
    let (_, menu) = look_drawn(Some(odd), Some(1));
    assert_eq!(menu.look(), sane);
    assert_eq!(menu.row_rect(0).h, r.row);
    assert_eq!(
        row_span_with(&menu.items, 1, &odd),
        row_span(&menu.items, 1)
    );
    // Set on a menu that was made already, the same.
    let mut made = Menu::new(many(3), 10.0, 10.0, false);
    made.set_look(odd);
    assert_eq!(made.look(), sane);
    made.set_look(Look::FLAT);
    assert_eq!(made.look(), Look::FLAT);
    let mut bmp = Bitmap::new(300, 300);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    made.place(&mut p, 300.0, 300.0);
    assert_eq!(made.row_rect(0).h, Look::FLAT.row);
}

/// The shadow as `Painter::shadow` drew it before it had a strength, copied here.
fn old_shadow(p: &mut Painter, r: Rect, radius: f32, dark: bool) {
    let steps = 10;
    let strength = if dark { 0.024 } else { 0.010 };
    for i in (1..=steps).rev() {
        let d = i as f32 * 1.4;
        let rr = Rect::new(r.x - d, r.y - d + 3.0, r.w + 2.0 * d, r.h + 2.0 * d);
        p.rbox_a(rr, radius + d, [0, 0, 0], strength);
    }
}

fn shadow_drawn(scale: f32, dark: bool, draw: impl Fn(&mut Painter, Rect)) -> Bitmap {
    let mut bmp = Bitmap::new((160.0 * scale) as u32, (120.0 * scale) as u32);
    // A light ground in both, so that the integer blends of a faint shadow still differ.
    bmp.clear(if dark {
        [200, 200, 200]
    } else {
        [250, 250, 250]
    });
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    draw(&mut p, Rect::new(30.0, 25.0, 90.0, 60.0));
    bmp
}

#[test]
fn a_shadow_at_strength_one_is_the_old_shadow_and_at_zero_is_none() {
    for scale in [1.0, 1.5] {
        for dark in [false, true] {
            let old = shadow_drawn(scale, dark, |p, r| old_shadow(p, r, 8.0, dark));
            let plain = shadow_drawn(scale, dark, |p, r| p.shadow(r, 8.0, dark));
            let one = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, 1.0));
            assert!(old.pixels == plain.pixels, "scale {scale} dark {dark}");
            assert!(old.pixels == one.pixels, "scale {scale} dark {dark}");
            let blank = shadow_drawn(scale, dark, |_, _| {});
            for k in [0.0, -1.0, f32::NAN, f32::NEG_INFINITY] {
                let none = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, k));
                assert!(none.pixels == blank.pixels, "k {k}");
            }
            // Half as dark is lighter than the old shadow and twice as dark is darker, under
            // the box where the shadow is deepest.
            let at = |b: &Bitmap| {
                let (x, y) = ((75.0 * scale) as usize, (87.0 * scale) as usize);
                b.pixels[(y * b.width as usize + x) * 4] as i32
            };
            let half = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, 0.5));
            let twice = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, 2.0));
            assert!(at(&blank) > at(&half) && at(&half) > at(&one) && at(&one) > at(&twice));
            // Held to four times: a hundred is four.
            let four = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, 4.0));
            let many = shadow_drawn(scale, dark, |p, r| p.shadow_with(r, 8.0, dark, 100.0));
            assert!(four.pixels == many.pixels);
        }
    }
}

#[test]
fn a_menu_draws_its_shadow_at_the_looks_strength() {
    use crate::theme::LIGHT;
    let (rounded, rm) = look_drawn(None, None);
    assert_eq!(Look::ROUNDED.shadow_strength, 1.0);
    assert_eq!(Look::FLAT.shadow_strength, 1.0);
    let with = |k: f32| {
        let mut look = Look::ROUNDED;
        look.shadow_strength = k;
        look_drawn(Some(look), None).0
    };
    let below = |b: &Bitmap| pixel(b, rm.rect().cx(), rm.rect().bottom() + 2.0)[0];
    // Strength one is the menu every caller has had.
    assert!(with(1.0).pixels == rounded.pixels);
    // None: the page under the menu is untouched, and the menu itself is the same.
    let none = with(0.0);
    assert_eq!(
        pixel(&none, rm.rect().cx(), rm.rect().bottom() + 2.0),
        LIGHT.page
    );
    assert_eq!(
        pixel(&none, rm.rect().cx(), rm.rect().cy()),
        pixel(&rounded, rm.rect().cx(), rm.rect().cy())
    );
    assert!(below(&none) > below(&with(0.5)));
    assert!(below(&with(0.5)) > below(&rounded));
    assert!(below(&rounded) > below(&with(2.0)));
}

#[test]
fn a_shadow_strength_out_of_reason_is_held_to_zero_to_two() {
    let with = |k: f32| {
        let mut look = Look::ROUNDED;
        look.shadow_strength = k;
        look.sane().shadow_strength
    };
    assert_eq!(with(0.5), 0.5);
    assert_eq!(with(2.0), 2.0);
    assert_eq!(with(5.0), 2.0);
    assert_eq!(with(-1.0), 0.0);
    assert_eq!(with(f32::NAN), 1.0);
    assert_eq!(with(f32::INFINITY), 1.0);
    // Through a menu, as every other size.
    let mut look = Look::ROUNDED;
    look.shadow_strength = 9.0;
    let menu = Menu::new(many(2), 0.0, 0.0, false).with_look(look);
    assert_eq!(menu.look().shadow_strength, 2.0);
}

/// The two looks side by side, light and dark, a row lit and a separator in each: run with
/// `cargo test -p mdshared-ui menu_look_sheet -- --ignored`, then open
/// target/specimen/menu-look.png and menu-look-150.png.
#[test]
#[ignore]
fn menu_look_sheet() {
    use crate::theme::{DARK, LIGHT};
    let soft = {
        let mut l = Look::FLAT;
        l.radius = 4.0;
        l.hover_radius = 3.0;
        l.inset = 4.0;
        l.shadow = true;
        l
    };
    for (scale, file) in [(1.0, "menu-look.png"), (1.5, "menu-look-150.png")] {
        let (w, h) = (720.0, 400.0);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (k, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = k as f32 * h / 2.0;
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.page);
            for (j, look) in [Look::ROUNDED, soft, Look::FLAT].into_iter().enumerate() {
                let items = vec![
                    Item::new(0u8, "Open files…").hint("Ctrl+O"),
                    Item::new(1, "Add folder…"),
                    Item::new(2, "Preferences").sep().hint("Ctrl+P"),
                    Item::new(3, "Exit"),
                ];
                let mut menu =
                    Menu::new(items, 20.0 + 235.0 * j as f32, oy + 16.0, false).with_look(look);
                menu.hover = Some(1);
                menu.draw_in(&mut p, pal, Rect::new(0.0, oy, w, h / 2.0));
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).expect("the specimen folder");
        bmp.write_png(&out.join(file)).expect("the sheet");
    }
}

/// A menu around `anchor` in `area`, its one row's label `n` letters long, placed at `scale`.
fn around_at(anchor: Rect, area: Rect, n: usize, scale: f32) -> Menu<u8> {
    let mut menu = Menu::new(vec![Item::new(0u8, "m".repeat(n))], 0.0, 0.0, false).around(anchor);
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    menu.place_in(&mut p, area);
    menu
}

const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];
const FRACTIONS: [f32; 7] = [0.0, 0.17, 0.33, 0.5, 0.667, 0.75, 0.9];

/// A button at a window's right edge opens its menu leftwards, and one at the left rightwards;
/// either way the menu joins it at every scale, whatever the fractions of the button's place
/// and size and of the menu's width.
#[test]
fn a_menu_around_a_control_joins_it_at_every_scale_leftwards_or_rightwards() {
    for scale in SCALES {
        for fx in FRACTIONS {
            for fw in FRACTIONS {
                for n in [14, 19, 23, 27, 31, 36, 40] {
                    let area = Rect::new(0.0, 30.0, 600.0 + fx, 500.0);
                    let size = (28.0 + fw, 24.0 + fx);
                    let right = Rect::new(area.right() - 4.0 - size.0, 40.0 + fw, size.0, size.1);
                    let menu = around_at(right, area, n, scale);
                    assert!(menu.opened_left(), "scale {scale} {fx} {fw} {n}");
                    assert_eq!(menu.rect().right(), right.right().round());
                    assert!(
                        joins(right, menu.rect()),
                        "leftward, scale {scale}: {right:?} {:?}",
                        menu.rect()
                    );
                    let left = Rect::new(10.0 + fx, 40.0 + fw, size.0, size.1);
                    let menu = around_at(left, area, n, scale);
                    assert!(!menu.opened_left());
                    assert!(
                        joins(left, menu.rect()),
                        "rightward, scale {scale}: {left:?} {:?}",
                        menu.rect()
                    );
                }
            }
        }
    }
}

/// A menu that stands on its control (no room below) keeps its bottom on the control's top,
/// rounded, even in a look whose sizes are fractions.
#[test]
fn a_menu_flipped_above_its_control_stands_on_its_rounded_top() {
    let look = Look {
        row: 27.3,
        pad: 4.6,
        ..Look::ROUNDED
    };
    for scale in SCALES {
        for f in FRACTIONS {
            let area = Rect::new(0.0, 0.0, 600.0, 400.0);
            let anchor = Rect::new(500.0 + f, 360.0 + f, 30.0, 24.0 + f);
            let mut menu = Menu::new(many(3), 0.0, 0.0, false)
                .around(anchor)
                .with_look(look);
            let mut bmp = Bitmap::new(8, 8);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            menu.place_in(&mut p, area);
            assert!(menu.opened_above() && menu.opened_left());
            assert_eq!(menu.rect().bottom(), anchor.y.round(), "scale {scale} {f}");
            assert_eq!(
                menu.rect().right(),
                anchor.right().round(),
                "scale {scale} {f}"
            );
        }
    }
}

/// A menu hung from a point and one opened rightwards around a control are where they were:
/// the point (or the control's corner) rounded, its width rounded.
#[test]
fn a_menu_from_a_point_or_opened_rightwards_is_placed_as_before() {
    for scale in SCALES {
        for f in FRACTIONS {
            for n in [14, 23, 31, 40] {
                let area = Rect::new(0.0, 30.0, 600.0, 500.0);
                let anchor = Rect::new(40.0 + f, 40.0 + f, 28.0 + f, 24.0 + f);
                let around = around_at(anchor, area, n, scale);
                let mut point = Menu::new(
                    vec![Item::new(0u8, "m".repeat(n))],
                    40.0 + f,
                    60.0 + f,
                    false,
                );
                let mut bmp = Bitmap::new(8, 8);
                let mut gfx = Gfx::new();
                let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                point.place_in(&mut p, area);
                let w = point.rect().w;
                assert_eq!(w, w.round());
                assert_eq!(
                    point.rect(),
                    Rect::new((40.0 + f).round(), (60.0 + f).round(), w, point.rect().h)
                );
                assert_eq!(
                    around.rect(),
                    Rect::new(
                        anchor.x.round(),
                        anchor.bottom().round(),
                        w,
                        around.rect().h
                    )
                );
            }
        }
    }
}

/// The standard normal distribution's cumulative function by plain summing of its density,
/// for the soft shadow's expected values.
fn normal_cdf(t: f32) -> f32 {
    let (lo, n) = (-9.0f64, 20_000);
    let t = t as f64;
    if t <= lo {
        return 0.0;
    }
    let dx = (t - lo) / n as f64;
    let f = |x: f64| (-x * x / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let inner: f64 = (1..n).map(|i| f(lo + i as f64 * dx)).sum();
    (((f(lo) + f(t)) / 2.0 + inner) * dx) as f32
}

/// A soft-shadowed menu with the given strength and corners, drawn on white in `pal`.
fn soft_menu(pal: &crate::theme::Palette, k: f32, t: f32) -> (Bitmap, Rect) {
    let mut look = Look::ROUNDED;
    look.soft_shadow = true;
    look.shadow_strength = k;
    look.radius = 6.0;
    let mut menu = Menu::new(many(5), 60.0, 30.0, false).with_look(look);
    let mut bmp = Bitmap::new(360, 320);
    bmp.clear([255, 255, 255]);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    menu.draw_in_presence(&mut p, pal, Rect::new(0.0, 0.0, 360.0, 320.0), t);
    let r = menu.rect();
    (bmp, r)
}

#[test]
fn the_soft_shadow_is_off_in_both_looks_and_kept_by_sane() {
    assert_eq!(
        (Look::ROUNDED.soft_shadow, Look::FLAT.soft_shadow),
        (false, false)
    );
    let mut look = Look::ROUNDED;
    look.soft_shadow = true;
    assert!(look.sane().soft_shadow);
    assert!(
        Menu::new(many(2), 0.0, 0.0, false)
            .with_look(look)
            .look()
            .soft_shadow
    );
}

#[test]
fn a_soft_shadowed_menu_casts_the_popup_recipe_below_it_lighter_on_a_light_palette() {
    use crate::theme::{DARK, LIGHT};
    // The prototype's menu, `0 8px 24px 55%, 0 2px 6px 35%` at the look's 60%, and .45 of
    // that on a light palette: the shadow below the middle of the menu's foot.
    for (pal, factor) in [(&DARK, 1.0), (&LIGHT, 0.45)] {
        let (bmp, r) = soft_menu(pal, 0.6, 1.0);
        let x = r.cx();
        for d in [0.0f32, 2.0, 4.0, 8.0, 16.0, 24.0, 32.0] {
            let mut keep = 1.0;
            for (dy, blur, a) in [(8.0f32, 24.0f32, 0.55f32), (2.0, 6.0, 0.35)] {
                keep *= 1.0 - a * 0.6 * factor * normal_cdf((dy - (d + 0.5)) / (blur / 2.0));
            }
            let want = 1.0 - keep;
            let got = 1.0 - pixel(&bmp, x, r.bottom() + d)[1] as f32 / 255.0;
            assert!(
                (got - want).abs() <= 1.5 / 255.0,
                "{d} px below, dark {}: {got} drawn, {want} the recipe",
                pal.dark
            );
        }
        // Well above the menu it is faint: the shadow falls downwards.
        let above = 1.0 - pixel(&bmp, x, r.y - 6.0)[1] as f32 / 255.0;
        assert!(above < 0.12 * factor, "above: {above}");
    }
}

#[test]
fn a_soft_shadowed_menu_that_fades_fades_its_whole_shadow() {
    use crate::theme::DARK;
    // 22 px below the menu is past the 18 px a menu's fade used to take, inside the soft
    // shadow's reach: half present, it is half as dark (the menu is 3 px higher by then, so a
    // little lighter still), where a shadow left out of the fade would be nearly as dark.
    let (full, r) = soft_menu(&DARK, 1.5, 1.0);
    let (half, _) = soft_menu(&DARK, 1.5, 0.5);
    let a = |b: &Bitmap| 1.0 - pixel(b, r.cx(), r.bottom() + 22.0)[1] as f32 / 255.0;
    assert!(a(&full) > 0.08, "{}", a(&full));
    assert!(
        a(&half) < a(&full) * 0.5,
        "{} faded, {} there",
        a(&half),
        a(&full)
    );
}

/// Where the label of the first row starts: its leftmost ink, from the row's left side, in a
/// menu of `items` drawn in `look` on the light palette.
fn label_start(items: Vec<Item<u8>>, look: Look) -> f32 {
    use crate::theme::LIGHT;
    let mut menu = Menu::new(items, 40.0, 30.0, false).with_look(look);
    let mut bmp = Bitmap::new(420, 240);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 420.0, 240.0), LIGHT.page);
        menu.draw(&mut p, &LIGHT, 420.0, 240.0);
    }
    let r = menu.rows[0];
    let mut x = r.x as usize;
    while x < r.right() as usize {
        let ink =
            (r.y as usize..r.bottom() as usize).any(|y| pixel(&bmp, x as f32, y as f32)[1] < 160);
        if ink {
            return x as f32 - r.x;
        }
        x += 1;
    }
    panic!("no label drawn");
}

#[test]
fn one_lead_puts_the_mark_and_the_icon_in_one_column_before_the_label() {
    // The first row is unchecked and has no icon, so its leftmost ink is its label's.
    let marks = || {
        vec![
            Item::new(0u8, "Hello").checked(false),
            Item::new(1, "World").checked(true),
        ]
    };
    let both = || {
        vec![
            Item::new(0u8, "Hello").checked(false),
            Item::new(1, "World")
                .checked(true)
                .icon(crate::glyph::CHECK),
        ]
    };
    let plain = || vec![Item::new(0u8, "Hello"), Item::new(1, "World")];
    let one = Look {
        one_lead: true,
        ..Look::ROUNDED
    };
    // The same glyph starts each, so the differences are the columns': a lead of 36 (marks or
    // icons), 56 (both) or 9 (neither) before, ONE_LEAD or NO_LEAD with one column.
    let classic = (
        label_start(marks(), Look::ROUNDED),
        label_start(both(), Look::ROUNDED),
        label_start(plain(), Look::ROUNDED),
    );
    let shared = (
        label_start(marks(), one),
        label_start(both(), one),
        label_start(plain(), one),
    );
    assert_eq!(
        classic.0 - shared.0,
        36.0 - ONE_LEAD,
        "marks: {classic:?} {shared:?}"
    );
    assert_eq!(
        classic.1 - shared.1,
        56.0 - ONE_LEAD,
        "both: {classic:?} {shared:?}"
    );
    assert_eq!(
        classic.2 - shared.2,
        9.0 - NO_LEAD,
        "neither: {classic:?} {shared:?}"
    );
    const { assert!(!Look::ROUNDED.one_lead && !Look::FLAT.one_lead) };
}

#[test]
fn a_looks_text_size_sizes_the_labels_and_the_menu() {
    let long = "A label long enough to set the width of its menu";
    let width = |text: f32| {
        let mut m = menu_of(&[long], false).with_look(Look {
            text,
            ..Look::ROUNDED
        });
        placed_width(&mut m)
    };
    assert!(width(13.0) < width(13.5), "{} {}", width(13.0), width(13.5));
    assert_eq!(Look::ROUNDED.text, 13.5);
    assert_eq!(Look::FLAT.text, 13.5);
    for odd in [f32::NAN, 0.0, 1000.0] {
        let l = Look {
            text: odd,
            ..Look::ROUNDED
        };
        assert_eq!(l.sane().text, 13.5, "{odd}");
    }
    // The label is drawn at that size: a smaller one has less ink.
    let ink = |text: f32| {
        let look = Look {
            text,
            ..Look::ROUNDED
        };
        let (b, m) = {
            use crate::theme::LIGHT;
            let mut menu =
                Menu::new(vec![Item::new(0u8, "Hello world")], 40.0, 30.0, false).with_look(look);
            let mut bmp = Bitmap::new(320, 160);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 320.0, 160.0), LIGHT.page);
                menu.draw(&mut p, &LIGHT, 320.0, 160.0);
            }
            (bmp, menu)
        };
        let r = m.rows[0];
        let mut dark = 0;
        for y in r.y as usize..r.bottom() as usize {
            for x in r.x as usize..r.right() as usize {
                if pixel(&b, x as f32, y as f32)[1] < 160 {
                    dark += 1;
                }
            }
        }
        dark
    };
    assert!(ink(10.0) < ink(13.5), "{} {}", ink(10.0), ink(13.5));
}

#[test]
fn a_menu_entering_any_way_is_drawn_as_before_once_there_and_each_way_differs_on_the_way() {
    use crate::theme::LIGHT;
    let area = Rect::new(0.0, 0.0, 300.0, 400.0);
    let draw = |t: Option<f32>, how: Entrance, above: bool| {
        let mut menu = Menu::new(many(4), 60.0, 150.0, false);
        if above {
            menu = menu.above();
        }
        let mut bmp = Bitmap::new(300, 400);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(area, LIGHT.page);
        match t {
            Some(t) => menu.draw_in_entering(&mut p, &LIGHT, area, t, how),
            None => menu.draw_in(&mut p, &LIGHT, area),
        }
        (bmp, menu.rect())
    };
    let ways = [
        Entrance::Drop,
        Entrance::Unfold,
        Entrance::Side { leftward: false },
        Entrance::Side { leftward: true },
        Entrance::Fade,
    ];
    for above in [false, true] {
        let (plain, rect) = draw(None, Entrance::Fade, above);
        for how in ways {
            let (there, _) = draw(Some(1.0), how, above);
            assert!(there.pixels == plain.pixels, "{how:?} above {above}: at 1");
        }
        let (drop, _) = draw(Some(0.5), Entrance::Drop, above);
        let (presence, _) = {
            let mut menu = Menu::new(many(4), 60.0, 150.0, false);
            if above {
                menu = menu.above();
            }
            let mut bmp = Bitmap::new(300, 400);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, LIGHT.page);
            menu.draw_in_presence(&mut p, &LIGHT, area, 0.5);
            (bmp, ())
        };
        assert!(drop.pixels == presence.pixels, "Drop is draw_in_presence");
        let mut seen = vec![drop.pixels.clone()];
        for how in ways {
            let (half, _) = draw(Some(0.5), how, above);
            if how == Entrance::Unfold && above {
                // Above its anchor it rises: the same as a drop there.
                assert!(half.pixels == drop.pixels, "unfold above rises");
                continue;
            }
            if how != Entrance::Drop {
                assert!(
                    !seen.contains(&half.pixels),
                    "{how:?} differs from the others"
                );
                seen.push(half.pixels.clone());
            }
            if how == Entrance::Unfold {
                // The lower half is not there yet: the page shows below the unfolded line.
                let y = (rect.y + rect.h * 0.75) as u32;
                let x = (rect.x + rect.w / 2.0) as u32;
                let o = ((y * half.width + x) * 4) as usize;
                let px = [half.pixels[o + 2], half.pixels[o + 1], half.pixels[o]];
                assert_eq!(px, LIGHT.page, "below the line at half way");
            }
        }
    }
}

#[test]
fn a_menu_entering_with_its_own_drawing_moves_that_drawing_with_it() {
    use crate::theme::LIGHT;
    let area = Rect::new(0.0, 0.0, 300.0, 400.0);
    // A mark drawn beside the menu's top-left corner, inside its bounds, as a cascade's arrow is.
    let draw = |t: f32| {
        let mut menu = Menu::new(many(4), 60.0, 150.0, false);
        let mut bmp = Bitmap::new(300, 400);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(area, LIGHT.page);
        let mut ran = false;
        menu.draw_entering_with(
            &mut p,
            area,
            t,
            Entrance::Side { leftward: false },
            |p, m| {
                ran = true;
                m.draw_in(p, &LIGHT, area);
                let r = m.rect();
                p.fill(Rect::new(r.x + 4.0, r.y + 4.0, 2.0, 2.0), [255, 0, 0]);
            },
        );
        let r = menu.rect();
        let o = (((r.y + 4.0) as u32 * 300 + (r.x + 4.0) as u32) * 4) as usize;
        (
            ran,
            [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]],
            menu.drawn_bounds(),
            r,
        )
    };
    let (ran, there, bounds, rect) = draw(1.0);
    assert!(ran && there == [255, 0, 0]);
    assert!(
        bounds.x < rect.x && bounds.right() > rect.right(),
        "the shadow's reach"
    );
    let (ran, _, _, _) = draw(0.0);
    assert!(!ran, "not drawn at 0");
    // Half way it is 3 px to the left and half there: the mark's place shows neither the mark
    // whole nor the page.
    let (_, half, _, _) = draw(0.5);
    assert!(half != [255, 0, 0] && half != LIGHT.page, "{half:?}");
}
