//! Maps source lines to page rows and back for synced scrolling.

use mdshared_md_core::model::Document;
use mdshared_md_render::BlockBox;

/// The places where the source and the page are known to line up, `(line, row)`, both
/// increasing: the page's top at line 1, each top-level block's first line at its row, and the
/// end of the source at the last block's end. Synced scrolling goes straight between them, so a
/// block's lines share its height evenly, and the two directions are each other's inverse.
pub(super) fn knots(doc: &Document, blocks: &[BlockBox], lines: u32) -> Vec<(f32, f32)> {
    let mut k = Vec::with_capacity(blocks.len() + 2);
    k.push((1.0f32, 0.0f32));
    for b in blocks {
        let line = doc.blocks.get(b.block as usize).map_or(0, |d| d.line) as f32;
        let &(l, r) = k.last().unwrap();
        // Out of order (a block drawn away from its source) or not below: skipped.
        if line > l && b.y >= r {
            k.push((line, b.y));
        }
    }
    if let Some(last) = blocks.last() {
        let end = (lines as f32 + 1.0, last.y + last.height);
        let &(l, r) = k.last().unwrap();
        if end.0 > l && end.1 >= r {
            k.push(end);
        }
    }
    k
}

/// How the editor's top and the page's top go together.
pub(super) struct Sync {
    pub(super) knots: Vec<(f32, f32)>,
    /// The page's largest top.
    pub(super) bottom: f32,
    /// The editor's last screen of scrolling, `(tail, end)`.
    pub(super) end: Option<(f32, f32)>,
}

impl Sync {
    /// The page's top for the editor's top `line`: the same source line at both tops, except
    /// over the editor's last screen, where the page goes on straight to its bottom. The page
    /// is mostly the longer, so its top line alone would leave its end unreachable; when it is
    /// the shorter, it stops at its bottom as before.
    pub(super) fn row(&self, line: f32) -> f32 {
        let same = row_of_line(&self.knots, line).clamp(0.0, self.bottom);
        match self.end {
            Some((tail, end)) if end > tail && self.bottom > 0.0 => {
                let t = ((line - tail) / (end - tail)).clamp(0.0, 1.0);
                same + (self.bottom - same) * t
            }
            _ => same,
        }
    }

    /// The editor's top for page row `row`: [`row`](Self::row)'s inverse. The page at its
    /// bottom is the editor at its end.
    pub(super) fn line(&self, row: f32) -> f32 {
        let Some((_, end)) = self.end.filter(|&(t, e)| e > t && self.bottom > 0.0) else {
            return line_of_row(&self.knots, row);
        };
        if row >= self.bottom - 0.5 {
            return end;
        }
        // `row` never goes down as the line goes on: halve the interval to the line.
        let (mut lo, mut hi) = (1.0f32, end.max(1.0));
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if self.row(mid) < row {
                lo = mid;
            } else {
                hi = mid;
            }
            if hi - lo < 1e-4 {
                break;
            }
        }
        (lo + hi) / 2.0
    }
}

/// Straight between the two knots around `x` (in `.0` when `by_line`, else in `.1`).
fn between(k: &[(f32, f32)], x: f32, by_line: bool) -> f32 {
    let key = |p: &(f32, f32)| if by_line { p.0 } else { p.1 };
    let val = |p: &(f32, f32)| if by_line { p.1 } else { p.0 };
    let i = k.partition_point(|p| key(p) <= x);
    match (i.checked_sub(1).map(|i| &k[i]), k.get(i)) {
        (Some(a), Some(b)) if key(b) > key(a) => {
            let t = (x - key(a)) / (key(b) - key(a));
            val(a) + t * (val(b) - val(a))
        }
        (Some(a), _) => val(a),
        (None, Some(b)) => val(b),
        (None, None) => 0.0,
    }
}

pub(super) fn row_of_line(k: &[(f32, f32)], line: f32) -> f32 {
    between(k, line, true)
}

pub(super) fn line_of_row(k: &[(f32, f32)], row: f32) -> f32 {
    between(k, row, false)
}
