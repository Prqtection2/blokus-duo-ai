//! 21 free Blokus pieces, expanded to 91 oriented (fixed) pieces by applying
//! the 8 rotation+reflection transforms and deduping.

use std::sync::OnceLock;

use crate::bitboard::{bit_index, Bitboard, PLAY_COLS, PLAY_ROWS};

pub const NUM_FREE_PIECES: usize = 21;
pub const NUM_ORIENTED_PIECES: usize = 91;
pub const MONOMINO_ID: u8 = 0;
pub const EXPECTED_HISTOGRAM: [usize; 5] = [1, 2, 6, 19, 63];

pub const FREE_PIECE_NAMES: [&str; NUM_FREE_PIECES] = [
    "I1", "I2", "I3", "V3",
    "I4", "O4", "T4", "L4", "S4",
    "F", "I5", "L5", "N5", "P5", "T5", "U5", "V5", "W5", "X5", "Y5", "Z5",
];

pub const PIECE_SIZES: [u8; NUM_FREE_PIECES] = [
    1, 2, 3, 3,
    4, 4, 4, 4, 4,
    5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
];

fn base_cells(id: usize) -> Vec<(i8, i8)> {
    match id {
        0  => vec![(0, 0)],                                       // I1
        1  => vec![(0, 0), (0, 1)],                               // I2
        2  => vec![(0, 0), (0, 1), (0, 2)],                       // I3
        3  => vec![(0, 0), (0, 1), (1, 0)],                       // V3
        4  => vec![(0, 0), (0, 1), (0, 2), (0, 3)],               // I4
        5  => vec![(0, 0), (0, 1), (1, 0), (1, 1)],               // O4
        6  => vec![(0, 0), (0, 1), (0, 2), (1, 1)],               // T4
        7  => vec![(0, 0), (0, 1), (0, 2), (1, 0)],               // L4
        8  => vec![(0, 1), (0, 2), (1, 0), (1, 1)],               // S4
        9  => vec![(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)],       // F
        10 => vec![(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)],       // I5
        11 => vec![(0, 0), (0, 1), (0, 2), (0, 3), (1, 0)],       // L5
        12 => vec![(0, 1), (0, 2), (0, 3), (1, 0), (1, 1)],       // N5
        13 => vec![(0, 0), (0, 1), (1, 0), (1, 1), (2, 0)],       // P5
        14 => vec![(0, 0), (0, 1), (0, 2), (1, 1), (2, 1)],       // T5
        15 => vec![(0, 0), (0, 2), (1, 0), (1, 1), (1, 2)],       // U5
        16 => vec![(0, 0), (1, 0), (2, 0), (2, 1), (2, 2)],       // V5
        17 => vec![(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)],       // W5
        18 => vec![(0, 1), (1, 0), (1, 1), (1, 2), (2, 1)],       // X5
        19 => vec![(0, 1), (1, 0), (1, 1), (2, 1), (3, 1)],       // Y5
        20 => vec![(0, 0), (0, 1), (1, 1), (2, 1), (2, 2)],       // Z5
        _  => unreachable!("invalid free-piece id {id}"),
    }
}

#[derive(Clone, Debug)]
pub struct OrientedPiece {
    pub free_id: u8,
    pub size: u8,
    pub height: u8,
    pub width: u8,
    /// Cells normalized so min row == 0 and min col == 0, sorted ascending.
    pub cells: Vec<(i8, i8)>,
}

fn normalize(mut cells: Vec<(i8, i8)>) -> Vec<(i8, i8)> {
    let min_r = cells.iter().map(|&(r, _)| r).min().unwrap();
    let min_c = cells.iter().map(|&(_, c)| c).min().unwrap();
    for c in cells.iter_mut() {
        c.0 -= min_r;
        c.1 -= min_c;
    }
    cells.sort();
    cells
}

fn rotate_cw(cells: &[(i8, i8)]) -> Vec<(i8, i8)> {
    // (r, c) -> (c, -r); normalization handles the negative coord.
    cells.iter().map(|&(r, c)| (c, -r)).collect()
}

