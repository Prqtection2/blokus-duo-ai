//! Evaluation: weighted feature sum from the side-to-move's perspective.
//!
//! All features are computed as (mine − opponent's), then multiplied by the
//! per-feature weights in [`EvalWeights`]. Phase 7 will tune the weights.
//!
//! At terminal nodes the search calls [`terminal_value`] instead, which uses
//! the exact final-score difference (including the +15 / +5 endgame bonuses).

use crate::bitboard::Bitboard;
use crate::board::Board;
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

/// Bitboard of cells within one Chebyshev step (orthogonal or diagonal) of `own`.
#[inline]
fn one_step_influence(own: Bitboard) -> Bitboard {
    own | own.ortho_neighbors() | own.diag_neighbors()
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
    let corner_diff = board.corners[stm].count_ones() as i32
        - board.corners[other].count_ones() as i32;

    let territory_diff = if w.territory != 0 {
        let me_reach = one_step_influence(board.own[stm]);
        let opp_reach = one_step_influence(board.own[other]);
        let empty = !board.occupied & Bitboard::PLAYABLE;
        let mine = me_reach & !opp_reach & empty;
        let theirs = opp_reach & !me_reach & empty;
        mine.count_ones() as i32 - theirs.count_ones() as i32
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
