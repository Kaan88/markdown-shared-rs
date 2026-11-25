//! A still mockup of Purge, an uninstaller whose user interface is written in WinUI, redrawn with
//! this toolkit: Purge's layout, the toolkit's palettes, widgets and fonts.
//!
//! `cargo run --release -p mdshared-ui --example purge_mockup` draws every screen into a
//! [`Bitmap`], with no window, and writes `target/specimen/purge-<screen>-{light,dark}.png` at
//! scale 1, with `-150` added for scale 1.5.
//!
//! Nothing here runs: no program is listed, removed or changed, and the data is made up. What the
//! toolkit has no widget for is drawn from boxes in this file; `docs/PURGE-MOCKUP.md` lists those.

use std::path::Path;

use mdshared_text::bitmap::Bitmap;
use mdshared_ui::check::{TICK, Tick, draw_tick};
use mdshared_ui::chrome::{WIN_BTN_W, WinButton, draw_win_button};
use mdshared_ui::dropdown::{self, DROPDOWN_H, Dropdown, draw_dropdown};
use mdshared_ui::expander;
use mdshared_ui::flyout::{self, Flyout, Side};
use mdshared_ui::gfx::{Face, Gfx, Painter, Rect, Rgb, rgb};
use mdshared_ui::glyph;
use mdshared_ui::infobar::{InfoBar, Severity, draw_infobar, severity_colors};
use mdshared_ui::link::{self, LINK_H, draw_link};
use mdshared_ui::menu::{Item, Menu};
use mdshared_ui::overlay::{
    DIALOG_FOOTER, DIALOG_PAD, dialog_button_w, dialog_card, draw_backdrop, draw_dialog_button,
    draw_dialog_card,
};
use mdshared_ui::pane::draw_row_fill;
use mdshared_ui::scroll::draw_scrollbar;
use mdshared_ui::sections::{Entry, Sections};
use mdshared_ui::segments::{self, SEGMENT_H, draw_selector};
use mdshared_ui::select::Selection;
use mdshared_ui::slider::{draw_progress, draw_spinner_at};
use mdshared_ui::table::{self, Align, Column, Sort, Table};
use mdshared_ui::textfield::{FieldColors, TextField};
use mdshared_ui::theme::{DARK, LIGHT, Palette};
use mdshared_ui::toggle::{self, TOGGLE_H, TOGGLE_W, Toggle, draw_toggle};

/// Purge's default window size.
const W: f32 = 1360.0;
const H: f32 = 860.0;
/// Purge's title bar and navigation rail.
const TITLE: f32 = 48.0;
const RAIL: f32 = 232.0;
/// The height of a push button.
const BUTTON_H: f32 = 32.0;

// Icons the toolkit's `glyph` set does not have, on the same 16 grid.
const SHIELD: &str = "M8 1.8l5.2 1.9v4c0 3.2-2.2 5.5-5.2 6.6-3-1.1-5.2-3.4-5.2-6.6v-4z";
const LOCK: &str = "M4 7h8a.5 .5 0 0 1 .5 .5v6a.5 .5 0 0 1-.5 .5H4a.5 .5 0 0 1-.5-.5v-6A.5 .5 0 0 1 4 7zM5.5 7V5a2.5 2.5 0 0 1 5 0v2";
const FILTER: &str = "M2.5 3h11L9.5 8v5l-3-1.5V8z";
const USER: &str =
    "M5.5 5.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0zM3 14c.4-2.7 2.4-4.2 5-4.2s4.600 1.500 5 4.200";
const BOX: &str = "M2 3h12v3H2zM3 6v7h10V6M6.500 8.500h3";
const GEAR: &str = "M5.800 8a2.200 2.200 0 1 0 4.400 0a2.200 2.200 0 1 0-4.400 0zM8 1.500v2M8 12.500v2M1.500 8h2M12.500 8h2M3.400 3.400l1.400 1.400M11.200 11.200l1.400 1.400M3.400 12.600l1.400-1.400M11.200 4.800l1.400-1.400";

/// The colours of the program tiles, picked by publisher as Purge picks them.
const TILES: [u32; 10] = [
    0x0F6CBD, 0x8764B8, 0xC239B3, 0xCA5010, 0x498205, 0x038387, 0x986F0B, 0x4F6BED, 0x3B3B3B,
    0xC4314B,
];

fn tile_colour(publisher: &str) -> Rgb {
    let hash = publisher.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    rgb(TILES[hash as usize % TILES.len()])
}

fn ok(pal: &Palette) -> Rgb {
    severity_colors(Severity::Success, pal).ink
}

// ----------------------------------------------------------------------------------------------
// Small pieces the toolkit has no widget for.

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Accent,
    Standard,
    /// No fill and no border until hovered.
    Subtle,
}

fn button_w(p: &mut Painter, label: &str, icon: Option<&str>) -> f32 {
    dialog_button_w(p, label).max(p.measure(Face::Semibold, 13.0, label) + 28.0)
        + if icon.is_some() { 22.0 } else { 0.0 }
}

/// A push button: the toolkit's dialog button, with an icon in front when given.
fn button(p: &mut Painter, pal: &Palette, r: Rect, label: &str, style: Style, icon: Option<&str>) {
    let text = match style {
        Style::Accent => pal.on_accent,
        Style::Standard | Style::Subtle => pal.text,
    };
    match style {
        Style::Accent => draw_dialog_button(p, pal, r, "", true, false, false),
        Style::Standard => draw_dialog_button(p, pal, r, "", false, false, false),
        Style::Subtle => {}
    }
    let face = if style == Style::Accent {
        Face::Semibold
    } else {
        Face::Ui
    };
    let tw = p.measure(face, 13.0, label);
    let iw = if icon.is_some() { 22.0 } else { 0.0 };
    let x = r.x + ((r.w - tw - iw) / 2.0).max(4.0);
    if let Some(g) = icon {
        p.icon(g, 15.0, x + 8.0, r.cy(), text);
    }
    p.text(face, 13.0, x + iw, r.cy(), label, text, r.w - iw);
}

/// Buttons laid from `right` leftwards; returns the left edge of the leftmost.
fn buttons_right(
    p: &mut Painter,
    pal: &Palette,
    right: f32,
    y: f32,
    list: &[(&str, Style, Option<&str>)],
) -> f32 {
    let mut x = right;
    for &(label, style, icon) in list.iter().rev() {
        let w = button_w(p, label, icon);
        x -= w;
        button(p, pal, Rect::new(x, y, w, BUTTON_H), label, style, icon);
        x -= 8.0;
    }
    x + 8.0
}

fn card(p: &mut Painter, pal: &Palette, r: Rect) {
    let fill = if pal.dark { pal.bar } else { pal.field };
    p.frame(r, 8.0, fill, pal.line);
}

fn divider(p: &mut Painter, pal: &Palette, x: f32, y: f32, w: f32) {
    let px = p.px();
    p.fill(Rect::new(x, y, w, px), pal.line);
}

#[derive(Clone, Copy)]
enum Tone {
    Good,
    Caution,
    Neutral,
    Accent,
    Outline,
}

fn chip_w(p: &mut Painter, label: &str) -> f32 {
    p.measure(Face::Ui, 11.5, label) + 16.0
}

/// A small labelled pill; returns its width.
fn chip(p: &mut Painter, pal: &Palette, x: f32, cy: f32, label: &str, tone: Tone) -> f32 {
    let w = chip_w(p, label);
    let r = Rect::new(x, cy - 10.0, w, 20.0);
    let (fill, line, ink) = match tone {
        Tone::Good | Tone::Caution | Tone::Accent => {
            let c = severity_colors(
                match tone {
                    Tone::Good => Severity::Success,
                    Tone::Caution => Severity::Warning,
                    _ => Severity::Info,
                },
                pal,
            );
            (c.fill, c.line, c.ink)
        }
        Tone::Neutral => (pal.hover, pal.line, pal.muted),
        Tone::Outline => (pal.page, pal.popup_line, pal.muted),
    };
    p.frame(r, 10.0, fill, line);
    p.text_center(Face::Ui, 11.5, r, label, ink);
    w
}

/// A rounded square with the first letter of a name.
fn tile(p: &mut Painter, r: Rect, name: &str, publisher: &str) {
    p.rbox(r, r.w * 0.23, tile_colour(publisher));
    let initial: String = name.chars().take(1).collect();
    p.text_center(Face::Semibold, r.w * 0.48, r, &initial, [255, 255, 255]);
}

/// The text field in its box, with a search icon.
fn search_box(p: &mut Painter, pal: &Palette, r: Rect, placeholder: &str) {
    p.frame(r, 5.0, pal.field, pal.popup_line);
    p.icon(glyph::SEARCH, 14.0, r.x + 15.0, r.cy(), pal.muted);
    let colors = FieldColors {
        text: pal.text,
        placeholder: pal.muted,
        selection: pal.selection,
        caret: pal.text,
    };
    let area = Rect::new(r.x + 28.0, r.y + 1.0, r.w - 34.0, r.h - 2.0);
    TextField::new().draw(p, area, Face::Ui, 13.0, &colors, placeholder, false, false);
}

fn tick_at(p: &mut Painter, pal: &Palette, x: f32, cy: f32, tick: Tick) {
    draw_tick(p, pal, Rect::new(x, cy - TICK / 2.0, TICK, TICK), tick);
}

/// A check box with some of its group ticked. `check::Tick` has no such state.
fn tick_mixed(p: &mut Painter, pal: &Palette, x: f32, cy: f32) {
    let r = Rect::new(x, cy - TICK / 2.0, TICK, TICK);
    p.rbox(r, 3.0, pal.accent);
    p.fill(
        Rect::new(r.x + 4.0, r.cy() - 1.0, r.w - 8.0, 2.0),
        pal.on_accent,
    );
}

fn drop_field(p: &mut Painter, pal: &Palette, r: Rect, options: &[&str], selected: usize) {
    let d = Dropdown::new(options.iter().copied().enumerate(), selected);
    draw_dropdown(p, pal, r, &d, dropdown::Look::default());
}

fn switch(p: &mut Painter, pal: &Palette, x: f32, cy: f32, on: bool) {
    draw_toggle(
        p,
        pal,
        Rect::new(x, cy - TOGGLE_H / 2.0, TOGGLE_W, TOGGLE_H),
        &Toggle::new(on),
        toggle::Look::default(),
    );
}

/// A paragraph that returns the y under it.
#[allow(clippy::too_many_arguments)] // a place, a width and a style
fn para(p: &mut Painter, face: Face, size: f32, x: f32, y: f32, w: f32, text: &str, c: Rgb) -> f32 {
    y + p.paragraph(face, size, x, y, w, text, c)
}

// ----------------------------------------------------------------------------------------------
// The window: title bar, rail, content card, page header.

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Programs,
    Pipeline,
    Leftovers,
    History,
    Quarantine,
    Settings,
}

