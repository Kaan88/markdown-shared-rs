//! Links the final page's task events back to decoded Markdown. A visible literal
//! `$$` makes writeback unsafe. The caller sends rendered prose, not math or code.
//! This does not change the page or open or write files.

use crate::model::Document;
use crate::tasks::{Marker, task_spans};
use memchr::{memchr_iter, memchr3_iter};

pub(super) struct Parsed {
    pub doc: Document,
    /// Where the original body starts and where its rendered boxes were written.
    pub tasks: Option<(usize, Vec<Marker>)>,
}

#[derive(Clone, Copy)]
pub(super) struct TaskSource<'a> {
    body: &'a str,
    body_at: usize,
    head_lines: u32,
}

/// Only the events of the final page parse are used. The caller starts with empty
/// tracking for every parse attempt and discards it with any rejected attempt.
pub(super) struct TaskEvents {
    body_at: usize,
    markers: Vec<(usize, usize, bool)>,
    literal_dollars: bool,
    // The block and rendered offset after a lone dollar; the next text event may finish the pair.
    dollar_tail: Option<(u32, usize)>,
}

impl TaskEvents {
    pub(super) fn marker(&mut self, span: usize, end: usize, checked: bool) {
        self.markers.push((span, end, checked));
    }

    pub(super) fn text(&mut self, source: usize, block: u32, start: usize, text: &str) {
        if self.literal_dollars || source < self.body_at || text.is_empty() {
            return;
        }
        self.literal_dollars = text.contains("$$")
            || (text.starts_with('$') && self.dollar_tail == Some((block, start)));
        self.dollar_tail = text.ends_with('$').then_some((block, start + text.len()));
    }

    /// An image, footnote reference or checkbox occupies no bytes in the model's text,
    /// but still separates the text before and after it on the page.
    pub(super) fn atom(&mut self) {
        self.dollar_tail = None;
    }
}

impl<'a> TaskSource<'a> {
    pub(super) fn new(body: &'a str, body_at: usize) -> Self {
        Self {
            body,
            body_at,
            head_lines: 0,
        }
    }

    pub(super) fn skip_front(&mut self, used: usize, replacement_lines: u32) {
        self.body = &self.body[used..];
        self.body_at += used;
        self.head_lines = replacement_lines;
    }

    pub(super) fn events(self, rendered: &str) -> Option<TaskEvents> {
        let body_at = match self.head_lines {
            0 => 0,
            n => memchr_iter(b'\n', rendered.as_bytes()).nth(n as usize - 1)? + 1,
        };
        Some(TaskEvents {
            body_at,
            markers: Vec::new(),
            literal_dollars: false,
            dollar_tail: None,
        })
    }

    pub(super) fn resolve(
        self,
        rendered: &str,
        doc: &Document,
        events: TaskEvents,
    ) -> Option<(usize, Vec<Marker>)> {
        // A separate footnote list and foldable details need not lay out as the source
        // blocks do. Do not guess which rendered block can be written in a document that
        // still shows literal dollars. Complete equations never take this path.
        if events.literal_dollars {
            return None;
        }
        // The finished document is the authority on which events actually made a box.
        if !task_spans(doc)
            .map(|(_, span, checked)| (span, checked))
            .eq(events
                .markers
                .iter()
                .map(|&(span, _, checked)| (span, checked)))
        {
            return None;
        }
        let mut markers = Vec::with_capacity(events.markers.len());
        if events.markers.is_empty() {
            return Some((self.body_at, markers));
        }
        // The body rewrites resize bytes but leave every `]`, CR and LF in place relative to
        // one another. Walk them together, including bare CR, instead of relying on a block's
        // LF-only line number or on a byte column that a rewrite could shift.
        let rendered = &rendered.as_bytes()[events.body_at..];
        let original = self.body.as_bytes();
        let mut pairs = memchr3_iter(b']', b'\r', b'\n', rendered)
            .zip(memchr3_iter(b']', b'\r', b'\n', original));
        for (_, end, checked) in events.markers {
            let close = end.checked_sub(events.body_at + 1)?;
            loop {
                let (shown, source) = pairs.next()?;
                if rendered[shown] != original[source] || shown > close {
                    return None;
                }
                if shown == close {
                    let marker = original.get(source.checked_sub(2)?..=source)?;
                    if !matches!(
                        (marker, checked),
                        ([b'[', b' ', b']'], false) | ([b'[', b'x' | b'X', b']'], true)
                    ) {
                        return None;
                    }
                    markers.push(Marker {
                        at: self.body_at + source - 1,
                        checked,
                    });
                    break;
                }
            }
        }
        Some((self.body_at, markers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_dollars_in_rendered_text_events_count_as_one_literal_pair() {
        let mut events = TaskSource::new("", 0).events("").unwrap();
        events.text(0, 1, 0, "$");
        events.text(1, 1, 1, "$");
        assert!(events.literal_dollars);
    }

    #[test]
    fn an_atom_or_a_block_boundary_keeps_two_dollars_apart() {
        for atom in [false, true] {
            let mut events = TaskSource::new("", 0).events("").unwrap();
            events.text(0, 1, 0, "$");
            if atom {
                events.atom();
            }
            events.text(1, if atom { 1 } else { 2 }, 1, "$");
            assert!(!events.literal_dollars);
        }
    }
}
