//! One test per "Not ported" bug that lives in core, plus the behaviours the plan calls out.

#[path = "../common/mod.rs"]
mod common;

use mdshared_md_core::find::{find, find_in_text, find_visible};
use mdshared_md_core::limits::*;
use mdshared_md_core::links::{ImageSource, LinkAction, classify_image, classify_link};
use mdshared_md_core::model::*;
use mdshared_md_core::parse;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn span_text(d: &Document, s: &Span) -> String {
    d.text[s.start as usize..s.end as usize].to_string()
}

fn links(d: &Document) -> Vec<(String, String)> {
    d.spans
        .iter()
        .filter_map(|s| match s.kind {
            SpanKind::Link(l) => {
                Some((span_text(d, s), d.str(d.links[l as usize].dest).to_string()))
            }
            _ => None,
        })
        .collect()
}

fn count(text: &str, q: &str) -> usize {
    find_in_text(text, q, FIND_CAP).hits.len()
}

fn math_spans(d: &Document) -> Vec<String> {
    d.spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Math { .. }))
        .map(|s| span_text(d, s))
        .collect()
}

fn tree(d: &Document) -> String {
    fn name(k: BlockKind) -> &'static str {
        match k {
            BlockKind::Paragraph { .. } => "P",
            BlockKind::Plain => "T",
            BlockKind::Heading { .. } => "H",
            BlockKind::BlockQuote => "Q",
            BlockKind::Alert(_) => "A",
            BlockKind::List { .. } => "L",
            BlockKind::ListItem { .. } => "I",
            BlockKind::CodeBlock { .. } => "C",
            BlockKind::Diagram { .. } => "G",
            BlockKind::Math { .. } => "M",
            BlockKind::ThematicBreak => "R",
            BlockKind::Table => "Tb",
            BlockKind::TableRow { .. } => "Tr",
            BlockKind::TableCell { .. } => "Td",
            BlockKind::Details { .. } => "D",
            BlockKind::Summary => "S",
            BlockKind::FootnoteDefinition { .. } => "F",
            BlockKind::Div { .. } => "V",
            BlockKind::DefTerm => "Dt",
            BlockKind::DefDesc => "Dd",
            BlockKind::Figure { .. } => "Fg",
            BlockKind::FigCaption => "Fc",
        }
    }
    fn walk(d: &Document, parent: u32, out: &mut Vec<String>) {
        for c in d.children(parent) {
            let b = d.blocks[c as usize];
            let has_children = b.end > c + 1;
            if has_children {
                let mut inner = Vec::new();
                walk(d, c, &mut inner);
                out.push(format!("{}[{}]", name(b.kind), inner.join(" ")));
            } else if d.block_text(c).trim().is_empty() {
                out.push(name(b.kind).to_string());
            } else {
                out.push(format!("{}({})", name(b.kind), d.block_text(c).trim_end()));
            }
        }
    }
    let mut out = Vec::new();
    walk(d, NONE, &mut out);
    out.join(" ")
}

fn kinds(d: &Document) -> Vec<BlockKind> {
    d.blocks.iter().map(|b| b.kind).collect()
}

fn has_alert(d: &Document) -> bool {
    d.blocks
        .iter()
        .any(|b| matches!(b.kind, BlockKind::Alert(_)))
}

fn spans_of(d: &Document, kind: SpanKind) -> Vec<&str> {
    d.spans
        .iter()
        .filter(|s| s.kind == kind)
        .map(|s| &d.text[s.start as usize..s.end as usize])
        .collect()
}

mod audit_fixes;
mod blocks_and_autolinks;
mod document_edges;
mod find_limits_and_links;
mod html_and_parser_limits;
mod wiki_and_extensions;