/// Draws the window around a page and returns the content card.
fn frame(p: &mut Painter, pal: &Palette, page: Page) -> Rect {
    p.fill(Rect::new(0.0, 0.0, W, H), pal.title);

    p.icon(glyph::DELETE, 16.0, 22.0, TITLE / 2.0, pal.accent);
    p.text(
        Face::Semibold,
        13.0,
        40.0,
        TITLE / 2.0,
        "Purge",
        pal.text,
        80.0,
    );
    let buttons = [WinButton::Min, WinButton::Max, WinButton::Close];
    for (i, b) in buttons.into_iter().enumerate() {
        let x = W - WIN_BTN_W * (3 - i) as f32;
        let r = Rect::new(x, 0.0, WIN_BTN_W, 32.0);
        draw_win_button(p, pal, b, r, false, false, false);
    }
    // Shown only when Purge runs elevated.
    if matches!(page, Page::Pipeline) {
        let label = "Administrator";
        let w = p.measure(Face::Ui, 12.0, label) + 40.0;
        let r = Rect::new(W - 3.0 * WIN_BTN_W - 12.0 - w, 10.0, w, 28.0);
        p.frame(r, 14.0, pal.page, pal.popup_line);
        p.icon(SHIELD, 14.0, r.x + 18.0, r.cy(), pal.muted);
        p.text(Face::Ui, 12.0, r.x + 30.0, r.cy(), label, pal.text, w);
    }

    let items = [
        (Page::Programs, glyph::OUTLINE, "Programs", "128", false),
        (
            Page::Pipeline,
            glyph::DELETE,
            "Uninstall pipeline",
            if page == Page::Pipeline { "1" } else { "" },
            true,
        ),
        (
            Page::Leftovers,
            glyph::SEARCH,
            "Leftovers",
            if page == Page::Leftovers { "9" } else { "" },
            true,
        ),
        (Page::History, glyph::RECENT, "History & undo", "", false),
        (Page::Quarantine, BOX, "Quarantine", "", false),
        (Page::Settings, GEAR, "Settings", "", false),
    ];
    for (i, (id, icon, label, count, badge)) in items.into_iter().enumerate() {
        let y = if id == Page::Settings {
            H - 8.0 - 36.0
        } else {
            TITLE + 4.0 + i as f32 * 38.0
        };
        let row = Rect::new(8.0, y, RAIL - 16.0, 36.0);
        draw_row_fill(p, pal, row, false, false, id == page);
        p.icon(icon, 16.0, row.x + 22.0, row.cy(), pal.text);
        let face = if id == page { Face::Semibold } else { Face::Ui };
        p.text(face, 14.0, row.x + 40.0, row.cy(), label, pal.text, 150.0);
        if count.is_empty() {
            continue;
        }
        if badge {
            let r = Rect::new(row.right() - 28.0, row.cy() - 9.0, 18.0, 18.0);
            p.rbox(r, 9.0, pal.accent);
            p.text_center(Face::Semibold, 11.0, r, count, pal.on_accent);
        } else {
            p.text_right(
                Face::Ui,
                12.0,
                row.right() - 12.0,
                row.cy(),
                count,
                pal.muted,
            );
        }
    }

    // The content card runs off the window at the right and the bottom: only its top-left corner
    // shows round, with a border above and at the left.
    let content = Rect::new(RAIL, TITLE, W - RAIL, H - TITLE);
    p.frame(
        Rect::new(content.x, content.y, content.w + 12.0, content.h + 12.0),
        8.0,
        pal.page,
        pal.line,
    );
    content
}

/// A page title with a secondary line beside it.
fn heading(p: &mut Painter, pal: &Palette, x: f32, cy: f32, title: &str, sub: &str) {
    let w = p.text(Face::Semibold, 28.0, x, cy, title, pal.text, 700.0);
    if !sub.is_empty() {
        p.text(
            Face::Ui,
            13.0,
            x + w + 14.0,
            cy + 6.0,
            sub,
            pal.muted,
            640.0,
        );
    }
}

fn info_bar(p: &mut Painter, pal: &Palette, x: f32, y: f32, w: f32, bar: &InfoBar) -> f32 {
    let h = bar.height(p, w);
    draw_infobar(p, pal, Rect::new(x, y, w, h), bar, None);
    y + h
}

// ----------------------------------------------------------------------------------------------
// Programs.

