//! Ticking a task-list box in the file it came from.
//!
//! Match the page parser's checkboxes to task events at the same source bytes. The parser's
//! rewrites and the loader's replacement characters are not file bytes. Only after both
//! mappings agree may the caller flip one byte between the brackets.

use crate::limits::MAX_FILE_BYTES;
use crate::model::{Document, SpanKind};
use mdshared_app::files;
use pulldown_cmark::{Event, Parser};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

/// A `[ ]` or `[x]` task marker in a file: where its middle byte is and what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub at: usize,
    pub checked: bool,
}

/// Why a box was not ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleError {
    /// The file is UTF-16, where one mark is not one byte.
    Encoding,
    /// The boxes in the file are not the boxes on the page: it changed on disk, or it holds a
    /// box that cannot be traced back to a source marker.
    Mismatch,
}

/// Every task box the document shows, in document order: its block and whether it is checked.
pub fn document_boxes(doc: &Document) -> Vec<(u32, bool)> {
    task_spans(doc)
        .map(|(block, _, checked)| (block, checked))
        .collect()
}

/// Each box's block, span index and state, in document order.
pub(crate) fn task_spans(doc: &Document) -> impl Iterator<Item = (u32, usize, bool)> + '_ {
    doc.blocks
        .iter()
        .enumerate()
        .flat_map(|(block_index, block)| {
            let spans = block.spans.usize();
            doc.spans[spans.clone()]
                .iter()
                .enumerate()
                .filter_map(move |(offset, span)| match span.kind {
                    SpanKind::Checkbox { checked } => {
                        Some((block_index as u32, spans.start + offset, checked))
                    }
                    _ => None,
                })
        })
}

/// The original file bytes of the task markers, in page order. Invalid UTF-8 elsewhere in the
/// file is replaced as it is for the page. Returns no markers if a checkbox on the page cannot
/// be matched to a unique task event at its exact source byte, if the file is UTF-16, or
/// when the page parser emits literal `$$` in prose. Complete equations, code and text
/// the parser discards do not trigger that conservative refusal.
pub fn markers(bytes: &[u8]) -> Vec<Marker> {
    matched_markers(bytes).unwrap_or_default()
}

fn matched_markers(bytes: &[u8]) -> Option<Vec<Marker>> {
    if bytes.len() > MAX_FILE_BYTES || files::utf16_marked(bytes) {
        return None;
    }
    let (decoded, invalid) = files::decode(bytes);
    let (body_at, mut markers) = crate::parse::source_tasks(&decoded)?;
    if markers.is_empty() {
        return Some(markers);
    }
    let body = &decoded[body_at..];
    let events = Parser::new_ext(body, crate::parse::options()).into_offset_iter();
    match_raw(body_at, &markers, events)?;
    let data = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let bom = bytes.len() - data.len();
    if invalid {
        map_lossy_offsets(data, &mut markers)?;
    }
    for marker in &mut markers {
        marker.at += bom;
    }
    Some(markers)
}

fn match_raw<'a>(
    body_at: usize,
    markers: &[Marker],
    events: impl IntoIterator<Item = (Event<'a>, Range<usize>)>,
) -> Option<()> {
    let mut matched = 0usize;
    for (event, range) in events {
        let Event::TaskListMarker(checked) = event else {
            continue;
        };
        let at = body_at + range.end.checked_sub(2)?;
        if let Some(last) = matched.checked_sub(1).and_then(|i| markers.get(i))
            && at <= last.at
        {
            return None;
        }
        if let Some(target) = markers.get(matched) {
            if at > target.at || at == target.at && checked != target.checked {
                return None;
            }
            if at == target.at {
                matched += 1;
            }
        }
    }
    (matched == markers.len()).then_some(())
}

