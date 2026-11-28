//! The field's clipboard actions. The application owns the popup and passes back its chosen item.

use std::borrow::Cow;

use super::TextField;
use crate::{menu::Item, platform};

/// An item in the single-line field's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAction {
    /// Copy the selection, then delete it only if the clipboard accepted it.
    Cut,
    /// Copy the selection, leaving the field unchanged.
    Copy,
    /// Insert clipboard text, replacing any selection and turning line breaks into spaces.
    Paste,
    /// Select the entire field.
    SelectAll,
}

/// Maximum UTF-8 byte length for menu Paste, before cleaning.
pub const MAX_PASTE_BYTES: usize = 64 * 1024;

/// The words of the edit menu, one per [`ContextAction`]: what
/// [`TextField::context_menu_labelled`] and [`FieldInput`](super::FieldInput) put on its rows.
/// [`EditLabels::ENGLISH`] (also the [`Default`]) is the words [`TextField::context_menu`] has
/// always used. The fields are private so that an item added to the menu later (Undo, Delete)
/// can come with a builder method instead of breaking every application that makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditLabels {
    cut: Cow<'static, str>,
    copy: Cow<'static, str>,
    paste: Cow<'static, str>,
    select_all: Cow<'static, str>,
}

impl EditLabels {
    /// "Cut", "Copy", "Paste", "Select all".
    pub const ENGLISH: EditLabels = EditLabels {
        cut: Cow::Borrowed("Cut"),
        copy: Cow::Borrowed("Copy"),
        paste: Cow::Borrowed("Paste"),
        select_all: Cow::Borrowed("Select all"),
    };

    /// The four words in the menu's order: Cut, Copy, Paste, Select all. A word may carry an
    /// access key as `menu::Item` labels do (`"&Kes"`); an empty word is kept as given (the row
    /// is drawn with no label), since a translation table that lacks one is the caller's to fix.
    pub fn new(
        cut: impl Into<Cow<'static, str>>,
        copy: impl Into<Cow<'static, str>>,
        paste: impl Into<Cow<'static, str>>,
        select_all: impl Into<Cow<'static, str>>,
    ) -> EditLabels {
        EditLabels {
            cut: cut.into(),
            copy: copy.into(),
            paste: paste.into(),
            select_all: select_all.into(),
        }
    }

    /// The word for `action`.
    pub fn label(&self, action: ContextAction) -> &str {
        match action {
            ContextAction::Cut => &self.cut,
            ContextAction::Copy => &self.copy,
            ContextAction::Paste => &self.paste,
            ContextAction::SelectAll => &self.select_all,
        }
    }
}

impl Default for EditLabels {
    fn default() -> EditLabels {
        EditLabels::ENGLISH
    }
}

pub(super) trait Clipboard {
    fn may_have_text(&mut self) -> bool;
    fn read(&mut self, limit: Option<usize>) -> Option<String>;
    fn write(&mut self, text: &str) -> bool;
}

pub(super) struct SystemClipboard;

#[cfg(test)]
thread_local! {
    static PASTE_FIXTURE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn paste_fixture(text: &'static str) -> impl Drop {
    // Exercise callers of the system adapter without reading or writing the desktop clipboard.
    struct Restore(Option<&'static str>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PASTE_FIXTURE.set(self.0);
        }
    }
    Restore(PASTE_FIXTURE.replace(Some(text)))
}

impl Clipboard for SystemClipboard {
    fn may_have_text(&mut self) -> bool {
        #[cfg(test)]
        if PASTE_FIXTURE.get().is_some() {
            return true;
        }
        platform::clipboard_may_have_text()
    }

    fn read(&mut self, limit: Option<usize>) -> Option<String> {
        #[cfg(test)]
        if let Some(text) = PASTE_FIXTURE.get() {
            return limit
                .is_none_or(|limit| text.len() <= limit)
                .then(|| text.to_owned());
        }
        match limit {
            Some(limit) => platform::clipboard_text_bounded(limit),
            None => platform::clipboard_text(),
        }
    }

    fn write(&mut self, text: &str) -> bool {
        #[cfg(test)]
        assert!(PASTE_FIXTURE.get().is_none(), "unexpected clipboard write");
        platform::set_clipboard_text(text)
    }
}