#[derive(Clone, Copy)]
enum Expect {
    Clean,
    Some(&'static str),
    Large(&'static str),
    Unknown,
}

struct Program {
    name: &'static str,
    publisher: &'static str,
    kind: &'static str,
    /// In MB.
    size: f32,
    expect: Expect,
    installed: &'static str,
    new: bool,
}

const fn program(
    name: &'static str,
    publisher: &'static str,
    kind: &'static str,
    size: f32,
    expect: Expect,
    installed: &'static str,
) -> Program {
    Program {
        name,
        publisher,
        kind,
        size,
        expect,
        installed,
        new: false,
    }
}

const PROGRAMS: [Program; 14] = [
    program(
        "7-Zip 24.08 (x64)",
        "Igor Pavlov",
        "NSIS",
        5.6,
        Expect::Clean,
        "2 Mar 2026",
    ),
    program(
        "Adobe Acrobat Reader",
        "Adobe",
        "MSI",
        612.0,
        Expect::Some("~84 MB cache and settings"),
        "18 Jan 2026",
    ),
    program(
        "Audacity 3.7.1",
        "Audacity Team",
        "Inno",
        128.0,
        Expect::Some("~12 MB profile"),
        "9 Sep 2025",
    ),
    program(
        "Blender",
        "Blender Foundation",
        "MSI",
        1130.0,
        Expect::Some("~40 MB settings"),
        "30 Jun 2026",
    ),
    program(
        "Discord",
        "Discord Inc.",
        "Other",
        412.0,
        Expect::Large("~1.3 GB cache and profile"),
        "14 Feb 2026",
    ),
    Program {
        new: true,
        ..program(
            "Git 2.51.0",
            "The Git Development Community",
            "Inno",
            362.0,
            Expect::Clean,
            "21 Nov 2026",
        )
    },
    program(
        "GIMP 3.0.4",
        "The GIMP Team",
        "Inno",
        890.0,
        Expect::Some("~22 MB profile"),
        "11 Aug 2026",
    ),
    program(
        "Mozilla Firefox (x64 en-US)",
        "Mozilla",
        "NSIS",
        247.0,
        Expect::Some("~5 MB profile and cache"),
        "6 Nov 2026",
    ),
    program(
        "Notepad++ (64-bit x64)",
        "Notepad++ Team",
        "NSIS",
        15.2,
        Expect::Clean,
        "4 Apr 2026",
    ),
    program(
        "OBS Studio",
        "OBS Project",
        "NSIS",
        486.0,
        Expect::Some("~30 MB scenes and logs"),
        "27 May 2026",
    ),
    program(
        "PuTTY release 0.83",
        "Simon Tatham",
        "MSI",
        8.9,
        Expect::Clean,
        "12 Dec 2025",
    ),
    program(
        "Spotify",
        "Spotify AB",
        "Store app",
        356.0,
        Expect::Unknown,
        "3 Oct 2026",
    ),
    program(
        "Steam",
        "Valve Corporation",
        "NSIS",
        2200.0,
        Expect::Large("~4.8 GB downloaded games"),
        "19 Jul 2025",
    ),
    program(
        "VLC media player",
        "VideoLAN",
        "NSIS",
        168.0,
        Expect::Clean,
        "8 Oct 2026",
    ),
];

const FIREFOX: usize = 7;
const SPOTIFY: usize = 11;

fn size_text(mb: f32) -> String {
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 100.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn verdict(p: &mut Painter, pal: &Palette, cell: Rect, expect: Expect) {
    let x = cell.x + table::CELL_PAD;
    let (icon, text, face, c) = match expect {
        Expect::Clean => (Some(glyph::CHECK), "Clean", Face::Ui, ok(pal)),
        Expect::Some(t) => (Some(glyph::WARN), t, Face::Ui, pal.warn),
        Expect::Large(t) => (
            Some(glyph::WARN),
            t,
            Face::Semibold,
            severity_colors(Severity::Error, pal).ink,
        ),
        Expect::Unknown => (None, "Not known yet", Face::Ui, pal.muted),
    };
    let mut x = x;
    if let Some(g) = icon {
        p.icon(g, 13.0, x + 7.0, cell.cy(), c);
        x += 19.0;
    }
    let max = cell.right() - x - 4.0;
    let s = p.elide(face, 12.5, text, max);
    p.text(face, 12.5, x, cell.cy(), &s, c, max);
}

/// What the Programs page shows over itself.
#[derive(Clone, Copy, PartialEq)]
enum Over {
    Nothing,
    FilterMenu,
}

/// The programs with their boxes ticked.
fn ticked_programs() -> Selection {
    let mut ticked = Selection::default();
    ticked.toggle(FIREFOX);
    ticked.toggle(SPOTIFY);
    ticked
}

fn programs(p: &mut Painter, pal: &Palette, over: Over) {
    let content = frame(p, pal, Page::Programs);
    let left = content.x + 28.0;
    let right = content.right() - 28.0;

    let y = content.y + 38.0;
    heading(
        p,
        pal,
        left,
        y,
        "Programs",
        "128 entries · 41.3 GB · 37 would leave leftovers",
    );
    let ticked = ticked_programs().len();
    let more = Rect::new(right - 32.0, y - 16.0, 32.0, BUTTON_H);
    button(p, pal, more, "", Style::Standard, None);
    p.icon(glyph::MORE, 16.0, more.cx(), more.cy(), pal.text);
    buttons_right(
        p,
        pal,
        more.x - 8.0,
        y - 16.0,
        &[(
            &format!("Uninstall {ticked}"),
            Style::Accent,
            Some(glyph::DELETE),
        )],
    );

    // Tabs, then the tools at the right.
    let y = content.y + 76.0;
    let tabs = ["Apps 128", "Components 64", "Unnamed 9", "Recent 12"];
    let tw = segments::bar_width(p, &tabs);
    draw_selector(
        p,
        pal,
        Rect::new(left, y, tw, SEGMENT_H),
        &tabs,
        Some(0),
        None,
    );

    let sort = Rect::new(right - 190.0, y, 190.0, DROPDOWN_H);
    drop_field(
        p,
        pal,
        sort,
        &[
            "Sort: Name",
            "Sort: Publisher",
            "Sort: Size",
            "Sort: Expected leftovers",
            "Sort: Install date",
        ],
        0,
    );
    let filter = Rect::new(sort.x - 86.0, y, 78.0, DROPDOWN_H);
    button(p, pal, filter, "Filter", Style::Subtle, Some(FILTER));
    let selected = &format!("{ticked} selected");
    p.text_right(
        Face::Ui,
        12.5,
        filter.x - 8.0,
        y + 14.0,
        selected,
        pal.muted,
    );
    let sw = p.measure(Face::Ui, 12.5, selected);
    search_box(
        p,
        pal,
        Rect::new(filter.x - 20.0 - sw - 240.0, y, 240.0, DROPDOWN_H),
        "Search programs (Ctrl+F)",
    );

    let status_h = 30.0;
    let top = y + SEGMENT_H + 12.0;
    let bottom = content.bottom() - status_h - 12.0;
    let details = Rect::new(right - 330.0, top, 330.0, bottom - top);
    let list = Rect::new(left, top, details.x - 16.0 - left, bottom - top);
    program_list(p, pal, list);
    program_details(p, pal, details);

    let sy = content.bottom() - status_h;
    divider(p, pal, content.x + 1.0, sy, content.w);
    let cy = sy + status_h / 2.0;
    p.text(
        Face::Ui,
        12.0,
        left,
        cy,
        "128 apps · 41.3 GB",
        pal.muted,
        300.0,
    );
    p.text_right(
        Face::Ui,
        12.0,
        right,
        cy,
        "Engine connected · list in 142 ms · on screen in 188 ms",
        pal.muted,
    );

    if over == Over::FilterMenu {
        let items = vec![
            Item::new(0, "Would leave leftovers").checked(true),
            Item::new(1, "Installed for this user only").checked(false),
            Item::new(2, "32-bit only").checked(false),
        ];
        let mut menu = Menu::new(items, filter.x, filter.bottom(), false).around(filter);
        menu.draw_in(p, pal, Rect::new(0.0, 0.0, W, H));
    }
}

fn program_list(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let inner = r.inset(6.0);

    let mut cols = Table::new(vec![
        Column::new(0, "", 72.0).min(72.0).unsortable().fixed(),
        Column::new(1, "Name", 220.0).min(160.0).flex(1.0),
        Column::new(2, "Size", 100.0).align(Align::Right),
        Column::new(3, "Expected leftovers", 214.0),
        Column::new(4, "Installed", 108.0),
    ]);
    cols.layout(inner.w);
    cols.set_sort(Some(Sort {
        column: 1,
        descending: false,
    }));
    let header = Rect::new(inner.x, inner.y, inner.w, table::HEADER_H);
    table::draw_header(p, pal, &cols, header, None, false);

    let view = Rect::new(
        inner.x,
        header.bottom() + 2.0,
        inner.w,
        inner.bottom() - header.bottom() - 2.0,
    );
    let mut list = Sections::new(view.h, 40.0);
    list.add_group(1, Some(26.0));
    list.add_group(PROGRAMS.len(), Some(30.0));
    // The one row of the first group is taller than a program row: it carries two buttons.
    if let Some(i) = list.index(Entry::Row { group: 0, row: 0 }) {
        list.set_height(i, 62.0);
    }
    let old = p.clip(view);
    for i in list.visible() {
        let row = list.rect(view, i);
        match list.entry(i) {
            Some(Entry::Heading(0)) => {
                p.icon(glyph::WARN, 13.0, row.x + 14.0, row.cy(), pal.warn);
                p.text(
                    Face::Semibold,
                    11.0,
                    row.x + 28.0,
                    row.cy(),
                    "REMOVED OUTSIDE PURGE",
                    pal.warn,
                    300.0,
                );
            }
            Some(Entry::Heading(_)) => {
                p.text(
                    Face::Semibold,
                    11.0,
                    row.x + 8.0,
                    row.cy() + 2.0,
                    "ALL APPS · 128",
                    pal.muted,
                    300.0,
                );
            }
            Some(Entry::Row { group: 0, .. }) => removed_row(p, pal, row),
            Some(Entry::Row { row: n, .. }) => program_row(p, pal, &cols, row, n),
            None => {}
        }
    }
    p.restore(old);
    // The list shows the top of 128 programs; the thumb stands for the whole of it.
    let track = Rect::new(r.right() - 9.0, view.y + 2.0, 6.0, view.h - 4.0);
    let thumb = Rect::new(track.x, track.y, track.w, track.h * 0.16);
    draw_scrollbar(p, pal, track, thumb, false);
}

fn removed_row(p: &mut Painter, pal: &Palette, row: Rect) {
    let c = severity_colors(Severity::Warning, pal);
    let r = Rect::new(row.x, row.y + 1.0, row.w - 6.0, row.h - 2.0);
    p.rbox(r, 5.0, c.fill);
    tile(
        p,
        Rect::new(r.x + 40.0, r.cy() - 14.0, 28.0, 28.0),
        "Zoom Workplace",
        "Zoom Communications",
    );
    let x = r.x + 80.0;
    p.text(
        Face::Ui,
        14.0,
        x,
        r.y + 15.0,
        "Zoom Workplace",
        pal.text,
        400.0,
    );
    let dismiss = button_w(p, "Dismiss", None);
    let d = Rect::new(r.right() - dismiss - 4.0, r.cy() - 14.0, dismiss, 28.0);
    button(p, pal, d, "Dismiss", Style::Subtle, None);
    let check = button_w(p, "Check for leftovers", None);
    let k = Rect::new(d.x - 4.0 - check, d.y, check, 28.0);
    button(p, pal, k, "Check for leftovers", Style::Standard, None);
    let max = k.x - x - 12.0;
    let note = "Removed outside Purge. Purge recorded it before, so it can look for what it left.";
    p.paragraph(Face::Ui, 12.0, x, r.y + 26.0, max, note, pal.muted);
}

fn program_row(p: &mut Painter, pal: &Palette, cols: &Table, row: Rect, n: usize) {
    let g = &PROGRAMS[n];
    let ticked = ticked_programs().contains(n);
    let body = Rect::new(row.x, row.y + 1.0, row.w - 6.0, row.h - 2.0);
    // Firefox is the row the details card shows.
    draw_row_fill(p, pal, body, false, false, n == FIREFOX);
    let cell = |k: usize| cols.cell(row, k);

    tick_at(p, pal, cell(0).x + 12.0, row.cy(), Tick::new(ticked));
    let t = cell(0);
    tile(
        p,
        Rect::new(t.x + 40.0, row.cy() - 13.0, 26.0, 26.0),
        g.name,
        g.publisher,
    );

    let c = cell(1);
    let x = c.x + table::CELL_PAD;
    let max = c.w - table::CELL_PAD * 2.0;
    let name = p.elide(Face::Ui, 14.0, g.name, max - 34.0);
    let w = p.text(Face::Ui, 14.0, x, row.cy() - 8.0, &name, pal.text, max);
    if g.new {
        p.text(
            Face::Semibold,
            11.0,
            x + w + 8.0,
            row.cy() - 8.0,
            "new",
            ok(pal),
            40.0,
        );
    }
    let by = format!("{} · {}", g.publisher, g.kind);
    let by = p.elide(Face::Ui, 12.0, &by, max);
    p.text(Face::Ui, 12.0, x, row.cy() + 9.0, &by, pal.muted, max);

    // The size and, under it, a bar of its share of the largest.
    let s = cell(2);
    table::draw_cell_text(
        p,
        Rect::new(s.x, s.y - 3.0, s.w, s.h),
        Align::Right,
        &size_text(g.size),
        pal.text,
    );
    let share = (g.size / 2200.0).sqrt();
    let bw = ((s.w - table::CELL_PAD * 2.0).min(96.0) * share).max(4.0);
    p.rbox_a(
        Rect::new(s.right() - table::CELL_PAD - bw, row.cy() + 8.0, bw, 3.0),
        1.5,
        pal.accent,
        0.7,
    );

    verdict(p, pal, cell(3), g.expect);
    table::draw_cell_text(p, cell(4), Align::Left, g.installed, pal.muted);
}

fn program_details(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let x = r.x + 18.0;
    let w = r.w - 36.0;
    let mut y = r.y + 18.0;

    tile(p, Rect::new(x, y, 44.0, 44.0), "Mozilla Firefox", "Mozilla");
    let tx = x + 56.0;
    let name = p.elide(
        Face::Semibold,
        18.0,
        "Mozilla Firefox (x64 en-US)",
        w - 56.0,
    );
    p.text(
        Face::Semibold,
        18.0,
        tx,
        y + 13.0,
        &name,
        pal.text,
        w - 56.0,
    );
    p.text(
        Face::Ui,
        12.0,
        tx,
        y + 34.0,
        "Mozilla · 146.0.1 · NSIS",
        pal.muted,
        w - 56.0,
    );
    y += 56.0;
    button(
        p,
        pal,
        Rect::new(x, y, w, BUTTON_H),
        "Uninstall",
        Style::Accent,
        Some(glyph::DELETE),
    );
    y += BUTTON_H + 12.0;
    divider(p, pal, x, y, w);
    y += 18.0;

    p.text(
        Face::Semibold,
        13.0,
        x,
        y,
        "What Purge expects",
        pal.text,
        w,
    );
    y += 24.0;
    let expects: [(&str, Rgb, &str, &str); 3] = [
        (
            glyph::CHECK,
            ok(pal),
            "Its uninstaller removes the program files",
            "",
        ),
        (
            glyph::WARN,
            pal.warn,
            "Cache and update data stay",
            "about 2.7 MB",
        ),
        (
            USER,
            pal.muted,
            "Your profile stays: bookmarks, history, logins",
            "2.1 MB",
        ),
    ];
    for (icon, c, text, detail) in expects {
        let dw = p.measure(Face::Ui, 12.0, detail);
        let tw = w - 22.0 - if detail.is_empty() { 0.0 } else { dw + 10.0 };
        p.icon(icon, 14.0, x + 7.0, y + 8.0, c);
        let h = p.paragraph(Face::Ui, 12.5, x + 22.0, y, tw, text, pal.text);
        if !detail.is_empty() {
            p.text_right(Face::Ui, 12.0, x + w, y + 8.0, detail, pal.muted);
        }
        y += h + 8.0;
    }
    y += 4.0;
    divider(p, pal, x, y, w);
    y += 16.0;

    let label_w = 76.0;
    let facts = [
        ("Uninstaller", "NSIS · quiet mode available"),
        ("Location", "C:\\Program Files\\Mozilla Firefox"),
        ("Installed", "6 Nov 2026 · for all users"),
        ("Size", "247 MB"),
        ("Platform", "64-bit"),
        ("Also owns", "Mozilla Maintenance Service"),
    ];
    for (label, value) in facts {
        p.text(Face::Ui, 12.0, x, y, label, pal.muted, label_w);
        if label == "Location" {
            let lr = link::link_rect(p, x + label_w, y - LINK_H / 2.0, value);
            let lr = Rect::new(lr.x, lr.y, lr.w.min(w - label_w), lr.h);
            draw_link(p, pal, lr, value, link::Look::default());
        } else {
            p.text(Face::Ui, 12.5, x + label_w, y, value, pal.text, w - label_w);
        }
        y += 23.0;
    }
    y += 4.0;
    expander::draw_header(
        p,
        pal,
        Rect::new(x - 6.0, y, w + 12.0, expander::HEADER_H),
        "Commands and registry key",
        true,
        expander::Look::default(),
    );
    y += expander::HEADER_H + 6.0;
    let commands = [
        (
            "Uninstall",
            "\"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\"",
        ),
        (
            "Quiet",
            "\"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\" /S",
        ),
        (
            "Registry key",
            "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Mozilla Firefox",
        ),
    ];
    for (label, value) in commands {
        p.text(Face::Ui, 12.0, x, y, label, pal.muted, w);
        y += 10.0;
        for line in wrap_path(p, 11.5, value, w) {
            p.text(Face::Mono, 11.5, x, y + 6.0, &line, pal.text, w);
            y += 15.0;
        }
        y += 14.0;
    }
}

/// Breaks a path or a command after a backslash or a space, so that no line is wider than `max`.
/// (`Painter::paragraph` breaks at spaces only; `seltext` is not in this branch.)
fn wrap_path(p: &mut Painter, size: f32, text: &str, max: f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for part in text.split_inclusive(['\\', ' ']) {
        let line = lines.last().expect("never empty");
        if !line.is_empty() && p.measure(Face::Mono, size, &format!("{line}{part}")) > max {
            lines.push(String::new());
        }
        lines.last_mut().expect("never empty").push_str(part);
    }
    lines
}

/// Text in the monospaced face, wrapped as a path; returns the y under it.
#[allow(clippy::too_many_arguments)] // a place, a width and a style
fn mono(p: &mut Painter, size: f32, x: f32, y: f32, w: f32, text: &str, c: Rgb) -> f32 {
    let mut y = y;
    for line in wrap_path(p, size, text, w) {
        p.text(Face::Mono, size, x, y + size * 0.7, &line, c, w);
        y += size * 1.4;
    }
    y
}

// ----------------------------------------------------------------------------------------------
// The two dialogs.

/// The backdrop, the card and its footer buttons (from the right). Returns the card.
fn dialog(p: &mut Painter, pal: &Palette, size: (f32, f32), buttons: &[(&str, bool)]) -> Rect {
    draw_backdrop(p, pal, W, H);
    let r = dialog_card(size.0, size.1, W, H);
    let footer = draw_dialog_card(p, pal, r);
    let mut x = footer.right() - DIALOG_PAD;
    for &(label, primary) in buttons.iter().rev() {
        let w = dialog_button_w(p, label);
        x -= w;
        let b = Rect::new(x, footer.y + (DIALOG_FOOTER - BUTTON_H) / 2.0, w, BUTTON_H);
        draw_dialog_button(p, pal, b, label, primary, false, false);
        x -= 8.0;
    }
    r
}

fn confirm(p: &mut Painter, pal: &Palette) {
    programs(p, pal, Over::Nothing);
    let r = dialog(
        p,
        pal,
        (540.0 + DIALOG_PAD * 2.0, 622.0),
        &[
            ("Uninstall", true),
            ("Check again", false),
            ("Not now", false),
        ],
    );
    let x = r.x + DIALOG_PAD;
    let w = r.w - DIALOG_PAD * 2.0;
    let mut y = r.y + DIALOG_PAD;
    p.text(
        Face::Semibold,
        20.0,
        x,
        y + 10.0,
        "Uninstall Mozilla Firefox (x64 en-US)?",
        pal.text,
        w,
    );
    y += 40.0;
    p.text(Face::Semibold, 13.0, x, y + 8.0, "Purge will:", pal.text, w);
    y += 24.0;
    for step in [
        "1. Record what the program registered",
        "2. Run its uninstaller as administrator",
        "3. Check that it is gone",
        "4. Find its leftovers",
        "5. Review the list before anything moves",
    ] {
        p.text(Face::Ui, 13.0, x + 8.0, y + 9.0, step, pal.text, w);
        y += 21.0;
    }
    y += 10.0;
    p.text(Face::Ui, 12.0, x, y + 7.0, "Command", pal.muted, w);
    y += 20.0;
    let command = "\"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\"";
    let box_r = Rect::new(x, y, w, 32.0);
    p.frame(box_r, 5.0, pal.page, pal.line);
    p.text(
        Face::Mono,
        12.0,
        x + 10.0,
        box_r.cy(),
        command,
        pal.text,
        w - 20.0,
    );
    y += 40.0;
    p.text(
        Face::Ui,
        12.0,
        x,
        y + 7.0,
        "Signed by Mozilla Corporation",
        pal.muted,
        w,
    );
    y += 24.0;
    let bar = InfoBar::new(
        Severity::Warning,
        "Still running: firefox.exe",
        "Close it yourself; Purge never closes a program for you. Check again once it is closed.",
    )
    .closable(false);
    y = info_bar(p, pal, x, y, w, &bar) + 16.0;
    p.text(Face::Ui, 13.0, x, y + 10.0, "Use quiet mode", pal.text, w);
    switch(p, pal, x + w - TOGGLE_W, y + 10.0, true);
    y += 28.0;
    let quiet = "Quiet command: \"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\" /S";
    y = para(p, Face::Ui, 12.0, x, y, w, quiet, pal.muted) + 14.0;
    p.text(
        Face::Ui,
        13.0,
        x,
        y + 8.0,
        "Restore point for the cleanup step",
        pal.text,
        w,
    );
    y += 22.0;
    drop_field(
        p,
        pal,
        Rect::new(x, y, 280.0, DROPDOWN_H),
        &["Off", "High-risk changes only (default)", "Always"],
        1,
    );
    y += DROPDOWN_H + 14.0;
    para(
        p,
        Face::Ui,
        12.0,
        x,
        y,
        w,
        "Nothing is moved or removed by Purge until you confirm the list on the next page.",
        pal.muted,
    );
}

fn elevation(p: &mut Painter, pal: &Palette) {
    programs(p, pal, Over::Nothing);
    let r = dialog(
        p,
        pal,
        (520.0, 280.0),
        &[("Continue", true), ("Not now", false)],
    );
    let x = r.x + DIALOG_PAD;
    let w = r.w - DIALOG_PAD * 2.0;
    let mut y = r.y + DIALOG_PAD;
    p.text(
        Face::Semibold,
        20.0,
        x,
        y + 10.0,
        "This needs administrator permission",
        pal.text,
        w,
    );
    y += 36.0;
    y = para(
        p,
        Face::Ui,
        13.0,
        x,
        y,
        w,
        "For this, Purge starts a helper called purge-broker.exe with administrator permission. Windows asks you first, and may show the publisher as unknown until Purge is signed.",
        pal.text,
    );
    para(
        p,
        Face::Ui,
        13.0,
        x,
        y + 12.0,
        w,
        "Any program can show a prompt like that, so the Windows prompt is not your approval. Before anything is moved or removed, Purge shows its own review and waits for you.",
        pal.text,
    );
}

// ----------------------------------------------------------------------------------------------
// The uninstall pipeline.

#[derive(Clone, Copy)]
enum Mark {
    Done,
    Active,
    Warning,
    Pending,
}

fn mark(p: &mut Painter, pal: &Palette, cx: f32, cy: f32, m: Mark) {
    match m {
        Mark::Done => p.icon(glyph::CHECK, 16.0, cx, cy, ok(pal)),
        Mark::Active => draw_spinner_at(p, pal, cx, cy, 7.0, None),
        Mark::Warning => p.icon(glyph::WARN, 16.0, cx, cy, pal.warn),
        Mark::Pending => p.ring(cx, cy, 6.0, 1.3, pal.faint, 1.0),
    }
}

fn pipeline(p: &mut Painter, pal: &Palette, finished: bool) {
    let content = frame(p, pal, Page::Pipeline);
    let left = content.x + 28.0;
    let width = (content.w - 56.0).min(1100.0 - 56.0);
    let y = content.y + 38.0;
    heading(
        p,
        pal,
        left,
        y,
        "Uninstall pipeline",
        "Mozilla Firefox (x64 en-US) · 1 of 2",
    );
    let cw = chip_w(p, "Administrator");
    chip(p, pal, left + width - cw, y, "Administrator", Tone::Accent);

    let mut y = content.y + 78.0;
    let bar = InfoBar::new(
        Severity::Info,
        "Sample data.",
        "This run is an example. Nothing on this PC is uninstalled.",
    )
    .closable(false);
    y = info_bar(p, pal, left, y, width, &bar) + 16.0;

    if finished {
        let next = Rect::new(left, y, width, 66.0);
        card(p, pal, next);
        let title = "Next: Spotify (2 of 2).";
        p.text(
            Face::Semibold,
            14.0,
            left + 18.0,
            y + 22.0,
            title,
            pal.text,
            600.0,
        );
        let note = "It gets its own confirmation, prompts and leftover scan. The next one starts only when you choose.";
        p.text(
            Face::Ui,
            12.0,
            left + 18.0,
            y + 45.0,
            note,
            pal.muted,
            700.0,
        );
        let actions = [
            ("Uninstall Spotify", Style::Accent, None),
            ("Skip", Style::Standard, None),
        ];
        buttons_right(p, pal, next.right() - 18.0, y + 17.0, &actions);
        y = next.bottom() + 16.0;
    }

    let side = 320.0;
    let main_w = width - side - 16.0;
    let top = y;

    let program = Rect::new(left, y, main_w, 124.0);
    card(p, pal, program);
    tile(
        p,
        Rect::new(left + 18.0, y + 16.0, 36.0, 36.0),
        "Mozilla Firefox",
        "Mozilla",
    );
    let x = left + 68.0;
    p.text(
        Face::Semibold,
        15.0,
        x,
        y + 24.0,
        "Mozilla Firefox (x64 en-US)",
        pal.text,
        main_w - 90.0,
    );
    p.text(
        Face::Ui,
        12.0,
        x,
        y + 46.0,
        if finished {
            "Its uninstaller ran at administrator."
        } else {
            "Its uninstaller is running at administrator."
        },
        pal.muted,
        main_w - 90.0,
    );
    p.text(
        Face::Mono,
        12.5,
        x,
        y + 72.0,
        "\"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\" /S",
        pal.text,
        main_w - 90.0,
    );
    p.text(
        Face::Ui,
        12.0,
        x,
        y + 98.0,
        "Signed by Mozilla Corporation",
        pal.muted,
        main_w - 90.0,
    );
    y = program.bottom() + 12.0;

    let titles = [
        "Record what it registered",
        "Run its own uninstaller",
        "Check that it is gone",
        "Find its leftovers",
        "Ready for review",
    ];
    let states = if finished {
        [
            (Mark::Done, "Recorded by the administrator part"),
            (Mark::Done, "It finished with exit code 0"),
            (Mark::Done, "Confirmed gone"),
            (Mark::Done, "It left 10 items"),
            (Mark::Done, "Review them on Leftovers"),
        ]
    } else {
        [
            (Mark::Done, "Recorded by the administrator part"),
            (Mark::Active, "Waiting for it to finish"),
            (Mark::Pending, "Not checked yet"),
            (Mark::Pending, "Not scanned yet"),
            (Mark::Pending, "You confirm the list before anything moves"),
        ]
    };
    let steps: Vec<(Mark, &str, &str)> = states
        .iter()
        .zip(titles)
        .map(|(&(m, detail), title)| (m, title, detail))
        .collect();
    let progress = Rect::new(left, y, main_w, 48.0 + steps.len() as f32 * 44.0 + 6.0);
    card(p, pal, progress);
    p.text(
        Face::Semibold,
        14.0,
        left + 18.0,
        y + 26.0,
        "Progress",
        pal.text,
        200.0,
    );
    let mut sy = y + 50.0;
    for (i, (m, title, detail)) in steps.into_iter().enumerate() {
        mark(p, pal, left + 28.0, sy + 9.0, m);
        if i + 1 < titles.len() {
            p.fill(Rect::new(left + 27.5, sy + 20.0, 1.0, 20.0), pal.line);
        }
        let c = if matches!(m, Mark::Pending) {
            pal.muted
        } else {
            pal.text
        };
        p.text(
            Face::Semibold,
            13.0,
            left + 46.0,
            sy + 9.0,
            title,
            c,
            main_w - 70.0,
        );
        p.text(
            Face::Ui,
            12.0,
            left + 46.0,
            sy + 28.0,
            detail,
            pal.muted,
            main_w - 70.0,
        );
        sy += 44.0;
    }
    let end = if finished {
        progress.bottom()
    } else {
        still_running(p, pal, left, progress.bottom() + 12.0, main_w)
    };
    let s = Rect::new(left + main_w + 16.0, top, side, 392.0);
    safety_net(p, pal, s);
    p.text(
        Face::Ui,
        12.0,
        left,
        end.max(s.bottom()) + 22.0,
        "Nothing is moved or removed by Purge until you confirm the list on the leftovers page.",
        pal.muted,
        width,
    );
}

/// The card for a program that is still running; returns the y under it.
fn still_running(p: &mut Painter, pal: &Palette, left: f32, y: f32, main_w: f32) -> f32 {
    let running = Rect::new(left, y, main_w, 126.0);
    card(p, pal, running);
    mark(p, pal, left + 26.0, y + 24.0, Mark::Warning);
    p.text(
        Face::Semibold,
        14.0,
        left + 42.0,
        y + 24.0,
        "Still running",
        pal.text,
        200.0,
    );
    p.text(
        Face::Ui,
        13.0,
        left + 18.0,
        y + 48.0,
        "firefox.exe is still running from the program's folder. Its files are in use.",
        pal.text,
        main_w - 36.0,
    );
    p.text(
        Face::Ui,
        12.0,
        left + 18.0,
        y + 70.0,
        "Close it yourself, then check again. Purge never closes a program for you.",
        pal.muted,
        main_w - 36.0,
    );
    let w = button_w(p, "Check again", Some(glyph::RELOAD));
    button(
        p,
        pal,
        Rect::new(left + 10.0, y + 84.0, w, BUTTON_H),
        "Check again",
        Style::Subtle,
        Some(glyph::RELOAD),
    );
    running.bottom()
}

fn safety_net(p: &mut Painter, pal: &Palette, s: Rect) {
    card(p, pal, s);
    let x = s.x + 18.0;
    let w = s.w - 36.0;
    let mut y = s.y + 18.0;
    p.text(Face::Semibold, 14.0, x, y + 8.0, "Safety net", pal.text, w);
    y += 30.0;
    p.text(
        Face::Semibold,
        13.0,
        x,
        y + 8.0,
        "Restore point at the cleanup step",
        pal.text,
        w,
    );
    y += 24.0;
    drop_field(
        p,
        pal,
        Rect::new(x, y, w, DROPDOWN_H),
        &["Off", "High-risk changes only (default)", "Always"],
        1,
    );
    y += DROPDOWN_H + 8.0;
    y = para(
        p,
        Face::Ui,
        12.0,
        x,
        y,
        w,
        "Purge makes the restore point just before it moves the leftovers, not before the uninstaller runs.",
        pal.muted,
    ) + 16.0;
    p.text(Face::Ui, 13.0, x, y + 10.0, "Use quiet mode", pal.text, w);
    switch(p, pal, x + w - TOGGLE_W, y + 10.0, true);
    y += 28.0;
    y = para(
        p,
        Face::Ui,
        12.0,
        x,
        y,
        w,
        "Quiet mode skips the program's own dialogs. Purge never answers \"Keep my settings?\" for you.",
        pal.muted,
    ) + 14.0;
    divider(p, pal, x, y, w);
    y += 18.0;
    p.text(
        Face::Ui,
        13.0,
        x,
        y,
        "Leftovers go to quarantine first",
        pal.text,
        w,
    );
    y += 20.0;
    p.text(
        Face::Ui,
        12.0,
        x,
        y,
        "Undo any time in History for 30 days",
        pal.muted,
        w,
    );
    y += 18.0;
    para(
        p,
        Face::Ui,
        12.0,
        x,
        y,
        w,
        "Purge's administrator helper closes when the cleanup and the scan are done.",
        pal.muted,
    );
}

// ----------------------------------------------------------------------------------------------
// Leftovers.

#[derive(Clone, Copy, PartialEq)]
enum Rating {
    Safe,
    Likely,
    Protected,
}

struct Leftover {
    path: &'static str,
    why: &'static str,
    rating: Rating,
    size: &'static str,
    ticked: bool,
}

const fn leftover(
    path: &'static str,
    why: &'static str,
    rating: Rating,
    size: &'static str,
    ticked: bool,
) -> Leftover {
    Leftover {
        path,
        why,
        rating,
        size,
        ticked,
    }
}

/// Group title, count of items, share of the bar, size, and the items shown.
type Group = (
    &'static str,
    &'static str,
    f32,
    &'static str,
    &'static [Leftover],
);

const GROUPS: [Group; 4] = [
    (
        "Files & folders",
        "3",
        0.56,
        "4.8 MB",
        &[
            leftover(
                "C:\\Users\\tester\\AppData\\Local\\Mozilla",
                "App cache · only Firefox data inside · vendor folder",
                Rating::Safe,
                "2.6 MB",
                true,
            ),
            leftover(
                "C:\\ProgramData\\Mozilla-1de4eec8-1241-4177-a864-e594e8d1fb38",
                "Update data named after this install",
                Rating::Safe,
                "96 KB",
                true,
            ),
            leftover(
                "C:\\Users\\tester\\AppData\\Roaming\\Mozilla",
                "User data: profiles, bookmarks, saved logins",
                Rating::Likely,
                "2.1 MB",
                false,
            ),
        ],
    ),
    (
        "Registry",
        "3",
        0.0,
        "",
        &[
            leftover(
                "HKCU\\Software\\Mozilla\\Firefox",
                "Recorded before the uninstaller ran",
                Rating::Safe,
                "",
                true,
            ),
            leftover(
                "HKLM\\Software\\Classes\\FirefoxHTML-308046B0AF4A39CB",
                "File type this install registered",
                Rating::Safe,
                "",
                true,
            ),
            leftover(
                "HKLM\\Software\\Classes\\CLSID\\{1814CEEB-49E2-407F-AF99-FA755A7D2607}",
                "Points into the removed folder",
                Rating::Likely,
                "",
                false,
            ),
        ],
    ),
    (
        "Services, tasks & other",
        "2",
        0.0,
        "",
        &[
            leftover(
                "Task: Firefox Default Browser Agent 308046B0AF4A39CB",
                "Runs a program that is gone",
                Rating::Safe,
                "",
                true,
            ),
            leftover(
                "Service: Mozilla Maintenance Service",
                "Shared with other Mozilla programs",
                Rating::Likely,
                "",
                true,
            ),
        ],
    ),
    (
        "Protected — never removed",
        "1",
        0.0,
        "",
        &[leftover(
            "C:\\Windows\\Prefetch",
            "Windows' own folder",
            Rating::Protected,
            "",
            false,
        )],
    ),
];

fn leftovers_header(p: &mut Painter, pal: &Palette, content: Rect, sub: &str) -> f32 {
    let left = content.x + 28.0;
    let y = content.y + 40.0;
    tile(
        p,
        Rect::new(left, y - 18.0, 36.0, 36.0),
        "Mozilla Firefox",
        "Mozilla",
    );
    p.text(
        Face::Semibold,
        24.0,
        left + 48.0,
        y - 6.0,
        "Leftovers of Mozilla Firefox",
        pal.text,
        600.0,
    );
    p.text(Face::Ui, 12.0, left + 48.0, y + 16.0, sub, pal.muted, 600.0);
    content.y + 72.0
}

fn leftovers(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::Leftovers);
    let left = content.x + 28.0;
    let right = content.right() - 28.0;
    let width = right - left;
    let mut y = leftovers_header(p, pal, content, "Post-uninstall scan · 10 items");
    buttons_right(
        p,
        pal,
        right,
        content.y + 24.0,
        &[
            ("Collapse all", Style::Subtle, None),
            ("Show weak matches (1)", Style::Subtle, None),
        ],
    );

    let bar = InfoBar::new(
        Severity::Info,
        "Sample data.",
        "This list is an example, not a scan of this PC. Nothing can be moved from it.",
    )
    .closable(false);
    y = info_bar(p, pal, left, y + 4.0, width, &bar) + 12.0;

    // The verdict card.
    let v = Rect::new(left, y, width, 96.0);
    card(p, pal, v);
    p.text(
        Face::Semibold,
        11.0,
        left + 18.0,
        y + 18.0,
        "READY TO MOVE",
        pal.muted,
        200.0,
    );
    p.text(
        Face::Semibold,
        20.0,
        left + 18.0,
        y + 40.0,
        "6 items · 2.7 MB",
        pal.text,
        300.0,
    );
    let track = Rect::new(left + 18.0, y + 58.0, width - 36.0, 10.0);
    p.rbox(track, 5.0, pal.faint);
    let old = p.clip(Rect::new(track.x, track.y, track.w * 0.86, track.h));
    p.rbox(track, 5.0, pal.warn);
    p.restore(old);
    let old = p.clip(Rect::new(track.x, track.y, track.w * 0.55, track.h));
    p.rbox(track, 5.0, ok(pal));
    p.restore(old);
    let mut lx = left + 18.0;
    for (icon, c, text) in [
        (glyph::CHECK, ok(pal), "Safe 5 · 2.7 MB"),
        (glyph::WARN, pal.warn, "Likely 3 · 2.1 MB"),
        (LOCK, pal.muted, "Protected 1"),
    ] {
        p.icon(icon, 13.0, lx + 7.0, y + 82.0, c);
        lx += 20.0;
        lx += p.text(Face::Ui, 12.0, lx, y + 82.0, text, pal.text, 200.0) + 22.0;
    }
    y = v.bottom() + 16.0;

    let footer_h = 60.0;
    let bottom = content.bottom() - footer_h - 12.0;
    let side = 340.0;
    let list = Rect::new(left, y, width - side - 16.0, bottom - y);
    leftover_list(p, pal, list);
    leftover_selected(p, pal, Rect::new(right - side, y, side, bottom - y));

    let fy = content.bottom() - footer_h;
    divider(p, pal, content.x + 1.0, fy, content.w);
    let cy = fy + footer_h / 2.0;
    let w = p.text(
        Face::Semibold,
        13.0,
        left,
        cy - 9.0,
        "6 of 9 selected",
        pal.text,
        200.0,
    );
    p.text(
        Face::Ui,
        12.0,
        left + w,
        cy - 9.0,
        " · 2.7 MB, 2 file items, 2 registry items, 2 tasks or services",
        pal.muted,
        500.0,
    );
    tick_at(p, pal, left, cy + 12.0, Tick::new(false));
    p.text(
        Face::Ui,
        12.5,
        left + 24.0,
        cy + 12.0,
        "Delete instead of keeping in quarantine",
        pal.text,
        300.0,
    );
    let x = buttons_right(
        p,
        pal,
        right,
        cy - 16.0,
        &[
            ("Skip", Style::Standard, None),
            ("Move 6 items to quarantine…", Style::Accent, Some(SHIELD)),
        ],
    );
    p.text_right(
        Face::Ui,
        12.0,
        x - 14.0,
        cy,
        "You'll confirm in the Purge security prompt",
        pal.muted,
    );
}

fn leftover_list(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let view = r.inset(6.0);
    let mut list = Sections::new(view.h, 40.0);
    for g in &GROUPS {
        list.add_group(g.4.len(), Some(40.0));
    }
    let old = p.clip(view);
    for i in list.visible() {
        let row = list.rect(view, i);
        match list.entry(i) {
            Some(Entry::Heading(g)) => group_header(p, pal, row, g, !list.is_collapsed(g)),
            Some(Entry::Row { group, row: n }) => {
                leftover_row(p, pal, row, &GROUPS[group].4[n], group == 0 && n == 2);
            }
            None => {}
        }
    }
    p.restore(old);
    if let Some((track, thumb)) = list.thumb(view) {
        draw_scrollbar(p, pal, track, thumb, false);
    }
}

fn group_header(p: &mut Painter, pal: &Palette, row: Rect, g: usize, open: bool) {
    let (title, count, share, size, items) = GROUPS[g];
    let chevron = if open { glyph::DOWN } else { glyph::RIGHT };
    p.icon(chevron, 14.0, row.x + 12.0, row.cy(), pal.muted);
    let ticked = items.iter().filter(|i| i.ticked).count();
    let selectable = items
        .iter()
        .filter(|i| i.rating != Rating::Protected)
        .count();
    let x = row.x + 26.0;
    if selectable == 0 {
        p.icon(LOCK, 14.0, x + 8.0, row.cy(), pal.muted);
    } else if ticked == selectable {
        tick_at(p, pal, x, row.cy(), Tick::new(true));
    } else if ticked == 0 {
        tick_at(p, pal, x, row.cy(), Tick::new(false));
    } else {
        tick_mixed(p, pal, x, row.cy());
    }
    let w = p.text(
        Face::Semibold,
        14.0,
        x + 26.0,
        row.cy(),
        title,
        pal.text,
        300.0,
    );
    p.text(
        Face::Ui,
        12.0,
        x + 34.0 + w,
        row.cy(),
        count,
        pal.muted,
        40.0,
    );
    if share > 0.0 {
        let bar = Rect::new(row.right() - 206.0, row.cy() - 3.0, 120.0, 6.0);
        draw_progress(p, pal, bar, share);
        p.text_right(
            Face::Semibold,
            12.5,
            row.right() - 14.0,
            row.cy(),
            size,
            pal.text,
        );
    }
}

fn leftover_row(p: &mut Painter, pal: &Palette, row: Rect, item: &Leftover, selected: bool) {
    let body = Rect::new(row.x, row.y + 1.0, row.w - 8.0, row.h - 2.0);
    draw_row_fill(p, pal, body, false, false, selected);
    let x = row.x + 34.0;
    if item.rating == Rating::Protected {
        p.icon(LOCK, 14.0, x + 8.0, row.cy(), pal.muted);
    } else {
        tick_at(p, pal, x, row.cy(), Tick::new(item.ticked));
    }
    let tx = x + 28.0;
    let max = row.right() - 190.0 - tx;
    let path = p.elide(Face::Mono, 12.0, item.path, max);
    p.text(Face::Mono, 12.0, tx, row.cy() - 8.0, &path, pal.text, max);
    p.text(
        Face::Ui,
        12.0,
        tx,
        row.cy() + 10.0,
        item.why,
        pal.muted,
        max,
    );
    let (label, tone) = match item.rating {
        Rating::Safe => ("Safe", Tone::Good),
        Rating::Likely => ("Likely", Tone::Caution),
        Rating::Protected => ("Protected", Tone::Outline),
    };
    chip(p, pal, row.right() - 170.0, row.cy(), label, tone);
    p.text_right(
        Face::Ui,
        12.0,
        row.right() - 14.0,
        row.cy(),
        item.size,
        pal.muted,
    );
}

fn leftover_selected(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let x = r.x + 18.0;
    let w = r.w - 36.0;
    let mut y = r.y + 16.0;
    p.text(Face::Ui, 12.0, x, y + 6.0, "Selected item", pal.muted, w);
    y += 22.0;
    p.text(Face::Semibold, 16.0, x, y + 10.0, "Mozilla", pal.text, w);
    y += 26.0;
    y = mono(
        p,
        12.0,
        x,
        y,
        w,
        "C:\\Users\\tester\\AppData\\Roaming\\Mozilla",
        pal.text,
    ) + 10.0;
    let mut cx = x;
    cx += chip(p, pal, cx, y + 10.0, "Likely", Tone::Caution) + 6.0;
    cx += chip(p, pal, cx, y + 10.0, "User data", Tone::Neutral) + 6.0;
    chip(p, pal, cx, y + 10.0, "Not ticked", Tone::Outline);
    y += 34.0;
    p.text(Face::Semibold, 13.0, x, y + 8.0, "Evidence", pal.text, w);
    y += 24.0;
    for (m, text) in [
        (0, "The folder is named for the vendor, Mozilla"),
        (0, "Firefox wrote here before it was removed"),
        (1, "It holds profiles you may want again"),
        (2, "Another Mozilla program could use it"),
    ] {
        match m {
            0 => p.rbox(Rect::new(x + 3.0, y + 5.0, 7.0, 7.0), 3.5, ok(pal)),
            1 => p.icon(glyph::WARN, 13.0, x + 7.0, y + 8.0, pal.warn),
            _ => p.ring(x + 6.5, y + 8.5, 3.5, 1.3, pal.muted, 1.0),
        }
        y = para(p, Face::Ui, 12.5, x + 20.0, y, w - 20.0, text, pal.text) + 6.0;
    }
    y += 8.0;
    divider(p, pal, x, y, w);
    y += 14.0;
    for (label, value) in [
        ("Contents", "38 files in 6 folders · 2.1 MB"),
        (
            "If removed",
            "Moved to quarantine, restorable from History & undo",
        ),
        ("Kind", "Folder"),
    ] {
        p.text(Face::Ui, 12.0, x, y + 8.0, label, pal.muted, 76.0);
        y = para(p, Face::Ui, 12.5, x + 80.0, y, w - 80.0, value, pal.text) + 8.0;
    }
}

fn leftovers_waiting(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::Leftovers);
    let y = leftovers_header(p, pal, content, "Post-uninstall scan · 10 items");
    let w = 720.0;
    let r = Rect::new(content.cx() - w / 2.0, y + 60.0, w, 440.0);
    card(p, pal, r);
    let centre = |p: &mut Painter, face: Face, size: f32, cy: f32, text: &str, c: Rgb| {
        p.text_center(
            face,
            size,
            Rect::new(r.x, cy - size, r.w, size * 2.0),
            text,
            c,
        );
    };
    draw_spinner_at(p, pal, r.cx(), r.y + 58.0, 21.0, None);
    centre(
        p,
        Face::Semibold,
        22.0,
        r.y + 112.0,
        "Confirm in the Purge security prompt.",
        pal.text,
    );
    centre(
        p,
        Face::Ui,
        13.0,
        r.y + 142.0,
        "Nothing has been moved yet.",
        pal.muted,
    );
    let code = Rect::new(r.cx() - 160.0, r.y + 170.0, 320.0, 92.0);
    p.frame(code, 8.0, pal.page, pal.popup_line);
    centre(
        p,
        Face::Semibold,
        11.0,
        code.y + 22.0,
        "CODE IN THE SECURITY PROMPT",
        pal.muted,
    );
    centre(p, Face::Mono, 28.0, code.y + 58.0, "4821", pal.text);
    centre(
        p,
        Face::Semibold,
        13.0,
        r.y + 296.0,
        "Nothing has been moved yet.",
        pal.text,
    );
    let x = r.x + 40.0;
    divider(p, pal, x, r.y + 322.0, r.w - 80.0);
    expander::draw_header(
        p,
        pal,
        Rect::new(x, r.y + 334.0, r.w - 80.0, expander::HEADER_H),
        "Items that need attention (2)",
        true,
        expander::Look::default(),
    );
    let mut y = r.y + 378.0;
    for line in [
        "C:\\Users\\tester\\AppData\\Roaming\\Mozilla",
        "Service: Mozilla Maintenance Service",
    ] {
        p.text(Face::Mono, 12.0, x + 24.0, y, line, pal.text, r.w - 120.0);
        y += 22.0;
    }
}

fn leftovers_result(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::Leftovers);
    let y = leftovers_header(p, pal, content, "Post-uninstall scan · 10 items");
    let w = 820.0;
    let r = Rect::new(content.cx() - w / 2.0, y + 40.0, w, 420.0);
    card(p, pal, r);
    let x = r.x + 28.0;
    let inner = r.w - 56.0;
    p.icon(glyph::WARN, 28.0, x + 14.0, r.y + 42.0, pal.warn);
    p.text(
        Face::Semibold,
        24.0,
        x + 40.0,
        r.y + 42.0,
        "The move finished",
        pal.text,
        inner - 40.0,
    );
    p.text(
        Face::Ui,
        13.0,
        x,
        r.y + 80.0,
        "4 items moved to quarantine (2.7 MB). 1 is held and 1 was not moved.",
        pal.text,
        inner,
    );
    p.text(
        Face::Ui,
        12.0,
        x,
        r.y + 102.0,
        "Files can be put back until 25 Dec 2026; registry and task backups until 24 May 2027.",
        pal.muted,
        inner,
    );
    let mut y = r.y + 132.0;
    for (title, path, why) in [
        (
            "Held — in use by another process",
            "C:\\Users\\tester\\AppData\\Local\\Mozilla\\updates",
            "updater.exe has files open here",
        ),
        (
            "Not moved",
            "Service: Mozilla Maintenance Service",
            "It changed after the scan, so Purge left it",
        ),
    ] {
        p.text(Face::Semibold, 13.0, x, y + 8.0, title, pal.text, inner);
        let b = Rect::new(x, y + 24.0, inner, 50.0);
        p.frame(b, 6.0, pal.page, pal.line);
        p.text(
            Face::Mono,
            12.0,
            x + 12.0,
            b.y + 17.0,
            path,
            pal.text,
            inner - 24.0,
        );
        p.text(
            Face::Ui,
            12.0,
            x + 12.0,
            b.y + 35.0,
            why,
            pal.muted,
            inner - 24.0,
        );
        y = b.bottom() + 20.0;
    }
    buttons_right(
        p,
        pal,
        r.right() - 28.0,
        r.bottom() - 28.0 - BUTTON_H,
        &[
            ("Put back all", Style::Standard, Some(glyph::UNDO)),
            ("Try the held items again", Style::Standard, None),
            ("Done", Style::Accent, None),
        ],
    );
}

