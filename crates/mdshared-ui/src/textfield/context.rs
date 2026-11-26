//! The field's clipboard actions. The application owns the popup and passes back its chosen item.

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

/// Maximum UTF-8 byte length a field will accept from the clipboard, before single-line cleaning.
pub const MAX_PASTE_BYTES: usize = 64 * 1024;

pub(super) trait Clipboard {
    fn may_have_text(&mut self) -> bool;
    fn read(&mut self, limit: usize) -> Option<String>;
    fn write(&mut self, text: &str) -> bool;
}

pub(super) struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn may_have_text(&mut self) -> bool {
        platform::clipboard_may_have_text()
    }

    fn read(&mut self, limit: usize) -> Option<String> {
        platform::clipboard_text_bounded(limit)
    }

    fn write(&mut self, text: &str) -> bool {
        platform::set_clipboard_text(text)
    }
}

impl TextField {
    /// The four items for a [`crate::menu::Menu`] or [`crate::submenu::Cascade`]. Call when
    /// opening the menu. Paste is enabled when text is advertised or the platform cannot
    /// cheaply check. The clipboard payload is not read until Paste is chosen.
    /// A secret field never consults the clipboard; only Select all can be enabled.
    pub fn context_menu(&self) -> Vec<Item<ContextAction>> {
        self.context_menu_with(&mut SystemClipboard)
    }

    /// Applies an item chosen from the menu, checking again that it is allowed. Returns true
    /// when the field was edited, a selection was made, or text was copied successfully.
    /// A failed clipboard write never deletes the selection. Secret fields refuse all three
    /// clipboard actions. This does not change the existing keyboard editing methods.
    pub fn apply_context_action(&mut self, action: ContextAction) -> bool {
        self.apply_with(action, &mut SystemClipboard)
    }

    fn context_menu_with(&self, clipboard: &mut impl Clipboard) -> Vec<Item<ContextAction>> {
        use ContextAction::*;
        let can_copy = self.selection_for_export().is_some();
        let can_paste = !self.secret && clipboard.may_have_text();
        vec![
            Item::new(Cut, "Cut").enabled(can_copy),
            Item::new(Copy, "Copy").enabled(can_copy),
            Item::new(Paste, "Paste").enabled(can_paste),
            Item::new(SelectAll, "Select all").enabled(!self.is_empty()),
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
            Paste => self.paste_with(true, false, clipboard),
            SelectAll if !self.is_empty() => {
                self.select_all();
                true
            }
            _ => false,
        }
    }

    pub(super) fn paste_with(
        &mut self,
        allowed: bool,
        first_line: bool,
        clipboard: &mut impl Clipboard,
    ) -> bool {
        if !allowed || self.secret {
            return false;
        }
        let Some(text) = clipboard
            .read(MAX_PASTE_BYTES)
            .filter(|text| text.len() <= MAX_PASTE_BYTES)
        else {
            return false;
        };
        let text = if first_line {
            text.lines().next().unwrap_or("")
        } else {
            text.as_str()
        };
        let clean = super::clean(text);
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

        fn read(&mut self, limit: usize) -> Option<String> {
            self.reads += 1;
            self.requested_limit = Some(limit);
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
    fn a_secret_menu_cannot_read_write_or_edit_through_the_clipboard() {
        let mut f = TextField::new();
        let mut clipboard = Fake {
            text: Some("outside".to_owned()),
            ..Fake::default()
        };
        f.set_text("secret");
        f.select_all();
        f.set_secret(true);
        assert_eq!(enabled(&f, &mut clipboard), [SelectAll]);
        for action in [Cut, Copy, Paste] {
            assert!(!f.apply_with(action, &mut clipboard));
        }
        assert!(f.has_selection());
        assert_eq!(f.selection_for_export(), None);
        assert_eq!(
            (clipboard.probes, clipboard.reads, clipboard.writes),
            (0, 0, 0)
        );
        // The public system adapters are safe to call in the secret case as well.
        assert!(!f.context_menu()[2].enabled);
        assert!(!f.apply_context_action(Paste));
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
    fn keyboard_checks_permission_before_reading_and_both_pastes_clean_unicode_separators() {
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
        f.set_secret(true);
        assert_eq!(
            f.handle_key_with(&key, mods, None, true, &mut clipboard),
            Some(false)
        );
        assert_eq!(clipboard.reads, 0);
        assert_eq!(f.text(), "kept");
        f.set_secret(false);
        assert_eq!(
            f.handle_key_with(&key, mods, None, true, &mut clipboard),
            Some(true)
        );
        assert_eq!(f.text(), "a b c");
        f.select_all();
        assert!(f.apply_with(Paste, &mut clipboard));
        assert_eq!(f.text(), "a b c");
        assert_eq!(clipboard.reads, 2);
    }
}
