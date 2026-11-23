//! Splitting by AI (`docs/ai-library-plan.md` 5.2, phase 4b): what is sent, and how the answer
//! becomes a [`Plan`].
//!
//! **What is sent** is an outline, never the document: each heading in full (to
//! [`OUTLINE_HEADING_MAX`] bytes) and the first [`OUTLINE_PARAGRAPH_MAX`] bytes of each
//! paragraph, cut at a character, each after its 1-based line number ([`outline`]). Over
//! [`OUTLINE_MAX`] bytes in that form, only the headings are sent; if even that is over, or there
//! are no headings, the split is refused ([`TooLong`]). [`SYSTEM`] and [`prompt`] are the
//! request; the model is given no path and returns line numbers.
//!
//! **The answer** is read by [`read`] and turned into a plan by [`plan`], through the eleven
//! checks of 5.2 in their order. None of them can panic (the release build of an app that uses
//! this aborts on a panic): every field is optional when read, every number is range-checked
//! before it is used, and the JSON reader is the depth-bounded one of the run records. What a
//! model's words can reach is a title and a section name, and both go through the same cleaning
//! as the names the owner types (6.6).

use super::{Fence, Fix, Heading, Lines, Note, Plan, base, clean, line_count};
use crate::runs::{Json, parse_json};
use std::ops::Range;
use std::sync::Arc;

/// The most an outline may be: one request's text, as a rewrite's (6.4).
pub const OUTLINE_MAX: usize = 200_000;
/// A heading is sent to this many bytes.
pub const OUTLINE_HEADING_MAX: usize = 200;
/// A paragraph is sent to this many bytes: its start.
pub const OUTLINE_PARAGRAPH_MAX: usize = 80;
/// The most notes kept from one answer (6.4); more are dropped and counted.
pub const ANSWER_MAX_NOTES: usize = 200;

/// The rules a split is asked with. Short, because Claude Code takes it on the command line.
pub const SYSTEM: &str = "You split a document into notes. You get its headings and the start \
of each paragraph, each with its line number. Answer with one JSON object and nothing else: \
{\"version\":1,\"notes\":[{\"from\":N,\"to\":N,\"title\":\"…\",\"sections\":[\"…\"]}]}. `from` \
and `to` are line numbers from the list, inclusive. Do not include any text of the document. \
Notes must not overlap. Lines that belong in no note are left out.";

// ---- what is sent ----------------------------------------------------------------------------------

/// The outline of a document, as it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    /// One line per heading or paragraph: its 1-based line number, a space, its start.
    pub text: String,
    /// The paragraphs did not fit, so only the headings are in it.
    pub headings_only: bool,
    /// The document's lines, as the request names them.
    pub lines: usize,
}

/// Why a document cannot be split by AI: even its outline is over [`OUTLINE_MAX`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLong {
    pub lines: usize,
    pub headings: usize,
}

impl TooLong {
    /// The sentence the plan card says.
    pub fn text(&self) -> String {
        let lines = thousands(self.lines);
        if self.headings == 0 {
            format!(
                "Too long to split by AI: {lines} lines and no headings. Split it at headings by hand first, or in two."
            )
        } else {
            format!(
                "Too long to split by AI: {lines} lines, and its {} headings alone are over 200 KB. Split it at headings by hand first, or in two.",
                thousands(self.headings)
            )
        }
    }
}

/// 31,000.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `text` as one line of the outline: control characters as spaces, the end trimmed, cut to
/// `max` bytes at a character.
fn cut(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len().saturating_add(c.len_utf8()) > max {
            break;
        }
        out.push(c);
    }
    out.trim_end().to_string()
}

fn is_blank(line: &str) -> bool {
    line.trim_matches([' ', '\t']).is_empty()
}

/// A setext heading's underline: a run of `=` or of `-`, nothing else but blanks.
fn is_underline(line: &str) -> bool {
    let t = line.trim_matches([' ', '\t']);
    !t.is_empty() && (t.bytes().all(|b| b == b'=') || t.bytes().all(|b| b == b'-'))
}

