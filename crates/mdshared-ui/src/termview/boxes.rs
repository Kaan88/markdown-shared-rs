//! The box-drawing characters (U+2500 to U+257F) and the block elements (U+2580 to U+259F) as
//! shapes of a cell rather than glyphs of a font: a font's lines stop short of the cell's edge or
//! overlap the next one, depending on the font, its size and the cell's height, so frames drawn
//! by full-screen programs show gaps. Drawn here, every line runs to the cell's edge at a
//! position that depends only on the cell's size, so it meets the line of the next cell exactly.
//!
//! [`pieces`] gives the shapes of one character in device px relative to the cell's top left
//! corner; the drawing fills them. Pure arithmetic, tested on its own.

/// A part of a character's shape, in device px from the cell's top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Piece {
    /// A rectangle `x0..x1` by `y0..y1`, at `alpha` (1 for lines and blocks, less for shades).
    Fill {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        alpha: f32,
    },
    /// A quarter of a ring of radius `r` and thickness `t` round `cx, cy`, on the side of the
    /// centre that `sx` and `sy` (each -1 or 1) point to: a rounded corner.
    Arc {
        cx: f32,
        cy: f32,
        r: f32,
        t: f32,
        sx: f32,
        sy: f32,
    },
    /// A line of thickness `t` from corner to corner: from the bottom left to the top right when
    /// `rising`, from the top left to the bottom right otherwise.
    Diagonal { rising: bool, t: f32 },
}

/// The four arms of a line character, up, right, down, left: 0 none, 1 light, 2 heavy,
/// 3 double. The characters drawn another way (dashes, arcs, diagonals) are zero here.
#[rustfmt::skip]
const ARMS: [[u8; 4]; 128] = [
    [0,1,0,1], [0,2,0,2], [1,0,1,0], [2,0,2,0], [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,0,0,0], // 2500
    [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,1,1,0], [0,2,1,0], [0,1,2,0], [0,2,2,0], // 2508
    [0,0,1,1], [0,0,1,2], [0,0,2,1], [0,0,2,2], [1,1,0,0], [1,2,0,0], [2,1,0,0], [2,2,0,0], // 2510
    [1,0,0,1], [1,0,0,2], [2,0,0,1], [2,0,0,2], [1,1,1,0], [1,2,1,0], [2,1,1,0], [1,1,2,0], // 2518
    [2,1,2,0], [2,2,1,0], [1,2,2,0], [2,2,2,0], [1,0,1,1], [1,0,1,2], [2,0,1,1], [1,0,2,1], // 2520
    [2,0,2,1], [2,0,1,2], [1,0,2,2], [2,0,2,2], [0,1,1,1], [0,1,1,2], [0,2,1,1], [0,2,1,2], // 2528
    [0,1,2,1], [0,1,2,2], [0,2,2,1], [0,2,2,2], [1,1,0,1], [1,1,0,2], [1,2,0,1], [1,2,0,2], // 2530
    [2,1,0,1], [2,1,0,2], [2,2,0,1], [2,2,0,2], [1,1,1,1], [1,1,1,2], [1,2,1,1], [1,2,1,2], // 2538
    [2,1,1,1], [1,1,2,1], [2,1,2,1], [2,1,1,2], [2,2,1,1], [1,1,2,2], [1,2,2,1], [2,2,1,2], // 2540
    [1,2,2,2], [2,1,2,2], [2,2,2,1], [2,2,2,2], [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,0,0,0], // 2548
    [0,3,0,3], [3,0,3,0], [0,3,1,0], [0,1,3,0], [0,3,3,0], [0,0,1,3], [0,0,3,1], [0,0,3,3], // 2550
    [1,3,0,0], [3,1,0,0], [3,3,0,0], [1,0,0,3], [3,0,0,1], [3,0,0,3], [1,3,1,0], [3,1,3,0], // 2558
    [3,3,3,0], [1,0,1,3], [3,0,3,1], [3,0,3,3], [0,3,1,3], [0,1,3,1], [0,3,3,3], [1,3,0,3], // 2560
    [3,1,0,1], [3,3,0,3], [1,3,1,3], [3,1,3,1], [3,3,3,3], [0,0,0,0], [0,0,0,0], [0,0,0,0], // 2568
    [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,0,0,0], [0,0,0,1], [1,0,0,0], [0,1,0,0], [0,0,1,0], // 2570
    [0,0,0,2], [2,0,0,0], [0,2,0,0], [0,0,2,0], [0,2,0,1], [1,0,2,0], [0,1,0,2], [2,0,1,0], // 2578
];

