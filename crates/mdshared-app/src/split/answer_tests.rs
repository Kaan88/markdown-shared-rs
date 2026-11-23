//! The checks of 5.2, each tripped by an answer made for it (the plan's Tests paragraph), and
//! the outline that is sent.

use super::super::tests::{assert_ok, headings_of};
use super::super::{Lines, Text, note_slug};
use super::*;
use crate::library::folder_name;

/// Line by line (1-based): a title and an introduction before the first chapter; a chapter with
/// a paragraph, a code block with a blank line inside it, and a table; a second chapter with two
/// paragraphs.
const DOC: &str = "# Trip

Intro para line a
intro b

## Tokyo

Para one a
para one b
para one c

```code
x = 1

y = 2
```

| a | b |
|---|---|
| 1 | 2 |

## Kyoto

Para two
more

Last para
";

fn doc() -> Arc<dyn Lines> {
    Arc::new(Text::new(DOC))
}

fn split(json: &str) -> (Plan, Dropped) {
    split_known(json, &[])
}

fn split_known(json: &str, known: &[Vec<String>]) -> (Plan, Dropped) {
    let answer = read(json).expect("the answer reads");
    let (p, dropped) = plan(doc(), "Trip.md", &headings_of(DOC), 2, &answer, known);
    assert_ok(&p);
    (p, dropped)
}

/// Each note as (first line, last line), 1-based and inclusive.
fn ranges(p: &Plan) -> Vec<(usize, usize)> {
    p.notes()
        .iter()
        .map(|n| (n.first_line(), n.last_line()))
        .collect()
}

