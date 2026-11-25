//! How many cells a character takes: one, two for the wide characters of East Asian scripts and
//! for emoji, none for marks that combine with the character before them.
//!
//! The tables are written by hand and are small on purpose: the blocks that are wide as a whole,
//! the emoji that every system draws wide, and the common combining marks. They are not the
//! Unicode data files, and where a block mixes widths the table takes the width most of it has.
//! A program that draws with exact Unicode widths can therefore disagree with this about a rare
//! character, and its line is then off by a cell; see the crate's list of gaps.

/// Ranges of characters that take no cell: combining marks, zero-width spaces and joiners,
/// variation selectors, direction marks. Sorted, inclusive.
const ZERO: &[(u32, u32)] = &[
    (0x0080, 0x009F),
    (0x00AD, 0x00AD),
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0900, 0x0902),
    (0x093C, 0x093C),
    (0x0941, 0x0948),
    (0x094D, 0x094D),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x200B, 0x200F),
    (0x2028, 0x202E),
    (0x2060, 0x2064),
    (0x20D0, 0x20F0),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0xFEFF, 0xFEFF),
    (0x1F3FB, 0x1F3FF),
    (0xE0001, 0xE007F),
    (0xE0100, 0xE01EF),
];

/// Ranges of characters that take two cells. Sorted, inclusive.
const WIDE: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x231A, 0x231B),
    (0x2329, 0x232A),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x2E80, 0x303E),
    (0x3041, 0x3098),
    (0x309B, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F200, 0x1F2FF),
    (0x1F300, 0x1F3FA),
    (0x1F400, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F900, 0x1F9FF),
    (0x1FA70, 0x1FAFF),
    (0x20000, 0x3FFFD),
];

fn within(table: &[(u32, u32)], c: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The cells `c` takes on a terminal: 0, 1 or 2. ASCII is answered without a search.
pub fn char_width(c: char) -> u8 {
    let c = c as u32;
    if c < 0x80 {
        return 1;
    }
    if within(ZERO, c) {
        0
    } else if within(WIDE, c) {
        2
    } else {
        1
    }
}

/// The cells a string takes: the sum over its characters.
pub fn str_width(s: &str) -> usize {
    s.chars().map(|c| char_width(c) as usize).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_sorted_and_do_not_overlap() {
        for table in [ZERO, WIDE] {
            for pair in table.windows(2) {
                assert!(pair[0].1 < pair[1].0, "{:x?} and {:x?}", pair[0], pair[1]);
            }
            for &(lo, hi) in table {
                assert!(lo <= hi, "{lo:x}..{hi:x}");
            }
        }
        // And no character is in both.
        for &(lo, hi) in ZERO {
            for c in lo..=hi {
                assert!(!within(WIDE, c), "{c:x} is both wide and zero");
            }
        }
    }

    #[test]
    fn widths_of_the_usual_characters() {
        for c in ['a', ' ', '~', 'é', 'ß', 'Ж', '─', '│', '→', '€', '\u{fffd}'] {
            assert_eq!(char_width(c), 1, "{c}");
        }
        for c in [
            '漢', 'あ', 'カ', '한', '，', 'Ａ', '😀', '🚀', '🦀', '⌚', '✅',
        ] {
            assert_eq!(char_width(c), 2, "{c}");
        }
        for c in [
            '\u{0301}',
            '\u{200d}',
            '\u{fe0f}',
            '\u{200b}',
            '\u{feff}',
            '\u{1f3fd}',
        ] {
            assert_eq!(char_width(c), 0, "{:x}", c as u32);
        }
        assert_eq!(str_width("ab漢字e\u{0301}"), 7);
        assert_eq!(str_width(""), 0);
    }
}