impl TextField {
    /// The four items for a [`crate::menu::Menu`] or [`crate::submenu::Cascade`]. Call when
    /// opening the menu. Paste is enabled when text is advertised or the platform cannot
    /// cheaply check. The clipboard payload is not read until Paste is chosen.
    /// Secret fields allow Paste but never offer Cut or Copy.
    pub fn context_menu(&self) -> Vec<Item<ContextAction>> {
        self.context_menu_with(&mut SystemClipboard)
    }

    /// [`context_menu`](Self::context_menu) with the items worded by `labels`, for an
    /// application whose interface is not in English. The items, their order and which are
    /// enabled are the same; only the words differ. `context_menu()` is this with
    /// [`EditLabels::ENGLISH`].
    pub fn context_menu_labelled(&self, labels: &EditLabels) -> Vec<Item<ContextAction>> {
        self.context_menu_labelled_with(labels, &mut SystemClipboard)
    }

    /// Applies an item chosen from the menu, checking again that it is allowed. Returns true
    /// when the field was edited, a selection was made, or text was copied successfully.
    /// A failed clipboard write never deletes the selection. As with the keyboard shortcuts,
    /// secret fields accept Paste but refuse Cut and Copy so the secret cannot be exported.
    pub fn apply_context_action(&mut self, action: ContextAction) -> bool {
        self.apply_with(action, &mut SystemClipboard)
    }

    fn context_menu_with(&self, clipboard: &mut impl Clipboard) -> Vec<Item<ContextAction>> {
        self.context_menu_labelled_with(&EditLabels::ENGLISH, clipboard)
    }

    pub(super) fn context_menu_labelled_with(
        &self,
        labels: &EditLabels,
        clipboard: &mut impl Clipboard,
    ) -> Vec<Item<ContextAction>> {
        use ContextAction::*;
        let can_copy = self.selection_for_export().is_some();
        let can_paste = clipboard.may_have_text();
        let item = |action| Item::new(action, labels.label(action));
        vec![
            item(Cut).enabled(can_copy),
            item(Copy).enabled(can_copy),
            item(Paste).enabled(can_paste),
            item(SelectAll).enabled(!self.is_empty()),
        ]
    }

    pub(super) fn apply_with(
        &mut self,
        action: ContextAction,
        clipboard: &mut impl Clipboard,
    ) -> bool {
        use ContextAction::*;
        match action {
            Cut | Copy => {
                self.selection_for_export()
                    .is_some_and(|text| clipboard.write(text))
                    && (action == Copy || self.delete_selection())
            }
            Paste => self.paste_from_menu(clipboard),
            SelectAll if !self.is_empty() => {
                self.select_all();
                true
            }
            _ => false,
        }
    }

    /// The keyboard shortcut takes the first line even from a large clipboard. A line that
    /// cleans to nothing still replaces a selection. Only secret fields keep Unicode separators.
    pub(super) fn paste_first_line(&mut self, clipboard: &mut impl Clipboard) -> bool {
        clipboard.read(None).is_some_and(|text| {
            let first = text.lines().next().unwrap_or("");
            self.insert_clean(&super::clean_with(first, !self.is_secret()))
        })
    }

    fn paste_from_menu(&mut self, clipboard: &mut impl Clipboard) -> bool {
        let Some(text) = clipboard
            .read(Some(MAX_PASTE_BYTES))
            .filter(|text| text.len() <= MAX_PASTE_BYTES)
        else {
            return false;
        };
        let clean = super::clean(&text);
        !clean.is_empty() && self.insert_clean(&clean)
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextAction::*, *};

    #[derive(Default)]
    struct Fake {
        text: Option<String>,
        probes: usize,
        reads: usize,
        requested_limit: Option<usize>,
        writes: usize,
        fail_write: bool,
    }

    impl Clipboard for Fake {
        fn may_have_text(&mut self) -> bool {
            self.probes += 1;
            self.text.is_some()
        }

        fn read(&mut self, limit: Option<usize>) -> Option<String> {
            self.reads += 1;
            self.requested_limit = limit;
            self.text.clone()
        }

        fn write(&mut self, text: &str) -> bool {
            self.writes += 1;
            if self.fail_write {
                return false;
            }
            self.text = Some(text.to_owned());
            true
        }
    }

    fn enabled(field: &TextField, clipboard: &mut Fake) -> Vec<ContextAction> {
        field
            .context_menu_with(clipboard)
            .into_iter()
            .filter(|item| item.enabled)
            .map(|item| item.id)
            .collect()
    }

