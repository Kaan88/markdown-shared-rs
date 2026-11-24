//! The shape of Purge's window, the consumer that lets the Purge pieces reach `main` (roadmap,
//! "Purge (fast track)"): `cargo run --release -p mdshared-ui --features a11y --example
//! purge_shape`.
//!
//! It holds what Purge's main window holds, drawn with the toolkit and exposed to UI Automation
//! through [`mdshared_ui::a11y`]:
//! - a 2,000-row multi-select list with a column header (`ProgramList`, rows `Row0`..`Row1999`),
//!   only its visible rows drawn, every row in the accessibility tree, each keyed by its program
//!   so that its id and AutomationId stay with the program whatever the search shows;
//! - a search field (`SearchBox`, Ctrl+F) that filters the list. The selection is kept by
//!   program: a program the search hides leaves the selection, the others stay selected (so
//!   Uninstall never acts on a program the list does not show);
//! - a grouped list whose groups open and close (`LeftoverPane`, headers `GroupHeader0`.., each
//!   group's items in a list `LeftoverList0`.., items `Leftover0_0`..);
//! - a modal dialog (`ConfirmDialog`, buttons `ConfirmYes` and `ConfirmNo`) that Uninstall opens;
//!   while it is open the rest of the window is hidden from UIA;
//! - a progress bar (`SelectedProgress`), a check box (`ShowComponents`) and a footer
//!   (`FooterCount`, read back as its Name) a test reads back.
//!
//! A row takes UIA's SelectionItem `Select` as "this row only"; `Select` on a selected row keeps
//! it. `AddToSelection` and `RemoveFromSelection` reach the window as the same click and are not
//! supported ([`mdshared_ui::a11y`]).
//!
//! `tests/uia/purge_shape.ps1` drives it with UIA, the way Purge's VM rig drives its WinUI
//! window. Each request the window receives from UIA is printed on stdout, and so is the time to
//! the first frame (`--first-frame` exits after it). The example grows with each Purge piece as
//! it lands.

#[cfg(not(windows))]
fn main() {
    println!("purge_shape is for Windows (UI Automation) so far");
}

#[cfg(windows)]
fn main() {
    shape::run();
}

#[cfg(windows)]
mod shape {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use mdshared_text::bitmap::Bitmap;
    use mdshared_ui::a11y::accesskit::{ActivationHandler, NodeId, TreeUpdate};
    use mdshared_ui::a11y::{self, Item, List, Request, Row, RowIndex, Tree};
    use mdshared_ui::check::{Tick, draw_tick};
    use mdshared_ui::expander::{self, Look};
    use mdshared_ui::gfx::{Face, Gfx, Painter, Rect};
    use mdshared_ui::keys::{KeyName, Mods, key_name};
    use mdshared_ui::overlay;
    use mdshared_ui::rows::{Nav, Rows};
    use mdshared_ui::sections::{Entry, Sections};
    use mdshared_ui::select::Selection;
    use mdshared_ui::slider::draw_progress;
    use mdshared_ui::textfield::{FieldColors, TextField};
    use mdshared_ui::theme::{LIGHT, Palette};
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
    use winit::window::{Window, WindowId};

    const COUNT: usize = 2_000;
    const ROW_H: f32 = 40.0;
    const TOP: f32 = 56.0;
    const HEAD_H: f32 = 28.0;
    const FOOT_H: f32 = 28.0;
    const PANE_W: f32 = 300.0;
    const GROUP_H: f32 = 32.0;
    const ITEM_H: f32 = 28.0;
    const GROUPS: [&str; 4] = ["Files", "Folders", "Registry keys", "Services"];
    const ITEMS: usize = 6;
    const DIALOG: (f32, f32) = (420.0, 200.0);

