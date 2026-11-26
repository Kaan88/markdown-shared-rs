//! Two buttons that bring a menu: a button whose whole face opens one, and a split button whose
//! label runs its action and whose chevron opens the same menu. The menus are
//! [`crate::menu::Menu`]s, placed by [`crate::menu::Menu::around`] (through [`menu_under`]), so
//! a menu below the button, above it near the bottom of the window, and pushed inside at an edge
//! is the placement every other menu already gets.
//!
//! The caller owns the pointer, the keyboard, the focus and the menu itself: it keeps the `Menu`
//! in its own state, draws it after the button with `Menu::draw_in`, and reads `Menu::chosen`
//! back. The keys this module answers are the Windows ones: Space, Enter and Down open a menu
//! button; Space and Enter run a split button's action while Down and Alt+Down open its menu.
//! Nothing here tracks whether a menu is open; while it is, the caller draws the button with
//! [`Look::open`](crate::controls::button::Look::open), which draws the pressed fill whatever
//! the pointer is doing.
//!
//! Deliberately not here: the menu's items, its keyboard (the `Menu` has `step`, `pointer` and
//! `chosen`), Escape to close it (the menu's scope does that), a menu that follows the button
//! when the window scrolls or moves (the caller places it each frame, which is what `Menu::around`
//! is for), key repeat (the caller ignores a repeat for Enter, Space and Down), and motion.

use crate::gfx::{Painter, Rect, mix};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::menu::Menu;
use crate::theme::Palette;

use super::button::{self, Kind};

/// The icon size of the chevron, in logical px.
pub const CHEVRON: f32 = 12.0;

/// The room between a menu button's label and its chevron, in logical px.
pub const GAP: f32 = 6.0;

/// The width of a split button's chevron half, in logical px.
pub const CHEVRON_W: f32 = 28.0;

/// What a key asks a menu or split button to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Press {
    /// The key is not this control's.
    #[default]
    None,
    /// Run the main action (a split button's label; a menu button has none).
    Activate,
    /// Open the menu.
    Open,
}

/// A button whose whole face opens a menu. Its label and kind are all it holds; the menu is the
/// caller's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuButton {
    text: String,
    kind: Kind,
}

impl MenuButton {
    /// A menu button that shows `text`, drawn in `kind`.
    pub fn new(text: impl Into<String>, kind: Kind) -> MenuButton {
        MenuButton {
            text: text.into(),
            kind,
        }
    }

    /// The label the button shows.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The kind the button is drawn in.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The width this button wants: its label, [`CHEVRON`] and [`GAP`] for the chevron, and
    /// [`button::PAD`] on each side, rounded. An empty label is still wide enough for the
    /// chevron.
    pub fn width(&self, p: &mut Painter) -> f32 {
        menu_button_w(p, &self.text, self.kind)
    }

    /// The rectangle this button wants with its top left at `x`, `y`: [`MenuButton::width`] wide
    /// and [`button::HEIGHT`] high.
    pub fn rect(&self, p: &mut Painter, x: f32, y: f32) -> Rect {
        Rect::new(x, y, self.width(p), button::HEIGHT)
    }

    /// Whether a point is on this button, by [`button::hit`].
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        button::hit(r, x, y)
    }

    /// Whether the key opens the menu: Enter and Space (with or without Shift, without Alt,
    /// Ctrl or the Mac's Control) and Down (with or without Alt, the usual Alt+Down) do; every
    /// other key does not. The caller decides whether the keyboard is on the button at all, and
    /// leaves a disabled button out of its keys. A repeat is the caller's to ignore, as for
    /// [`button::key`].
    pub fn key(&self, k: &KeyName, mods: Mods) -> bool {
        matches!(key(k, mods), Press::Open | Press::Activate)
    }
}

impl Default for MenuButton {
    /// An empty secondary menu button.
    fn default() -> MenuButton {
        MenuButton::new("", Kind::Secondary)
    }
}

/// The width a menu button with this label wants, in logical px: [`button::width`] plus the
/// chevron's [`CHEVRON`] and [`GAP`].
pub fn menu_button_w(p: &mut Painter, label: &str, kind: Kind) -> f32 {
    (button::width(p, label, kind) + GAP + CHEVRON).round()
}

/// Where a menu button's chevron is drawn, in logical px: [`CHEVRON`] square, at the right edge
/// with [`button::PAD`] to its right, vertically centred in `r`.
pub fn chevron_rect(r: Rect) -> Rect {
    Rect::new(
        r.right() - button::PAD - CHEVRON,
        r.cy() - CHEVRON / 2.0,
        CHEVRON,
        CHEVRON,
    )
}

/// The room a menu button's label is centred in: [`button::PAD`] from the left edge, and
/// [`GAP`] short of the chevron. With [`menu_button_w`] that is exactly the label's width, so
/// the label sits 12 px from the left and 6 px from the chevron, as Windows' menu buttons do.
/// A narrower rectangle leaves a rectangle that is not positive wide, and drawing it draws no
/// text.
pub fn label_rect(r: Rect) -> Rect {
    Rect::new(
        r.x + button::PAD,
        r.y,
        (r.w - 2.0 * button::PAD - GAP - CHEVRON).max(0.0),
        r.h,
    )
}