/// Whether `ch` is drawn as shapes by [`pieces`] rather than from the font: U+2500 to U+259F,
/// every box-drawing character and every block element.
pub fn is_box_char(ch: char) -> bool {
    ('\u{2500}'..='\u{259F}').contains(&ch)
}

/// The start of a line `t` thick centred in `len`: the same in every cell of a size, which is
/// what makes lines meet.
fn lane(len: i32, t: i32) -> i32 {
    (len - t).div_euclid(2)
}

/// The shapes of `ch` in a cell `w` by `h` device px with light lines `light` px thick, or
/// `None` for a character that is not drawn here ([`is_box_char`]). Heavy lines are twice as
/// thick as light ones, and the two strokes of a double line are light ones a light line apart.
pub(super) fn pieces(ch: char, w: i32, h: i32, light: i32) -> Option<Vec<Piece>> {
    if !is_box_char(ch) || w <= 0 || h <= 0 {
        return None;
    }
    let light = light.clamp(1, (w.min(h) / 3).max(1));
    let code = ch as u32;
    let mut out = Vec::new();
    match code {
        0x2580..=0x259F => blocks(code, w, h, &mut out),
        // Dashed lines: horizontal and vertical, light and heavy, of 3, 4 and 2 dashes.
        0x2504..=0x250B | 0x254C..=0x254F => {
            let (n, rest) = match code {
                0x2504..=0x2507 => (3, code - 0x2504),
                0x2508..=0x250B => (4, code - 0x2508),
                _ => (2, code - 0x254C),
            };
            let (vertical, heavy) = (rest >= 2, rest % 2 == 1);
            let t = if heavy { light * 2 } else { light };
            dashes(n, vertical, t, w, h, &mut out);
        }
        0x256D..=0x2570 => arc(code, w, h, light, &mut out),
        0x2571 => out.push(Piece::Diagonal {
            rising: true,
            t: light as f32,
        }),
        0x2572 => out.push(Piece::Diagonal {
            rising: false,
            t: light as f32,
        }),
        0x2573 => {
            out.push(Piece::Diagonal {
                rising: true,
                t: light as f32,
            });
            out.push(Piece::Diagonal {
                rising: false,
                t: light as f32,
            });
        }
        _ => lines(ARMS[(code - 0x2500) as usize], w, h, light, &mut out),
    }
    // In a cell too small for its lines they are cut to the cell.
    out.retain_mut(|p| match p {
        Piece::Fill { x0, y0, x1, y1, .. } => {
            (*x0, *y0, *x1, *y1) = ((*x0).max(0), (*y0).max(0), (*x1).min(w), (*y1).min(h));
            x1 > x0 && y1 > y0
        }
        _ => true,
    });
    Some(out)
}

fn fill(out: &mut Vec<Piece>, x0: i32, y0: i32, x1: i32, y1: i32) {
    if x1 > x0 && y1 > y0 {
        out.push(Piece::Fill {
            x0,
            y0,
            x1,
            y1,
            alpha: 1.0,
        });
    }
}

/// A band across a line: its start and its end, perpendicular to the line.
type Band = (i32, i32);

