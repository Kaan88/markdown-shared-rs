//! Horizontal geometry from one Parley line. The field keeps logical byte offsets; the
//! side of a bidi run boundary determines which visual caret is shown there.

use super::{TextField, class};
use crate::gfx::Brush;
use parley::{Affinity, Layout};
use std::ops::Range;

struct Cell {
    bytes: Range<usize>,
    span: Range<f32>,
    rtl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stop {
    pub byte: usize,
    pub affinity: Affinity,
}

impl Cell {
    fn edge(&self, left: bool) -> Stop {
        if left != self.rtl {
            Stop {
                byte: self.bytes.start,
                affinity: Affinity::Downstream,
            }
        } else {
            Stop {
                byte: self.bytes.end,
                affinity: Affinity::Upstream,
            }
        }
    }
}

pub(super) struct Line {
    cells: Vec<Cell>,
    pub width: f32,
}

impl Line {
    pub fn new(layout: &Layout<Brush>, scale: f32) -> Self {
        let mut cells = Vec::new();
        let mut width = 0.0;
        if let Some(line) = layout.get(0) {
            let metrics = line.metrics();
            let mut x = metrics.offset + metrics.inline_min_coord;
            for run in line.runs() {
                let mut at = x;
                for cluster in run.visual_clusters() {
                    let end = at + cluster.advance();
                    cells.push(Cell {
                        bytes: cluster.text_range(),
                        span: at / scale..end / scale,
                        rtl: cluster.is_rtl(),
                    });
                    at = end;
                }
                x += run.advance();
            }
            width = x / scale;
        }
        Self { cells, width }
    }

    fn cell_at(&self, x: f32) -> Option<&Cell> {
        let i = self.cells.partition_point(|cell| cell.span.end <= x);
        self.cells.get(i).or_else(|| self.cells.last())
    }

    pub fn hit(&self, x: f32) -> Stop {
        self.cell_at(x).map_or(
            Stop {
                byte: 0,
                affinity: Affinity::Downstream,
            },
            |cell| cell.edge(x <= f32::midpoint(cell.span.start, cell.span.end)),
        )
    }

    pub fn x(&self, stop: Stop) -> f32 {
        let mut alternate = None;
        for cell in &self.cells {
            for (edge, x) in [
                (cell.edge(true), cell.span.start),
                (cell.edge(false), cell.span.end),
            ] {
                if edge == stop {
                    return x;
                }
                if edge.byte == stop.byte {
                    alternate = Some(x);
                }
            }
            // Legacy keyboard and low-level APIs can still leave a caret inside a cluster.
            if cell.bytes.start < stop.byte && stop.byte < cell.bytes.end {
                return if cell.rtl {
                    cell.span.end
                } else {
                    cell.span.start
                };
            }
        }
        alternate.unwrap_or(self.width)
    }

    pub fn edges(&self) -> impl DoubleEndedIterator<Item = f32> + '_ {
        self.cells
            .iter()
            .map(|cell| cell.span.start)
            .chain(std::iter::once(self.width))
    }

    pub fn word_at(&self, text: &str, x: f32) -> Range<usize> {
        let Some(hit) = self.cell_at(x) else {
            return 0..0;
        };
        let kind = class(text[hit.bytes.clone()].chars().next().unwrap_or(' '));
        let mut logical: Vec<_> = self.cells.iter().map(|cell| &cell.bytes).collect();
        logical.sort_unstable_by_key(|range| range.start);
        let i = logical.partition_point(|range| range.start < hit.bytes.start);
        let same = |range: &&Range<usize>| {
            class(text[(*range).clone()].chars().next().unwrap_or(' ')) == kind
        };
        let start = logical[..i]
            .iter()
            .rev()
            .take_while(|r| same(r))
            .last()
            .map_or(hit.bytes.start, |r| r.start);
        let end = logical[i + 1..]
            .iter()
            .take_while(|r| same(r))
            .last()
            .map_or(hit.bytes.end, |r| r.end);
        start..end
    }

    /// Calls back once for each contiguous visual part of the logical selection.
    pub fn spans(&self, bytes: Range<usize>, mut visit: impl FnMut(Range<f32>)) {
        let mut span: Option<Range<f32>> = None;
        for cell in &self.cells {
            if cell.span.start >= cell.span.end {
                continue;
            }
            if cell.bytes.start < bytes.end && cell.bytes.end > bytes.start {
                match &mut span {
                    Some(s) => s.end = cell.span.end,
                    None => span = Some(cell.span.clone()),
                }
            } else if let Some(s) = span.take() {
                visit(s);
            }
        }
        if let Some(s) = span {
            visit(s);
        }
    }
}

impl TextField {
    pub(super) fn shaped_stop(&self, byte: usize) -> Stop {
        Stop {
            byte: if self.secret { self.masked(byte) } else { byte },
            affinity: self.affinity.unwrap_or(Affinity::Downstream),
        }
    }

    pub(super) fn move_to_stop(&mut self, stop: Stop, extend: bool) {
        self.restart = true;
        self.caret = if self.secret {
            self.text
                .char_indices()
                .nth(stop.byte / super::MASK.len_utf8())
                .map_or(self.text.len(), |(byte, _)| byte)
        } else {
            stop.byte
        };
        self.affinity = Some(stop.affinity);
        if !extend {
            self.anchor = self.caret;
        }
    }
}