// ----------------------------------------------------------------------------------------------
// History.

fn history(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::History);
    let left = content.x + 28.0;
    let right = content.right() - 28.0;
    let y = content.y + 38.0;
    heading(p, pal, left, y, "History & undo", "");
    p.text(
        Face::Ui,
        12.5,
        left,
        y + 30.0,
        "Files can be put back for 30 days, registry and task backups for 180 days",
        pal.muted,
        600.0,
    );
    buttons_right(
        p,
        pal,
        right,
        y - 16.0,
        &[
            (
                "Check for interrupted cleanups",
                Style::Subtle,
                Some(glyph::RELOAD),
            ),
            ("Open quarantine", Style::Subtle, Some(BOX)),
        ],
    );

    // The timeline: days and cleanups on one line, the oldest at the left.
    let strip = Rect::new(left, content.y + 88.0, right - left, 78.0);
    card(p, pal, strip);
    let line_y = strip.y + 28.0;
    p.fill(
        Rect::new(strip.x + 16.0, line_y, strip.w - 32.0, 1.0),
        pal.popup_line,
    );
    let mut x = strip.x + 16.0;
    for (day, name, note) in [
        ("18 Nov", "", ""),
        ("", "OBS Studio 09:14", "Nothing to restore"),
        ("", "VLC media player 10:15", "Interrupted"),
        ("", "Audacity 17:02", "Files 23 d · registry 173 d"),
        ("Yesterday", "", ""),
        ("", "Zoom Workplace 11:40", "Files 29 d · registry 179 d"),
        ("Today", "", ""),
        ("", "Mozilla Firefox 14:32", "Files 30 d · registry 180 d"),
    ] {
        if name.is_empty() {
            p.fill(Rect::new(x + 46.0, line_y - 8.0, 1.0, 17.0), pal.muted);
            p.text_center(
                Face::Semibold,
                11.0,
                Rect::new(x, strip.y + 44.0, 92.0, 16.0),
                day,
                pal.muted,
            );
            x += 92.0;
        } else {
            p.rbox(
                Rect::new(x + 69.0, line_y - 4.5, 10.0, 10.0),
                5.0,
                pal.accent,
            );
            p.text_center(
                Face::Ui,
                12.0,
                Rect::new(x, strip.y + 39.0, 148.0, 16.0),
                name,
                pal.text,
            );
            p.text_center(
                Face::Ui,
                11.0,
                Rect::new(x, strip.y + 56.0, 148.0, 16.0),
                note,
                pal.muted,
            );
            x += 148.0;
        }
    }

    let top = strip.bottom() + 16.0;
    let bottom = content.bottom() - 24.0;
    let detail = Rect::new(right - 460.0, top, 460.0, bottom - top);
    history_list(
        p,
        pal,
        Rect::new(left, top, detail.x - 16.0 - left, bottom - top),
    );
    history_detail(p, pal, detail);
}

