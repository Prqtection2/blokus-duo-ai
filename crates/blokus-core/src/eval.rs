//! Evaluation: weighted feature sum from the side-to-move's perspective.
//!
//! All features are computed as (mine − opponent's), then multiplied by the
//! per-feature weights in [`EvalWeights`]. Phase 7 will tune the weights.
//!
//! At terminal nodes the search calls [`terminal_value`] instead, which uses
//! the exact final-score difference (including the +15 / +5 endgame bonuses).

use crate::bitboard::Bitboard;
use crate::board::Board;
use crate::movegen::coverable_cells;
use crate::pieces::{NUM_FREE_PIECES, PIECE_SIZES};

/// Sum of all 21 piece sizes (1+2+2*3+5*4+12*5).
pub const TOTAL_SQUARES: i32 = 89;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalWeights {
    pub placed_squares: i32,
    pub corner_count: i32,
    pub territory: i32,
    /// Negative: large pieces in hand are a scoring liability.
    pub piece_liability: i32,
}

impl Default for EvalWeights {
    /// Reasonable starting weights. Phase 7 will tune from self-play.
    fn default() -> Self {
        Self {
            placed_squares: 100,
            corner_count: 80,
            territory: 20,
            piece_liability: -10,
        }
    }
}

impl EvalWeights {
    /// Phase 3 baseline: only placed_squares. Useful for A/B testing the real
    /// eval against the placeholder via the same code path.
    pub const fn placeholder() -> Self {
        Self {
            placed_squares: 1,
            corner_count: 0,
            territory: 0,
            piece_liability: 0,
        }
    }
}

#[inline]
pub fn squares_placed(board: &Board, p: usize) -> i32 {
    TOTAL_SQUARES - board.squares_left(p)
}

/// One-pass scan over the 21 free pieces: `(squares_placed, piece_liability)`.
/// Combining the two loops cuts ~30% of eval CPU vs. two separate scans.
#[inline]
fn placed_and_liability(board: &Board, p: usize) -> (i32, i32) {
    let mask = board.pieces_left[p];
    let mut placed: i32 = 0;
    let mut liability: i32 = 0;
    for id in 0..NUM_FREE_PIECES {
        let s = PIECE_SIZES[id] as i32;
        if (mask >> id) & 1 != 0 {
            liability += s * s;
        } else {
            placed += s;
        }
    }
    (placed, liability)
}

/// Live corners for player `p`: corners with at least one orthogonally
/// adjacent extension-eligible cell. Used by the corner-count term.
#[inline]
fn live_corners(board: &Board, p: usize) -> Bitboard {
    let extendable = !board.occupied & !board.forbidden[p] & Bitboard::PLAYABLE;
    board.corners[p] & extendable.ortho_neighbors()
}

/// Cell classification codes returned by [`contested_partition`].
///
/// New (2026-05-27) semantics — piece-coverage-based, not BFS distance:
///   * SAFE_P0/SAFE_P1: only that player has a legal placement covering the
///     cell this turn. They own it (this turn) by exclusion.
///   * TIED: both players have legal placements covering the cell. It's a
///     race; whoever moves first wins it.
///   * UNREACHABLE: neither player can cover the cell this turn.
/// CONTESTED_P0/CONTESTED_P1 are no longer produced — kept for code-stability
/// of the visualization color table.
pub mod partition {
    pub const OWN_P0: u8 = 0;
    pub const OWN_P1: u8 = 1;
    pub const SAFE_P0: u8 = 2;
    pub const SAFE_P1: u8 = 3;
    pub const CONTESTED_P0: u8 = 4;
    pub const CONTESTED_P1: u8 = 5;
    pub const TIED: u8 = 6;
    pub const UNREACHABLE: u8 = 7;
}

/// Classify every 14x14 cell for visualization. Returns 196 bytes in
/// row-major order; see the [`partition`] submodule for the code legend.
///
/// Uses the same coverable-cells sets as [`piece_aware_territory_diff`] so
/// the heatmap matches what the eval actually scores.
pub fn contested_partition(board: &Board) -> Vec<u8> {
    let cov_p0 = coverable_cells(board, 0);
    let cov_p1 = coverable_cells(board, 1);
    let mut out = vec![partition::UNREACHABLE; 196];
    for r in 0..14usize {
        for c in 0..14usize {
            let idx = r * 14 + c;
            let bit_idx = r * 16 + c;
            if board.own[0].get_bit(bit_idx) {
                out[idx] = partition::OWN_P0;
            } else if board.own[1].get_bit(bit_idx) {
                out[idx] = partition::OWN_P1;
            } else {
                let p0 = cov_p0.get_bit(bit_idx);
                let p1 = cov_p1.get_bit(bit_idx);
                out[idx] = match (p0, p1) {
                    (true, false) => partition::SAFE_P0,
                    (false, true) => partition::SAFE_P1,
                    (true, true) => partition::TIED,
                    (false, false) => partition::UNREACHABLE,
                };
            }
        }
    }
    out
}

