//! Blokus Duo board state. Owns `Move`, make/unmake, and incremental masks.

use crate::bitboard::{bit_index, Bitboard};
use crate::pieces::{MONOMINO_ID, NUM_FREE_PIECES, PIECE_SIZES};
use crate::zobrist;

pub const NUM_PLAYERS: usize = 2;

/// 0-indexed start cells for each player. **Verify against the physical Mattel
/// rulebook before relying on these for tournament play** — kept as named
/// constants so they're trivial to change.
pub const START_CELLS: [(usize, usize); 2] = [(4, 4), (9, 9)];

/// All free-piece bits set (bits 0..21).
const ALL_PIECES_MASK: u32 = (1u32 << NUM_FREE_PIECES) - 1;

/// A move: either pass, or place a free piece with the given bitboard footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Move {
    Pass,
    Place { piece: u8, placement: Bitboard },
}

#[derive(Clone)]
pub struct Board {
    pub side_to_move: u8,
    pub ply: u32,
    pub occupied: Bitboard,
    pub own: [Bitboard; 2],
    pub forbidden: [Bitboard; 2],
    pub corners: [Bitboard; 2],
    pub pieces_left: [u32; 2],
    pub last_placed: [Option<u8>; 2],
    pub consecutive_passes: u8,
    pub zobrist: u64,
    undo_stack: Vec<Undo>,
}

#[derive(Clone)]
struct Undo {
    side_to_move: u8,
    ply: u32,
    occupied: Bitboard,
    own: [Bitboard; 2],
    forbidden: [Bitboard; 2],
    corners: [Bitboard; 2],
    pieces_left: [u32; 2],
    last_placed: [Option<u8>; 2],
    consecutive_passes: u8,
    zobrist: u64,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    pub fn new() -> Self {
        let mut b = Self {
            side_to_move: 0,
            ply: 0,
            occupied: Bitboard::EMPTY,
            own: [Bitboard::EMPTY; 2],
            forbidden: [Bitboard::EMPTY; 2],
            corners: [Bitboard::EMPTY; 2],
            pieces_left: [ALL_PIECES_MASK; 2],
            last_placed: [None; 2],
            consecutive_passes: 0,
            zobrist: 0,
            undo_stack: Vec::new(),
        };
        b.recompute_masks();
        b
    }

    #[inline]
    pub fn has_piece(&self, p: usize, free_id: u8) -> bool {
        (self.pieces_left[p] >> free_id) & 1 != 0
    }

    pub fn is_first_move(&self, p: usize) -> bool {
        self.own[p].is_empty()
    }

    /// Recompute `forbidden[p]` and `corners[p]` from `own[p]` and `occupied`.
    /// For first moves, `corners[p]` is overridden to be exactly `{start_cell}`.
    pub fn recompute_masks(&mut self) {
        for p in 0..NUM_PLAYERS {
            let own = self.own[p];
            self.forbidden[p] = own | own.ortho_neighbors();
            if own.is_empty() {
                let mut c = Bitboard::EMPTY;
                let (r, cc) = START_CELLS[p];
                c.set_bit(bit_index(r, cc));
                self.corners[p] = c;
            } else {
                self.corners[p] = own.diag_neighbors()
                    & !self.forbidden[p]
                    & !self.occupied
                    & Bitboard::PLAYABLE;
            }
        }
    }

    pub fn make_move(&mut self, mv: &Move) {
        self.undo_stack.push(self.snapshot());
        let prev_passes = self.consecutive_passes;
        match *mv {
            Move::Pass => {
                self.consecutive_passes = self.consecutive_passes.saturating_add(1);
            }
            Move::Place { piece, placement } => {
                let p = self.side_to_move as usize;
                let other = 1 - p;
                debug_assert!(self.has_piece(p, piece), "piece {piece} already used");
                self.own[p] = self.own[p] | placement;
                self.occupied = self.occupied | placement;
                self.pieces_left[p] &= !(1u32 << piece);
                let prev_last = self.last_placed[p];
                self.last_placed[p] = Some(piece);
                self.consecutive_passes = 0;

                // Incremental mask updates: avoid a full recompute_masks pass.
                let ortho_pl = placement.ortho_neighbors();
                let diag_pl = placement.diag_neighbors();
                self.forbidden[p] = self.forbidden[p] | placement | ortho_pl;
                // forbidden[other] is unchanged (their own[] hasn't moved).
                self.corners[p] = (self.corners[p] | diag_pl)
                    & !self.forbidden[p]
                    & !self.occupied
                    & Bitboard::PLAYABLE;
                // The opponent's corners only lose cells that we just occupied.
                self.corners[other] = self.corners[other] & !placement;

                let z = zobrist::table();
                for bit in placement.iter_bits() {
                    self.zobrist ^= z.cell[p][bit];
                }
                self.zobrist ^= z.piece_used[p][piece as usize];
                // last_mono marker: toggle whenever the player's last_placed
                // monomino-ness changes.
                let prev_is_mono = prev_last == Some(MONOMINO_ID);
                let new_is_mono = piece == MONOMINO_ID;
                if prev_is_mono != new_is_mono {
                    self.zobrist ^= z.last_mono[p];
                }
            }
        }
        let new_passes = self.consecutive_passes;
        if prev_passes != new_passes {
            let z = zobrist::table();
            self.zobrist ^= z.pass_count[prev_passes as usize];
            self.zobrist ^= z.pass_count[new_passes as usize];
        }
        self.zobrist ^= zobrist::table().side_to_move;
        self.side_to_move = 1 - self.side_to_move;
        self.ply += 1;
    }