type Cleanup = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [(&'static str, u8)],
);

const CLEANUPS: [&[Cleanup]; 3] = [
    &[(
        "Mozilla Firefox",
        "Mozilla",
        "14:32",
        "23 items moved · 14.2 MB · 1 held, in use",
        &[("Undoable", 0), ("Check items", 2)],
    )],
    &[(
        "Zoom Workplace",
        "Zoom Communications",
        "11:40",
        "11 items moved · 220 MB",
        &[("Undoable", 0)],
    )],
    &[
        (
            "Audacity",
            "Audacity Team",
            "17:02",
            "8 items moved · 12.4 MB",
            &[("Partly put back", 1)],
        ),
        (
            "VLC media player",
            "VideoLAN",
            "10:15",
            "The cleanup stopped part way",
            &[("Interrupted", 1)],
        ),
        (
            "OBS Studio",
            "OBS Project",
            "09:14",
            "14 items · 30.1 MB",
            &[("Put back", 3)],
        ),
    ],
];

fn history_list(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let view = r.inset(6.0);
    let mut list = Sections::new(view.h, 56.0);
    for day in CLEANUPS {
        list.add_group(day.len(), Some(32.0));
    }
    for (g, day) in CLEANUPS.iter().enumerate() {
        for n in 0..day.len() {
            if let Some(i) = list.index(Entry::Row { group: g, row: n }) {
                list.set_height(i, 56.0);
            }
        }
    }
    let days = ["Today", "Yesterday", "18 November"];
    for i in list.visible() {
        let row = list.rect(view, i);
        match list.entry(i) {
            Some(Entry::Heading(g)) => {
                p.text(
                    Face::Semibold,
                    12.0,
                    row.x + 10.0,
                    row.cy() + 2.0,
                    days[g],
                    pal.muted,
                    200.0,
                );
            }
            Some(Entry::Row { group, row: n }) => {
                let (name, publisher, time, summary, chips) = CLEANUPS[group][n];
                let body = Rect::new(row.x, row.y + 1.0, row.w, row.h - 2.0);
                draw_row_fill(p, pal, body, false, false, group == 0);
                tile(
                    p,
                    Rect::new(row.x + 12.0, row.cy() - 16.0, 32.0, 32.0),
                    name,
                    publisher,
                );
                let x = row.x + 56.0;
                let w = p.text(
                    Face::Semibold,
                    14.0,
                    x,
                    row.cy() - 9.0,
                    name,
                    pal.text,
                    300.0,
                );
                p.text(
                    Face::Ui,
                    12.0,
                    x + w + 10.0,
                    row.cy() - 9.0,
                    time,
                    pal.muted,
                    60.0,
                );
                p.text(
                    Face::Ui,
                    12.0,
                    x,
                    row.cy() + 11.0,
                    summary,
                    pal.muted,
                    300.0,
                );
                let mut cx = row.right() - 10.0;
                for &(label, tone) in chips.iter().rev() {
                    let tone = match tone {
                        0 => Tone::Good,
                        1 | 2 => Tone::Caution,
                        _ => Tone::Neutral,
                    };
                    cx -= chip_w(p, label);
                    chip(p, pal, cx, row.cy(), label, tone);
                    cx -= 6.0;
                }
            }
            None => {}
        }
    }
}

fn history_detail(p: &mut Painter, pal: &Palette, r: Rect) {
    card(p, pal, r);
    let x = r.x + 18.0;
    let w = r.w - 36.0;
    let mut y = r.y + 18.0;
    p.text(
        Face::Semibold,
        16.0,
        x,
        y + 8.0,
        "Mozilla Firefox · 25 Nov 2026 14:32",
        pal.text,
        w,
    );
    y += 28.0;
    p.text(
        Face::Ui,
        12.5,
        x,
        y + 7.0,
        "23 items moved · 14.2 MB · 1 held, in use",
        pal.muted,
        w,
    );
    y += 24.0;
    expander::draw_header(
        p,
        pal,
        Rect::new(x - 6.0, y, w + 12.0, expander::HEADER_H),
        "Show items",
        true,
        expander::Look::default(),
    );
    y += expander::HEADER_H + 4.0;
    let items = [
        ("AppData\\Local\\Mozilla", "Folder", "In quarantine", true),
        (
            "ProgramData\\Mozilla-1de4eec8",
            "Folder",
            "In quarantine",
            true,
        ),
        (
            "AppData\\Roaming\\Mozilla",
            "Folder · user data",
            "In quarantine",
            false,
        ),
        (
            "HKCU\\Software\\Mozilla\\Firefox",
            "Registry key",
            "Backed up",
            false,
        ),
        (
            "HKLM\\Software\\Classes\\FirefoxHTML-3080",
            "Registry key",
            "Backed up",
            false,
        ),
        (
            "Task: Firefox Default Browser Agent",
            "Scheduled task",
            "Backed up",
            false,
        ),
        (
            "AppData\\Local\\Mozilla\\updates",
            "Folder",
            "Held, in use",
            false,
        ),
    ];
    let b = Rect::new(x, y, w, items.len() as f32 * 40.0 + 8.0);
    p.frame(b, 6.0, pal.page, pal.line);
    let mut iy = b.y + 4.0;
    for (name, kind, outcome, ticked) in items {
        let cy = iy + 20.0;
        let held = outcome.starts_with("Held");
        let tick = if held {
            Tick::disabled(false)
        } else {
            Tick::new(ticked)
        };
        tick_at(p, pal, x + 12.0, cy, tick);
        p.text(
            Face::Mono,
            12.0,
            x + 40.0,
            cy - 8.0,
            name,
            pal.text,
            w - 140.0,
        );
        p.text(
            Face::Ui,
            11.0,
            x + 40.0,
            cy + 9.0,
            kind,
            pal.muted,
            w - 140.0,
        );
        let c = if held { pal.warn } else { pal.muted };
        p.text_right(Face::Ui, 12.0, x + w - 12.0, cy, outcome, c);
        iy += 40.0;
    }
    y = b.bottom() + 12.0;
    let bar = InfoBar::new(
        Severity::Info,
        "",
        "Undo puts the leftovers back exactly as they were. It doesn't reinstall the program: run its installer for that.",
    ).closable(false);
    y = info_bar(p, pal, x, y, w, &bar) + 12.0;
    let first = button_w(p, "Put back 2 selected", Some(glyph::UNDO));
    button(
        p,
        pal,
        Rect::new(x, y, first, BUTTON_H),
        "Put back 2 selected",
        Style::Accent,
        Some(glyph::UNDO),
    );
    let second = button_w(p, "Put back all", None);
    button(
        p,
        pal,
        Rect::new(x + first + 8.0, y, second, BUTTON_H),
        "Put back all",
        Style::Standard,
        None,
    );
    y += BUTTON_H + 10.0;
    para(
        p,
        Face::Ui,
        12.0,
        x,
        y,
        w,
        "Files restorable until 25 Dec 2026 (30 days), registry and task backups until 24 May 2027",
        pal.muted,
    );
}

// ----------------------------------------------------------------------------------------------
// Quarantine.

struct Batch {
    name: &'static str,
    publisher: &'static str,
    when: &'static str,
    holds: &'static str,
    until: &'static str,
    state: &'static str,
    tone: Tone,
    days: u32,
}

const BATCHES: [Batch; 4] = [
    Batch {
        name: "Mozilla Firefox",
        publisher: "Mozilla",
        when: "Today 14:32",
        holds: "23 items · 14.2 MB on C:",
        until: "Files restorable for 30 more days · registry backups 180 days",
        state: "Restorable",
        tone: Tone::Good,
        days: 30,
    },
    Batch {
        name: "Zoom Workplace",
        publisher: "Zoom Communications",
        when: "Yesterday 11:40",
        holds: "11 items · 220 MB on C:",
        until: "Files restorable for 29 more days · registry backups 179 days",
        state: "Restorable",
        tone: Tone::Good,
        days: 29,
    },
    Batch {
        name: "Paint.NET",
        publisher: "dotPDN",
        when: "31 Oct 2026",
        holds: "5 items · 8.1 MB on C:",
        until: "Files restorable for 5 more days · registry backups 155 days",
        state: "Expiring",
        tone: Tone::Caution,
        days: 5,
    },
    Batch {
        name: "HandBrake",
        publisher: "HandBrake Team",
        when: "2 Oct 2026",
        holds: "3 registry backups",
        until: "Files past retention · registry backups 126 days",
        state: "Registry undo only",
        tone: Tone::Neutral,
        days: 0,
    },
];

fn quarantine(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::Quarantine);
    let left = content.x + 28.0;
    let right = content.right() - 28.0;
    let width = right - left;
    let y = content.y + 38.0;
    heading(p, pal, left, y, "Quarantine", "");
    p.text(
        Face::Ui,
        12.5,
        left,
        y + 30.0,
        "Where removed leftovers wait. Nothing here is gone until you delete it.",
        pal.muted,
        600.0,
    );
    buttons_right(
        p,
        pal,
        right,
        y - 16.0,
        &[(
            "Review and delete expired…",
            Style::Standard,
            Some(glyph::DELETE),
        )],
    );

    let mut y = content.y + 90.0;
    for (i, (volume, used, share, note)) in [(
        "C:",
        "242 MB of 10 GB limit",
        0.024,
        "Expired or expiring soon: 8.1 MB",
    )]
    .into_iter()
    .enumerate()
    {
        let r = Rect::new(left + i as f32 * 334.0, y, 320.0, 94.0);
        card(p, pal, r);
        let w = p.text(
            Face::Semibold,
            20.0,
            r.x + 16.0,
            r.y + 24.0,
            volume,
            pal.text,
            60.0,
        );
        p.text(
            Face::Ui,
            12.0,
            r.x + 26.0 + w,
            r.y + 26.0,
            used,
            pal.muted,
            220.0,
        );
        draw_progress(
            p,
            pal,
            Rect::new(r.x + 16.0, r.y + 48.0, r.w - 32.0, 6.0),
            share,
        );
        let c = if i == 0 { pal.warn } else { pal.muted };
        p.text(Face::Ui, 12.0, r.x + 16.0, r.y + 74.0, note, c, r.w - 32.0);
    }
    y += 94.0 + 14.0;

    let bar = InfoBar::new(
        Severity::Warning,
        "One cleanup is past its retention.",
        "Purge offers to delete expired items and asks first in its security prompt; nothing is deleted on its own. User data is asked for one item at a time.",
    ).closable(false)
    .action(0, "Review and delete…");
    y = info_bar(p, pal, left, y, width, &bar) + 14.0;

    let list = Rect::new(left, y, width, 48.0 + BATCHES.len() as f32 * 64.0 + 6.0);
    card(p, pal, list);
    p.text(
        Face::Semibold,
        14.0,
        left + 16.0,
        y + 24.0,
        "By cleanup",
        pal.text,
        200.0,
    );
    p.text_right(
        Face::Ui,
        12.0,
        right - 16.0,
        y + 24.0,
        "4 cleanups · 242 MB",
        pal.muted,
    );
    let mut delete_now = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut ry = y + 48.0;
    for (i, b) in BATCHES.iter().enumerate() {
        divider(p, pal, left + 1.0, ry, width - 2.0);
        let cy = ry + 32.0;
        tile(
            p,
            Rect::new(left + 16.0, cy - 16.0, 32.0, 32.0),
            b.name,
            b.publisher,
        );
        let x = left + 60.0;
        let w = p.text(Face::Semibold, 13.5, x, cy - 18.0, b.name, pal.text, 220.0);
        p.text(
            Face::Ui,
            12.0,
            x + w + 10.0,
            cy - 18.0,
            b.when,
            pal.muted,
            140.0,
        );
        p.text(Face::Ui, 12.0, x, cy + 1.0, b.holds, pal.text, 360.0);
        p.text(Face::Ui, 12.0, x, cy + 19.0, b.until, pal.muted, 360.0);

        // The actions, from the right.
        let mut bx = right - 12.0;
        for (label, style) in [
            ("Details", Style::Subtle),
            ("Choose items…", Style::Subtle),
            ("Delete now…", Style::Standard),
            ("Put back…", Style::Standard),
        ] {
            let bw = button_w(p, label, None);
            bx -= bw;
            let br = Rect::new(bx, cy - 16.0, bw, BUTTON_H);
            button(p, pal, br, label, style, None);
            if i == 2 && label == "Delete now…" {
                delete_now = br;
            }
            bx -= 6.0;
        }
        // Days left, as a ring.
        let ring_x = bx - 34.0;
        p.ring(ring_x, cy, 21.0, 3.0, pal.line, 1.0);
        let c = if b.days <= 7 { pal.warn } else { pal.accent };
        p.arc(ring_x, cy, 21.0, 3.0, 0.0, b.days as f32 / 30.0, c, 1.0);
        let around = |dy: f32| Rect::new(ring_x - 20.0, cy + dy - 7.0, 40.0, 14.0);
        p.text_center(
            Face::Semibold,
            12.5,
            around(-4.0),
            &b.days.to_string(),
            pal.text,
        );
        p.text_center(Face::Ui, 9.0, around(8.0), "days", pal.muted);
        let cw = chip_w(p, b.state);
        chip(p, pal, ring_x - 34.0 - cw, cy, b.state, b.tone);
        ry += 64.0;
    }

    // "Delete now…" asks in a flyout under its button.
    let size = (340.0, 142.0);
    let mut fly = Flyout::new(delete_now, size).side(Side::Below);
    fly.place_in(Rect::new(0.0, 0.0, W, H));
    flyout::draw_card(p, pal, fly.rect());
    let c = fly.content_rect();
    let mut y = para(
        p,
        Face::Ui,
        13.0,
        c.x,
        c.y,
        c.w,
        "Deletes this cleanup's quarantined files for good, before their retention ends. Purge names every file in its security prompt first; nothing is deleted without it.",
        pal.text,
    ) + 10.0;
    tick_at(p, pal, c.x, y + 8.0, Tick::new(false));
    y = para(
        p,
        Face::Ui,
        12.5,
        c.x + 24.0,
        y,
        c.w - 24.0,
        "Also delete its registry backups (its registry changes can then no longer be undone)",
        pal.text,
    ) + 12.0;
    let label = "Continue to the security prompt…";
    let w = button_w(p, label, None);
    button(
        p,
        pal,
        Rect::new(c.x, y, w, BUTTON_H),
        label,
        Style::Standard,
        None,
    );
}