    /// Where the keyboard is.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Focus {
        Button,
        Check,
        Search,
        List,
        Group(usize),
        Yes,
        No,
    }

    /// The node ids the window acts on, made once from their author ids.
    struct Ids {
        root: NodeId,
        button: NodeId,
        check: NodeId,
        search: NodeId,
        progress: NodeId,
        list: NodeId,
        footer: NodeId,
        pane: NodeId,
        dialog: NodeId,
        yes: NodeId,
        no: NodeId,
        rows: RowIndex,
    }

    impl Ids {
        fn new() -> Ids {
            Ids {
                root: a11y::id("PurgeShape"),
                button: a11y::id("UninstallButton"),
                check: a11y::id("ShowComponents"),
                search: a11y::id("SearchBox"),
                progress: a11y::id("SelectedProgress"),
                list: a11y::id("ProgramList"),
                footer: a11y::id("FooterCount"),
                pane: a11y::id("LeftoverPane"),
                dialog: a11y::id("ConfirmDialog"),
                yes: a11y::id("ConfirmYes"),
                no: a11y::id("ConfirmNo"),
                rows: RowIndex::new("ProgramList", 0..COUNT),
            }
        }

        fn group(g: usize) -> NodeId {
            a11y::id(&format!("Group{g}"))
        }

        fn header(g: usize) -> NodeId {
            a11y::id(&format!("GroupHeader{g}"))
        }

        fn header_of(&self, node: NodeId) -> Option<usize> {
            (0..GROUPS.len()).find(|g| Ids::header(*g) == node)
        }

        /// The window's children other than the dialog, hidden while it is open.
        fn main(&self) -> [NodeId; 7] {
            [
                self.button,
                self.check,
                self.search,
                self.progress,
                self.list,
                self.pane,
                self.footer,
            ]
        }
    }

    /// The part of `r` inside `area`, if any: the bounds of a pane's entry, so that one scrolled
    /// out of the pane cannot take the hits meant for what is drawn where it would be (see
    /// `a11y` on bounds).
    fn clip(r: Rect, area: Rect) -> Option<Rect> {
        let (x0, y0) = (r.x.max(area.x), r.y.max(area.y));
        let (x1, y1) = (r.right().min(area.right()), r.bottom().min(area.bottom()));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// The window's model: everything drawn and everything in the accessibility tree comes
    /// from here.
    struct Model {
        size: (f32, f32),
        scale: f32,
        scroll: f32,
        /// The programs the search shows, by index into all of them, in order. A program's
        /// index is its row's key in the accessibility tree.
        shown: Vec<usize>,
        /// Over `shown`'s positions; [`refilter`](Self::refilter) carries it over by program.
        sel: Selection,
        search: TextField,
        leftovers: Sections,
        show_components: bool,
        invoked: u32,
        uninstalled: usize,
        dialog: bool,
        focus: Focus,
        pointer: (f32, f32),
        mods: Mods,
    }

    fn name(program: usize) -> String {
        format!("Program {:04}", program + 1)
    }

    impl Model {
        fn new(size: (f32, f32), scale: f32) -> Model {
            let mut leftovers = Sections::new(0.0, ITEM_H);
            for _ in GROUPS {
                leftovers.add_group(ITEMS, Some(GROUP_H));
            }
            let mut m = Model {
                size,
                scale,
                scroll: 0.0,
                shown: (0..COUNT).collect(),
                sel: Selection::default(),
                search: TextField::new(),
                leftovers,
                show_components: false,
                invoked: 0,
                uninstalled: 0,
                dialog: false,
                focus: Focus::List,
                pointer: (0.0, 0.0),
                mods: Mods::default(),
            };
            m.layout();
            m
        }

        fn layout(&mut self) {
            self.leftovers.view = self.pane_area().h;
            self.leftovers.scroll = self.leftovers.clamp_scroll(self.leftovers.scroll);
            self.scroll = self.rows().clamp_scroll(self.scroll);
        }

        fn button_rect(&self) -> Rect {
            Rect::new(16.0, 12.0, 120.0, 32.0)
        }
        fn check_rect(&self) -> Rect {
            Rect::new(152.0, 18.0, 20.0, 20.0)
        }
        fn search_rect(&self) -> Rect {
            Rect::new(330.0, 12.0, 260.0, 32.0)
        }
        fn progress_rect(&self) -> Rect {
            Rect::new(self.size.0 - 216.0, 26.0, 200.0, 4.0)
        }
        fn list_area(&self) -> Rect {
            let y = TOP + HEAD_H;
            Rect::new(
                0.0,
                y,
                (self.size.0 - PANE_W).max(0.0),
                (self.size.1 - y - FOOT_H).max(0.0),
            )
        }
        fn pane_area(&self) -> Rect {
            let y = TOP + HEAD_H;
            Rect::new(
                (self.size.0 - PANE_W).max(0.0),
                y,
                PANE_W.min(self.size.0),
                (self.size.1 - y - FOOT_H).max(0.0),
            )
        }
        fn dialog_rect(&self) -> Rect {
            overlay::dialog_card(DIALOG.0, DIALOG.1, self.size.0, self.size.1)
        }
        fn dialog_buttons(&self) -> (Rect, Rect) {
            let d = self.dialog_rect();
            let y = d.bottom() - overlay::DIALOG_FOOTER + 20.0;
            let w = (d.w - 2.0 * overlay::DIALOG_PAD - 8.0) / 2.0;
            let yes = Rect::new(d.x + overlay::DIALOG_PAD, y, w, 32.0);
            let no = Rect::new(yes.right() + 8.0, y, w, 32.0);
            (yes, no)
        }
        fn rows(&self) -> Rows {
            let mut r = Rows::new(ROW_H, self.list_area().h, self.shown.len());
            r.scroll = self.scroll;
            r
        }
        fn fraction(&self) -> f32 {
            (self.sel.len() as f32 / COUNT as f32).clamp(0.0, 1.0)
        }
        /// The dialog's text under its title; also its description in the tree.
        fn dialog_body(&self) -> String {
            format!("{} programs will be removed.", self.sel.len())
        }
        fn footer(&self) -> String {
            format!(
                "{} selected, {} shown, uninstall pressed {} times, uninstalled {}, components {}",
                self.sel.len(),
                self.shown.len(),
                self.invoked,
                self.uninstalled,
                if self.show_components { "on" } else { "off" }
            )
        }

        /// Shows the programs whose name holds the search text. The selection, the cursor and the
        /// anchor stay on their programs where those are still shown; a program the search hides
        /// leaves the selection (and does not come back when it is shown again).
        fn refilter(&mut self) {
            let q = self.search.text().to_lowercase();
            let old = std::mem::take(&mut self.shown);
            self.shown = (0..COUNT)
                .filter(|i| q.is_empty() || name(*i).to_lowercase().contains(&q))
                .collect();
            let shown = &self.shown;
            self.sel
                .remap(|at| old.get(at).and_then(|p| shown.binary_search(p).ok()));
            self.scroll = 0.0;
            if let Some(c) = self.sel.cursor() {
                self.reveal(c);
            }
        }

        fn reveal(&mut self, i: usize) {
            self.scroll = self.rows().reveal(i);
        }

        /// Moves the keyboard to `f`, scrolling a group header into the pane when it is out.
        fn set_focus(&mut self, f: Focus) {
            self.focus = f;
            if let Focus::Group(g) = f
                && let Some(hi) = self.leftovers.index(Entry::Heading(g))
            {
                self.leftovers.scroll = self.leftovers.reveal(hi);
            }
        }

        fn press_uninstall(&mut self) {
            self.invoked += 1;
            self.dialog = true;
            self.focus = Focus::Yes;
        }

        fn close_dialog(&mut self, confirmed: bool) {
            if confirmed {
                self.uninstalled += self.sel.len();
            }
            self.dialog = false;
            self.focus = Focus::Button;
        }

        fn toggle_group(&mut self, g: usize) {
            self.leftovers.toggle(g);
            self.layout();
            self.set_focus(Focus::Group(g));
        }

        fn focus_node(&self, ids: &Ids) -> NodeId {
            match self.focus {
                Focus::Button => ids.button,
                Focus::Check => ids.check,
                Focus::Search => ids.search,
                Focus::List => match self.sel.cursor().and_then(|i| self.shown.get(i)) {
                    Some(p) => a11y::item_id(ids.list, *p),
                    None => ids.list,
                },
                Focus::Group(g) => Ids::header(g),
                Focus::Yes => ids.yes,
                Focus::No => ids.no,
            }
        }

        /// The accessibility tree, built the way the window is drawn.
        fn build(&self, tree: &mut Tree, ids: &Ids) -> TreeUpdate {
            tree.begin();
            let mut button =
                a11y::button("Uninstall", "UninstallButton", self.button_rect(), false);
            button.set_description("Uninstalls the selected programs");
            tree.add(ids.button, button);
            tree.add(
                ids.check,
                a11y::check_box(
                    "Show components",
                    "ShowComponents",
                    self.check_rect(),
                    Some(self.show_components),
                    false,
                ),
            );
            tree.add(
                ids.search,
                a11y::text_input(
                    "Search",
                    "SearchBox",
                    self.search_rect(),
                    self.search.text(),
                    false,
                ),
            );
            tree.add(
                ids.progress,
                a11y::progress(
                    "Selected",
                    "SelectedProgress",
                    self.progress_rect(),
                    Some(self.fraction()),
                ),
            );
            let l = List {
                label: "Programs",
                author: "ProgramList",
                area: self.list_area(),
                row_h: ROW_H,
                count: self.shown.len(),
                scroll: self.scroll,
                multiselect: true,
            };
            let list = a11y::list(tree, &l, |i| {
                let p = self.shown[i];
                Row {
                    key: p,
                    label: name(p),
                    author: format!("Row{p}"),
                    selected: self.sel.contains(i),
                }
            });

            // The leftovers: a pane of groups, each a header that opens and closes (UIA
            // ExpandCollapse) and, while open, a list of its items. The group has no name of its
            // own: the header names it, and a reader would otherwise say "Files" twice. The list
            // says what the items are ("Leftover files, list"), and carries the set's size each
            // item's "2 of 6" needs.
            let area = self.pane_area();
            let sticky = self.leftovers.sticky(area);
            let mut groups = Vec::new();
            for (g, title) in GROUPS.iter().enumerate() {
                let Some(hi) = self.leftovers.index(Entry::Heading(g)) else {
                    continue;
                };
                let open = !self.leftovers.is_collapsed(g);
                // A sticky header is where it is drawn, and its items are cut below it, so that
                // the header, not an item under it, takes the hits there.
                let (header_rect, items_area) = match sticky {
                    Some((s, r)) if s == hi => {
                        let below =
                            Rect::new(area.x, r.bottom(), area.w, area.bottom() - r.bottom());
                        (r, below)
                    }
                    _ => (self.leftovers.rect(area, hi), area),
                };
                let header_author = format!("GroupHeader{g}");
                let mut header = a11y::expander(title, &header_author, header_rect, open);
                match clip(header_rect, area) {
                    Some(r) => header.set_bounds(a11y::bounds(r)),
                    None => header.clear_bounds(),
                }
                tree.add(Ids::header(g), header);
                let mut children = vec![Ids::header(g)];
                let mut whole = header_rect;
                if open {
                    let rows = (0..ITEMS).filter_map(|r| {
                        let ei = self.leftovers.index(Entry::Row { group: g, row: r })?;
                        let rect = self.leftovers.rect(area, ei);
                        whole.h = rect.bottom() - whole.y;
                        Some(Item {
                            key: r,
                            label: format!("{title} {}", r + 1),
                            author: format!("Leftover{g}_{r}"),
                            bounds: clip(rect, items_area),
                        })
                    });
                    let rows: Vec<Item> = rows.collect();
                    let items_rect = Rect::new(
                        whole.x,
                        header_rect.bottom(),
                        whole.w,
                        whole.bottom() - header_rect.bottom(),
                    );
                    children.push(a11y::plain_list(
                        tree,
                        &format!("Leftover {}", title.to_lowercase()),
                        &format!("LeftoverList{g}"),
                        clip(items_rect, items_area),
                        rows,
                    ));
                }
                // The group covers its header and items, cut to the pane, so that it cannot
                // take hits meant for what is above or below the pane.
                let group_author = format!("Group{g}");
                let mut group = a11y::group("", &group_author, whole, children);
                match clip(whole, area) {
                    Some(r) => group.set_bounds(a11y::bounds(r)),
                    None => group.clear_bounds(),
                }
                tree.add(Ids::group(g), group);
                groups.push(Ids::group(g));
            }
            tree.add(
                ids.pane,
                a11y::group("Leftovers", "LeftoverPane", area, groups),
            );

            let f = Rect::new(0.0, self.size.1 - FOOT_H, self.size.0, FOOT_H);
            tree.add(ids.footer, a11y::text(&self.footer(), "FooterCount", f));

            let mut children = vec![
                ids.button,
                ids.check,
                ids.search,
                ids.progress,
                list,
                ids.pane,
                ids.footer,
            ];
            if self.dialog {
                let (yes, no) = self.dialog_buttons();
                tree.add(ids.yes, a11y::button("Uninstall", "ConfirmYes", yes, false));
                tree.add(ids.no, a11y::button("Cancel", "ConfirmNo", no, false));
                let mut d = a11y::dialog(
                    "Uninstall the selected programs?",
                    "ConfirmDialog",
                    self.dialog_rect(),
                    true,
                    vec![ids.yes, ids.no],
                );
                d.set_description(self.dialog_body());
                tree.add(ids.dialog, d);
                children.push(ids.dialog);
                // Modal: the rest stays in the tree, hidden, so that a reader and a UIA client
                // see only the dialog, and nothing is sent again but these seven when it closes.
                for n in ids.main() {
                    if let Some(node) = tree.get_mut(n) {
                        node.set_hidden();
                    }
                }
            }
            tree.add(ids.root, a11y::window("Purge shape", self.scale, children));
            tree.end(self.focus_node(ids))
        }

        /// What an assistive technology asked for. True when something changed.
        fn request(&mut self, req: Request, ids: &Ids) -> bool {
            println!("a11y request {req:?}");
            // While the dialog is open, only the dialog takes requests.
            if self.dialog {
                match req {
                    Request::Click(n) if n == ids.yes => self.close_dialog(true),
                    Request::Click(n) if n == ids.no => self.close_dialog(false),
                    Request::Focus(n) if n == ids.yes => self.focus = Focus::Yes,
                    Request::Focus(n) if n == ids.no => self.focus = Focus::No,
                    _ => return false,
                }
                return true;
            }
            let header = |n: NodeId| ids.header_of(n);
            // A row's node names its program (the key); the row's position is where the search
            // shows that program now, if it does.
            let row = |n: NodeId| {
                let p = ids.rows.key(n)?;
                self.shown.binary_search(&p).ok()
            };
            match req {
                Request::Click(n) if n == ids.button => self.press_uninstall(),
                Request::Click(n) if n == ids.check => {
                    self.show_components = !self.show_components;
                }
                Request::Focus(n) if n == ids.button => self.focus = Focus::Button,
                Request::Focus(n) if n == ids.check => self.focus = Focus::Check,
                Request::Focus(n) if n == ids.search => self.focus = Focus::Search,
                Request::Focus(n) if n == ids.list => self.focus = Focus::List,
                Request::SetValue(n, v) if n == ids.search => {
                    self.search.set_text(&v);
                    self.refilter();
                }
                Request::Click(n) if header(n).is_some() => {
                    let g = header(n).unwrap_or(0);
                    self.toggle_group(g);
                }
                Request::Expand(n) | Request::Collapse(n) if header(n).is_some() => {
                    let g = header(n).unwrap_or(0);
                    let want = matches!(req, Request::Expand(_));
                    if want == self.leftovers.is_collapsed(g) {
                        self.toggle_group(g);
                    }
                }
                Request::Focus(n) if header(n).is_some() => {
                    let g = header(n).unwrap_or(0);
                    self.set_focus(Focus::Group(g));
                }
                // SelectionItem's Select, AddToSelection and RemoveFromSelection all come as
                // this click. An unselected row: Select, that row only. A selected row: nothing,
                // so that Select on it keeps it selected.
                Request::Click(n) if row(n).is_some() => {
                    let i = row(n).unwrap_or(0);
                    if self.sel.contains(i) {
                        return false;
                    }
                    self.sel.select_only(i);
                    self.focus = Focus::List;
                }
                Request::Focus(n) if row(n).is_some() => {
                    self.sel.set_cursor(row(n).unwrap_or(0));
                    self.focus = Focus::List;
                }
                Request::ScrollIntoView(n) if row(n).is_some() => {
                    let i = row(n).unwrap_or(0);
                    self.reveal(i);
                }
                _ => return false,
            }
            true
        }

        fn click(&mut self) -> bool {
            let (x, y) = self.pointer;
            if self.dialog {
                let (yes, no) = self.dialog_buttons();
                if yes.contains(x, y) {
                    self.close_dialog(true);
                } else if no.contains(x, y) {
                    self.close_dialog(false);
                } else {
                    return false;
                }
                return true;
            }
            if self.button_rect().contains(x, y) {
                self.press_uninstall();
            } else if self.check_rect().contains(x, y) {
                self.show_components = !self.show_components;
                self.focus = Focus::Check;
            } else if self.search_rect().contains(x, y) {
                self.focus = Focus::Search;
            } else if let Some(i) = self.rows().row_at(self.list_area(), x, y) {
                self.sel.click(i, self.mods);
                self.focus = Focus::List;
            } else if let Some(e) = self.leftovers.entry_at(self.pane_area(), x, y) {
                match self.leftovers.entry(e) {
                    Some(Entry::Heading(g)) => self.toggle_group(g),
                    _ => return false,
                }
            } else {
                return false;
            }
            true
        }

        fn next_focus(&self, back: bool) -> Focus {
            if self.dialog {
                return if self.focus == Focus::Yes {
                    Focus::No
                } else {
                    Focus::Yes
                };
            }
            let mut ring = vec![Focus::Button, Focus::Check, Focus::Search, Focus::List];
            ring.extend((0..GROUPS.len()).map(Focus::Group));
            let at = ring.iter().position(|f| *f == self.focus).unwrap_or(0);
            let n = ring.len();
            ring[if back { (at + n - 1) % n } else { (at + 1) % n }]
        }

        fn key(&mut self, key: &KeyName, typed: Option<&str>) -> bool {
            if *key == KeyName::Tab {
                let f = self.next_focus(self.mods.shift);
                self.set_focus(f);
                return true;
            }
            if self.dialog {
                return match key {
                    KeyName::Escape => {
                        self.close_dialog(false);
                        true
                    }
                    KeyName::Enter | KeyName::Space => {
                        self.close_dialog(self.focus == Focus::Yes);
                        true
                    }
                    _ => false,
                };
            }
            if *key == KeyName::Char('f') && self.mods.ctrl {
                self.focus = Focus::Search;
                self.search.select_all();
                return true;
            }
            if self.focus == Focus::Search {
                if *key == KeyName::Escape && !self.search.is_empty() {
                    self.search.clear();
                    self.refilter();
                    return true;
                }
                if *key == KeyName::Down || *key == KeyName::Enter {
                    self.focus = Focus::List;
                    return true;
                }
                return match self.search.handle_key(key, self.mods, typed, true) {
                    Some(changed) => {
                        if changed {
                            self.refilter();
                        }
                        true
                    }
                    None => false,
                };
            }
            match (key, self.focus) {
                (KeyName::Space | KeyName::Enter, Focus::Button) => {
                    self.press_uninstall();
                    return true;
                }
                (KeyName::Space | KeyName::Enter, Focus::Check) => {
                    self.show_components = !self.show_components;
                    return true;
                }
                (KeyName::Space | KeyName::Enter, Focus::Group(g)) => {
                    self.toggle_group(g);
                    return true;
                }
                _ => {}
            }
            if self.focus != Focus::List {
                return false;
            }
            let nav = match key {
                KeyName::Down => Nav::Down,
                KeyName::Up => Nav::Up,
                KeyName::PageDown => Nav::PageDown,
                KeyName::PageUp => Nav::PageUp,
                KeyName::Home => Nav::Home,
                KeyName::End => Nav::End,
                _ => return false,
            };
            let rows = self.rows();
            match self.sel.nav(&rows, nav, self.mods.shift) {
                Some(i) => {
                    self.reveal(i);
                    true
                }
                None => false,
            }
        }

        /// A group's header: the expander's header (`expander::draw_header`, the same height) on
        /// a band of the bar colour. The expander draws no background at rest, and a group header
        /// needs one, to stand apart from the items and to cover them while it sticks.
        fn draw_group_header(&self, p: &mut Painter, pal: &Palette, r: Rect, g: usize) {
            p.fill(r, pal.bar);
            let look = Look {
                hovered: self.hovered_header() == Some(g),
                pressed: false,
                focused: self.focus == Focus::Group(g),
                disabled: false,
            };
            let open = !self.leftovers.is_collapsed(g);
            expander::draw_header(p, pal, r, GROUPS[g], open, look);
        }

        /// The group whose header is under the pointer, if any (none while the dialog is open).
        fn hovered_header(&self) -> Option<usize> {
            if self.dialog {
                return None;
            }
            let (x, y) = self.pointer;
            let e = self.leftovers.entry_at(self.pane_area(), x, y)?;
            match self.leftovers.entry(e) {
                Some(Entry::Heading(g)) => Some(g),
                _ => None,
            }
        }

        fn draw(&mut self, p: &mut Painter, pal: &Palette) {
            let b = self.button_rect();
            let fill = if self.focus == Focus::Button {
                pal.press
            } else {
                pal.hover
            };
            p.fill(b, fill);
            p.text_center(Face::Semibold, 14.0, b, "Uninstall", pal.text);
            let mut tick = Tick::new(self.show_components);
            if self.focus == Focus::Check {
                tick = tick.with_focus();
            }
            draw_tick(p, pal, self.check_rect(), tick);
            p.text(
                Face::Ui,
                14.0,
                180.0,
                28.0,
                "Show components",
                pal.text,
                140.0,
            );

            let s = self.search_rect();
            let focused = self.focus == Focus::Search;
            p.frame(
                s,
                4.0,
                pal.page,
                if focused { pal.accent } else { pal.line },
            );
            let colors = FieldColors {
                text: pal.text,
                placeholder: pal.faint,
                selection: pal.accent,
                caret: pal.text,
            };
            self.search.draw(
                p,
                s.inset(8.0),
                Face::Ui,
                14.0,
                &colors,
                "Search (Ctrl+F)",
                focused,
                true,
            );
            draw_progress(p, pal, self.progress_rect(), self.fraction());

            let area = self.list_area();
            let head = Rect::new(area.x, TOP, area.w, HEAD_H);
            p.fill(head, pal.bar);
            p.text(
                Face::Semibold,
                13.0,
                16.0,
                head.cy(),
                "Name",
                pal.muted,
                300.0,
            );
            p.text(
                Face::Semibold,
                13.0,
                head.right() - 120.0,
                head.cy(),
                "Size",
                pal.muted,
                100.0,
            );
            let rows = self.rows();
            let saved = p.clip(area);
            for i in rows.visible() {
                let r = rows.rect(area, i);
                if self.sel.contains(i) {
                    p.fill(r, pal.selection);
                }
                if self.focus == Focus::List && self.sel.cursor() == Some(i) {
                    p.fill(Rect::new(r.x, r.y, 3.0, r.h), pal.accent);
                }
                let prog = self.shown[i];
                p.text(
                    Face::Ui,
                    14.0,
                    16.0,
                    r.cy(),
                    &name(prog),
                    pal.text,
                    r.w - 160.0,
                );
                let size = format!("{} MB", (prog * 37) % 900 + 12);
                p.text(
                    Face::Ui,
                    13.0,
                    r.right() - 120.0,
                    r.cy(),
                    &size,
                    pal.muted,
                    100.0,
                );
            }
            p.restore(saved);

            let pane = self.pane_area();
            p.fill(Rect::new(pane.x, TOP, pane.w, HEAD_H), pal.bar);
            p.text(
                Face::Semibold,
                13.0,
                pane.x + 16.0,
                TOP + HEAD_H / 2.0,
                "Leftovers",
                pal.muted,
                pane.w - 32.0,
            );
            let px = p.px();
            p.fill(Rect::new(pane.x, TOP, px, pane.bottom() - TOP), pal.line);
            let saved = p.clip(pane);
            for e in self.leftovers.visible() {
                let r = self.leftovers.rect(pane, e);
                match self.leftovers.entry(e) {
                    Some(Entry::Heading(g)) => self.draw_group_header(p, pal, r, g),
                    Some(Entry::Row { group, row }) => {
                        let label = format!("{} {}", GROUPS[group], row + 1);
                        p.text(
                            Face::Ui,
                            13.0,
                            r.x + 36.0,
                            r.cy(),
                            &label,
                            pal.text,
                            r.w - 52.0,
                        );
                    }
                    None => {}
                }
            }
            // The heading of the group at the top sticks there while its rows scroll under it,
            // drawn over them; `Sections::entry_at` already gives it the clicks there.
            if let Some((hi, r)) = self.leftovers.sticky(pane)
                && let Some(Entry::Heading(g)) = self.leftovers.entry(hi)
            {
                self.draw_group_header(p, pal, r, g);
            }
            p.restore(saved);

            let f = Rect::new(0.0, self.size.1 - FOOT_H, self.size.0, FOOT_H);
            p.fill(f, pal.bar);
            let footer = self.footer();
            p.text(Face::Ui, 13.0, 16.0, f.cy(), &footer, pal.muted, f.w - 32.0);

            if self.dialog {
                overlay::draw_backdrop(p, pal, self.size.0, self.size.1);
                let d = self.dialog_rect();
                overlay::draw_dialog_card(p, pal, d);
                let inner = d.w - 2.0 * overlay::DIALOG_PAD;
                p.text(
                    Face::Semibold,
                    16.0,
                    d.x + overlay::DIALOG_PAD,
                    d.y + overlay::DIALOG_PAD + 10.0,
                    "Uninstall the selected programs?",
                    pal.text,
                    inner,
                );
                let msg = self.dialog_body();
                p.text(
                    Face::Ui,
                    14.0,
                    d.x + overlay::DIALOG_PAD,
                    d.y + overlay::DIALOG_PAD + 40.0,
                    &msg,
                    pal.muted,
                    inner,
                );
                let (yes, no) = self.dialog_buttons();
                overlay::draw_dialog_button(p, pal, yes, "Uninstall", true, false, false);
                overlay::draw_dialog_button(p, pal, no, "Cancel", false, false, false);
                let ring = if self.focus == Focus::Yes { yes } else { no };
                let px = p.px();
                p.fill(
                    Rect::new(ring.x, ring.bottom() + 2.0, ring.w, 2.0 * px),
                    pal.text,
                );
            }
        }
    }

    /// Serves the whole tree at once when a screen reader or UIA client first asks, on whichever
    /// thread asks.
    struct Latest(Arc<Mutex<Option<TreeUpdate>>>);

    impl ActivationHandler for Latest {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            let tree = self.0.lock().ok()?.clone();
            println!("a11y: initial tree requested, served: {}", tree.is_some());
            tree
        }
    }

    struct State {
        latest: Arc<Mutex<Option<TreeUpdate>>>,
        window: Rc<Window>,
        surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
        adapter: accesskit_winit::Adapter,
        gfx: Gfx,
        pal: Palette,
        model: Model,
        tree: Tree,
        ids: Ids,
        /// The focus of the last update sent.
        focus: NodeId,
    }

    impl State {
        /// Builds the tree again, sends the adapter what changed and keeps the whole tree for a
        /// client that connects later.
        fn changed(&mut self) {
            self.window.request_redraw();
            let update = self.model.build(&mut self.tree, &self.ids);
            if update.nodes.is_empty() && update.tree.is_none() && update.focus == self.focus {
                // Nothing in the tree changed: no update, and the kept tree is still right.
                return;
            }
            self.focus = update.focus;
            // The whole tree, kept for a client that connects later (or again).
            let full = self.tree.full(update.focus);
            if let Ok(mut l) = self.latest.lock() {
                *l = Some(full);
            }
            self.adapter.update_if_active(|| update);
        }

        fn draw(&mut self) {
            let (w, h) = (
                (self.model.size.0 * self.model.scale).round().max(1.0) as u32,
                (self.model.size.1 * self.model.scale).round().max(1.0) as u32,
            );
            let mut bmp = Bitmap::new(w, h);
            let pal = self.pal;
            bmp.clear(pal.page);
            {
                let mut p = Painter::new(&mut bmp, &mut self.gfx, self.model.scale);
                self.model.draw(&mut p, &pal);
            }
            let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
                return;
            };
            if self.surface.resize(nw, nh).is_err() {
                return;
            }
            let Ok(mut buf) = self.surface.buffer_mut() else {
                return;
            };
            for (dst, px) in buf.iter_mut().zip(bmp.pixels.as_chunks::<4>().0) {
                *dst = u32::from_le_bytes([px[0], px[1], px[2], 0]);
            }
            let _ = buf.present();
        }
    }

    struct App {
        proxy: EventLoopProxy<accesskit_winit::Event>,
        state: Option<State>,
        started: std::time::Instant,
    }

    impl ApplicationHandler<accesskit_winit::Event> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.state.is_some() {
                return;
            }
            let attrs = Window::default_attributes()
                .with_title("Purge shape")
                .with_inner_size(LogicalSize::new(1000.0, 680.0))
                .with_min_inner_size(LogicalSize::new(820.0, 520.0))
                .with_visible(false);
            let window = Rc::new(el.create_window(attrs).expect("a window"));
            let scale = window.scale_factor() as f32;
            let size = window.inner_size().to_logical::<f32>(f64::from(scale));
            let ids = Ids::new();
            let model = Model::new((size.width, size.height), scale);
            // The tree is built before the adapter exists, so that the first request for it
            // (which can come as soon as the window does) is answered with the whole tree.
            let mut tree = Tree::new(ids.root);
            let focus = model.build(&mut tree, &ids).focus;
            let latest = Arc::new(Mutex::new(Some(tree.full(focus))));
            let adapter = accesskit_winit::Adapter::with_mixed_handlers(
                el,
                &window,
                Latest(latest.clone()),
                self.proxy.clone(),
            );
            let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
            let surface = softbuffer::Surface::new(&context, window.clone()).expect("surface");
            let mut state = State {
                latest,
                window: window.clone(),
                surface,
                adapter,
                gfx: Gfx::new(),
                pal: LIGHT,
                model,
                tree,
                focus,
                ids,
            };
            state.draw();
            window.set_visible(true);
            // Check C1: from the start of main to the first frame presented.
            println!(
                "first frame after {:.1} ms",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
            self.state = Some(state);
            if std::env::args().any(|a| a == "--first-frame") {
                el.exit();
            }
        }

        fn user_event(&mut self, _el: &ActiveEventLoop, event: accesskit_winit::Event) {
            let Some(s) = self.state.as_mut() else { return };
            match event.window_event {
                // Sent when the activation handler had no tree to give; the whole tree, then.
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    let (tree, focus) = (&s.tree, s.focus);
                    s.adapter.update_if_active(|| tree.full(focus));
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    if s.model.request(a11y::request(req), &s.ids) {
                        s.changed();
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                    println!("a11y: deactivated");
                }
            }
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
            let Some(s) = self.state.as_mut() else { return };
            s.adapter.process_event(&s.window, &event);
            let m = &mut s.model;
            let changed = match event {
                WindowEvent::CloseRequested => {
                    el.exit();
                    false
                }
                WindowEvent::Resized(size) => {
                    let l = size.to_logical::<f32>(f64::from(m.scale));
                    m.size = (l.width, l.height);
                    m.layout();
                    true
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    m.scale = scale_factor as f32;
                    true
                }
                WindowEvent::RedrawRequested => {
                    s.draw();
                    false
                }
                WindowEvent::ModifiersChanged(mods) => {
                    m.mods = Mods::from_winit(mods.state());
                    false
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let l = position.to_logical::<f32>(f64::from(m.scale));
                    let was = m.hovered_header();
                    m.pointer = (l.x, l.y);
                    // Only the drawing changes (a header's hover fill), not the tree.
                    if m.hovered_header() != was {
                        s.window.request_redraw();
                    }
                    false
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => m.click(),
                WindowEvent::MouseWheel { delta, .. } if !m.dialog => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * ROW_H * 3.0,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / m.scale,
                    };
                    let (x, y) = m.pointer;
                    if m.pane_area().contains(x, y) {
                        let to = m.leftovers.scroll - f64::from(dy);
                        m.leftovers.scroll = m.leftovers.clamp_scroll(to);
                    } else {
                        m.scroll = m.rows().clamp_scroll(m.scroll - dy);
                    }
                    true
                }
                WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                    match key_name(&event.logical_key) {
                        Some(k) => m.key(&k, event.text.as_deref()),
                        None => false,
                    }
                }
                _ => false,
            };
            if changed {
                s.changed();
            }
        }
    }

    pub fn run() {
        let started = std::time::Instant::now();
        let event_loop = EventLoop::<accesskit_winit::Event>::with_user_event()
            .build()
            .expect("an event loop");
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = App {
            proxy: event_loop.create_proxy(),
            state: None,
            started,
        };
        event_loop.run_app(&mut app).expect("the event loop");
    }
}