/// What a menu button or a split button does with a key: Enter and Space (without Alt, Ctrl or
/// the Mac's Control) [`Press::Activate`]; Down, with or without Alt, [`Press::Open`]; everything
/// else, and anything with one of those modifiers, [`Press::None`]. A menu button takes
/// [`Press::Activate`] as [`Press::Open`] as well, in [`MenuButton::key`]. Repeat is not here:
/// the key event carries no such flag, and the caller ignores a repeat for these keys.
pub fn key(key: &KeyName, mods: Mods) -> Press {
    if mods.ctrl || mods.control {
        return Press::None;
    }
    match key {
        KeyName::Enter | KeyName::Space if !mods.alt => Press::Activate,
        KeyName::Down => Press::Open,
        _ => Press::None,
    }
}

/// `menu` anchored to `button`, as a drop-down's list is anchored to its field: it opens below
/// the button with its left edge at the button's, above it where there is no room below, to the
/// left where there is none on the right, and pushed inside the area at a corner. This is
/// [`crate::menu::Menu::around`] with nothing added: the caller still chooses the items, the
/// minimum width and the area it is placed in.
pub fn menu_under<T: Clone>(menu: Menu<T>, button: Rect) -> Menu<T> {
    menu.around(button)
}

/// Draws a menu button with `label` in `r`: the box `kind` draws (with every state of
/// [`button::Look`], an open menu included), the label centred in the room left of the chevron,
/// and a down chevron in the same ink. Disabled, it is the quiet frame with a faint label and
/// chevron, as [`button::draw`] draws a disabled button. Nothing else differs from
/// [`button::draw`], and nothing is drawn outside `r` except a focused button's ring.
pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, label: &str, kind: Kind, look: button::Look) {
    let (_, ink) = button::draw_box(p, pal, r, kind, look);
    if !sane(r) {
        return;
    }
    let c = chevron_rect(r);
    p.icon(glyph::DOWN, CHEVRON, c.cx(), c.cy(), ink);
    let face = if look.disabled {
        crate::gfx::Face::Semibold
    } else {
        button::face(kind)
    };
    p.text_center(face, button::TEXT, label_rect(r), label, ink);
}

/// A button whose label runs an action and whose chevron opens a menu, as Windows' split
/// buttons do. The two halves share one rounded box and one focus ring; each lights up on its
/// own under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitButton {
    text: String,
    kind: Kind,
}

/// Which half of a split button a point is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Half {
    /// The label: the main action.
    Main,
    /// The chevron: the menu.
    Chevron,
}

/// The two halves of a split button's rectangle. They meet at one x and do not overlap: a point
/// on that line is the chevron's, as [`Rect::contains`] has the right edge.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Halves {
    pub main: Rect,
    pub chevron: Rect,
}

impl SplitButton {
    /// A split button that shows `text`, drawn in `kind`.
    pub fn new(text: impl Into<String>, kind: Kind) -> SplitButton {
        SplitButton {
            text: text.into(),
            kind,
        }
    }

    /// The label the main half shows.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The kind the button is drawn in.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The width this button wants: [`button::width`] for the main half plus [`CHEVRON_W`] for
    /// the chevron half, rounded.
    pub fn width(&self, p: &mut Painter) -> f32 {
        split_button_w(p, &self.text, self.kind)
    }

    /// The rectangle this button wants with its top left at `x`, `y`: [`SplitButton::width`] wide
    /// and [`button::HEIGHT`] high.
    pub fn rect(&self, p: &mut Painter, x: f32, y: f32) -> Rect {
        Rect::new(x, y, self.width(p), button::HEIGHT)
    }

    /// The two halves of `r`, by [`halves`].
    pub fn halves(&self, r: Rect) -> Halves {
        halves(r)
    }

    /// Which half a point is on, by [`hit`]; `None` off the button.
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> Option<Half> {
        hit(r, x, y)
    }

    /// What the key does, by [`key`]: Enter and Space [`Press::Activate`] the main action, Down
    /// and Alt+Down [`Press::Open`] the menu, and the rest is none.
    pub fn key(&self, k: &KeyName, mods: Mods) -> Press {
        key(k, mods)
    }
}

impl Default for SplitButton {
    /// An empty secondary split button.
    fn default() -> SplitButton {
        SplitButton::new("", Kind::Secondary)
    }
}

/// The width a split button with this label wants, in logical px: [`button::width`] plus
/// [`CHEVRON_W`], rounded.
pub fn split_button_w(p: &mut Painter, label: &str, kind: Kind) -> f32 {
    (button::width(p, label, kind) + CHEVRON_W).round()
}

/// The two halves of `r`: the chevron [`CHEVRON_W`] wide at the right (less when `r` is
/// narrower), and the main half all the rest. A rectangle that is not finite or has no width
/// gives two empty rectangles at its left edge, so a hit on them is false.
pub fn halves(r: Rect) -> Halves {
    if !sane(r) {
        let empty = Rect::new(r.x, r.y, 0.0, 0.0);
        return Halves {
            main: empty,
            chevron: empty,
        };
    }
    let w = CHEVRON_W.min(r.w);
    Halves {
        main: Rect::new(r.x, r.y, r.w - w, r.h),
        chevron: Rect::new(r.right() - w, r.y, w, r.h),
    }
}