    #[test]
    fn menu_rows_follow_selection_and_advertised_clipboard_text() {
        let mut f = TextField::new();
        let mut clipboard = Fake::default();
        assert_eq!(
            f.context_menu_with(&mut clipboard)
                .into_iter()
                .map(|row| (row.id, row.label))
                .collect::<Vec<_>>(),
            vec![
                (Cut, "Cut".to_owned()),
                (Copy, "Copy".to_owned()),
                (Paste, "Paste".to_owned()),
                (SelectAll, "Select all".to_owned()),
            ]
        );
        assert!(enabled(&f, &mut clipboard).is_empty());
        clipboard.text = Some("text".to_owned());
        assert_eq!(enabled(&f, &mut clipboard), [Paste]);
        f.set_text("Jane");
        assert_eq!(enabled(&f, &mut clipboard), [Paste, SelectAll]);
        f.select_all();
        assert_eq!(enabled(&f, &mut clipboard), [Cut, Copy, Paste, SelectAll]);
        for text in [None, Some(""), Some("\0")] {
            clipboard.text = text.map(str::to_owned);
            let rows = enabled(&f, &mut clipboard);
            assert_eq!(rows.contains(&Paste), text.is_some());
            assert!(!f.apply_with(Paste, &mut clipboard));
            assert_eq!(f.selected_text(), "Jane");
        }
    }

    /// The rows exactly as `context_menu_with` built them before labels could be chosen.
    fn old_rows(f: &TextField, clipboard: &mut Fake) -> Vec<Item<ContextAction>> {
        let can_copy = f.selection_for_export().is_some();
        let can_paste = clipboard.may_have_text();
        vec![
            Item::new(Cut, "Cut").enabled(can_copy),
            Item::new(Copy, "Copy").enabled(can_copy),
            Item::new(Paste, "Paste").enabled(can_paste),
            Item::new(SelectAll, "Select all").enabled(!f.is_empty()),
        ]
    }

    fn row_parts(rows: Vec<Item<ContextAction>>) -> Vec<(ContextAction, String, bool, bool)> {
        rows.into_iter()
            .map(|r| (r.id, r.label, r.enabled, r.separator_before))
            .collect()
    }

    #[test]
    fn english_labels_give_the_rows_the_menu_always_had_in_every_state() {
        let states: [(&str, bool, bool, Option<&str>); 5] = [
            ("", false, false, None),
            ("Jane", false, false, Some("x")),
            ("Jane", true, false, None),
            ("Jane", true, false, Some("x")),
            ("hunter2", true, true, Some("x")),
        ];
        for (text, select, secret, clip) in states {
            let mut f = TextField::new();
            f.set_text(text);
            f.set_secret(secret);
            if select {
                f.select_all();
            }
            let mut clipboard = Fake {
                text: clip.map(str::to_owned),
                ..Fake::default()
            };
            let old = row_parts(old_rows(&f, &mut clipboard));
            assert_eq!(row_parts(f.context_menu_with(&mut clipboard)), old);
            assert_eq!(
                row_parts(f.context_menu_labelled_with(&EditLabels::ENGLISH, &mut clipboard)),
                old
            );
        }
    }

    #[test]
    fn other_labels_change_only_the_words() {
        let tr = EditLabels::new("Kes", "Kopyala", "Yapıştır", String::from("Tümünü seç"));
        let mut f = TextField::new();
        f.set_text("kelime");
        f.select_all();
        let mut clipboard = Fake {
            text: Some("x".into()),
            ..Fake::default()
        };
        let english = row_parts(f.context_menu_with(&mut clipboard));
        let turkish = row_parts(f.context_menu_labelled_with(&tr, &mut clipboard));
        let words: Vec<_> = turkish.iter().map(|r| r.1.as_str()).collect();
        assert_eq!(words, ["Kes", "Kopyala", "Yapıştır", "Tümünü seç"]);
        for (e, t) in english.iter().zip(&turkish) {
            assert_eq!((e.0, e.2, e.3), (t.0, t.2, t.3));
        }
        assert_eq!(EditLabels::default(), EditLabels::ENGLISH);
        assert_eq!(tr.label(SelectAll), "Tümünü seç");
        assert_eq!(EditLabels::ENGLISH.label(Paste), "Paste");
        // An empty word is kept, not replaced by English.
        assert_eq!(EditLabels::new("", "", "", "").label(Cut), "");
    }