/// The outline of `doc` (5.2): its headings (`headings`, as the application found them) and the
/// start of each paragraph, with line numbers; only the headings when that is over `max` bytes;
/// refused when even they are, or when there are none and the paragraphs are over. A fenced code
/// block counts as one paragraph, its opening line sent; a setext heading's underline is not
/// sent. One pass over the lines; the text is never longer than `max`.
pub fn outline(doc: &dyn Lines, headings: &[Heading], max: usize) -> Result<Outline, TooLong> {
    let total = line_count(doc);
    let mut heads: Vec<usize> = headings
        .iter()
        .map(|h| h.line)
        .filter(|&l| l < total)
        .collect();
    heads.sort_unstable();
    heads.dedup();
    let mut full = String::new();
    let mut full_over = false;
    let mut only = String::new();
    let mut only_over = false;
    let mut fence = Fence::default();
    let mut body = String::new();
    // What the line before was: blank (or none), a heading, a paragraph's line, or a fence's.
    let mut after_break = true;
    let mut after_heading = false;
    for line in 0..total {
        doc.line_text(line, &mut body);
        let was_open = fence.open.is_some();
        let code = fence.line(&body);
        let heading = !was_open && heads.binary_search(&line).is_ok();
        let underline = !code && after_heading && is_underline(&body);
        let entry = if heading {
            Some(cut(&body, OUTLINE_HEADING_MAX))
        } else if code {
            // The opening line of a fence starts a block; the lines inside it do not.
            (!was_open).then(|| cut(&body, OUTLINE_PARAGRAPH_MAX))
        } else if is_blank(&body) || underline {
            None
        } else if after_break || after_heading {
            Some(cut(&body, OUTLINE_PARAGRAPH_MAX))
        } else {
            None
        };
        if let Some(text) = entry {
            let item = format!("{} {text}\n", line.saturating_add(1));
            if !full_over {
                if full.len().saturating_add(item.len()) > max {
                    full_over = true;
                    full = String::new();
                } else {
                    full.push_str(&item);
                }
            }
            if heading && !only_over {
                if only.len().saturating_add(item.len()) > max {
                    only_over = true;
                    only = String::new();
                } else {
                    only.push_str(&item);
                }
            }
        }
        after_heading = heading;
        // A closing fence ends its block: the next line starts another.
        after_break = is_blank(&body) || heading || underline || (code && fence.open.is_none());
    }
    if !full_over {
        return Ok(Outline {
            text: full,
            headings_only: false,
            lines: total,
        });
    }
    if !heads.is_empty() && !only_over {
        return Ok(Outline {
            text: only,
            headings_only: true,
            lines: total,
        });
    }
    Err(TooLong {
        lines: total,
        headings: heads.len(),
    })
}

/// The request's text: the instruction, then the outline inside a `<document>` that names the
/// document and its lines. The name loses the characters that would end the attribute or the tag.
pub fn prompt(instruction: &str, name: &str, outline: &Outline) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if matches!(c, '"' | '<' | '>') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!(
        "<instruction>{}</instruction>\n<document name=\"{name}\" lines=\"{}\">\n{}</document>",
        instruction.trim(),
        outline.lines,
        outline.text
    )
}

/// The request once more, after an answer that could not be read: the fault named (check 2).
pub fn retry_prompt(prompt: &str, fault: Fault) -> String {
    format!(
        "{prompt}\n\nYour last answer could not be read: {}. Answer with the one JSON object only.",
        fault.text()
    )
}

// ---- what comes back ---------------------------------------------------------------------------------

/// Why an answer could not be read (check 2): said once to the model in a retry, then to the
/// owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// There is no balanced `{…}` in it.
    NoObject,
    /// The first one is not JSON.
    NotJson,
    /// `version` is there and is not 1.
    Version,
    /// There is no `notes` array.
    NoNotes,
}

impl Fault {
    pub fn text(self) -> &'static str {
        match self {
            Fault::NoObject => "it held no JSON object",
            Fault::NotJson => "its JSON object did not parse",
            Fault::Version => "its \"version\" was not 1",
            Fault::NoNotes => "it had no \"notes\" array",
        }
    }
}

/// One note as the model asked for it, before any check but the reading: every field optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asked {
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub title: String,
    pub sections: Vec<String>,
}

/// An answer that was read: its first [`ANSWER_MAX_NOTES`] notes, and how many more there were.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub notes: Vec<Asked>,
    /// Notes past the first [`ANSWER_MAX_NOTES`], dropped (check 3).
    pub over: usize,
}

