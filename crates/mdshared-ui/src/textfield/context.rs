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

trait Clipboard {
    fn read(&mut self) -> Option<String>;
    fn write(&mut self, text: &str) -> bool;
}

struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Option<String> {
        platform::clipboard_text()
    }

    fn write(&mut self, text: &str) -> bool {
        platform::set_clipboard_text(text)
    }
}

impl TextField {
    /// The four items for a [`crate::menu::Menu`] or [`crate::submenu::Cascade`]. Call when
    /// opening the menu so the enabled state reflects the current selection and clipboard.
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
        let can_copy = !self.secret && self.has_selection();
        let can_paste = !self.secret && paste_text(clipboard.read().as_deref()).is_some();
        vec![
            Item::new(Cut, "Cut").enabled(can_copy),
            Item::new(Copy, "Copy").enabled(can_copy),
            Item::new(Paste, "Paste").enabled(can_paste),
            Item::new(SelectAll, "Select all").enabled(!self.is_empty()),
        ]
    }

    fn apply_with(&mut self, action: ContextAction, clipboard: &mut impl Clipboard) -> bool {
        use ContextAction::*;
        match action {
            Cut | Copy if !self.secret && self.has_selection() => {
                clipboard.write(self.selected_text()) && (action == Copy || self.delete_selection())
            }
            Paste if !self.secret => {
                paste_text(clipboard.read().as_deref()).is_some_and(|text| self.insert(text))
            }
            SelectAll if !self.is_empty() => {
                self.select_all();
                true
            }
            _ => false,
        }
    }
}

fn paste_text(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !super::clean(text).is_empty())
}

#[cfg(test)]
mod tests {
    use super::{ContextAction::*, *};

    #[derive(Default)]
    struct Fake {
        text: Option<String>,
        reads: usize,
        writes: usize,
        fail_write: bool,
    }

    impl Clipboard for Fake {
        fn read(&mut self) -> Option<String> {
            self.reads += 1;
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
    fn menu_rows_follow_selection_and_usable_clipboard_text() {
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
            assert_eq!(enabled(&f, &mut clipboard), [Cut, Copy, SelectAll]);
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
        assert_eq!(f.selected_text(), "secret");
        assert_eq!((clipboard.reads, clipboard.writes), (0, 0));
        // The public system adapters are safe to call in the secret case as well.
        assert!(!f.context_menu()[2].enabled);
        assert!(!f.apply_context_action(Paste));
    }
}