/// The bands of an arm of weight `weight` in a cell `len` px across the line: one for light and
/// heavy, two for double (the light line moved a light line's width each way).
fn bands(weight: u8, len: i32, light: i32) -> Vec<Band> {
    match weight {
        0 => vec![],
        1 => {
            let a = lane(len, light);
            vec![(a, a + light)]
        }
        2 => {
            let a = lane(len, light * 2);
            vec![(a, a + light * 2)]
        }
        _ => {
            let a = lane(len, light);
            vec![(a - light, a), (a + light, a + light * 2)]
        }
    }
}

/// The lines of a box-drawing character from its arms (up, right, down, left).
///
/// Light and heavy arms run from the cell's edge into the middle as far as the far side of the
/// lines that cross them, so corners and tees are filled. The strokes of a double arm stop
/// where the strokes of a double arm across them run, so the double lines of a corner or a tee
/// turn into each other and leave their inside open; across a light or heavy line they stop at
/// it, and with nothing across they meet in the middle.
fn lines(arms: [u8; 4], w: i32, h: i32, light: i32, out: &mut Vec<Piece>) {
    let [up, right, down, left] = arms;
    // Horizontal arms lie in bands of y; vertical ones in bands of x.
    let hb = |weight| bands(weight, h, light);
    let vb = |weight| bands(weight, w, light);
    let span = |bs: &[Band]| {
        bs.iter()
            .fold(None, |acc: Option<Band>, &(s, e)| match acc {
                None => Some((s, e)),
                Some((a, b)) => Some((a.min(s), b.max(e))),
            })
    };
    // What lies across the middle, each way.
    let vertical: Vec<Band> = [up, down].iter().flat_map(|&k| vb(k)).collect();
    let horizontal: Vec<Band> = [left, right].iter().flat_map(|&k| hb(k)).collect();
    let (vcore, hcore) = (span(&vertical), span(&horizontal));
    let (cx, cy) = (w / 2, h / 2);
    // The double strokes' positions, for the turning rule.
    let (dx, dy) = (lane(w, light), lane(h, light));
    let (dl, dr) = (dx - light, dx + light); // the two vertical double strokes' starts
    let (du, dd) = (dy - light, dy + light); // the two horizontal double strokes' starts

    // Horizontal arms.
    for (weight, toward_left) in [(left, true), (right, false)] {
        if weight == 0 {
            continue;
        }
        let strokes = hb(weight);
        for (i, &(y0, y1)) in strokes.iter().enumerate() {
            // How far this stroke reaches into the middle, as the x where it ends (left arm)
            // or starts (right arm).
            let reach = if weight == 3 {
                let upper = i == 0;
                let (near, far) = if upper { (up, down) } else { (down, up) };
                if near == 3 {
                    // Stops at the near double's stroke on its own side.
                    if toward_left { dl + light } else { dr }
                } else if far == 3 {
                    // Turns into the far double's outer stroke.
                    if toward_left { dr + light } else { dl }
                } else if let Some((a, b)) = vcore {
                    if toward_left { b } else { a }
                } else {
                    cx
                }
            } else if let Some((a, b)) = vcore {
                if toward_left { b } else { a }
            } else {
                cx
            };
            if toward_left {
                fill(out, 0, y0, reach, y1);
            } else {
                fill(out, reach, y0, w, y1);
            }
        }
    }
    // Vertical arms, the same turned a quarter.
    for (weight, toward_top) in [(up, true), (down, false)] {
        if weight == 0 {
            continue;
        }
        let strokes = vb(weight);
        for (i, &(x0, x1)) in strokes.iter().enumerate() {
            let reach = if weight == 3 {
                let leftward = i == 0;
                let (near, far) = if leftward {
                    (left, right)
                } else {
                    (right, left)
                };
                if near == 3 {
                    if toward_top { du + light } else { dd }
                } else if far == 3 {
                    if toward_top { dd + light } else { du }
                } else if let Some((a, b)) = hcore {
                    if toward_top { b } else { a }
                } else {
                    cy
                }
            } else if let Some((a, b)) = hcore {
                if toward_top { b } else { a }
            } else {
                cy
            };
            if toward_top {
                fill(out, x0, 0, x1, reach);
            } else {
                fill(out, x0, reach, x1, h);
            }
        }
    }
}