/// Walk the same valid and invalid chunks as the loader's lossy decoder. A replacement uses
/// three decoded bytes even when it stands for one or two file bytes. Marker bytes must be
/// in the valid chunks. This walks the map in order, without allocating a table per byte.
fn map_lossy_offsets(data: &[u8], markers: &mut [Marker]) -> Option<()> {
    let mut file = 0;
    let mut decoded = 0;
    let mut markers = markers.iter_mut().peekable();
    for chunk in data.utf8_chunks() {
        let valid_end = decoded + chunk.valid().len();
        while markers.peek().is_some_and(|m| m.at < valid_end) {
            let marker = markers.next()?;
            marker.at = file + marker.at.checked_sub(decoded)?;
        }
        if markers.peek().is_none() {
            return Some(());
        }
        file += chunk.valid().len() + chunk.invalid().len();
        decoded = valid_end + char::REPLACEMENT_CHARACTER.len_utf8();
    }
    None
}

/// Where to write and which byte, to flip box number `index` of a file whose boxes should be in
/// the states `expected` (what the page shows).
pub fn toggle(bytes: &[u8], index: usize, expected: &[bool]) -> Result<(usize, u8), ToggleError> {
    if files::utf16_marked(bytes) {
        return Err(ToggleError::Encoding);
    }
    let found = markers(bytes);
    let agrees =
        found.len() == expected.len() && found.iter().zip(expected).all(|(m, &e)| m.checked == e);
    match found.get(index) {
        Some(marker) if agrees => Ok((marker.at, if marker.checked { b' ' } else { b'x' })),
        _ => Err(ToggleError::Mismatch),
    }
}

/// Flips box number `index` in the regular file at `path` and returns its new state. The
/// path check follows symbolic links to regular files, and the opened handle is checked
/// before reading. The path can still change between the first check and the open itself.
/// Its boxes are checked by [`toggle`]. Only the one byte is written; the text of a failure
/// is for the reader. `expected` alone cannot detect an external edit that leaves the same
/// sequence of checkbox states.
pub fn toggle_in_file(path: &Path, index: usize, expected: &[bool]) -> Result<bool, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let failed = |e: std::io::Error| format!("Could not change {name}: {e}");
    let metadata = std::fs::metadata(path).map_err(failed)?;
    if !metadata.is_file() {
        return Err(format!(
            "Could not change {name}: the path is not a regular file"
        ));
    }
    if metadata.permissions().readonly() {
        return Err(format!("{name} is read-only"));
    }
    let mut file = open_source(path).map_err(failed)?;
    let bytes = read_source(&mut file).map_err(failed)?;
    let (at, byte) = toggle(&bytes, index, expected).map_err(|e| match e {
        ToggleError::Encoding => format!("{name} is UTF-16; its boxes cannot be changed here"),
        ToggleError::Mismatch => {
            format!("{name} does not match the page; nothing was changed")
        }
    })?;
    file.seek(SeekFrom::Start(at as u64)).map_err(failed)?;
    file.write_all(&[byte]).map_err(failed)?;
    file.sync_data().map_err(failed)?;
    Ok(byte == b'x')
}

fn open_source(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    // The path may have been replaced since the path check. In particular, a FIFO
    // opened read/write on Linux supplies its own writer, so a read need not ever see EOF.
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the opened file is not a regular file",
        ));
    }
    Ok(file)
}