// ----------------------------------------------------------------------------------------------
// Settings.

fn settings(p: &mut Painter, pal: &Palette) {
    let content = frame(p, pal, Page::Settings);
    let left = content.x + 28.0;
    let width = 860.0 - 56.0;
    heading(p, pal, left, content.y + 38.0, "Settings", "");
    let mut y = content.y + 72.0;

    let facts = [
        "After an uninstall, Purge ticks only leftovers it rates Safe. Likely items and weak matches wait for your own tick, and Protected items can never be ticked.",
        "When the uninstaller failed or Purge could not check that the program is gone, the scan is a forced one and nothing is ticked.",
        "You see the whole list and confirm it in Purge's security prompt before anything moves. Everything moved can be put back from History & undo.",
    ];
    let inner = width - 32.0;
    let h: f32 = facts
        .iter()
        .map(|f| p.paragraph_height(Face::Ui, 13.0, inner - 18.0, f) + 8.0)
        .sum();
    let r = Rect::new(left, y, width, 46.0 + h + 6.0);
    card(p, pal, r);
    p.text(
        Face::Semibold,
        14.0,
        left + 16.0,
        y + 24.0,
        "What Purge ticks for you",
        pal.text,
        inner,
    );
    let mut fy = y + 46.0;
    for f in facts {
        p.rbox(Rect::new(left + 19.0, fy + 6.0, 5.0, 5.0), 2.5, pal.muted);
        fy = para(
            p,
            Face::Ui,
            13.0,
            left + 34.0,
            fy,
            inner - 18.0,
            f,
            pal.text,
        ) + 8.0;
    }
    y = r.bottom() + 4.0;

    let r = Rect::new(left, y, width, 84.0);
    card(p, pal, r);
    p.text(
        Face::Semibold,
        14.0,
        left + 16.0,
        y + 22.0,
        "Quarantine",
        pal.text,
        300.0,
    );
    let bw = button_w(p, "Open quarantine", None);
    button(
        p,
        pal,
        Rect::new(r.right() - 16.0 - bw, r.cy() - 16.0, bw, BUTTON_H),
        "Open quarantine",
        Style::Standard,
        None,
    );
    para(
        p,
        Face::Ui,
        12.0,
        left + 16.0,
        y + 38.0,
        width - bw - 60.0,
        "Files are kept 30 days; registry, service and task backups 180 days. When something expires, Purge offers to delete it and asks first; nothing is deleted on its own.",
        pal.muted,
    );
    y = r.bottom() + 4.0;

    let r = Rect::new(left, y, width, 68.0);
    card(p, pal, r);
    p.text(
        Face::Semibold,
        14.0,
        left + 16.0,
        y + 24.0,
        "Theme",
        pal.text,
        300.0,
    );
    p.text(
        Face::Ui,
        12.0,
        left + 16.0,
        y + 46.0,
        "Light, dark, or follow Windows",
        pal.muted,
        300.0,
    );
    drop_field(
        p,
        pal,
        Rect::new(
            r.right() - 216.0,
            r.cy() - DROPDOWN_H / 2.0,
            200.0,
            DROPDOWN_H,
        ),
        &["Use Windows setting", "Light", "Dark"],
        0,
    );
    y = r.bottom() + 4.0;

    let r = Rect::new(left, y, width, 68.0);
    card(p, pal, r);
    p.text(
        Face::Semibold,
        14.0,
        left + 16.0,
        y + 24.0,
        "Purge 0.1.2 · engine 0.1.2 · sample data",
        pal.text,
        inner,
    );
    p.text(
        Face::Ui,
        12.0,
        left + 16.0,
        y + 46.0,
        "Leftover scanners include code ported from Bulk Crap Uninstaller (Apache-2.0).",
        pal.muted,
        inner,
    );
}

