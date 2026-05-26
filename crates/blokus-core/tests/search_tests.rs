//! Phase 3 correctness gates: zobrist consistency, TT neutrality, AB == MM at
//! small depth, plus a release-mode nodes/sec readout.

use blokus_core::bitboard::Bitboard;
use blokus_core::board::{Board, Move};
use blokus_core::movegen::generate_moves;
use blokus_core::search::{plain_minimax, SearchEngine};
use blokus_core::zobrist;

// ---------- Zobrist ----------

fn compute_oracle(b: &Board) -> u64 {
    zobrist::compute_from_state(
        [&b.own[0], &b.own[1]],
        b.pieces_left,
        b.last_placed,
        b.side_to_move,
        b.consecutive_passes,
    )
}

#[test]
fn zobrist_matches_oracle_on_empty_board() {
    let b = Board::new();
    assert_eq!(b.zobrist, compute_oracle(&b));
}

#[test]
fn zobrist_matches_oracle_through_random_game() {
    use std::num::Wrapping;
    let mut s = Wrapping(0xDEAD_BEEFu64);
    fn next(s: &mut Wrapping<u64>) -> u64 {
        *s += Wrapping(0x9E37_79B9_7F4A_7C15);
        let mut z = s.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    for _ in 0..30 {
        let mut board = Board::new();
        for _ in 0..40 {
            if board.game_over() {
                break;
            }
            let moves = generate_moves(&board);
            let mv = if moves.is_empty() {
                Move::Pass
            } else {
                moves[(next(&mut s) as usize) % moves.len()]
            };
            board.make_move(&mv);
            assert_eq!(
                board.zobrist,
                compute_oracle(&board),
                "incremental hash diverged from oracle at ply {}",
                board.ply
            );
        }
    }
}

#[test]
fn zobrist_equal_for_transposition_equivalent_pass_state() {
    // Reach the same (own, pieces_left, last_placed, stm, consec) state two
    // different ways: a real Board produced by sequenced moves, and a
    // hand-built Board that sets fields directly. They must agree on zobrist.
    let mut board_a = Board::new();
    let moves_a = generate_moves(&board_a);
    let first_move = moves_a[0]; // some legal opening for P0
    board_a.make_move(&first_move);

    // Reconstruct an equivalent state from scratch.
    let (piece, placement) = match first_move {
        Move::Place { piece, placement } => (piece, placement),
        Move::Pass => unreachable!(),
    };
    let mut board_b = Board::new();
    board_b.own[0] = placement;
    board_b.occupied = placement;
    board_b.pieces_left[0] &= !(1u32 << piece);
    board_b.last_placed[0] = Some(piece);
    board_b.side_to_move = 1;
    board_b.recompute_masks();
    // Now manually reproduce the zobrist contributions.
    let z = zobrist::table();
    for bit in placement.iter_bits() {
        board_b.zobrist ^= z.cell[0][bit];
    }
    board_b.zobrist ^= z.piece_used[0][piece as usize];
    if piece == 0 {
        board_b.zobrist ^= z.last_mono[0];
    }
    board_b.zobrist ^= z.side_to_move;

    assert_eq!(board_a.zobrist, board_b.zobrist);
    assert_eq!(board_a.zobrist, compute_oracle(&board_a));
    assert_eq!(board_b.zobrist, compute_oracle(&board_b));
}

// ---------- TT neutrality ----------

#[test]
fn tt_on_equals_tt_off_at_fixed_depth() {
    for depth in 1..=3 {
        let mut b1 = Board::new();
        let mut b2 = Board::new();
        let mut e_with = SearchEngine::new(16);
        let mut e_no = SearchEngine::new(16);
        e_no.set_tt_enabled(false);
        let r_with = e_with.search_fixed_depth(&mut b1, depth);
        let r_no = e_no.search_fixed_depth(&mut b2, depth);
        assert_eq!(
            r_with.value, r_no.value,
            "depth {depth}: TT changed value (with TT {} vs without {})",
            r_with.value, r_no.value
        );
    }
}

// ---------- Alpha-beta == plain minimax ----------

#[test]
fn alpha_beta_equals_minimax_depth_1_2_from_start() {
    for depth in 1..=2 {
        let mut b_ab = Board::new();
        let mut b_mm = Board::new();
        let mut engine = SearchEngine::new(16);
        engine.set_tt_enabled(false);
        let ab = engine.search_fixed_depth(&mut b_ab, depth);
        let (mm_val, _mm_nodes) = plain_minimax(&mut b_mm, depth);
        assert_eq!(
            ab.value, mm_val,
            "depth {depth}: alpha-beta value {} != minimax value {}",
            ab.value, mm_val
        );
    }
}

#[test]
fn alpha_beta_equals_minimax_depth_3_from_constrained_position() {
    // Walk a few plies to shrink branching, then compare AB and MM at depth 3.
    let mut board = Board::new();
    let mut e_warm = SearchEngine::new(16);
    for _ in 0..6 {
        if board.game_over() {
            break;
        }
        let r = e_warm.search_fixed_depth(&mut board, 1);
        match r.best_move {
            Some(mv) => board.make_move(&mv),
            None => board.make_move(&Move::Pass),
        }
    }
    let mut ab_engine = SearchEngine::new(16);
    ab_engine.set_tt_enabled(false);
    let mut ab_board = board.clone();
    let mut mm_board = board.clone();
    let ab = ab_engine.search_fixed_depth(&mut ab_board, 3);
    let (mm_val, mm_nodes) = plain_minimax(&mut mm_board, 3);
    println!(
        "depth-3 sanity (post 6 plies): ab.value={}, ab.nodes={}, mm.value={}, mm.nodes={}",
        ab.value, ab.nodes, mm_val, mm_nodes
    );
    assert_eq!(ab.value, mm_val);
    assert!(ab.nodes <= mm_nodes, "alpha-beta should visit ≤ minimax nodes");
}

// ---------- Nodes/sec readout (release mode preferred) ----------

#[test]
fn nodes_per_second_at_depth_3_from_start() {
    let mut board = Board::new();
    let mut engine = SearchEngine::new(18);
    let r = engine.search_fixed_depth(&mut board, 3);
    let nps = if r.time_ms == 0 { f64::INFINITY } else {
        r.nodes as f64 * 1000.0 / r.time_ms as f64
    };
    println!(
        "[Phase 3 baseline] depth=3 from start: nodes={}, tt_hits={}, time={}ms, nps={:.0}",
        r.nodes, r.tt_hits, r.time_ms, nps
    );
    assert!(r.nodes > 100);
    assert!(r.best_move.is_some());
}

// ---------- Incremental masks (Phase 5 optimization) ----------

/// After every move, the incrementally-updated `forbidden` and `corners`
/// masks must match what `recompute_masks` would compute from scratch.
/// Perft compares two generators that both read these masks, so a bug here
/// would not be caught by perft alone.
#[test]
fn incremental_masks_agree_with_recompute_through_random_games() {
    use std::num::Wrapping;
    let mut s = Wrapping(0xFEEDFACE_DEADBEEFu64);
    fn next(s: &mut Wrapping<u64>) -> u64 {
        *s += Wrapping(0x9E37_79B9_7F4A_7C15);
        let mut z = s.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    for game in 0..30 {
        let mut board = Board::new();
        for _ in 0..50 {
            if board.game_over() {
                break;
            }
            let moves = blokus_core::movegen::generate_moves(&board);
            let mv = if moves.is_empty() {
                Move::Pass
            } else {
                moves[(next(&mut s) as usize) % moves.len()]
            };
            board.make_move(&mv);
            let mut probe = board.clone();
            probe.recompute_masks();
            assert_eq!(
                board.forbidden, probe.forbidden,
                "game {game}, ply {}: forbidden mismatch", board.ply
            );
            assert_eq!(
                board.corners, probe.corners,
                "game {game}, ply {}: corners mismatch", board.ply
            );
        }
    }
}

// Helper to silence unused-import warning if Bitboard isn't used elsewhere.
#[allow(dead_code)]
fn _unused(_: Bitboard) {}