/// Which half of the split button in `r` the point `x`, `y` is on: [`Half::Chevron`] in the last
/// [`CHEVRON_W`] px, [`Half::Main`] everywhere else inside `r`, and `None` outside it or for a
/// point that is not finite.
pub fn hit(r: Rect, x: f32, y: f32) -> Option<Half> {
    if !r.contains(x, y) {
        return None;
    }
    let h = halves(r);
    if h.chevron.contains(x, y) {
        Some(Half::Chevron)
    } else {
        Some(Half::Main)
    }
}

/// How a split button's two halves are drawn just now, from the caller's own state. Each half
/// has the states of a [`button::Look`]; the focus ring and a default mark cover the whole
/// button when either half asks for one, and either half's `open` draws its own fill pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SplitLook {
    pub main: button::Look,
    pub chevron: button::Look,
}

/// Draws a split button with `label` in `r`: one rounded box in `kind`'s colours, its two halves
/// lit on their own ([`SplitLook`]), a divider line between them, the label centred in the main
/// half and a down chevron in the chevron half.
///
/// The box and the default mark are drawn with [`Painter::frame`], so each line is one device
/// pixel; a half whose fill differs from the rest is painted inside that line, so neither the
/// frame nor the default mark is painted over. A subtle button draws no divider until one half
/// is hovered or pressed, since at rest it has no fill for a line to sit on. Disabled when
/// either half says so: then the whole control is the one quiet frame, with a faint label and
/// chevron, and no ring, hover, press, open or default mark is drawn.
pub fn draw_split(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    label: &str,
    kind: Kind,
    look: SplitLook,
) {
    if !sane(r) {
        return;
    }
    let h = halves(r);
    if look.main.disabled || look.chevron.disabled {
        p.frame(r, button::RADIUS, pal.field, pal.line);
        p.icon(
            glyph::DOWN,
            CHEVRON,
            h.chevron.cx(),
            h.chevron.cy(),
            pal.faint,
        );
        p.text_center(
            crate::gfx::Face::Semibold,
            button::TEXT,
            h.main,
            label,
            pal.faint,
        );
        return;
    }
    if look.main.focused || look.chevron.focused {
        crate::focus::draw_ring(p, pal, r, button::RADIUS);
    }
    let hc = button::high_contrast(pal, look.main.high_contrast || look.chevron.high_contrast);
    let style = button::style(pal, kind, hc);
    let (main_fill, main_ink) = button::state(&style, look.main);
    let (chev_fill, chev_ink) = button::state(&style, look.chevron);
    let default = look.main.default || look.chevron.default;
    let px = p.px();
    let base = style.fill.unwrap_or(pal.field);
    let ring = if default {
        Some(button::default_line(pal, kind, main_ink))
    } else {
        style.border
    };
    match ring {
        Some(stroke) => {
            p.frame(r, button::RADIUS, base, stroke);
            // A half that differs from the rest is painted inside the frame's one device pixel,
            // so the frame (or the default mark) stays whole.
            let inner = r.inset(px);
            let radius = (button::RADIUS - px).max(0.0);
            let main = Rect::new(
                h.main.x + px,
                inner.y,
                (h.main.right() - px - (h.main.x + px)).max(0.0),
                inner.h,
            );
            let chevron = Rect::new(
                h.chevron.x,
                inner.y,
                (inner.right() - h.chevron.x).max(0.0),
                inner.h,
            );
            if main_fill != style.fill
                && let Some(c) = main_fill
            {
                p.fill_path(&button::side(main, radius, false), c, 1.0);
            }
            if chev_fill != style.fill
                && let Some(c) = chev_fill
            {
                p.fill_path(&button::side(chevron, radius, true), c, 1.0);
            }
        }
        None => {
            if let Some(c) = main_fill {
                p.fill_path(&button::side(h.main, button::RADIUS, false), c, 1.0);
            }
            if let Some(c) = chev_fill {
                p.fill_path(&button::side(h.chevron, button::RADIUS, true), c, 1.0);
            }
        }
    }
    if main_fill.is_some() || chev_fill.is_some() || style.fill.is_some() {
        let base = main_fill.or(chev_fill).unwrap_or(pal.popup);
        p.fill(
            Rect::new(
                h.chevron.x - px / 2.0,
                r.y + px,
                px,
                (r.h - 2.0 * px).max(0.0),
            ),
            mix(base, pal.text, 0.15),
        );
    }
    p.icon(
        glyph::DOWN,
        CHEVRON,
        h.chevron.cx(),
        h.chevron.cy(),
        chev_ink,
    );
    let face = button::face(kind);
    p.text_center(face, button::TEXT, h.main, label, main_ink);
}

/// Whether a rectangle can be drawn at all: every number finite and both sides positive.
fn sane(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0
}

#[cfg(test)]
#[path = "menu_button_tests.rs"]
mod tests;
