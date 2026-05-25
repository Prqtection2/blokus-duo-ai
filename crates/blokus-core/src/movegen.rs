//! Move generation for the side to move.
//!
//! Two generators are provided, both required to produce identical move sets:
//! - [`generate_moves`]: corner-anchored — iterates own corner cells × oriented
//!   pieces × piece-cells. The production path.
//! - [`generate_moves_reference`]: brute-force — iterates every board cell ×
//!   every oriented piece × every piece-cell. Used only as a slow oracle.

use std::collections::HashSet;

use crate::bitboard::{Bitboard, PLAY_COLS, PLAY_ROWS};
use crate::board::{Board, Move};
use crate::pieces::{oriented_pieces, placement_at};

/// Corner-anchored generation. Anchors on every cell of every oriented piece
/// in `board.corners[stm]`, deduping by `(piece, placement)`.
pub fn generate_moves(board: &Board) -> Vec<Move> {
    let player = board.side_to_move as usize;
    let mut seen: HashSet<(u8, Bitboard)> = HashSet::new();
    let mut moves: Vec<Move> = Vec::new();

    for anchor_idx in board.corners[player].iter_bits() {
        let ar = (anchor_idx / 16) as i8;
        let ac = (anchor_idx % 16) as i8;
        for op in oriented_pieces() {
            if !board.has_piece(player, op.free_id) {
                continue;
            }
            for ci in 0..op.cells.len() {
                let placement = match placement_at(op, ci, (ar, ac)) {
                    Some(bb) => bb,
                    None => continue,
                };
                if !board.placement_is_legal(player, op.free_id, placement) {
                    continue;
                }
                if seen.insert((op.free_id, placement)) {
                    moves.push(Move::Place { piece: op.free_id, placement });
                }
            }
        }
    }

    moves
}

/// Brute-force reference generator. Scans every (oriented_piece × board cell ×
/// piece-cell) combination and accepts whatever the legality predicate accepts.
/// Slow — used only as an independent oracle for `generate_moves`.
pub fn generate_moves_reference(board: &Board) -> Vec<Move> {
    let player = board.side_to_move as usize;
    let mut seen: HashSet<(u8, Bitboard)> = HashSet::new();
    let mut moves: Vec<Move> = Vec::new();

    for r in 0..PLAY_ROWS as i8 {
        for c in 0..PLAY_COLS as i8 {
            for op in oriented_pieces() {
                if !board.has_piece(player, op.free_id) {
                    continue;
                }
                for ci in 0..op.cells.len() {
                    let placement = match placement_at(op, ci, (r, c)) {
                        Some(bb) => bb,
                        None => continue,
                    };
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
        // Sanity bound: at most 414 placements (sum of cells across 91 oriented
        // pieces) anchored on the single start cell, though many fit.
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
}
