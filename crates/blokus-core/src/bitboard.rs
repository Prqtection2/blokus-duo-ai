//! 256-bit bitboard over a 16x16 padded grid. Playable region is 14x14
//! (rows 0..14, cols 0..14). Rows 14-15 and cols 14-15 are guard bands that
//! catch wrap from directional shifts; they are never legal placement targets.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitXor, Not};

pub const ROWS: usize = 16;
pub const COLS: usize = 16;
pub const PLAY_ROWS: usize = 14;
pub const PLAY_COLS: usize = 14;
pub const NUM_CELLS: usize = 256;

#[inline]
pub const fn bit_index(row: usize, col: usize) -> usize {
    row * ROWS + col
}

#[inline]
pub const fn from_bit_index(idx: usize) -> (usize, usize) {
    (idx / ROWS, idx % ROWS)
}

#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bitboard(pub [u64; 4]);

impl Bitboard {
    pub const EMPTY: Self = Self([0; 4]);

    /// Mask of the 14x14 playable region.
    pub const PLAYABLE: Self = Self([
        0x3FFF_3FFF_3FFF_3FFF, // rows 0..4
        0x3FFF_3FFF_3FFF_3FFF, // rows 4..8
        0x3FFF_3FFF_3FFF_3FFF, // rows 8..12
        0x0000_0000_3FFF_3FFF, // rows 12..14, then guard rows 14-15 (zero)
    ]);

    /// Bits in column 0 cleared in every row.
    pub const NOT_COL_0: Self = Self([
        0xFFFE_FFFE_FFFE_FFFE,
        0xFFFE_FFFE_FFFE_FFFE,
        0xFFFE_FFFE_FFFE_FFFE,
        0xFFFE_FFFE_FFFE_FFFE,
    ]);

    /// Bits in column 15 cleared in every row (guard column).
    pub const NOT_COL_15: Self = Self([
        0x7FFF_7FFF_7FFF_7FFF,
        0x7FFF_7FFF_7FFF_7FFF,
        0x7FFF_7FFF_7FFF_7FFF,
        0x7FFF_7FFF_7FFF_7FFF,
    ]);

    #[inline]
    pub fn is_empty(self) -> bool {
        (self.0[0] | self.0[1] | self.0[2] | self.0[3]) == 0
    }

    #[inline]
    pub fn count_ones(self) -> u32 {
        self.0[0].count_ones()
            + self.0[1].count_ones()
            + self.0[2].count_ones()
            + self.0[3].count_ones()
    }

    #[inline]
    pub fn set_bit(&mut self, idx: usize) {
        debug_assert!(idx < NUM_CELLS);
        self.0[idx / 64] |= 1u64 << (idx % 64);
    }

    #[inline]
    pub fn clear_bit(&mut self, idx: usize) {
        debug_assert!(idx < NUM_CELLS);
        self.0[idx / 64] &= !(1u64 << (idx % 64));
    }

    #[inline]
    pub fn get_bit(self, idx: usize) -> bool {
        debug_assert!(idx < NUM_CELLS);
        (self.0[idx / 64] >> (idx % 64)) & 1 != 0
    }

    pub fn from_cells<I: IntoIterator<Item = (usize, usize)>>(cells: I) -> Self {
        let mut bb = Self::EMPTY;
        for (r, c) in cells {
            bb.set_bit(bit_index(r, c));
        }
        bb
    }

    pub fn iter_bits(self) -> BitIter {
        BitIter { bb: self }
    }

    // Multi-word shifts. n must be in [0, 64).
    #[inline]
    fn shl(self, n: u32) -> Self {
        if n == 0 {
            return self;
        }
        debug_assert!(n < 64);
        let inv = 64 - n;
        let [a, b, c, d] = self.0;
        Bitboard([
            a << n,
            (b << n) | (a >> inv),
            (c << n) | (b >> inv),
            (d << n) | (c >> inv),
        ])
    }

