//! Fuzzy matching for pickers: a case-insensitive subsequence match with a score, and the
//! positions of the matched characters for highlighting. Std only, no window system.
//!
//! The best alignment of the query in the candidate is found by dynamic programming, so a query
//! prefers word starts ("fb" in "foo bar"), runs of consecutive characters and a match at the
//! start. A candidate that begins with the whole query beats every one that does not.
//!
//! `positions` are indices of `char`s in the candidate (cut to [`MAX_CANDIDATE`] chars), not
//! bytes, so a caller that draws the label one char at a time can use them as they are.

/// A candidate longer than this many chars is cut before matching.
pub const MAX_CANDIDATE: usize = 256;
/// A query longer than this many chars is cut.
pub const MAX_QUERY: usize = 64;

const MATCH: i32 = 10;
const START_BONUS: i32 = 10;
const WORD_BONUS: i32 = 8;
const CONSECUTIVE: i32 = 6;
const GAP_START: i32 = 3;
const GAP_EXTEND: i32 = 1;
const LEADING_CAP: i32 = 5;
/// Added per query char to a candidate that starts with the query; larger than any other
/// per-char gain (10 + 8 + 6), so a prefix always wins.
const PREFIX_PER_CHAR: i32 = 30;
const PREFIX_BASE: i32 = 100;