    #[test]
    fn copy_keeps_selection_cut_waits_for_success_and_paste_replaces_selection() {
        let mut f = TextField::new();
        let mut clipboard = Fake::default();
        f.set_text("é!");
        assert!(!f.apply_with(Cut, &mut clipboard));
        assert!(!f.apply_with(Copy, &mut clipboard));
        assert_eq!(clipboard.writes, 0);
        assert!(f.apply_with(SelectAll, &mut clipboard));
        assert!(f.apply_with(Copy, &mut clipboard));
        assert_eq!(clipboard.text.as_deref(), Some("é!"));
        assert_eq!(f.selected_text(), "é!");
        clipboard.fail_write = true;
        assert!(!f.apply_with(Cut, &mut clipboard));
        assert_eq!(f.selected_text(), "é!");
        clipboard.fail_write = false;
        clipboard.text = None;
        assert!(f.apply_with(Cut, &mut clipboard));
        assert_eq!(clipboard.text.as_deref(), Some("é!"));
        assert!(f.is_empty());
        assert!(!f.apply_with(SelectAll, &mut clipboard));
        f.set_text("replace me");
        f.select_all();
        clipboard.text = Some("new\r\ntext\t!".to_owned());
        assert!(f.apply_with(Paste, &mut clipboard));
        assert_eq!(f.text(), "new text !");
        assert!(!f.has_selection());
    }

    #[test]
    fn a_secret_menu_pastes_without_exporting_the_selection() {
        let mut f = TextField::new();
        let mut clipboard = Fake {
            text: Some("outside".to_owned()),
            ..Fake::default()
        };
        f.set_text("secret");
        f.select_all();
        f.set_secret(true);
        assert_eq!(enabled(&f, &mut clipboard), [Paste, SelectAll]);
        assert_eq!(clipboard.reads, 0);
        for action in [Cut, Copy] {
            assert!(!f.apply_with(action, &mut clipboard));
        }
        assert!(f.has_selection());
        assert_eq!(f.selection_for_export(), None);
        assert!(f.apply_with(Paste, &mut clipboard));
        assert_eq!(f.text(), "outside");
        assert_eq!(
            (clipboard.probes, clipboard.reads, clipboard.writes),
            (1, 1, 0)
        );
        // The public system adapters are safe to call in the secret case as well.
        let _clipboard = paste_fixture(" again");
        assert!(f.context_menu()[2].enabled);
        assert!(f.apply_context_action(Paste));
        assert_eq!(f.text(), "outside again");
    }

    #[test]
    fn menu_only_probes_and_oversized_paste_reads_once_without_editing() {
        let mut f = TextField::new();
        f.set_text("replace");
        f.select_all();
        let mut clipboard = Fake {
            text: Some(format!("x{}", "\x01".repeat(MAX_PASTE_BYTES))),
            ..Fake::default()
        };
        assert!(enabled(&f, &mut clipboard).contains(&Paste));
        assert_eq!((clipboard.probes, clipboard.reads), (1, 0));
        assert!(!f.apply_with(Paste, &mut clipboard));
        assert_eq!(clipboard.reads, 1);
        assert_eq!(clipboard.requested_limit, Some(MAX_PASTE_BYTES));
        assert_eq!(f.selection_for_export(), Some("replace"));
        clipboard.text = Some("x".repeat(MAX_PASTE_BYTES));
        assert!(f.apply_with(Paste, &mut clipboard));
        assert_eq!(clipboard.reads, 2);
        assert_eq!(f.text().len(), MAX_PASTE_BYTES);
    }