    #[inline]
    fn shr(self, n: u32) -> Self {
        if n == 0 {
            return self;
        }
        debug_assert!(n < 64);
        let inv = 64 - n;
        let [a, b, c, d] = self.0;
        Bitboard([
            (a >> n) | (b << inv),
            (b >> n) | (c << inv),
            (c >> n) | (d << inv),
            d >> n,
        ])
    }

    // Direction shifts. Source pre-mask kills bits that would wrap across rows.
    #[inline] pub fn shift_n(self)  -> Self { self.shr(16) }
    #[inline] pub fn shift_s(self)  -> Self { self.shl(16) }
    #[inline] pub fn shift_e(self)  -> Self { (self & Self::NOT_COL_15).shl(1) }
    #[inline] pub fn shift_w(self)  -> Self { (self & Self::NOT_COL_0).shr(1) }
    #[inline] pub fn shift_ne(self) -> Self { (self & Self::NOT_COL_15).shr(15) }
    #[inline] pub fn shift_nw(self) -> Self { (self & Self::NOT_COL_0).shr(17) }
    #[inline] pub fn shift_se(self) -> Self { (self & Self::NOT_COL_15).shl(17) }
    #[inline] pub fn shift_sw(self) -> Self { (self & Self::NOT_COL_0).shl(15) }

    #[inline]
    pub fn ortho_neighbors(self) -> Self {
        self.shift_n() | self.shift_s() | self.shift_e() | self.shift_w()
    }

    #[inline]
    pub fn diag_neighbors(self) -> Self {
        self.shift_ne() | self.shift_nw() | self.shift_se() | self.shift_sw()
    }
}

impl BitAnd for Bitboard {
    type Output = Self;
    #[inline]
    fn bitand(self, o: Self) -> Self {
        Bitboard([
            self.0[0] & o.0[0],
            self.0[1] & o.0[1],
            self.0[2] & o.0[2],
            self.0[3] & o.0[3],
        ])
    }
}
impl BitOr for Bitboard {
    type Output = Self;
    #[inline]
    fn bitor(self, o: Self) -> Self {
        Bitboard([
            self.0[0] | o.0[0],
            self.0[1] | o.0[1],
            self.0[2] | o.0[2],
            self.0[3] | o.0[3],
        ])
    }
}
impl BitXor for Bitboard {
    type Output = Self;
    #[inline]
    fn bitxor(self, o: Self) -> Self {
        Bitboard([
            self.0[0] ^ o.0[0],
            self.0[1] ^ o.0[1],
            self.0[2] ^ o.0[2],
            self.0[3] ^ o.0[3],
        ])
    }
}
impl Not for Bitboard {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        Bitboard([!self.0[0], !self.0[1], !self.0[2], !self.0[3]])
    }
}