/// The first balanced `{…}` in `text`, braces inside JSON strings not counted (check 1).
pub fn first_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let b = text.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    let mut i = start;
    while let Some(&c) = b.get(i) {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth = depth.saturating_add(1),
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return text.get(start..=i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Reads a model's answer (checks 1 to 3): the first balanced object, which must parse, with a
/// `version` of 1 or none, and a `notes` array, of which the first [`ANSWER_MAX_NOTES`] are kept.
/// An entry that is not an object is kept as an [`Asked`] with no numbers, so check 4 drops and
/// counts it.
pub fn read(text: &str) -> Result<Answer, Fault> {
    let object = first_object(text).ok_or(Fault::NoObject)?;
    let json = parse_json(object).ok_or(Fault::NotJson)?;
    if !matches!(json, Json::Obj(_)) {
        return Err(Fault::NotJson);
    }
    if let Some(v) = json.get("version")
        && v.num() != Some(1.0)
    {
        return Err(Fault::Version);
    }
    let notes = json
        .get("notes")
        .and_then(Json::arr)
        .ok_or(Fault::NoNotes)?;
    let over = notes.len().saturating_sub(ANSWER_MAX_NOTES);
    let notes = notes
        .iter()
        .take(ANSWER_MAX_NOTES)
        .map(|n| Asked {
            from: n.get("from").and_then(Json::num),
            to: n.get("to").and_then(Json::num),
            title: n
                .get("title")
                .and_then(Json::str)
                .unwrap_or_default()
                .to_string(),
            sections: n
                .get("sections")
                .and_then(Json::arr)
                .map(|a| {
                    a.iter()
                        .filter_map(Json::str)
                        .take(64)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    Ok(Answer { notes, over })
}

/// The notes of an answer the checks dropped, by reason: the line above the list says them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dropped {
    /// The notes the answer had in all.
    pub asked: usize,
    /// Past the first [`ANSWER_MAX_NOTES`] (check 3).
    pub over: usize,
    /// Line numbers that were not whole numbers, or not in the document (check 4).
    pub not_lines: usize,
    /// Left with no line by a moved boundary, an overlap or the blank lines trimmed (checks 5 to
    /// 7).
    pub emptied: usize,
}

impl Dropped {
    pub fn total(&self) -> usize {
        self.over
            .saturating_add(self.not_lines)
            .saturating_add(self.emptied)
    }

    /// "Dropped 52 of the AI's 252 notes: 50 past the first 200, 2 with lines not in the
    /// document." `None` when nothing was dropped.
    pub fn text(&self) -> Option<String> {
        if self.total() == 0 {
            return None;
        }
        let mut why: Vec<String> = Vec::new();
        if self.over > 0 {
            why.push(format!(
                "{} past the first {ANSWER_MAX_NOTES}",
                thousands(self.over)
            ));
        }
        if self.not_lines > 0 {
            why.push(format!(
                "{} with lines not in the document",
                thousands(self.not_lines)
            ));
        }
        // A note is emptied by a boundary moved to a block's edge, an overlap with the note
        // before, or its blank lines trimmed: the words say all three (audit 4, N4).
        if self.emptied > 0 {
            why.push(format!(
                "{} left with no lines of their own",
                thousands(self.emptied)
            ));
        }
        Some(format!(
            "Dropped {} of the AI\u{2019}s {} notes: {}.",
            thousands(self.total()),
            thousands(self.asked),
            why.join(", ")
        ))
    }
}

/// What kind of block a line is in, for check 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Paragraph,
    Heading,
    Code,
    Table,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Block {
    range: Range<usize>,
    kind: Kind,
}

/// A table's delimiter row: `|---|:--:|`, or `--- | ---`.
fn is_delimiter_row(line: &str) -> bool {
    let t = line.trim_matches([' ', '\t']);
    t.contains('-')
        && t.contains('|')
        && t.bytes()
            .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

/// The blocks of the document, in order: each fenced code block (its fences included), each
/// heading (with a setext underline), each table and each paragraph. Blank lines outside a fence
/// are in none.
fn blocks(doc: &dyn Lines, total: usize, heads: &[usize]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut fence = Fence::default();
    let mut body = String::new();
    let mut run: Option<(usize, bool)> = None;
    let close = |run: &mut Option<(usize, bool)>, end: usize, out: &mut Vec<Block>| {
        if let Some((start, table)) = run.take() {
            out.push(Block {
                range: start..end,
                kind: if table { Kind::Table } else { Kind::Paragraph },
            });
        }
    };
    let mut line = 0usize;
    while line < total {
        doc.line_text(line, &mut body);
        let was_open = fence.open.is_some();
        let code = fence.line(&body);
        if code {
            close(&mut run, line, &mut out);
            if was_open {
                if let Some(b) = out.last_mut().filter(|b| b.kind == Kind::Code) {
                    b.range.end = line.saturating_add(1);
                }
            } else {
                out.push(Block {
                    range: line..line.saturating_add(1),
                    kind: Kind::Code,
                });
            }
        } else if heads.binary_search(&line).is_ok() {
            close(&mut run, line, &mut out);
            let mut end = line.saturating_add(1);
            if end < total {
                doc.line_text(end, &mut body);
                if is_underline(&body) {
                    end = end.saturating_add(1);
                }
            }
            out.push(Block {
                range: line..end,
                kind: Kind::Heading,
            });
            line = end;
            continue;
        } else if is_blank(&body) {
            close(&mut run, line, &mut out);
        } else {
            match run.as_mut() {
                None => run = Some((line, false)),
                Some((start, table)) => {
                    if line == start.saturating_add(1) && is_delimiter_row(&body) {
                        *table = true;
                    }
                }
            }
        }
        line = line.saturating_add(1);
    }
    close(&mut run, total, &mut out);
    out
}

/// The block holding `line`, if one does.
fn block_at(blocks: &[Block], line: usize) -> Option<&Block> {
    let i = blocks.partition_point(|b| b.range.end <= line);
    blocks.get(i).filter(|b| b.range.start <= line)
}

/// Check 5 for a note's first line: inside a code block or a table it moves past the block's end;
/// inside a paragraph to the nearer of its edges (its start on a tie).
fn snap_start(blocks: &[Block], s: usize) -> (usize, Option<Fix>) {
    match block_at(blocks, s) {
        Some(b) if b.range.start < s => match b.kind {
            Kind::Code => (b.range.end, Some(Fix::CodeBlock)),
            Kind::Table => (b.range.end, Some(Fix::Table)),
            Kind::Paragraph | Kind::Heading => {
                let up = s - b.range.start;
                let down = b.range.end.saturating_sub(s);
                if up <= down {
                    (b.range.start, Some(Fix::Paragraph))
                } else {
                    (b.range.end, Some(Fix::Paragraph))
                }
            }
        },
        _ => (s, None),
    }
}

/// Check 5 for a note's end (exclusive): a last line inside a code block or a table moves to the
/// block's end; inside a paragraph to the nearer of its edges (its end on a tie).
fn snap_end(blocks: &[Block], e: usize) -> (usize, Option<Fix>) {
    let Some(last) = e.checked_sub(1) else {
        return (e, None);
    };
    match block_at(blocks, last) {
        Some(b) if b.range.end > e => match b.kind {
            Kind::Code => (b.range.end, Some(Fix::CodeBlock)),
            Kind::Table => (b.range.end, Some(Fix::Table)),
            Kind::Paragraph | Kind::Heading => {
                let up = e.saturating_sub(b.range.start);
                let down = b.range.end - e;
                if down <= up {
                    (b.range.end, Some(Fix::Paragraph))
                } else {
                    (b.range.start, Some(Fix::Paragraph))
                }
            }
        },
        _ => (e, None),
    }
}

/// A section name as a plan keeps it (check 9): one line, trimmed, cut to the folder length;
/// nothing when it has no text or is only dots, which would name the folder above.
fn section_name(name: &str) -> Option<String> {
    let name = clean(name, super::SPLIT_SECTION_MAX);
    let bare = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
    (!bare.is_empty()).then_some(name)
}

/// The section path `path` spelled as a path already seen is, name by name, without regard to
/// ASCII case (as the library matches a section); the first spelling wins.
fn canonical(seen: &mut Vec<Vec<String>>, path: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(path.len());
    for (i, name) in path.into_iter().enumerate() {
        let found = seen
            .iter()
            .find(|p| {
                p.len() > i
                    && p.get(..i) == Some(out.as_slice())
                    && p.get(i).is_some_and(|n| n.eq_ignore_ascii_case(&name))
            })
            .and_then(|p| p.get(i).cloned());
        out.push(found.unwrap_or(name));
    }
    if !seen.contains(&out) {
        seen.push(out.clone());
    }
    out
}

/// A line number of the answer as a whole number, or `None` (check 4).
fn whole(n: Option<f64>) -> Option<f64> {
    n.filter(|n| n.is_finite() && n.fract() == 0.0)
}

/// The plan an answer makes of `doc`, after checks 4 to 10, and what they dropped. `headings` are
/// the document's (as for [`super::plan`]); `level` is what the plan would be cut at if the owner
/// goes back to headings; `known` are the section paths the group already has, so a section the
/// model spells in another case is the one that exists (check 9). Every note starts ticked; the
/// warnings of check 11 are [`Plan::warnings`], as for any plan.
pub fn plan(
    doc: Arc<dyn Lines>,
    name: &str,
    headings: &[Heading],
    level: u8,
    answer: &Answer,
    known: &[Vec<String>],
) -> (Plan, Dropped) {
    let mut p = base(doc, name, headings, level);
    let total = p.total;
    let mut dropped = Dropped {
        asked: answer.notes.len().saturating_add(answer.over),
        over: answer.over,
        ..Dropped::default()
    };
    let blocks = blocks(
        &*p.doc,
        total,
        &p.headings.iter().map(|h| h.line).collect::<Vec<_>>(),
    );
    // Checks 4 and 5: whole numbers, in order, inside the document, then on block edges.
    struct Cut<'a> {
        range: Range<usize>,
        fix: Option<Fix>,
        asked: &'a Asked,
    }
    let mut cuts: Vec<Cut> = Vec::new();
    for asked in &answer.notes {
        let (Some(a), Some(b)) = (whole(asked.from), whole(asked.to)) else {
            dropped.not_lines += 1;
            continue;
        };
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if b < 1.0 || a > total as f64 {
            dropped.not_lines += 1;
            continue;
        }
        // Both are whole numbers inside 1..=total now, so the casts are exact.
        let first = a.max(1.0) as usize;
        let last = b.min(total as f64) as usize;
        let (start, f1) = snap_start(&blocks, first.saturating_sub(1));
        let (end, f2) = snap_end(&blocks, last);
        let range = start..end.max(start);
        if range.is_empty() {
            dropped.emptied += 1;
            continue;
        }
        let fix = match (f1, f2) {
            (Some(Fix::CodeBlock | Fix::Table), _) => f1,
            (_, Some(Fix::CodeBlock | Fix::Table)) => f2,
            _ => f1.or(f2),
        };
        cuts.push(Cut { range, fix, asked });
    }
    // Check 6: in order; a later note that overlaps starts after the one before.
    cuts.sort_by_key(|c| c.range.start);
    let mut prev_end = 0usize;
    let mut seen: Vec<Vec<String>> = known.to_vec();
    let mut notes: Vec<Note> = Vec::with_capacity(cuts.len());
    for mut c in cuts {
        if c.range.start < prev_end {
            c.range.start = prev_end;
            c.fix = Some(Fix::Overlap);
        }
        // Check 7: blank lines at the ends are trimmed.
        let range = p.trim(c.range.clone());
        if range.is_empty() {
            dropped.emptied += 1;
            continue;
        }
        prev_end = range.end;
        // Check 9: three names at most, each cleaned; the known spelling wins.
        let sections: Vec<String> = c
            .asked
            .sections
            .iter()
            .filter_map(|s| section_name(s))
            .take(super::SPLIT_MAX_SECTIONS)
            .collect();
        let sections = canonical(&mut seen, sections);
        // Check 8: a heading's words are the title; otherwise the model's, cleaned.
        let mut note = p.note_for(range, sections, true);
        if note.title_added {
            let title = clean(&c.asked.title, super::SPLIT_TITLE_MAX);
            if !title.is_empty() {
                note.title = title;
            }
        }
        note.fix = c.fix;
        notes.push(note);
    }
    // Check 10: two notes of one title and section are kept; the screen names the second file.
    p.notes = notes;
    for i in 0..p.notes.len() {
        p.refresh_bytes(i);
    }
    (p, dropped)
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
