//! Move generation for the side to move.
//!
//! Two generators are provided, both required to produce identical move sets:
//! - [`generate_moves`]: corner-anchored — iterates own corner cells × oriented
//!   pieces × piece-cells. The production path.
//! - [`generate_moves_reference`]: brute-force — iterates every board cell ×
//!   every oriented piece × every piece-cell. Used only as a slow oracle.
//!
//! Both consult [`precomputed_placements`] for the placement bitboard given a
//! (piece, bbox-origin) pair instead of constructing it per call.

use std::collections::HashSet;

use crate::bitboard::{Bitboard, PLAY_COLS, PLAY_ROWS};
use crate::board::{Board, Move};
use crate::pieces::{oriented_pieces, precomputed_placements};

/// Corner-anchored generation. Anchors on every cell of every oriented piece
/// in `board.corners[stm]`, deduping by `(piece, placement)`.
pub fn generate_moves(board: &Board) -> Vec<Move> {
    let mut moves: Vec<Move> = Vec::new();
    generate_moves_into(board, &mut moves);
    moves
}

/// Corner-anchored generation, writing into a caller-provided buffer.
pub fn generate_moves_into(board: &Board, moves: &mut Vec<Move>) {
    moves.clear();
    let player = board.side_to_move as usize;
    let pieces = oriented_pieces();
    let pre = precomputed_placements();
    let mut seen: HashSet<(u8, Bitboard)> = HashSet::new();

    for anchor_idx in board.corners[player].iter_bits() {
        let anchor_row = anchor_idx / 16;
        let anchor_col = anchor_idx % 16;
        for (op_idx, op) in pieces.iter().enumerate() {
            if !board.has_piece(player, op.free_id) {
                continue;
            }
            for &(pr_i, pc_i) in &op.cells {
                let pr = pr_i as usize;
                let pc = pc_i as usize;
                // bbox origin = anchor − piece-cell offset. Cheap saturating
                // bounds check via comparisons.
                if pr > anchor_row || pc > anchor_col {
                    continue;
                }
                let origin_row = anchor_row - pr;
                let origin_col = anchor_col - pc;
                let placement = pre.get(op_idx, origin_row, origin_col);
                if placement.is_empty() {
                    continue;
                }
                if !board.placement_is_legal(player, op.free_id, placement) {
                    continue;
                }
                if seen.insert((op.free_id, placement)) {
                    moves.push(Move::Place { piece: op.free_id, placement });
                }
            }
        }
    }
}

/// Brute-force reference generator. Scans every (oriented_piece × board cell ×
/// piece-cell) combination and uses [`Board::placement_is_legal_mask_free`] —
/// i.e., legality is derived from raw `own[]` + `occupied` via neighbor shifts,
/// independent of the incrementally-maintained `corners`/`forbidden` masks.
/// This keeps perft a check on the Blokus rules, not just on mask consistency.
pub fn generate_moves_reference(board: &Board) -> Vec<Move> {
    let player = board.side_to_move as usize;
    let pieces = oriented_pieces();
    let pre = precomputed_placements();
    let mut seen: HashSet<(u8, Bitboard)> = HashSet::new();
    let mut moves: Vec<Move> = Vec::new();

    for r in 0..PLAY_ROWS {
        for c in 0..PLAY_COLS {
            for (op_idx, op) in pieces.iter().enumerate() {
                if !board.has_piece(player, op.free_id) {
                    continue;
                }
                for &(pr_i, pc_i) in &op.cells {
                    let pr = pr_i as usize;
                    let pc = pc_i as usize;
                    if pr > r || pc > c {
                        continue;
                    }
                    let origin_row = r - pr;
                    let origin_col = c - pc;
                    let placement = pre.get(op_idx, origin_row, origin_col);
                    if placement.is_empty() {
                        continue;
                    }
                    if !board.placement_is_legal_mask_free(player, op.free_id, placement) {
                        continue;
                    }
                    if seen.insert((op.free_id, placement)) {
                        moves.push(Move::Place { piece: op.free_id, placement });
                    }
                }
            }
        }
    }

    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_move_count_is_nonzero() {
        let b = Board::new();
        let moves = generate_moves(&b);
        assert!(!moves.is_empty(), "P0 must have moves at root");
        // Theoretical max: 414 placements (sum of cells across 91 oriented
        // pieces) anchored on the single start cell.
        assert!(moves.len() <= 414);
    }

    #[test]
    fn root_fast_and_reference_agree() {
        let b = Board::new();
        let mut fast = generate_moves(&b);
        let mut slow = generate_moves_reference(&b);
        let key = |m: &Move| -> (u8, [u64; 4]) {
            match *m {
                Move::Place { piece, placement } => (piece, placement.0),
                Move::Pass => (255, [0; 4]),
            }
        };
        fast.sort_by_key(key);
        slow.sort_by_key(key);
        assert_eq!(fast, slow, "fast and reference generators disagree at root");
    }

    #[test]
    fn generate_moves_into_clears_buffer() {
        let b = Board::new();
        let mut buf = Vec::new();
        buf.push(Move::Pass);
        buf.push(Move::Pass);
        generate_moves_into(&b, &mut buf);
        // Buffer should now hold exactly the same moves as a fresh call.
        let fresh = generate_moves(&b);
        assert_eq!(buf.len(), fresh.len());
    }
}