const NONE: i32 = i32::MIN / 2;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Match {
    /// Higher is better; only comparable between candidates of one query.
    pub score: i32,
    /// Char indices of the matched characters, ascending; empty for an empty query.
    pub positions: Vec<u32>,
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The bonus for matching at `j`: the start of the candidate, or the start of a word (after a
/// separator, a lower to upper case change, or a letter to digit change).
fn bonus(chars: &[char], j: usize) -> i32 {
    if j == 0 {
        return START_BONUS;
    }
    let (p, c) = (chars[j - 1], chars[j]);
    let sep = |c: char| !c.is_alphanumeric();
    if (sep(p) && !sep(c)) || (p.is_lowercase() && c.is_uppercase()) {
        WORD_BONUS
    } else if p.is_alphabetic() && c.is_ascii_digit() {
        WORD_BONUS / 2
    } else {
        0
    }
}

/// Matches `query` against `candidate`. `None` when the query is not a subsequence of it. An
/// empty query matches everything with score 0.
pub fn score(query: &str, candidate: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().take(MAX_QUERY).map(fold).collect();
    if q.is_empty() {
        return Some(Match::default());
    }
    let raw: Vec<char> = candidate.chars().take(MAX_CANDIDATE).collect();
    let low: Vec<char> = raw.iter().map(|&c| fold(c)).collect();
    score_chars(&q, &raw, &low)
}

fn score_chars(q: &[char], raw: &[char], low: &[char]) -> Option<Match> {
    let (m, n) = (q.len(), low.len());
    if m > n {
        return None;
    }
    // Cheap rejection first: most candidates are not a subsequence at all.
    let mut at = 0;
    for &c in low {
        if at < m && c == q[at] {
            at += 1;
        }
    }
    if at < m {
        return None;
    }

    // dp[i * n + j]: the best score with q[i] matched at j; from[..]: where q[i - 1] was.
    let mut dp = vec![NONE; m * n];
    let mut from = vec![usize::MAX; m * n];
    for j in 0..n {
        if low[j] == q[0] {
            dp[j] = MATCH + bonus(raw, j) - (j as i32).min(LEADING_CAP);
        }
    }
    for i in 1..m {
        // `run`: the best earlier match plus the cost of the gap up to the current j, over
        // matches of q[i - 1] at least two before j; `run_at` is where it came from.
        let (mut run, mut run_at) = (NONE, usize::MAX);
        for j in 0..n {
            if j >= 2 {
                let cand = dp[(i - 1) * n + j - 2];
                // The match at j - 2 now has one char between it and j.
                run -= GAP_EXTEND;
                if cand > NONE && cand - GAP_START > run {
                    run = cand - GAP_START;
                    run_at = j - 2;
                }
            }
            if low[j] != q[i] || j < i {
                continue;
            }
            let mut best = NONE;
            let mut best_at = usize::MAX;
            if j >= 1 && dp[(i - 1) * n + j - 1] > NONE {
                best = dp[(i - 1) * n + j - 1] + CONSECUTIVE;
                best_at = j - 1;
            }
            if run > best {
                best = run;
                best_at = run_at;
            }
            if best > NONE / 2 {
                dp[i * n + j] = best + MATCH + bonus(raw, j);
                from[i * n + j] = best_at;
            }
        }
    }
    let last = (m - 1) * n;
    let (mut end, mut best) = (usize::MAX, NONE);
    for j in 0..n {
        if dp[last + j] > best {
            best = dp[last + j];
            end = j;
        }
    }
    if end == usize::MAX {
        return None;
    }
    let mut positions = vec![0u32; m];
    let mut j = end;
    for i in (0..m).rev() {
        positions[i] = j as u32;
        if i > 0 {
            j = from[i * n + j];
        }
    }
    if low[..m] == *q {
        best += PREFIX_BASE + PREFIX_PER_CHAR * m as i32;
        // The alignment of a prefix is the prefix itself.
        positions = (0..m as u32).collect();
    }
    Some(Match {
        score: best,
        positions,
    })
}

/// The best `limit` of `items` for `query`, best first: higher score, then the shorter
/// candidate, then the earlier one. Pairs are (index into `items`, match). An empty query returns
/// the first `limit` items in order.
pub fn rank(query: &str, items: &[&str], limit: usize) -> Vec<(usize, Match)> {
    if query.is_empty() {
        return (0..items.len().min(limit))
            .map(|i| (i, Match::default()))
            .collect();
    }
    let q: Vec<char> = query.chars().take(MAX_QUERY).map(fold).collect();
    let mut out: Vec<(usize, Match, usize)> = Vec::new();
    let (mut raw, mut low) = (Vec::new(), Vec::new());
    for (i, item) in items.iter().enumerate() {
        raw.clear();
        raw.extend(item.chars().take(MAX_CANDIDATE));
        low.clear();
        low.extend(raw.iter().map(|&c| fold(c)));
        if let Some(m) = score_chars(&q, &raw, &low) {
            out.push((i, m, raw.len()));
        }
    }
    out.sort_by(|a, b| {
        b.1.score
            .cmp(&a.1.score)
            .then(a.2.cmp(&b.2))
            .then(a.0.cmp(&b.0))
    });
    out.truncate(limit);
    out.into_iter().map(|(i, m, _)| (i, m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(q: &str, items: &[&str]) -> Vec<usize> {
        rank(q, items, usize::MAX)
            .into_iter()
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn subsequence_and_case() {
        assert!(score("fb", "foo bar").is_some());
        assert!(score("FB", "foo bar").is_some());
        assert!(score("bf", "foo bar").is_none());
        assert!(score("abc", "ab").is_none());
        assert_eq!(score("", "anything"), Some(Match::default()));
        assert_eq!(score("x", ""), None);
    }

    #[test]
    fn positions_are_char_indices() {
        let m = score("fb", "foo bar").unwrap();
        assert_eq!(m.positions, [0, 4]);
        let m = score("bär", "Foo Bär baz").unwrap();
        assert_eq!(m.positions, [4, 5, 6]);
        let m = score("sch", "Größe Schrift").unwrap();
        assert_eq!(m.positions, [6, 7, 8]);
    }

    #[test]
    fn word_starts_beat_scattered_letters() {
        // "of" is the start of both words in "Open File" and scattered in "Soft wrap".
        assert_eq!(order("of", &["Soft wrap", "Open File"]), [1, 0]);
        // Camel case humps.
        assert_eq!(order("gt", &["gotoLine", "goToLine"]), [1, 0]);
        // Separators of paths.
        assert_eq!(order("sm", &["assembly.rs", "src/main.rs"]), [1, 0]);
    }

    #[test]
    fn consecutive_beats_split() {
        assert_eq!(order("fin", &["fxixn", "find"]), [1, 0]);
        assert_eq!(order("wrap", &["wxrxaxp", "swrap"]), [1, 0]);
    }

    #[test]
    fn a_prefix_wins() {
        // A longer query keeps the rule: the prefix beats a candidate made of word starts.
        let items = ["Close all other tabs", "Close all"];
        assert_eq!(order("close all", &items), [1, 0]);
        let items = ["Reopen closed tab", "Open recent", "Open"];
        assert_eq!(order("open", &items), [2, 1, 0]);
        let items = ["File: New Entry", "find in files"];
        assert_eq!(order("fi", &items), [1, 0]);
    }

    #[test]
    fn ties_go_to_the_shorter_then_the_earlier() {
        let items = ["save all", "save", "save", "save file"];
        assert_eq!(order("sa", &items), [1, 2, 0, 3]);
    }

    #[test]
    fn empty_query_keeps_order_and_limit() {
        let items = ["b", "a", "c"];
        assert_eq!(order("", &items), [0, 1, 2]);
        let r = rank("", &items, 2);
        assert_eq!(r.len(), 2);
        assert!(
            r.iter()
                .all(|(_, m)| m.positions.is_empty() && m.score == 0)
        );
        assert_eq!(rank("a", &items, 0), []);
        assert_eq!(rank("zz", &items, 5), []);
    }

    #[test]
    fn limit_cuts_after_sorting() {
        let items = ["xaxbx", "ab", "xxab", "a b"];
        let r = rank("ab", &items, 2);
        assert_eq!(r.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn long_candidates_are_cut() {
        let long = format!("{}z", "a".repeat(300));
        assert!(score("az", &long).is_none());
        let m = score("aa", &long).unwrap();
        assert_eq!(m.positions, [0, 1]);
        assert!(score(&"a".repeat(80), &"a".repeat(80)).is_some());
    }

    #[test]
    fn positions_are_ascending_and_match_the_query() {
        let q = "mdd";
        let cand = "markdown viewer: dark mode";
        let m = score(q, cand).unwrap();
        let chars: Vec<char> = cand.chars().collect();
        assert!(m.positions.windows(2).all(|w| w[0] < w[1]));
        for (p, c) in m.positions.iter().zip(q.chars()) {
            assert_eq!(chars[*p as usize].to_lowercase().next().unwrap(), c);
        }
    }
}
