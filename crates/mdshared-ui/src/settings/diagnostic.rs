//! Settings diagnostics leave raw text values to the owner-facing value API.

use super::{Row, Value};
use std::fmt;

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
            Self::Choice(v) => f.debug_tuple("Choice").field(v).finish(),
            Self::Number(v) => f.debug_tuple("Number").field(v).finish(),
            // Value::Text has no indication of whether its originating row is secret.
            Self::Text(_) => f.debug_tuple("Text").field(&"[redacted]").finish(),
        }
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header(s) => f.debug_tuple("Header").field(s).finish(),
            Self::Toggle {
                id,
                label,
                help,
                value,
            } => f
                .debug_struct("Toggle")
                .field("id", id)
                .field("label", label)
                .field("help", help)
                .field("value", value)
                .finish(),
            Self::Choice {
                id,
                label,
                help,
                options,
                value,
            } => f
                .debug_struct("Choice")
                .field("id", id)
                .field("label", label)
                .field("help", help)
                .field("options", options)
                .field("value", value)
                .finish(),
            Self::Number {
                id,
                label,
                help,
                value,
                min,
                max,
                step,
                unit,
            } => f
                .debug_struct("Number")
                .field("id", id)
                .field("label", label)
                .field("help", help)
                .field("value", value)
                .field("min", min)
                .field("max", max)
                .field("step", step)
                .field("unit", unit)
                .finish(),
            Self::Text {
                id,
                label,
                help,
                value,
                placeholder,
                secret,
            } => f
                .debug_struct("Text")
                .field("id", id)
                .field("label", label)
                .field("help", help)
                .field(
                    "value",
                    &if *secret {
                        "[redacted]"
                    } else {
                        value.as_str()
                    },
                )
                .field("placeholder", placeholder)
                .field("secret", secret)
                .finish(),
            Self::Action { id, label, button } => f
                .debug_struct("Action")
                .field("id", id)
                .field("label", label)
                .field("button", button)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textfield::{Preedit, TextField};

    #[test]
    fn settings_and_field_diagnostics_do_not_expose_secret_values() {
        let token = "a-secret-token";
        let mut row = Row::text(7, "API key", token);
        assert!(format!("{row:?}").contains(token));
        if let Row::Text { secret, .. } = &mut row {
            *secret = true;
        }
        assert!(!format!("{row:#?}").contains(token));
        assert!(!format!("{:?}", row.value()).contains(token));
        let mut field = TextField::new();
        field.set_text(token);
        field.select_all();
        assert_eq!(field.selection_for_export(), Some(token));
        field.set_secret(true);
        field.set_preedit(Preedit::new(token.into(), None));
        assert_eq!(field.selection_for_export(), None);
        assert_eq!(field.text(), token);
        assert!(!format!("{field:#?} {:?}", field.preedit()).contains(token));
    }
}