fn read_source(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the file exceeds the Markdown size limit",
        ))
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(text: &str) -> Vec<bool> {
        markers(text.as_bytes()).iter().map(|m| m.checked).collect()
    }

    #[test]
    fn finds_boxes_in_lists_quotes_and_nested_items() {
        let text =
            "- [ ] a\n* [x] b\n  + [X] c\n> - [ ] quoted\n1. [x] one\n12) [ ] two\n- - [ ] both\n";
        assert_eq!(states(text), [false, true, true, false, true, false, false]);
        for marker in markers(text.as_bytes()) {
            assert_eq!(text.as_bytes()[marker.at - 1], b'[');
            assert_eq!(text.as_bytes()[marker.at + 1], b']');
        }
    }

    #[test]
    fn skips_what_is_not_a_task_item() {
        let text = "[ ] no list\n- [y] no\n- [ ]tight\n-[ ] no space\n- text [ ] later\n\
                    ```\n- [ ] in a fence\n```\n~~~~\n- [x] too\n```\n~~~~\n- [x] real\n";
        assert_eq!(states(text), [true]);
    }

    #[test]
    fn crlf_and_a_bom_do_not_move_the_marker() {
        let text = "\u{feff}- [ ] a\r\n- [x] b\r\n";
        let found = markers(text.as_bytes());
        assert_eq!(found.len(), 2);
        assert_eq!(text.as_bytes()[found[0].at], b' ');
        assert_eq!(text.as_bytes()[found[1].at], b'x');
    }

    #[test]
    fn toggle_needs_the_file_to_agree_with_the_page() {
        let text = b"- [ ] a\n- [x] b\n";
        assert_eq!(toggle(text, 0, &[false, true]), Ok((3, b'x')));
        assert_eq!(toggle(text, 1, &[false, true]), Ok((11, b' ')));
        assert_eq!(toggle(text, 1, &[false, false]), Err(ToggleError::Mismatch));
        assert_eq!(toggle(text, 0, &[false]), Err(ToggleError::Mismatch));
        assert_eq!(toggle(text, 2, &[false, true]), Err(ToggleError::Mismatch));
        assert_eq!(
            toggle(&[0xFF, 0xFE, b'-', 0], 0, &[]),
            Err(ToggleError::Encoding)
        );
    }

    #[test]
    fn the_scan_agrees_with_the_parser() {
        let text = "# Tasks\n\n- [ ] one\n- [x] two\n  - [ ] nested\n\n> - [x] quoted\n\n\
                    ```md\n- [ ] code\n```\n\n1. [ ] numbered\n- plain\n";
        let doc = crate::parse(text);
        let page: Vec<bool> = document_boxes(&doc).iter().map(|b| b.1).collect();
        assert_eq!(page, states(text));
        assert_eq!(page.len(), 5);
    }

    #[test]
    fn a_commented_out_task_does_not_shift_either_visible_box() {
        let text = "<!--\n- [ ] old idea\n-->\n\n- [ ] visible\n- [ ]\n";
        let doc = crate::parse(text);
        let page: Vec<_> = document_boxes(&doc).iter().map(|b| b.1).collect();
        assert_eq!(page, [false, false]);
        for (index, at) in [
            text.find("[ ] visible").unwrap(),
            text.rfind("[ ]").unwrap(),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(toggle(text.as_bytes(), index, &page), Ok((at + 1, b'x')));
        }
    }

    #[test]
    fn a_duplicate_footnote_cannot_steal_a_task_after_bare_carriage_returns() {
        only_the_last_box_can_be_toggled("[^a]: first\n\n[^a]: - [ ] hidden\r\r- [ ] visible\n");
    }

    #[test]
    fn a_comment_opener_in_front_matter_does_not_hide_the_body_task() {
        only_the_last_box_can_be_toggled("---\nnote: |\n  <!--\n---\n\n- [ ] visible\n");
        only_the_last_box_can_be_toggled("---\ntags:\n- [ ] yaml\n<!--\n---\n\n- [ ]\n");
    }

    #[test]
    fn resized_front_matter_and_body_markup_do_not_move_the_task_byte() {
        only_the_last_box_can_be_toggled(
            "---\nurl: https://example.org/a_b\n---\n\n\
             $`x`$ and https://example.org/a_b.\0 ]\r\n\r\n- [ ] visible\n",
        );
    }

    #[test]
    fn two_events_for_the_same_source_byte_are_refused() {
        let marker = Marker {
            at: 3,
            checked: false,
        };
        let event = (Event::TaskListMarker(false), 2..5);
        assert_eq!(match_raw(0, &[marker], [event.clone()]), Some(()));
        assert_eq!(match_raw(0, &[marker], [event.clone(), event]), None);
    }

    #[test]
    fn lossy_decoding_keeps_each_task_at_its_original_byte() {
        let bytes = b"- [ ] caf\xe9\n- [x] after \xef\xbf\xbd and \xe2\x82\n- [ ] last\n";
        let page: Vec<_> = document_boxes(&crate::parse_bytes(bytes))
            .into_iter()
            .map(|(_, state)| state)
            .collect();
        assert_eq!(page, [false, true, false]);
        assert_eq!(toggle(bytes, 0, &page), Ok((3, b'x')));
        assert_eq!(toggle(bytes, 1, &page), Ok((14, b' ')));
        assert_eq!(toggle(bytes, 2, &page), Ok((bytes.len() - 8, b'x')));
    }

    #[test]
    fn a_multibyte_note_crossing_the_front_matter_scan_limit_still_toggles() {
        let head = "title: Note\n---\n";
        let pad = "a".repeat(16 * 1024 - head.len() - 1);
        only_the_last_box_can_be_toggled(&format!("---\n{head}{pad}📝\n\n- [ ] visible\n"));
    }

    #[test]
    fn reading_a_growing_file_stops_at_the_first_extra_byte() {
        use crate::limits::MAX_FILE_BYTES;
        let mut bytes_left = MAX_FILE_BYTES + 10;
        let reader = |buf: &mut [u8]| {
            let n = buf.len().min(bytes_left);
            buf[..n].fill(b'a');
            bytes_left -= n;
            Ok(n)
        };
        assert!(read_source(TestReader(reader)).is_err());
        assert_eq!(bytes_left, 9);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_before_any_read() {
        use std::os::unix::fs::FileTypeExt;

        let dir = std::env::temp_dir().join(format!("mdshared-task-fifo-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("tasks.md");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo is available");
        assert!(made.success());
        assert!(std::fs::metadata(&path).unwrap().file_type().is_fifo());

        // Test the production open step on its own: without the check this returns
        // an open FIFO, but trying to read it would hang rather than fail the test.
        let opened = open_source(&path);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(dir).unwrap();
        assert_eq!(
            opened
                .expect_err("a FIFO must not be handed to the reader")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_directory_is_refused_by_the_path_check_before_opening() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        assert!(path.is_dir());
        let error =
            toggle_in_file(&path, 0, &[false]).expect_err("a directory is not a Markdown file");
        assert!(
            error.ends_with(": the path is not a regular file"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_toggle_through_a_symbolic_link_changes_only_the_target_checkbox_byte() {
        let dir = std::env::temp_dir().join(format!("mdshared-task-link-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("target.md");
        let link = dir.join("link.md");
        std::fs::write(&target, b"- [ ] visible\n- [x] already checked\n").unwrap();
        std::os::unix::fs::symlink("target.md", &link).unwrap();
        let result = toggle_in_file(&link, 0, &[false, true]);
        let after = std::fs::read(&target).unwrap();
        let link_target = std::fs::read_link(&link).unwrap();
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(target).unwrap();
        std::fs::remove_dir(dir).unwrap();

        assert_eq!(result, Ok(true));
        assert_eq!(after, b"- [x] visible\n- [x] already checked\n");
        assert_eq!(link_target, Path::new("target.md"));
    }

    struct TestReader<F>(F);

    impl<F: FnMut(&mut [u8]) -> std::io::Result<usize>> Read for TestReader<F> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            (self.0)(buf)
        }
    }

    fn only_the_last_box_can_be_toggled(text: &str) {
        let doc = crate::parse(text);
        assert_eq!(document_boxes(&doc).len(), 1);
        let at = text.rfind("[ ]").unwrap() + 1;
        assert_eq!(markers(text.as_bytes()), [Marker { at, checked: false }]);
        assert_eq!(toggle(text.as_bytes(), 0, &[false]), Ok((at, b'x')));
    }

    #[test]
    fn an_html_comment_is_not_a_task_list() {
        only_the_last_box_can_be_toggled("<!--\n- [ ] hidden\n-->\n\n- [ ] visible\n");
    }

    #[test]
    fn indented_code_is_not_a_task_list() {
        only_the_last_box_can_be_toggled("    - [ ] example\n\n- [ ] visible\n");
    }

    #[test]
    fn display_math_is_not_a_task_list() {
        only_the_last_box_can_be_toggled("$$\n2. [ ] example\n$$\n\n- [ ] visible\n");
    }

    #[test]
    fn complete_display_math_on_either_side_does_not_disable_an_ordinary_task() {
        only_the_last_box_can_be_toggled("$$\nx\n$$\n\n- [ ] visible\n\n$$\ny\n$$\n");
    }

    #[test]
    fn a_list_that_interrupts_display_math_is_refused() {
        let text = "$$\n- [ ] example\n$$\n\n- [ ] visible\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 2);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false, false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_nested_list_closer_still_guards_the_parent_item_task() {
        let text = "$$\n- [ ] example\n  - continuation\n    $$\n";
        let doc = crate::parse(text);
        let boxes = document_boxes(&doc);
        assert_eq!(boxes.len(), 1);
        assert_eq!(task_item_depth(&doc, boxes[0].0), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_parent_item_closer_still_guards_the_nested_item_task() {
        let text = "- $$\n\n  - [ ] example\n\n  $$\n";
        let doc = crate::parse(text);
        let boxes = document_boxes(&doc);
        assert_eq!(boxes.len(), 1);
        assert_eq!(task_item_depth(&doc, boxes[0].0), 2);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    fn task_item_depth(doc: &Document, mut block: u32) -> usize {
        use crate::model::{BlockKind, NONE};

        let mut depth = 0;
        while block != NONE {
            let b = &doc.blocks[block as usize];
            depth += usize::from(matches!(b.kind, BlockKind::ListItem { .. }));
            block = b.parent;
        }
        depth
    }

    #[test]
    fn a_closer_in_rejected_display_math_still_blocks_a_task_write() {
        for prefix in ["", "\u{e003}\n\n"] {
            // A private-use character forces the parser's literal fallback rather
            // than a reparse with the rejected delimiters hidden.
            let text = format!("{prefix}$$\n- [ ] a$$b\n  $$\n");
            assert_eq!(document_boxes(&crate::parse(&text)).len(), 1);
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Err(ToggleError::Mismatch)
            );
        }
    }

    #[test]
    fn a_literal_private_use_character_is_not_a_dollar_delimiter() {
        only_the_last_box_can_be_toggled("\u{e003}\u{e003}\n\n- [ ] visible\n");
    }

    #[test]
    fn hidden_ruby_fallback_dollars_do_not_disable_an_ordinary_task() {
        let text = "x <rp>\n$$\n</rp>\n\n- [ ] visible\n";
        assert!(!crate::parse(text).text.contains("$$"));
        only_the_last_box_can_be_toggled(text);
    }

    #[test]
    fn inline_html_code_dollars_do_not_disable_an_ordinary_task() {
        for tag in ["code", "tt", "samp", "kbd"] {
            let text = format!(
                "Example <{tag}>\n$$\n</{tag}>\n\n- [ ] todo\n\nExample <{tag}>\n$$\n</{tag}>\n"
            );
            let doc = crate::parse(&text);
            let styled = doc
                .spans
                .iter()
                .filter(|s| matches!(s.kind, SpanKind::Code | SpanKind::Kbd))
                .filter(|s| doc.text[s.start as usize..s.end as usize].contains("$$"))
                .count();
            assert_eq!(styled, 2, "{tag}");
            only_the_last_box_can_be_toggled(&text);
        }
    }

    #[test]
    fn ruby_annotation_dollars_do_not_disable_an_ordinary_task() {
        let text = "Example <ruby>base<rt>\n$$\n</rt></ruby>\n\n- [ ] todo\n";
        let doc = crate::parse(text);
        assert!(doc.spans.iter().any(|s| {
            s.kind == SpanKind::RubyNote
                && doc.text[s.start as usize..s.end as usize].contains("$$")
        }));
        only_the_last_box_can_be_toggled(text);
    }

    #[test]
    fn literal_dollars_in_a_footnote_refuse_writeback_in_the_document() {
        for (prefix, number) in [("", 0), ("Ref[^a]\n\n", 1)] {
            let text = format!("{prefix}[^a]: note\n    $$\n\n- [ ] todo\n");
            let doc = crate::parse(&text);
            let note = &doc.footnotes[0];
            assert_eq!(note.number, number);
            assert!(doc.text[doc.blocks[note.block as usize].text.usize()].contains("$$"));
            assert_eq!(document_boxes(&doc).len(), 1);
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Err(ToggleError::Mismatch)
            );
        }
    }

    #[test]
    fn a_task_between_standalone_dollars_in_its_footnote_is_refused() {
        let text = "Ref[^a]\n\n[^a]: note\n\n    $$\n    - [ ] example\n    $$\n";
        let doc = crate::parse(text);
        let boxes = document_boxes(&doc);
        assert_eq!(boxes.len(), 1);
        let note = doc.footnotes[0].block;
        assert!((note..doc.blocks[note as usize].end).contains(&boxes[0].0));
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_task_between_two_delimiters_in_one_alert_is_refused() {
        let text = "> [!NOTE]\n> $$\n> - [ ] example\n> $$\n";
        let doc = crate::parse(text);
        let boxes = document_boxes(&doc);
        assert!(matches!(
            doc.blocks[0].kind,
            crate::model::BlockKind::Alert(_)
        ));
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].0 < doc.blocks[0].end);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_lazy_dollar_line_in_an_alert_can_still_guard_an_alert_task() {
        let text = "> [!NOTE]\n> Text\n$$\n> - [ ] example\n> $$\n";
        let doc = crate::parse(text);
        let alert = &doc.blocks[0];
        assert!(matches!(alert.kind, crate::model::BlockKind::Alert(_)));
        let boxes = document_boxes(&doc);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].0 < alert.end);
        assert!(doc.text[alert.text.usize()].contains("Text $$"));
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_lone_footnote_opener_refuses_a_main_text_task() {
        let text = "Ref[^a]\n\n[^a]: $$\n\n- [ ] todo\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_footnote_label_on_the_opening_line_is_not_delimiter_text() {
        let text = "Ref[^a]\n\n[^a]: $$\n\n    - [ ] example\n    $$\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn an_ordered_item_marker_is_not_delimiter_text() {
        let text = "12. $$\n\n    - [ ] example\n    $$\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_literal_pair_in_any_sibling_block_refuses_writeback() {
        for text in [
            "- $$\n- [ ] todo\n- $$\n",
            "> $$\n\n- [ ] todo\n\n> $$\n",
            "> $$\n\n- [ ] todo\n\n$$\n",
            "\\> $$\n\n- [ ] todo\n\n$$\n",
        ] {
            assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Err(ToggleError::Mismatch)
            );
        }
    }

    #[test]
    fn nested_quote_prefixes_are_removed_on_crlf_lines() {
        let text = ">> $$\r\n>> - [ ] example\r\n>> $$\r\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn literal_dollars_after_a_partial_quote_tab_refuse_writeback() {
        let text = "> $$\n> - [ ] example\n>\n\t> $$\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn literal_dollars_after_a_footnote_label_and_tab_refuse_writeback() {
        let text = "[^a]:   -\tparent\n\n        > $$\n        > - [ ] example\n        > $$\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_document_with_complete_display_math_and_a_task_list_can_be_written() {
        let text = "# Notes\n\n$$\nx^2+y^2=z^2\n$$\n\n- [ ] Check the calculation\n";
        let doc = crate::parse(text);
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b.kind, crate::model::BlockKind::Math { .. }))
        );
        only_the_last_box_can_be_toggled(text);
    }

    #[test]
    fn a_quote_at_the_layout_depth_limit_still_guards_its_task() {
        let quote = "> ".repeat(crate::limits::MAX_BLOCK_DEPTH + 1);
        let text = format!("{quote}$$\n{quote}- [ ] example\n{quote}$$\n");
        assert_eq!(document_boxes(&crate::parse(&text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn literal_dollars_in_details_refuse_writeback_in_the_document() {
        for attr in ["", " open"] {
            let text = format!(
                "<details{attr}>\n<summary>Summary</summary>\n\n$$\n\n</details>\n\n- [ ] todo\n"
            );
            let doc = crate::parse(&text);
            assert!(matches!(
                doc.blocks[0].kind,
                crate::model::BlockKind::Details { .. }
            ));
            assert!(doc.text[doc.blocks[0].text.usize()].contains("$$"));
            assert_eq!(document_boxes(&doc).len(), 1);
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Err(ToggleError::Mismatch)
            );
        }
    }

    #[test]
    fn display_math_is_still_checked_within_one_details_body() {
        let text =
            "<details open>\n<summary>Summary</summary>\n\n$$\n- [ ] example\n$$\n\n</details>\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn code_comments_image_alt_and_duplicate_footnotes_do_not_disable_tasks() {
        for prefix in [
            "`example\n$$\nexample`",
            "Example <pre>\n$$\n</pre>",
            "<!--\n$$\n-->",
            "![example\n$$\nexample](picture.png)",
            "[^a]: first\n\n[^a]: duplicate\n    $$",
        ] {
            only_the_last_box_can_be_toggled(&format!("{prefix}\n\n- [ ] todo\n"));
        }
    }

    #[test]
    fn visible_raw_html_dollars_refuse_writeback() {
        let text = "<div>\n$$\n</div>\n\n- [ ] todo\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn ordinary_text_in_an_alert_still_supplies_a_delimiter() {
        let text = "> [!NOTE]\n> Text\n$$\n\n- [ ] todo\n\n$$\n";
        let doc = crate::parse(text);
        assert!(matches!(
            doc.blocks[0].kind,
            crate::model::BlockKind::Alert(_)
        ));
        assert!(doc.text[doc.blocks[0].text.usize()].contains("$$"));
        assert_eq!(document_boxes(&doc).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn an_earlier_literal_dollar_line_does_not_unpair_the_lines_around_a_task() {
        let text = "$$\n\n$$\n- [ ] example\n$$\n";
        assert_eq!(document_boxes(&crate::parse(text)).len(), 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn fenced_dollars_cannot_hide_a_list_that_interrupts_display_math() {
        for newline in ["\n", "\r\n"] {
            let text = "~~~\n$$\n~~~\n\n$$\n- [ ] example\n$$\n".replace('\n', newline);
            assert_eq!(document_boxes(&crate::parse(&text)).len(), 1);
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Err(ToggleError::Mismatch),
                "line ending {newline:?}"
            );
        }
    }

    #[test]
    fn fenced_dollars_on_either_side_do_not_disable_an_ordinary_task() {
        for newline in ["\n", "\r\n"] {
            let text = "~~~\n$$\n~~~\n\n- [ ] visible\n\n~~~\n$$\n~~~\n".replace('\n', newline);
            assert_eq!(document_boxes(&crate::parse(&text)).len(), 1);
            let at = text.find("[ ]").unwrap() + 1;
            assert_eq!(
                toggle(text.as_bytes(), 0, &[false]),
                Ok((at, b'x')),
                "line ending {newline:?}"
            );
        }
    }

    #[test]
    fn front_matter_is_not_a_task_list() {
        only_the_last_box_can_be_toggled(
            "---\r\ntags:\r\n- [ ] yaml\r\n---\r\n\r\n- [ ] visible\r\n",
        );
    }

    #[test]
    fn an_equal_state_on_the_wrong_source_line_is_refused() {
        // The backticks in the YAML close its generated code fence early. The renderer
        // sees the next box, but the old byte scanner saw only the decoy before the fence.
        let text =
            "---\nnote: |\n  - [ ] decoy\n  ```\n  - [ ] from yaml\n---\n\n- [ ] from body\n";
        let doc = crate::parse(text);
        let boxes = document_boxes(&doc);
        assert_eq!(boxes.len(), 1);
        assert_eq!(doc.blocks[boxes[0].0 as usize].line, 1);
        assert_eq!(
            toggle(text.as_bytes(), 0, &[false]),
            Err(ToggleError::Mismatch)
        );
    }

    #[test]
    fn a_newline_can_follow_the_closing_bracket() {
        only_the_last_box_can_be_toggled("- [ ]\n");
    }
}