    #[test]
    fn keyboard_checks_permission_and_only_legacy_secret_paste_keeps_unicode_separators() {
        use crate::keys::{KeyName, Mods};
        let mut f = TextField::new();
        let mut clipboard = Fake {
            text: Some("a\u{2028}b\u{2029}c".to_owned()),
            ..Fake::default()
        };
        let mods = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let key = KeyName::Char('v');
        f.set_text("kept");
        f.select_all();
        assert_eq!(
            f.handle_key_with(&key, mods, None, false, &mut clipboard),
            Some(false)
        );
        assert_eq!(clipboard.reads, 0);
        assert_eq!(f.text(), "kept");
        f.set_secret(true);
        assert_eq!(
            f.handle_key_with(&key, mods, None, true, &mut clipboard),
            Some(true)
        );
        assert_eq!(clipboard.reads, 1);
        assert_eq!(f.text(), "a\u{2028}b\u{2029}c");
        f.set_secret(false);
        f.set_text("kept");
        f.select_all();
        assert_eq!(
            f.handle_key_with(&key, mods, None, true, &mut clipboard),
            Some(true)
        );
        assert_eq!(f.text(), "a b c");
        f.select_all();
        assert!(f.apply_with(Paste, &mut clipboard));
        assert_eq!(f.text(), "a b c");
        assert_eq!(clipboard.reads, 3);
    }

    #[test]
    fn secret_keyboard_paste_takes_the_first_line_even_with_a_large_tail() {
        let mut f = TextField::new();
        f.set_secret(true);
        f.set_text("old");
        f.select_all();
        let mut clipboard = Fake {
            text: Some(format!("key\r\n{}", "x".repeat(MAX_PASTE_BYTES))),
            ..Fake::default()
        };
        assert_eq!(ctrl(&mut f, 'v', &mut clipboard), Some(true));
        assert_eq!(clipboard.requested_limit, None);
        assert_eq!(f.text(), "key");
        assert!(!f.has_selection());
    }

    #[test]
    fn secret_keyboard_paste_replaces_the_selection_with_a_cleaned_empty_line() {
        let mut f = TextField::new();
        f.set_secret(true);
        f.set_text("old");
        f.select_all();
        let mut clipboard = Fake {
            text: Some("\x01\x7f\nignored".to_owned()),
            ..Fake::default()
        };
        assert_eq!(ctrl(&mut f, 'v', &mut clipboard), Some(true));
        assert!(f.is_empty());
        assert!(!f.has_selection());
    }

    #[test]
    fn non_secret_keyboard_paste_takes_the_first_line_even_with_a_large_tail() {
        let mut f = TextField::new();
        f.set_text("old");
        f.select_all();
        let mut clipboard = Fake {
            text: Some(format!("first\r\n{}", "x".repeat(MAX_PASTE_BYTES))),
            ..Fake::default()
        };
        assert!(!f.apply_with(Paste, &mut clipboard));
        assert_eq!(f.selected_text(), "old");
        assert_eq!(clipboard.requested_limit, Some(MAX_PASTE_BYTES));
        assert_eq!(ctrl(&mut f, 'v', &mut clipboard), Some(true));
        assert_eq!(clipboard.requested_limit, None);
        assert_eq!(f.text(), "first");
        assert!(!f.has_selection());
    }

    #[test]
    fn non_secret_keyboard_paste_replaces_the_selection_with_a_cleaned_empty_first_line() {
        use crate::textfield::EditKey;

        let mut f = TextField::new();
        f.set_text("keep-old");
        for _ in 0..3 {
            f.key(EditKey::Left, false, true);
        }
        assert_eq!(f.selected_text(), "old");
        let mut clipboard = Fake {
            text: Some("\x01\x7f\nignored".to_owned()),
            ..Fake::default()
        };
        assert_eq!(ctrl(&mut f, 'v', &mut clipboard), Some(true));
        assert_eq!(f.text(), "keep-");
        assert!(!f.has_selection());
    }

    #[test]
    fn secret_keyboard_copy_and_cut_leave_the_clipboard_and_selection_alone() {
        let mut f = TextField::new();
        f.set_secret(true);
        f.set_text("secret");
        f.select_all();
        let mut clipboard = Fake {
            text: Some("outside".to_owned()),
            ..Fake::default()
        };
        for key in ['c', 'x'] {
            assert_eq!(ctrl(&mut f, key, &mut clipboard), Some(false));
            assert_eq!(clipboard.writes, 0);
            assert_eq!(clipboard.text.as_deref(), Some("outside"));
            assert_eq!(f.text(), "secret");
            assert_eq!(f.selected_text(), "secret");
        }
    }

    fn ctrl(f: &mut TextField, key: char, clipboard: &mut Fake) -> Option<bool> {
        use crate::keys::{KeyName, Mods};
        let mods = Mods {
            ctrl: true,
            ..Mods::default()
        };
        f.handle_key_with(&KeyName::Char(key), mods, None, true, clipboard)
    }
}