/// `n` dashes along the middle of the cell, `t` thick, with a gap at each end of each dash so
/// that a row of them stays dashed across cells.
fn dashes(n: i32, vertical: bool, t: i32, w: i32, h: i32, out: &mut Vec<Piece>) {
    let len = if vertical { h } else { w };
    let gap = (len / (n * 4)).max(1);
    for i in 0..n {
        let (s, e) = (i * len / n + gap / 2, (i + 1) * len / n - (gap - gap / 2));
        if vertical {
            let x = lane(w, t);
            fill(out, x, s, x + t, e);
        } else {
            let y = lane(h, t);
            fill(out, s, y, e, y + t);
        }
    }
}

/// The rounded corners ╭ ╮ ╯ ╰: a quarter ring that meets a light line at the middle of the two
/// edges it joins, and the straight piece that is left when the cell is not square.
fn arc(code: u32, w: i32, h: i32, light: i32, out: &mut Vec<Piece>) {
    let (vx, hy) = (lane(w, light), lane(h, light));
    let t = light as f32;
    let (lx, ly) = (vx as f32 + t / 2.0, hy as f32 + t / 2.0);
    // Which way the two arms go: right or left, down or up.
    let (to_right, to_down) = match code {
        0x256D => (true, true),
        0x256E => (false, true),
        0x256F => (false, false),
        _ => (true, false),
    };
    let rx = if to_right { w as f32 - lx } else { lx };
    let ry = if to_down { h as f32 - ly } else { ly };
    let r = rx.min(ry);
    let (sx, sy) = (
        if to_right { 1.0 } else { -1.0 },
        if to_down { 1.0 } else { -1.0 },
    );
    let (cx, cy) = (lx + sx * r, ly + sy * r);
    // The quarter of the ring towards the line's middle from the centre.
    out.push(Piece::Arc {
        cx,
        cy,
        r,
        t,
        sx: -sx,
        sy: -sy,
    });
    // The straight rest of each arm, from where the ring ends to the edge.
    let (ax, ay) = (cx.round() as i32, cy.round() as i32);
    if to_right {
        fill(out, ax, hy, w, hy + light);
    } else {
        fill(out, 0, hy, ax, hy + light);
    }
    if to_down {
        fill(out, vx, ay, vx + light, h);
    } else {
        fill(out, vx, 0, vx + light, ay);
    }
}

/// The block elements: eighths from the bottom and from the left, halves, the shades and the
/// quadrants.
fn blocks(code: u32, w: i32, h: i32, out: &mut Vec<Piece>) {
    let eighth = |len: i32, k: i32| (len * k + 4) / 8;
    // The middle: the left half is `0..mx`, the lower half `my..h`, as the eighths count them.
    let (mx, my) = (eighth(w, 4), h - eighth(h, 4));
    match code {
        0x2580 => fill(out, 0, 0, w, my),
        // Lower one eighth to seven eighths, then the full block.
        0x2581..=0x2588 => fill(out, 0, h - eighth(h, (code - 0x2580) as i32), w, h),
        // Left seven eighths down to one eighth.
        0x2589..=0x258F => fill(out, 0, 0, eighth(w, (0x2590 - code) as i32), h),
        0x2590 => fill(out, mx, 0, w, h),
        0x2591..=0x2593 => {
            let alpha = (code - 0x2590) as f32 / 4.0;
            out.push(Piece::Fill {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
                alpha,
            });
        }
        0x2594 => fill(out, 0, 0, w, eighth(h, 1)),
        0x2595 => fill(out, w - eighth(w, 1), 0, w, h),
        _ => {
            // Quadrants: upper left, upper right, lower left, lower right.
            const UL: u8 = 1;
            const UR: u8 = 2;
            const LL: u8 = 4;
            const LR: u8 = 8;
            let q = match code {
                0x2596 => LL,
                0x2597 => LR,
                0x2598 => UL,
                0x2599 => UL | LL | LR,
                0x259A => UL | LR,
                0x259B => UL | UR | LL,
                0x259C => UL | UR | LR,
                0x259D => UR,
                0x259E => UR | LL,
                _ => UR | LL | LR,
            };
            for (bit, x0, y0, x1, y1) in [
                (UL, 0, 0, mx, my),
                (UR, mx, 0, w, my),
                (LL, 0, my, mx, h),
                (LR, mx, my, w, h),
            ] {
                if q & bit != 0 {
                    fill(out, x0, y0, x1, y1);
                }
            }
        }
    }
}

