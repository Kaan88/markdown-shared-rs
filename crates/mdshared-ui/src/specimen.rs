//! Specimens a person looks at: the palette and the settings form drawn into PNGs, light and dark,
//! at scale 1.0 and 1.5. `cargo test -p mdshared-ui specimen -- --ignored`, then open
//! `target/specimen/*.png` in the workspace.

use crate::gfx::{Face, Gfx, Painter, Rect};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::palette::{Entry, Palette};
use crate::theme::{DARK, LIGHT, Palette as Colors};
use mdshared_text::Bitmap;

const W: f32 = 820.0;
const H: f32 = 560.0;
const TITLE_H: f32 = 40.0;

fn out(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/specimen")
        .join(name)
}

/// A window with a title row and a few lines of page, so the card has something to sit on.
fn window(p: &mut Painter, pal: &Colors) {
    p.fill(Rect::new(0.0, 0.0, W, H), pal.page);
    p.fill(Rect::new(0.0, 0.0, W, TITLE_H), pal.title);
    p.rbox(Rect::new(8.0, 6.0, 150.0, 34.0), 6.0, pal.bar);
    p.text(Face::Ui, 12.5, 20.0, 23.0, "notes.md", pal.text, 120.0);
    p.fill(Rect::new(0.0, TITLE_H, W, 38.0), pal.bar);
    let mut y = 110.0;
    for (i, line) in [
        "# Release notes",
        "",
        "The palette finds a command, a file or a line by a few of its letters.",
        "Type > to run a command, : to go to a line, @ for a symbol.",
        "",
        "## Checks before a release",
        "- cargo test --workspace",
        "- cargo clippy --workspace --all-targets",
        "- run it on Windows",
    ]
    .iter()
    .enumerate()
    {
        let face = if i == 0 || i == 5 {
            Face::Semibold
        } else {
            Face::Ui
        };
        p.text(face, 14.0, 60.0, y, line, pal.text, W - 120.0);
        y += 26.0;
    }
}

fn commands() -> Vec<Entry> {
    let c =
        |id, label: &str, keys: &str, icon| Entry::new(id, label).hint(keys).icon(icon).group(0);
    vec![
        c(1, "Open file…", "Ctrl+O", glyph::OPEN),
        c(2, "Save", "Ctrl+S", glyph::SAVE),
        c(3, "Save as…", "Ctrl+Shift+S", glyph::SAVE_AS),
        c(4, "Find", "Ctrl+F", glyph::SEARCH),
        c(5, "Replace", "Ctrl+H", glyph::REPLACE),
        c(6, "Go to line…", "Ctrl+G", glyph::GO_TO_LINE),
        c(7, "Toggle word wrap", "Alt+Z", glyph::WRAP),
        c(8, "Toggle minimap", "", glyph::MINIMAP),
        c(9, "Fold all", "Ctrl+K Ctrl+0", glyph::FOLD_ALL),
        c(10, "Close tab", "Ctrl+W", glyph::CLOSE).enabled(false),
        c(11, "Keyboard shortcuts", "F1", glyph::KEYBOARD),
        Entry::new(20, "release-notes.md")
            .hint("docs/release-notes.md")
            .icon(glyph::DOC)
            .group(1),
        Entry::new(21, "PLAN.md")
            .hint("docs/PLAN.md")
            .icon(glyph::DOC)
            .group(1),
        Entry::new(22, "main.rs")
            .hint("bins/mdedit/src/main.rs")
            .icon(glyph::DOC)
            .group(2),
        Entry::new(23, "settings.rs")
            .hint("crates/mdedit-core/src/settings.rs")
            .icon(glyph::DOC)
            .group(2),
    ]
}

fn many() -> Vec<Entry> {
    let words = [
        "Open", "File", "Save", "Toggle", "Find", "Replace", "Line", "Word", "Fold", "Tab",
        "Close", "Next", "Select", "Copy", "Paste", "Move",
    ];
    (0..60u32)
        .map(|i| {
            let a = words[i as usize % words.len()];
            let b = words[(i as usize * 5 + 3) % words.len()];
            Entry::new(100 + i, format!("{a} {b} {}", i % 7))
                .hint(if i % 3 == 0 { "Ctrl+Alt+K" } else { "" })
                .icon(glyph::DOC)
        })
        .collect()
}

