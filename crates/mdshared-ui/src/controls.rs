//! Everyday controls: push buttons in four kinds, an icon button, a button that opens a menu, a
//! split button, and a framed text field. This is the first slice of roadmap K4; the check box,
//! radio group, chip, badge, card, divider and avatar are still to come.
//!
//! The pieces are models with no painter in them and free `draw_*` functions that change no
//! state, as every widget here is. The caller owns the pointer, the keyboard, the focus model
//! and the menu: it asks the model where the control is ([`button::Button::rect`],
//! [`menu_button::SplitButton::halves`], [`field::FramedField::clear_rect`]), passes the flags
//! it knows to a `draw` function ([`button::Look`], [`field::Look`]), and turns a press or a key
//! into what happened ([`button::key`], [`menu_button::SplitButton::key`],
//! [`field::FramedField::press`]).
//!
//! The four kinds of push button are:
//!
//! - primary: the accent fill, for the one action a view leads with;
//! - secondary: a quiet fill with an edge, the ordinary button;
//! - subtle: no fill until the pointer is over it, for toolbars and rows;
//! - destructive: the danger fill, for an action that removes or discards.
//!
//! Each draws at rest, hovered, pressed, focused (a ring through [`crate::focus`]) and disabled,
//! and can be marked as the default button (the one Enter presses) with
//! [`button::Look::default`]. A disabled button of any kind is the one quiet frame, as
//! [`crate::overlay::draw_dialog_button_disabled`] draws it: the kind no longer shows.
//!
//! The framed text field is built on [`crate::textfield::TextField`]: it owns the text and hands
//! the caret's blink to the caller, as the field does; it adds the frame, an optional leading
//! icon, a clear button while there is text, and an error message under the box.
//!
//! Colours come from the [`Palette`](crate::theme::Palette) passed in, so light and dark both
//! work. Motion is deliberately absent: a state is drawn on the frame it is given, and no frame
//! is asked for. Also deliberately not here: the pointer's tracking, the focus model, opening a
//! menu ([`crate::menu::Menu::around`] does the placing; [`menu_button::menu_under`] calls it),
//! the tooltip's timing ([`crate::tooltip`] has that), the clipboard and the keyboard focus of
//! a text field, and the accessibility nodes, which the application builds with
//! [`crate::a11y::button`] and [`crate::a11y::text_input`] from the same models.
//!
//! ```text
//! focus.begin_frame();
//! focus.add(Ctl::Save);
//! let r = save.rect(&mut p, 24.0, 72.0);
//! button::draw(&mut p, pal, r, save.text(), save.kind(), button::Look { focused: focus.is_focused(Ctl::Save), ..Default::default() });
//! if button::hit(r, x, y) && button::key(&key, mods) { save_it(); }
//! focus.end_frame();
//! ```

pub mod button;
pub mod field;
pub mod icon_button;
pub mod menu_button;