/// How much of the device pixel whose top left corner is `px, py` (cell-relative) a ring or a
/// diagonal covers, from 0 to 1: one minus the distance of the pixel's centre from the stroke's
/// edge, held to 0..=1. Zero for a rectangle (those are filled whole).
pub(super) fn coverage(piece: &Piece, px: i32, py: i32, w: i32, h: i32) -> f32 {
    let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
    match *piece {
        Piece::Fill { .. } => 0.0,
        Piece::Arc {
            cx,
            cy,
            r,
            t,
            sx,
            sy,
        } => {
            let (dx, dy) = (x - cx, y - cy);
            if dx * sx < -0.5 || dy * sy < -0.5 {
                return 0.0;
            }
            let d = dx.hypot(dy);
            (t / 2.0 + 0.5 - (d - r).abs()).clamp(0.0, 1.0)
        }
        Piece::Diagonal { rising, t } => {
            let (w, h) = (w as f32, h as f32);
            // The line through (0, h) and (w, 0), or (0, 0) and (w, h).
            let d = if rising {
                (h * x + w * y - w * h).abs()
            } else {
                (h * x - w * y).abs()
            };
            let d = d / w.hypot(h);
            (t / 2.0 + 0.5 - d).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cell's pixels as a grid of booleans: which a character covers (alpha 1 fills, or
    /// coverage over a half).
    fn raster(ch: char, w: i32, h: i32, light: i32) -> Vec<Vec<bool>> {
        let mut g = vec![vec![false; w as usize]; h as usize];
        for p in pieces(ch, w, h, light).unwrap() {
            for (y, row) in g.iter_mut().enumerate() {
                for (x, px) in row.iter_mut().enumerate() {
                    let (xi, yi) = (x as i32, y as i32);
                    *px |= match p {
                        Piece::Fill {
                            x0,
                            y0,
                            x1,
                            y1,
                            alpha,
                        } => alpha >= 1.0 && xi >= x0 && xi < x1 && yi >= y0 && yi < y1,
                        _ => coverage(&p, xi, yi, w, h) > 0.5,
                    };
                }
            }
        }
        g
    }

    /// The covered positions along one edge: `top`, `right`, `bottom` or `left`.
    fn edge(g: &[Vec<bool>], side: &str) -> Vec<bool> {
        let (h, w) = (g.len(), g[0].len());
        match side {
            "top" => g[0].clone(),
            "bottom" => g[h - 1].clone(),
            "left" => (0..h).map(|y| g[y][0]).collect(),
            _ => (0..h).map(|y| g[y][w - 1]).collect(),
        }
    }

    #[test]
    fn only_the_two_blocks_of_unicode_are_drawn_here() {
        assert!(is_box_char('─') && is_box_char('╬') && is_box_char('█') && is_box_char('▟'));
        assert!(!is_box_char('a') && !is_box_char('\u{24FF}') && !is_box_char('\u{25A0}'));
        assert_eq!(pieces('a', 8, 16, 1), None);
        assert_eq!(pieces('─', 0, 16, 1), None);
        assert_eq!(pieces('─', 8, -1, 1), None);
        // Every character of the range gives something to draw, at many sizes.
        for code in 0x2500..=0x259F {
            let ch = char::from_u32(code).unwrap();
            for (w, h, light) in [
                (1, 1, 1),
                (3, 5, 1),
                (7, 15, 1),
                (8, 17, 1),
                (13, 26, 2),
                (20, 41, 3),
            ] {
                let p = pieces(ch, w, h, light).unwrap();
                assert!(!p.is_empty() || w < 7, "{ch} {w}x{h}");
                for piece in p {
                    if let Piece::Fill { x0, y0, x1, y1, .. } = piece {
                        assert!(x0 >= 0 && y0 >= 0 && x1 <= w && y1 <= h, "{ch} {piece:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn lines_meet_the_line_of_the_next_cell_with_no_gap() {
        // A character's arm reaches its edge at the same pixels as the matching arm of the
        // character beside it, for every pair of line characters with arms of one weight.
        for (w, h, light) in [(8, 17, 1), (9, 18, 1), (13, 26, 2), (12, 25, 2)] {
            let all: Vec<_> = (0x2500u32..=0x257F)
                .map(|c| raster(char::from_u32(c).unwrap(), w, h, light))
                .collect();
            for a in 0x2500u32..=0x257F {
                let ca = char::from_u32(a).unwrap();
                let arms_a = ARMS[(a - 0x2500) as usize];
                if arms_a == [0; 4] {
                    continue;
                }
                let ga = &all[(a - 0x2500) as usize];
                for b in 0x2500u32..=0x257F {
                    let arms_b = ARMS[(b - 0x2500) as usize];
                    let gb = &all[(b - 0x2500) as usize];
                    // a on the left of b: a's right arm against b's left arm.
                    if arms_a[1] != 0 && arms_a[1] == arms_b[3] {
                        assert_eq!(edge(ga, "right"), edge(gb, "left"), "{ca} beside {b:x}");
                    }
                    // a above b: a's down arm against b's up arm.
                    if arms_a[2] != 0 && arms_a[2] == arms_b[0] {
                        assert_eq!(edge(ga, "bottom"), edge(gb, "top"), "{ca} above {b:x}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_line_with_an_arm_covers_its_edge_and_one_without_leaves_it_empty() {
        for a in 0x2500u32..=0x257F {
            let arms = ARMS[(a - 0x2500) as usize];
            if arms == [0; 4] {
                continue;
            }
            let g = raster(char::from_u32(a).unwrap(), 9, 18, 1);
            for (side, weight) in ["top", "right", "bottom", "left"].iter().zip(arms) {
                let covered = edge(&g, side).iter().filter(|&&b| b).count();
                let want = match weight {
                    0 => 0,
                    1 => 1,
                    2 => 2,
                    _ => 2,
                };
                assert_eq!(covered, want, "{} {side}", char::from_u32(a).unwrap());
            }
        }
    }

    #[test]
    fn corners_and_crossings_are_filled_and_double_corners_are_open_inside() {
        // ┼: the middle pixel is covered; ┌: the corner pixel where the arms meet is covered.
        let g = raster('┼', 9, 18, 1);
        assert!(g[8][4]);
        let g = raster('┌', 9, 18, 1);
        assert!(g[8][4] && g[8][8] && g[17][4] && !g[0][4] && !g[8][0]);
        // ╔: the outer stroke turns the corner and the inner one does too, with a gap between.
        let g = raster('╔', 9, 18, 1);
        let (outer_y, inner_y, outer_x, inner_x) = (7, 9, 3, 5);
        assert!(g[outer_y][outer_x] && g[outer_y][8] && g[17][outer_x]);
        assert!(g[inner_y][inner_x] && g[inner_y][8] && g[17][inner_x]);
        assert!(
            !g[inner_y][outer_x + 1] && !g[8][inner_x],
            "the inside stays open"
        );
        // ╬: four corners, the middle open.
        let g = raster('╬', 9, 18, 1);
        assert!(!g[8][4]);
        assert!(g[outer_y][0] && g[inner_y][0] && g[0][outer_x] && g[0][inner_x]);
        // ═ is two unbroken lines.
        let g = raster('═', 9, 18, 1);
        assert!((0..9).all(|x| g[outer_y][x] && g[inner_y][x] && !g[8][x]));
    }

    #[test]
    fn blocks_split_the_cell_at_the_same_pixels_in_every_cell() {
        let (w, h) = (9, 19);
        let g = raster('█', w, h, 1);
        assert!(g.iter().all(|r| r.iter().all(|&b| b)));
        let lower = raster('▄', w, h, 1);
        let upper = raster('▀', w, h, 1);
        // The two halves tile the cell exactly.
        for y in 0..h as usize {
            for x in 0..w as usize {
                assert!(lower[y][x] != upper[y][x], "{x},{y}");
            }
        }
        let left = raster('▌', w, h, 1);
        let right = raster('▐', w, h, 1);
        for y in 0..h as usize {
            for x in 0..w as usize {
                assert!(left[y][x] != right[y][x]);
            }
        }
        // The quadrants that make ▙ and those of ▝ tile the cell too.
        let a = raster('▙', w, h, 1);
        let b = raster('▝', w, h, 1);
        for y in 0..h as usize {
            for x in 0..w as usize {
                assert!(a[y][x] != b[y][x]);
            }
        }
        // A shade is one translucent fill of the whole cell.
        assert_eq!(
            pieces('▒', w, h, 1).unwrap(),
            vec![Piece::Fill {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
                alpha: 0.5
            }]
        );
    }

    #[test]
    fn a_rounded_corner_meets_straight_lines_at_its_two_edges() {
        let (w, h) = (9, 18);
        let corner = raster('╭', w, h, 1);
        let across = raster('─', w, h, 1);
        let down = raster('│', w, h, 1);
        assert_eq!(edge(&corner, "right"), edge(&across, "left"));
        assert_eq!(edge(&corner, "bottom"), edge(&down, "top"));
        assert!(
            edge(&corner, "top").iter().all(|b| !b) && edge(&corner, "left").iter().all(|b| !b)
        );
        let corner = raster('╯', w, h, 1);
        assert_eq!(edge(&corner, "left"), edge(&across, "right"));
        assert_eq!(edge(&corner, "top"), edge(&down, "bottom"));
    }

    #[test]
    fn a_diagonal_runs_from_corner_to_corner() {
        let g = raster('╱', 9, 18, 1);
        assert!(g[17][0] && g[0][8] && !g[0][0] && !g[17][8]);
        let g = raster('╲', 9, 18, 1);
        assert!(g[0][0] && g[17][8] && !g[17][0]);
        let g = raster('╳', 9, 18, 1);
        assert!(g[0][0] && g[17][8] && g[17][0] && g[0][8]);
    }

    #[test]
    fn dashes_leave_gaps_and_stay_on_the_line() {
        let g = raster('┄', 12, 18, 1);
        let y = lane(18, 1) as usize;
        let row: Vec<bool> = g[y].clone();
        let runs = row.windows(2).filter(|p| p[0] && !p[1]).count() + usize::from(row[11]);
        assert_eq!(runs, 3, "{row:?}");
        assert!(
            g.iter()
                .enumerate()
                .all(|(i, r)| i == y || r.iter().all(|b| !b))
        );
        let g = raster('╎', 9, 20, 1);
        let col: Vec<bool> = g.iter().map(|r| r[lane(9, 1) as usize]).collect();
        let runs = col.windows(2).filter(|p| p[0] && !p[1]).count() + usize::from(col[19]);
        assert_eq!(runs, 2);
    }
}