    pub fn unmake_move(&mut self) {
        let u = self.undo_stack.pop().expect("undo stack empty");
        self.side_to_move = u.side_to_move;
        self.ply = u.ply;
        self.occupied = u.occupied;
        self.own = u.own;
        self.forbidden = u.forbidden;
        self.corners = u.corners;
        self.pieces_left = u.pieces_left;
        self.last_placed = u.last_placed;
        self.consecutive_passes = u.consecutive_passes;
        self.zobrist = u.zobrist;
    }

    fn snapshot(&self) -> Undo {
        Undo {
            side_to_move: self.side_to_move,
            ply: self.ply,
            occupied: self.occupied,
            own: self.own,
            forbidden: self.forbidden,
            corners: self.corners,
            pieces_left: self.pieces_left,
            last_placed: self.last_placed,
            consecutive_passes: self.consecutive_passes,
            zobrist: self.zobrist,
        }
    }

    pub fn game_over(&self) -> bool {
        self.consecutive_passes >= 2
    }

    pub fn squares_left(&self, p: usize) -> i32 {
        let mut total = 0i32;
        for id in 0..NUM_FREE_PIECES {
            if self.has_piece(p, id as u8) {
                total += PIECE_SIZES[id] as i32;
            }
        }
        total
    }

    pub fn final_score(&self, p: usize) -> i32 {
        let mut s = -self.squares_left(p);
        let all_placed = self.pieces_left[p] == 0;
        if all_placed {
            s += 15;
            if self.last_placed[p] == Some(MONOMINO_ID) {
                s += 5;
            }
        }
        s
    }

    /// Validate that a placement by `player` is a legal non-pass move.
    /// Uses the incrementally-maintained `forbidden` and `corners` masks.
    /// Doesn't check that `piece` is still in hand — caller must verify.
    pub fn placement_is_legal(&self, player: usize, piece: u8, placement: Bitboard) -> bool {
        if !(placement & !Bitboard::PLAYABLE).is_empty() {
            return false;
        }
        if !(placement & self.occupied).is_empty() {
            return false;
        }
        if !(placement & self.forbidden[player]).is_empty() {
            return false;
        }
        if (placement & self.corners[player]).is_empty() {
            return false;
        }
        if placement.count_ones() != PIECE_SIZES[piece as usize] as u32 {
            return false;
        }
        true
    }