// ----------------------------------------------------------------------------------------------

type Screen = (&'static str, fn(&mut Painter, &Palette));

const SCREENS: [Screen; 12] = [
    ("programs", |p, pal| programs(p, pal, Over::Nothing)),
    ("programs-filter", |p, pal| {
        programs(p, pal, Over::FilterMenu)
    }),
    ("confirm", confirm),
    ("elevation", elevation),
    ("pipeline", |p, pal| pipeline(p, pal, false)),
    ("pipeline-next", |p, pal| pipeline(p, pal, true)),
    ("leftovers", leftovers),
    ("leftovers-waiting", leftovers_waiting),
    ("leftovers-result", leftovers_result),
    ("history", history),
    ("quarantine", quarantine),
    ("settings", settings),
];

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
    let mut gfx = Gfx::new();
    for (name, draw) in SCREENS {
        for (theme, pal) in [("light", &LIGHT), ("dark", &DARK)] {
            for (suffix, scale) in [("", 1.0_f32), ("-150", 1.5)] {
                let mut bmp = Bitmap::new((W * scale) as u32, (H * scale) as u32);
                let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                draw(&mut p, pal);
                let path = dir.join(format!("purge-{name}-{theme}{suffix}.png"));
                bmp.write_png(&path)?;
                println!("{}", path.display());
            }
        }
    }
    Ok(())
}
