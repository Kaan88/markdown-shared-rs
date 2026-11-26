//! Debug output redacts secret values and unlabelled IME text.

use super::{Preedit, TextField};
use std::fmt;

impl fmt::Debug for TextField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("TextField");
        d.field("secret", &self.secret);
        if self.secret {
            d.field("text", &"[redacted]");
        } else {
            d.field("text", &self.text)
                .field("caret", &self.caret)
                .field("anchor", &self.anchor)
                .field("preedit", &self.preedit);
        }
        d.finish_non_exhaustive()
    }
}

impl fmt::Debug for Preedit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Preedit")
            .field("text", &"[redacted]")
            .finish_non_exhaustive()
    }
}