fn palette(entries: Vec<Entry>, query: &str) -> Palette {
    let mut p = Palette::new();
    p.set_groups(vec![
        (0, "Commands".into()),
        (1, "Recent files".into()),
        (2, "Files in folder".into()),
    ]);
    p.set_placeholder("Type a command, or a file name");
    p.set_entries(entries);
    if !query.is_empty() {
        p.set_query(query);
    }
    p
}

fn render(name: &str, scale: f32, pal: &Colors, mut f: impl FnMut(&mut Painter)) {
    let mut bmp = Bitmap::new((W * scale) as u32, (H * scale) as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    window(&mut p, pal);
    f(&mut p);
    bmp.write_png(&out(name)).unwrap();
}

#[test]
#[ignore]
fn palette_sheets() {
    let win = Rect::new(0.0, 0.0, W, H);
    for (scale, sfx) in [(1.0, ""), (1.5, "-150")] {
        for (pal, theme) in [(&LIGHT, "light"), (&DARK, "dark")] {
            render(
                &format!("palette-empty-{theme}{sfx}.png"),
                scale,
                pal,
                |p| {
                    let mut pl = palette(commands(), "");
                    // The pointer rests on one row, as a person's would.
                    pl.layout(win, TITLE_H);
                    let r = pl.row_rect(2);
                    pl.pointer_move(r.x + 60.0, r.cy());
                    pl.draw(p, pal, win, TITLE_H, true);
                },
            );
            render(
                &format!("palette-query-{theme}{sfx}.png"),
                scale,
                pal,
                |p| {
                    let mut pl = palette(commands(), "");
                    for ch in "fi".chars() {
                        let s = ch.to_string();
                        pl.key(&KeyName::Char(ch), Mods::default(), Some(&s));
                    }
                    pl.draw(p, pal, win, TITLE_H, true);
                },
            );
            render(
                &format!("palette-scrolled-{theme}{sfx}.png"),
                scale,
                pal,
                |p| {
                    let mut pl = palette(many(), "");
                    pl.layout(win, TITLE_H);
                    pl.wheel(34.0 * 9.0);
                    for _ in 0..14 {
                        pl.key(&KeyName::Down, Mods::default(), None);
                    }
                    pl.draw(p, pal, win, TITLE_H, false);
                },
            );
        }
    }
}

// ---- the settings form ------------------------------------------------------------------------

use crate::settings::{Form, Row};

fn editor_rows() -> Vec<Row> {
    vec![
        Row::header("Editing"),
        Row::number(1, "Font size", 14, 8, 40)
            .unit("px")
            .help("The size of the text in the editor"),
        Row::number(2, "Tab width", 4, 1, 16).help("Columns a tab stop spans"),
        Row::toggle(3, "Indent with spaces", true)
            .help("Tab inserts spaces instead of a tab character"),
        Row::toggle(4, "Auto-pair brackets", true),
        Row::toggle(5, "Auto-indent", true),
        Row::header("Saving"),
        Row::toggle(6, "Trim trailing spaces on save", false),
        Row::toggle(7, "Final newline on save", true),
        Row::header("View"),
        Row::toggle(8, "Line numbers", true),
        Row::toggle(9, "Highlight current line", true),
        Row::toggle(10, "Minimap", false),
        Row::toggle(11, "Word wrap", false),
        Row::choice(12, "Theme", &["System", "Light", "Dark"], 0)
            .help("Follow the system or choose one"),
        Row::header("Updates"),
        Row::toggle(13, "Check for updates at launch", true),
        Row::text(14, "Update channel", "").placeholder("stable"),
        Row::action(15, "Reset all settings", "Reset to defaults"),
    ]
}

fn viewer_rows() -> Vec<Row> {
    vec![
        Row::header("Reading"),
        Row::choice(1, "Page width", &["Narrow", "Medium", "Wide", "Full"], 1),
        Row::choice(
            2,
            "Page font",
            &[
                "Segoe UI",
                "Georgia",
                "Cambria",
                "Consolas",
                "Inter",
                "Noto Serif",
            ],
            0,
        ),
        Row::number(3, "Text size", 100, 60, 250).step(10).unit("%"),
        Row::header("Content"),
        Row::toggle(4, "Load remote images", false)
            .help("Images in a document may come from the network"),
        Row::toggle(5, "Tick task boxes without asking", true),
        Row::toggle(6, "Play animated images", true),
        Row::header("General"),
        Row::choice(7, "Theme", &["System", "Light", "Dark"], 2),
        Row::toggle(8, "Check for updates at launch", true),
        Row::action(9, "Default app", "Make default app"),
    ]
}

fn form_sheet(
    name: &str,
    scale: f32,
    pal: &Colors,
    win: (f32, f32),
    mut form: Form,
    tweak: impl FnOnce(&mut Form, &mut Painter),
) {
    let mut bmp = Bitmap::new((win.0 * scale) as u32, (win.1 * scale) as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    // The page behind, as in the palette's sheets.
    p.fill(Rect::new(0.0, 0.0, win.0, win.1), pal.page);
    p.fill(Rect::new(0.0, 0.0, win.0, TITLE_H), pal.title);
    p.rbox(Rect::new(8.0, 6.0, 150.0, 34.0), 6.0, pal.bar);
    p.text(Face::Ui, 12.5, 20.0, 23.0, "notes.md", pal.text, 120.0);
    for k in 0..12 {
        p.text(
            Face::Ui,
            14.0,
            60.0,
            100.0 + k as f32 * 26.0,
            "The quick brown fox jumps over the lazy dog.",
            pal.text,
            600.0,
        );
    }
    let window = Rect::new(0.0, 0.0, win.0, win.1);
    form.layout(&mut p, window);
    tweak(&mut form, &mut p);
    form.draw(&mut p, pal, window, true);
    bmp.write_png(&out(name)).unwrap();
}

#[test]
#[ignore]
fn settings_sheets() {
    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    for (scale, sfx) in [(1.0, ""), (1.5, "-150")] {
        for (pal, theme) in [(&LIGHT, "light"), (&DARK, "dark")] {
            // The editor's, wide: a category list, the keyboard focus on one control, the pointer
            // on another.
            form_sheet(
                &format!("settings-editor-{theme}{sfx}.png"),
                scale,
                pal,
                (1000.0, 700.0),
                Form::new("Settings", editor_rows()),
                |f, _| {
                    f.key(&KeyName::Tab, NONE, None);
                    f.key(&KeyName::Tab, NONE, None);
                    let c = f.control_rect(3);
                    f.pointer_move(c.cx(), c.cy());
                },
            );
            // The same, scrolled to a later category in a short window.
            form_sheet(
                &format!("settings-editor-short-{theme}{sfx}.png"),
                scale,
                pal,
                (900.0, 460.0),
                Form::new("Settings", editor_rows()),
                |f, _| {
                    f.jump_to_category(2);
                    f.key(&KeyName::Tab, NONE, None);
                },
            );
            // The last group, with the text box focused and typed in.
            form_sheet(
                &format!("settings-editor-text-{theme}{sfx}.png"),
                scale,
                pal,
                (900.0, 520.0),
                Form::new("Settings", editor_rows()),
                |f, _| {
                    f.jump_to_category(3);
                    f.focus_row(14);
                    for ch in "beta".chars() {
                        let t = ch.to_string();
                        f.key(&KeyName::Char(ch), NONE, Some(&t));
                    }
                },
            );
            // The viewer's, narrow: no category list; a dropdown open.
            form_sheet(
                &format!("settings-viewer-{theme}{sfx}.png"),
                scale,
                pal,
                (640.0, 620.0),
                Form::new("Settings", viewer_rows()),
                |f, _| {
                    let c = f.control_rect(2);
                    f.press(c.cx(), c.cy(), NONE, 1);
                    f.release(c.cx(), c.cy());
                },
            );
            form_sheet(
                &format!("settings-viewer-wide-{theme}{sfx}.png"),
                scale,
                pal,
                (1000.0, 620.0),
                Form::new("Settings", viewer_rows()),
                |f, _| {
                    f.focus_row(3);
                    f.key(&KeyName::Right, NONE, None);
                    f.key(&KeyName::Tab, NONE, None);
                },
            );
        }
    }
}