    /// Like [`placement_is_legal`], but derives every check from raw
    /// `own[p]` and `occupied` via neighbor shifts — does NOT consult the
    /// incrementally-maintained `forbidden` or `corners` masks. Used by the
    /// brute-force reference move generator so that perft remains a check on
    /// the Blokus rules themselves (not just on "two mask-consumers agree").
    pub fn placement_is_legal_mask_free(
        &self,
        player: usize,
        piece: u8,
        placement: Bitboard,
    ) -> bool {
        if !(placement & !Bitboard::PLAYABLE).is_empty() {
            return false;
        }
        if placement.count_ones() != PIECE_SIZES[piece as usize] as u32 {
            return false;
        }
        if !(placement & self.occupied).is_empty() {
            return false;
        }
        let own = self.own[player];
        // No orthogonal contact with own stones.
        if !(placement & own.ortho_neighbors()).is_empty() {
            return false;
        }
        // Must touch own corner (first move special-case: must include start cell).
        if own.is_empty() {
            let (sr, sc) = START_CELLS[player];
            let mut start_bb = Bitboard::EMPTY;
            start_bb.set_bit(bit_index(sr, sc));
            if (placement & start_bb).is_empty() {
                return false;
            }
        } else if (placement & own.diag_neighbors()).is_empty() {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::FREE_PIECE_NAMES;

    fn id_of(name: &str) -> u8 {
        FREE_PIECE_NAMES.iter().position(|&n| n == name).unwrap() as u8
    }

    fn place(piece_id: u8, cells: &[(usize, usize)]) -> Move {
        Move::Place {
            piece: piece_id,
            placement: Bitboard::from_cells(cells.iter().copied()),
        }
    }

    #[test]
    fn first_move_must_cover_start_cell() {
        let board = Board::new();
        // Monomino at (0, 0): doesn't cover start cell (4, 4) → reject.
        if let Move::Place { piece, placement } = place(MONOMINO_ID, &[(0, 0)]) {
            assert!(!board.placement_is_legal(0, piece, placement));
        }
        // Monomino at (4, 4): covers start cell → accept.
        if let Move::Place { piece, placement } = place(MONOMINO_ID, &[(4, 4)]) {
            assert!(board.placement_is_legal(0, piece, placement));
        }
    }

    #[test]
    fn overlap_rejected() {
        let mut board = Board::new();
        board.make_move(&place(MONOMINO_ID, &[(4, 4)]));
        // P1 trying to place monomino on (4, 4): occupied → reject.
        // But P1's first move requires covering (9, 9), so we instead test
        // overlap using a fully constructed scenario.
        // Manually set up: P1 has played a stone at (9, 9). P0 to move at (5, 5)
        // (a corner anchor for P0). Try to place I2 covering (5, 5)-(9, 9)?
        // Simpler: just check directly: after P1 plays at (9, 9), P1 again
        // trying monomino at (9, 9) is overlap.
        board.make_move(&place(MONOMINO_ID, &[(9, 9)]));
        // Now P0 to move. P0 placing on (9, 9) is overlap.
        if let Move::Place { piece, placement } = place(MONOMINO_ID, &[(9, 9)]) {
            assert!(!board.placement_is_legal(0, piece, placement));
        }
    }

    #[test]
    fn own_edge_touch_rejected_diagonal_accepted() {
        let mut board = Board::new();
        board.make_move(&place(MONOMINO_ID, &[(4, 4)])); // P0
        board.make_move(&place(MONOMINO_ID, &[(9, 9)])); // P1
        // Now P0 to move. Domino at (4, 5)-(4, 6): (4, 5) edge-adjacent to (4, 4) own.
        let d = id_of("I2");
        if let Move::Place { piece, placement } = place(d, &[(4, 5), (4, 6)]) {
            assert!(!board.placement_is_legal(0, piece, placement),
                "own-color orthogonal touch must be rejected");
        }
        // Domino at (5, 5)-(5, 6): (5, 5) diagonally adjacent to (4, 4) own → legal anchor.
        if let Move::Place { piece, placement } = place(d, &[(5, 5), (5, 6)]) {
            assert!(board.placement_is_legal(0, piece, placement),
                "own diagonal contact (and no edge contact) must be legal");
        }
    }

    #[test]
    fn opponent_edge_touch_accepted() {
        // Construct a contrived position: P0 has stone at (4, 4), P1 has stone at (4, 5)
        // (orthogonally adjacent across colors). Then P0 places stone at (5, 5) — diagonal
        // anchor to (4, 4), orthogonally adjacent to (4, 5) which is opponent.
        let mut board = Board::new();
        board.own[0].set_bit(bit_index(4, 4));
        board.occupied.set_bit(bit_index(4, 4));
        board.own[1].set_bit(bit_index(4, 5));
        board.occupied.set_bit(bit_index(4, 5));
        // Mark both as having moved (so first-move override doesn't fire).
        board.pieces_left[0] &= !(1u32 << MONOMINO_ID);
        board.pieces_left[1] &= !(1u32 << MONOMINO_ID);
        board.last_placed[0] = Some(MONOMINO_ID);
        board.last_placed[1] = Some(MONOMINO_ID);
        board.recompute_masks();

        // P0 places monomino at (5, 5): diagonal to own (4, 4), orthogonal to
        // opponent (4, 5). Should be legal.
        let mv = place(MONOMINO_ID, &[(5, 5)]);
        if let Move::Place { piece, placement } = mv {
            assert!(board.placement_is_legal(0, piece, placement),
                "opponent edge contact is allowed; only own edge contact forbidden");
        }
    }

    #[test]
    fn final_score_empty_game() {
        let board = Board::new();
        // No pieces placed → -89 (sum of all 21 piece sizes = 89).
        assert_eq!(board.final_score(0), -89);
        assert_eq!(board.final_score(1), -89);
    }

    #[test]
    fn final_score_all_placed_with_monomino_last() {
        let mut b = Board::new();
        b.pieces_left[0] = 0;
        b.last_placed[0] = Some(MONOMINO_ID);
        assert_eq!(b.final_score(0), 15 + 5);
    }

    #[test]
    fn final_score_all_placed_without_monomino_last() {
        let mut b = Board::new();
        b.pieces_left[0] = 0;
        b.last_placed[0] = Some(10); // I5
        assert_eq!(b.final_score(0), 15);
    }

    #[test]
    fn make_unmake_round_trip_single_move() {
        let mut b = Board::new();
        let before = (b.occupied, b.own, b.forbidden, b.corners, b.pieces_left,
                      b.last_placed, b.zobrist, b.side_to_move, b.ply);
        b.make_move(&place(MONOMINO_ID, &[(4, 4)]));
        assert_ne!(b.zobrist, before.6, "zobrist must change after a move");
        b.unmake_move();
        let after = (b.occupied, b.own, b.forbidden, b.corners, b.pieces_left,
                     b.last_placed, b.zobrist, b.side_to_move, b.ply);
        assert_eq!(before, after);
    }
}