#[test]
fn a_reversed_range_is_swapped_and_a_heading_names_the_note() {
    let (p, d) = split(r#"{"version":1,"notes":[{"from":27,"to":22,"title":"Not this"}]}"#);
    assert_eq!(ranges(&p), [(22, 27)]);
    assert_eq!(p.notes()[0].title(), "Kyoto");
    assert!(!p.notes()[0].title_added());
    assert_eq!(p.notes()[0].fix(), None);
    assert_eq!(d.total(), 0);
}

#[test]
fn a_range_past_the_end_is_clipped_and_one_outside_is_dropped() {
    let (p, d) = split(
        r#"{"notes":[{"from":0,"to":4,"title":"Intro"},{"from":24,"to":900},{"from":500,"to":900},{"from":-5,"to":0}]}"#,
    );
    assert_eq!(ranges(&p), [(1, 4), (24, 27)]);
    assert_eq!(d.not_lines, 2);
    assert_eq!(d.asked, 4);
}

#[test]
fn numbers_that_are_not_whole_drop_the_note() {
    let (p, d) = split(
        r#"{"notes":[{"from":3.5,"to":4},{"from":"3","to":4},{"to":4},[1,2],{"from":1e400,"to":2},{"from":8,"to":10}]}"#,
    );
    assert_eq!(ranges(&p), [(8, 10)]);
    assert_eq!(d.not_lines, 5);
}

#[test]
fn an_overlap_starts_after_the_note_before_and_says_so() {
    let (p, d) = split(r#"{"notes":[{"from":10,"to":27},{"from":6,"to":16}]}"#);
    // The second asked for, first in order, ends at the code block's last line. The other
    // started inside a paragraph (moved to its end, line 11), then overlapped the first: it now
    // starts after it, at the table.
    assert_eq!(ranges(&p), [(6, 16), (18, 27)]);
    assert_eq!(p.notes()[1].fix(), Some(Fix::Overlap));
    assert_eq!(p.notes()[0].fix(), None);
    assert_eq!(d.total(), 0);
    // A note wholly inside the one before is left with nothing, and dropped.
    let (p, d) = split(r#"{"notes":[{"from":6,"to":27},{"from":8,"to":10}]}"#);
    assert_eq!(ranges(&p), [(6, 27)]);
    assert_eq!(d.emptied, 1);
}

#[test]
fn a_boundary_inside_a_code_block_moves_past_its_end() {
    // Starts on the blank line inside the fence: moves after the block, then the blank is
    // trimmed. Ends inside the next fence's block: moves to its closing line.
    let (p, _) = split(r#"{"notes":[{"from":14,"to":20},{"from":22,"to":23}]}"#);
    assert_eq!(ranges(&p)[0], (18, 20));
    assert_eq!(p.notes()[0].fix(), Some(Fix::CodeBlock));
    let (p, _) = split(r#"{"notes":[{"from":8,"to":13}]}"#);
    assert_eq!(ranges(&p), [(8, 16)]);
    assert_eq!(p.notes()[0].fix(), Some(Fix::CodeBlock));
}

#[test]
fn a_boundary_inside_a_table_moves_past_its_end() {
    let (p, _) = split(r#"{"notes":[{"from":6,"to":19}]}"#);
    assert_eq!(ranges(&p), [(6, 20)]);
    assert_eq!(p.notes()[0].fix(), Some(Fix::Table));
    let (p, _) = split(r#"{"notes":[{"from":19,"to":27}]}"#);
    assert_eq!(ranges(&p), [(22, 27)]);
    assert_eq!(p.notes()[0].fix(), Some(Fix::Table));
}

#[test]
fn a_boundary_inside_a_paragraph_moves_to_its_nearer_edge() {
    // Line 9 is the middle of 8-10: a tie goes up for a start. Line 25 ends a paragraph of two;
    // asking to end at 24 is one line from either edge, and a tie keeps the text.
    let (p, _) = split(r#"{"notes":[{"from":9,"to":24}]}"#);
    assert_eq!(ranges(&p), [(8, 25)]);
    assert_eq!(p.notes()[0].fix(), Some(Fix::Paragraph));
    // Line 10 is nearer the paragraph's end: the note starts after it.
    let (p, _) = split(r#"{"notes":[{"from":10,"to":16}]}"#);
    assert_eq!(ranges(&p), [(12, 16)]);
}

#[test]
fn blank_lines_at_the_ends_are_trimmed() {
    let (p, d) = split(r#"{"notes":[{"from":5,"to":11},{"from":2,"to":2}]}"#);
    assert_eq!(ranges(&p), [(6, 10)]);
    assert_eq!(d.emptied, 1);
    // Line 2 is blank: no note before emptied it, and the sentence does not say one did (audit
    // 4, N4).
    assert_eq!(
        d.text().as_deref(),
        Some("Dropped 1 of the AI\u{2019}s 2 notes: 1 left with no lines of their own.")
    );
}

#[test]
fn at_most_two_hundred_notes_are_kept_and_the_rest_counted() {
    let mut json = String::from(r#"{"version":1,"notes":["#);
    for k in 0..250 {
        if k > 0 {
            json.push(',');
        }
        json.push_str(&format!(r#"{{"from":{},"to":{}}}"#, k % 27 + 1, k % 27 + 1));
    }
    json.push_str("]}");
    let answer = read(&json).unwrap();
    assert_eq!(answer.notes.len(), ANSWER_MAX_NOTES);
    assert_eq!(answer.over, 50);
    let (p, d) = split(&json);
    assert!(p.notes().len() <= ANSWER_MAX_NOTES);
    assert_eq!(d.over, 50);
    assert_eq!(d.asked, 250);
    let text = d.text().unwrap();
    assert!(text.starts_with("Dropped "), "{text}");
    assert!(
        text.contains("of the AI\u{2019}s 250 notes: 50 past the first 200"),
        "{text}"
    );
}

#[test]
fn a_title_is_one_clean_line_and_never_a_path() {
    let (p, _) = split(
        r#"{"notes":[{"from":3,"to":4,"title":"..\\..\\Windows\\x/y\nsecond line"},{"from":24,"to":25,"title":""},{"from":27,"to":27,"title":"   "}]}"#,
    );
    let n = &p.notes()[0];
    assert!(n.title_added(), "the range does not start at a heading");
    assert_eq!(n.title(), r"..\..\Windows\x/y second line");
    let file = folder_name(&note_slug(n.title()));
    assert!(!file.contains(['/', '\\']), "{file}");
    assert_ne!(file.trim_matches('.'), "");
    // An empty title is named by its lines.
    assert_eq!(p.notes()[1].title(), "Lines 24-25");
    assert_eq!(p.notes()[2].title(), "Lines 27-27");
    // A long one is cut to the cap, at a character.
    let long = "é".repeat(100);
    let (p, _) = split(&format!(
        r#"{{"notes":[{{"from":3,"to":4,"title":"{long}"}}]}}"#
    ));
    assert_eq!(p.notes()[0].title().len(), 120);
}

#[test]
fn sections_are_cleaned_capped_and_spelled_as_the_library_has_them() {
    let (p, _) = split_known(
        r#"{"notes":[
            {"from":3,"to":4,"sections":["..", "tokyo", " ", "...", "a\nb", "Fourth", "Fifth"]},
            {"from":8,"to":10,"sections":["KYOTO"]},
            {"from":24,"to":25,"sections":["Kyoto", 7, null]},
            {"from":27,"to":27,"sections":"not a list"}
        ]}"#,
        &[vec!["Tokyo".to_string()]],
    );
    assert_eq!(p.notes()[0].sections(), ["Tokyo", "a b", "Fourth"]);
    // The first spelling of a new section is the one every later note takes.
    assert_eq!(p.notes()[1].sections(), ["KYOTO"]);
    assert_eq!(p.notes()[2].sections(), ["KYOTO"]);
    assert!(p.notes()[3].sections().is_empty());
    let long = "x".repeat(100);
    let (p, _) = split(&format!(
        r#"{{"notes":[{{"from":3,"to":4,"sections":["{long}"]}}]}}"#
    ));
    assert_eq!(p.notes()[0].sections()[0].len(), 64);
}

#[test]
fn two_notes_may_share_a_title_and_a_section() {
    let (p, _) = split(
        r#"{"notes":[{"from":3,"to":4,"title":"Same","sections":["S"]},{"from":27,"to":27,"title":"Same","sections":["S"]}]}"#,
    );
    assert_eq!(p.notes().len(), 2);
    assert_eq!(p.notes()[0].title(), p.notes()[1].title());
}

#[test]
fn an_answer_wrapped_in_prose_is_read_from_its_first_object() {
    let text = "Sure! Here is the plan:\n```json\n{\"version\":1,\"notes\":[{\"from\":22,\"to\":27,\"title\":\"a } in a string {\"}]}\n```\nHope that helps {really}.";
    let a = read(text).unwrap();
    assert_eq!(a.notes.len(), 1);
    assert_eq!(a.notes[0].title, "a } in a string {");
}

#[test]
fn an_answer_that_is_not_json_is_a_fault_the_retry_names() {
    assert_eq!(read("I cannot split this document."), Err(Fault::NoObject));
    assert_eq!(read("{\"notes\": [ {\"from\": 1"), Err(Fault::NoObject));
    assert_eq!(read("{from: 1, to: 2}"), Err(Fault::NotJson));
    assert_eq!(read(r#"{"version":2,"notes":[]}"#), Err(Fault::Version));
    assert_eq!(read(r#"{"version":"1","notes":[]}"#), Err(Fault::Version));
    assert_eq!(read(r#"{"version":1}"#), Err(Fault::NoNotes));
    assert_eq!(read(r#"{"notes":{"from":1}}"#), Err(Fault::NoNotes));
    let again = retry_prompt("P", Fault::NotJson);
    assert!(again.starts_with("P\n\n"));
    assert!(again.contains("its JSON object did not parse"));
    // A version of 1, or none, is read.
    assert!(read(r#"{"notes":[]}"#).is_ok());
}

#[test]
fn an_answer_nested_past_the_reader_depth_is_refused_not_a_crash() {
    let deep = format!(
        "{}{}",
        "{\"a\":".repeat(10_000),
        "1".to_string() + &"}".repeat(10_000)
    );
    assert_eq!(read(&deep), Err(Fault::NotJson));
}

#[test]
fn no_answer_breaks_the_plan() {
    // A small generator of hostile answers: numbers of every kind, overlaps, junk.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let nums = [
        "0", "1", "-1", "27", "28", "13", "14", "1e9", "-1e9", "3.0", "2.5", "1e400", "null",
        "\"5\"", "true", "[]",
    ];
    for _ in 0..400 {
        let n = next() % 12;
        let mut json = String::from("{\"notes\":[");
        for k in 0..n {
            if k > 0 {
                json.push(',');
            }
            let a = nums[(next() % nums.len() as u64) as usize];
            let b = nums[(next() % nums.len() as u64) as usize];
            let s = ["\"..\"", "\"x\"", "\"\"", "1"][(next() % 4) as usize];
            json.push_str(&format!(
                "{{\"from\":{a},\"to\":{b},\"title\":\"t{k}\",\"sections\":[{s}]}}"
            ));
        }
        json.push_str("]}");
        let Ok(answer) = read(&json) else { continue };
        let (p, d) = plan(doc(), "Trip.md", &headings_of(DOC), 2, &answer, &[]);
        assert_ok(&p);
        assert_eq!(p.notes().len() + d.total(), d.asked, "{json}");
    }
}

#[test]
fn the_outline_is_the_headings_and_the_start_of_each_paragraph() {
    let o = outline(&*doc(), &headings_of(DOC), OUTLINE_MAX).unwrap();
    assert!(!o.headings_only);
    assert_eq!(o.lines, 27);
    assert_eq!(
        o.text,
        "1 # Trip\n3 Intro para line a\n6 ## Tokyo\n8 Para one a\n12 ```code\n18 | a | b |\n22 ## Kyoto\n24 Para two\n27 Last para\n"
    );
    let p = prompt(" one note per chapter ", "Trip \"x\" <y>.md", &o);
    assert!(p.starts_with(
        "<instruction>one note per chapter</instruction>\n<document name=\"Trip _x_ _y_.md\" lines=\"27\">\n1 # Trip\n"
    ));
    assert!(p.ends_with("27 Last para\n</document>"));
}

#[test]
fn a_paragraph_is_sent_to_eighty_bytes_and_a_heading_to_two_hundred() {
    let long = "é".repeat(150);
    let text = format!("# {long}\n{long}\nSecond line of the paragraph\nTitle\n=====\nText\n");
    let doc = Text::new(text.as_str());
    let heads = vec![Heading { line: 0, level: 1 }, Heading { line: 3, level: 1 }];
    let o = outline(&doc, &heads, OUTLINE_MAX).unwrap();
    let lines: Vec<&str> = o.text.lines().collect();
    assert_eq!(lines.len(), 4, "{lines:?}");
    // "1 " then the heading cut at 200 bytes, at a character.
    assert_eq!(lines[0].len(), 2 + 200);
    assert_eq!(lines[1].len(), 2 + 80);
    // The setext underline is not sent; the line under the heading starts a paragraph.
    assert_eq!(lines[2], "4 Title");
    assert_eq!(lines[3], "6 Text");
}

#[test]
fn a_long_document_sends_its_headings_or_is_refused() {
    let o = outline(&*doc(), &headings_of(DOC), 40).unwrap();
    assert!(o.headings_only);
    assert_eq!(o.text, "1 # Trip\n6 ## Tokyo\n22 ## Kyoto\n");
    assert_eq!(
        outline(&*doc(), &headings_of(DOC), 10),
        Err(TooLong {
            lines: 27,
            headings: 3
        })
    );
    let plain = "words\n\n".repeat(15_500);
    let doc = Text::new(plain.as_str());
    let e = outline(&doc, &[], 1000).unwrap_err();
    assert_eq!(
        e.text(),
        "Too long to split by AI: 31,000 lines and no headings. Split it at headings by hand first, or in two."
    );
    assert!(outline(&doc, &[], OUTLINE_MAX).is_ok());
}