fn reflect(cells: &[(i8, i8)]) -> Vec<(i8, i8)> {
    cells.iter().map(|&(r, c)| (r, -c)).collect()
}

fn generate_orientations(base: &[(i8, i8)]) -> Vec<Vec<(i8, i8)>> {
    let mut out: Vec<Vec<(i8, i8)>> = Vec::new();
    let mut current = base.to_vec();
    for _ in 0..4 {
        let n = normalize(current.clone());
        if !out.contains(&n) {
            out.push(n);
        }
        let r = normalize(reflect(&current));
        if !out.contains(&r) {
            out.push(r);
        }
        current = rotate_cw(&current);
    }
    out
}

static ORIENTED: OnceLock<Vec<OrientedPiece>> = OnceLock::new();

pub fn oriented_pieces() -> &'static [OrientedPiece] {
    ORIENTED.get_or_init(|| {
        let mut all = Vec::new();
        let mut histogram = [0usize; 5];
        for id in 0..NUM_FREE_PIECES {
            let base = base_cells(id);
            for cells in generate_orientations(&base) {
                let height = cells.iter().map(|&(r, _)| r).max().unwrap() as u8 + 1;
                let width = cells.iter().map(|&(_, c)| c).max().unwrap() as u8 + 1;
                all.push(OrientedPiece {
                    free_id: id as u8,
                    size: PIECE_SIZES[id],
                    height,
                    width,
                    cells,
                });
                histogram[PIECE_SIZES[id] as usize - 1] += 1;
            }
        }
        assert_eq!(
            all.len(), NUM_ORIENTED_PIECES,
            "expected {NUM_ORIENTED_PIECES} oriented pieces, got {}", all.len()
        );
        assert_eq!(
            histogram, EXPECTED_HISTOGRAM,
            "orientation histogram mismatch: got {:?}, expected {:?}",
            histogram, EXPECTED_HISTOGRAM
        );
        all
    })
}

/// Translate `piece` so its cell at index `cell_idx` lands on board cell `anchor`,
/// returning the resulting placement bitboard. Returns `None` if any cell falls
/// outside the 14x14 playable region.
pub fn placement_at(
    piece: &OrientedPiece,
    cell_idx: usize,
    anchor: (i8, i8),
) -> Option<Bitboard> {
    let (ar, ac) = anchor;
    let (pr, pc) = piece.cells[cell_idx];
    let dr = ar - pr;
    let dc = ac - pc;
    let mut bb = Bitboard::EMPTY;
    for &(r, c) in &piece.cells {
        let rr = r + dr;
        let cc = c + dc;
        if rr < 0 || rr >= PLAY_ROWS as i8 || cc < 0 || cc >= PLAY_COLS as i8 {
            return None;
        }
        bb.set_bit(bit_index(rr as usize, cc as usize));
    }
    Some(bb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_is_91() {
        assert_eq!(oriented_pieces().len(), NUM_ORIENTED_PIECES);
    }

    #[test]
    fn histogram_matches() {
        let mut hist = [0usize; 5];
        for p in oriented_pieces() {
            hist[p.size as usize - 1] += 1;
        }
        assert_eq!(hist, EXPECTED_HISTOGRAM);
    }

    #[test]
    fn every_oriented_normalized_to_origin() {
        for p in oriented_pieces() {
            let min_r = p.cells.iter().map(|&(r, _)| r).min().unwrap();
            let min_c = p.cells.iter().map(|&(_, c)| c).min().unwrap();
            assert_eq!(min_r, 0);
            assert_eq!(min_c, 0);
            assert_eq!(p.cells.len() as u8, p.size);
        }
    }

    #[test]
    fn monomino_has_one_orientation() {
        let monos: Vec<_> = oriented_pieces()
            .iter()
            .filter(|p| p.free_id == 0)
            .collect();
        assert_eq!(monos.len(), 1);
    }

    #[test]
    fn x_pentomino_has_one_orientation() {
        let xs: Vec<_> = oriented_pieces()
            .iter()
            .filter(|p| FREE_PIECE_NAMES[p.free_id as usize] == "X5")
            .collect();
        assert_eq!(xs.len(), 1);
    }
}