/// Piece-aware territory: counts cells *only one player can legally cover*
/// this turn, scored as (mine-exclusive − theirs-exclusive) from stm view.
///
/// Replaces the BFS-distance "contested-reach" metric (2026-05-27): king-move
/// BFS over-claimed distant cells with no piece commitment. Using actual
/// movegen coverage closes that gap — a cell counts toward "yours" only if
/// you have a piece + orientation + anchor that lands on it AND the opponent
/// does not. Cells coverable by both are TIED (race), contributing 0.
fn piece_aware_territory_diff(board: &Board) -> i32 {
    let cov_p0 = coverable_cells(board, 0);
    let cov_p1 = coverable_cells(board, 1);
    let only_p0 = (cov_p0 & !cov_p1).count_ones() as i32;
    let only_p1 = (cov_p1 & !cov_p0).count_ones() as i32;
    if board.side_to_move == 0 {
        only_p0 - only_p1
    } else {
        only_p1 - only_p0
    }
}

/// Evaluation from side-to-move's perspective using the given weights.
#[inline]
pub fn heuristic_with(board: &Board, w: &EvalWeights) -> i32 {
    let stm = board.side_to_move as usize;
    let other = 1 - stm;

    // One-pass: placed-squares and piece-liability share a 21-piece scan.
    let (stm_placed, stm_liability) = placed_and_liability(board, stm);
    let (other_placed, other_liability) = placed_and_liability(board, other);

    let placed_diff = stm_placed - other_placed;
    // "corner_count" counts LIVE corners only (those with at least one
    // orthogonally-adjacent extension-eligible cell). A corner walled in by
    // own stones can't grow a multi-cell piece from it, so it doesn't count.
    let corner_diff = {
        let stm_live = live_corners(board, stm);
        let other_live = live_corners(board, other);
        stm_live.count_ones() as i32 - other_live.count_ones() as i32
    };

    // "territory" was redesigned a SECOND time (2026-05-27) — now piece-
    // coverage-based, not BFS distance. See `piece_aware_territory_diff` doc.
    // A cell counts iff a legal placement covers it. Diagnostic at ply 8
    // showed the prior BFS metric over-claimed distant cells (35 contested
    // for engine, 8 for opp) that the engine had no piece committed to,
    // collapsing from +1300 static to +120 after one opponent reply.
    let territory_diff = if w.territory != 0 {
        piece_aware_territory_diff(board)
    } else {
        0
    };

    let liability_diff = if w.piece_liability != 0 {
        stm_liability - other_liability
    } else {
        0
    };

    w.placed_squares * placed_diff
        + w.corner_count * corner_diff
        + w.territory * territory_diff
        + w.piece_liability * liability_diff
}

/// Evaluation with default weights — used by tests and as the fallback when
/// the caller hasn't customized weights.
#[inline]
pub fn heuristic(board: &Board) -> i32 {
    heuristic_with(board, &EvalWeights::default())
}

/// Exact value at a terminal node: final_score(stm) − final_score(opp).
#[inline]
pub fn terminal_value(board: &Board) -> i32 {
    let stm = board.side_to_move as usize;
    let other = 1 - stm;
    board.final_score(stm) - board.final_score(other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::generate_moves;

    fn play_n_plies(n: u32) -> Board {
        let mut b = Board::new();
        for _ in 0..n {
            if b.game_over() {
                break;
            }
            let moves = generate_moves(&b);
            if moves.is_empty() {
                b.make_move(&crate::board::Move::Pass);
            } else {
                b.make_move(&moves[0]);
            }
        }
        b
    }

    #[test]
    fn empty_board_eval_is_zero() {
        let b = Board::new();
        assert_eq!(heuristic(&b), 0);
    }

    #[test]
    fn heuristic_negates_when_stm_flips() {
        let mut b = play_n_plies(4);
        let v0 = heuristic(&b);
        b.side_to_move = 1 - b.side_to_move;
        let v1 = heuristic(&b);
        assert_eq!(v0 + v1, 0, "eval not perspective-symmetric: {v0} + {v1}");
    }

    #[test]
    fn placeholder_eval_matches_squares_diff() {
        let b = play_n_plies(6);
        let v = heuristic_with(&b, &EvalWeights::placeholder());
        let stm = b.side_to_move as usize;
        let other = 1 - stm;
        let expected = squares_placed(&b, stm) - squares_placed(&b, other);
        assert_eq!(v, expected);
    }
}