impl fmt::Debug for Bitboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Bitboard:")?;
        for r in 0..PLAY_ROWS {
            for c in 0..PLAY_COLS {
                let ch = if self.get_bit(bit_index(r, c)) { 'X' } else { '.' };
                write!(f, "{} ", ch)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

pub struct BitIter {
    bb: Bitboard,
}
impl Iterator for BitIter {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        for w in 0..4 {
            let word = self.bb.0[w];
            if word != 0 {
                let bit = word.trailing_zeros() as usize;
                self.bb.0[w] &= word - 1;
                return Some(w * 64 + bit);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(cells: &[(usize, usize)]) -> Bitboard {
        Bitboard::from_cells(cells.iter().copied())
    }

    #[test]
    fn playable_mask_has_196_bits() {
        assert_eq!(Bitboard::PLAYABLE.count_ones(), 196);
        for r in 0..PLAY_ROWS {
            for c in 0..PLAY_COLS {
                assert!(Bitboard::PLAYABLE.get_bit(bit_index(r, c)));
            }
        }
        // Guard row/col never set.
        for c in 0..16 {
            assert!(!Bitboard::PLAYABLE.get_bit(bit_index(14, c)));
            assert!(!Bitboard::PLAYABLE.get_bit(bit_index(15, c)));
        }
        for r in 0..16 {
            assert!(!Bitboard::PLAYABLE.get_bit(bit_index(r, 14)));
            assert!(!Bitboard::PLAYABLE.get_bit(bit_index(r, 15)));
        }
    }

    #[test]
    fn shift_n_drops_at_top_edge() {
        assert!(bb(&[(0, 5)]).shift_n().is_empty());
    }

    #[test]
    fn shift_s_at_bottom_lands_in_guard() {
        // (13, 5) shifts south to (14, 5), which is not in PLAYABLE.
        let s = bb(&[(13, 5)]).shift_s();
        assert!((s & Bitboard::PLAYABLE).is_empty());
    }

    #[test]
    fn shift_e_at_right_edge_does_not_wrap() {
        let s = bb(&[(5, 13)]).shift_e();
        // Goes to (5, 14) which is guard — masked out of playable.
        assert!((s & Bitboard::PLAYABLE).is_empty());
        // And critically: nothing in (6, 0).
        assert!(!s.get_bit(bit_index(6, 0)));
    }

    #[test]
    fn shift_w_at_left_edge_does_not_wrap() {
        let s = bb(&[(5, 0)]).shift_w();
        assert!(s.is_empty(), "shift_w at col 0 must drop entirely");
    }

    #[test]
    fn shift_ne_at_right_edge_does_not_wrap() {
        let s = bb(&[(5, 13)]).shift_ne();
        assert!((s & Bitboard::PLAYABLE).is_empty());
        // Critically: not (4, 0).
        assert!(!s.get_bit(bit_index(4, 0)));
    }

    #[test]
    fn shift_nw_at_left_edge_does_not_wrap() {
        let s = bb(&[(5, 0)]).shift_nw();
        assert!(s.is_empty());
    }

    #[test]
    fn shift_se_at_right_edge_does_not_wrap() {
        let s = bb(&[(5, 13)]).shift_se();
        assert!((s & Bitboard::PLAYABLE).is_empty());
        // Not (7, 0).
        assert!(!s.get_bit(bit_index(7, 0)));
    }

    #[test]
    fn shift_sw_at_left_edge_does_not_wrap() {
        let s = bb(&[(5, 0)]).shift_sw();
        assert!(s.is_empty());
    }

    #[test]
    fn ortho_neighbors_interior_cell() {
        let n = bb(&[(5, 5)]).ortho_neighbors();
        let want = bb(&[(4, 5), (6, 5), (5, 4), (5, 6)]);
        assert_eq!(n, want);
    }

    #[test]
    fn diag_neighbors_interior_cell() {
        let n = bb(&[(5, 5)]).diag_neighbors();
        let want = bb(&[(4, 4), (4, 6), (6, 4), (6, 6)]);
        assert_eq!(n, want);
    }

    #[test]
    fn ortho_neighbors_at_corner_clipped() {
        // (0, 0): orthogonal neighbors are (1, 0) and (0, 1). (-1, 0) and (0, -1) drop.
        let n = bb(&[(0, 0)]).ortho_neighbors() & Bitboard::PLAYABLE;
        let want = bb(&[(1, 0), (0, 1)]);
        assert_eq!(n, want);
    }

    #[test]
    fn iter_bits_returns_set_indices_in_order() {
        let cells = [(0, 0), (5, 5), (13, 13), (7, 3)];
        let b = bb(&cells);
        let got: Vec<(usize, usize)> = b.iter_bits().map(from_bit_index).collect();
        let mut want: Vec<(usize, usize)> = cells.to_vec();
        want.sort();
        let mut got_sorted = got.clone();
        got_sorted.sort();
        assert_eq!(got_sorted, want);
        // Also: iteration order matches ascending bit_index.
        let indices: Vec<usize> = b.iter_bits().collect();
        let mut sorted = indices.clone();
        sorted.sort();
        assert_eq!(indices, sorted);
    }
}
