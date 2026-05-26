//! Phase 4 evaluation gates: perspective symmetry, mid-game correlation with
//! outcome, and a guard against catastrophic search slowdown vs. the
//! placeholder eval.

use blokus_core::board::{Board, Move};
use blokus_core::eval::{self, EvalWeights};
use blokus_core::movegen::generate_moves;
use blokus_core::search::SearchEngine;

fn play_random(rng: &mut u64, board: &mut Board, plies: u32) {
    for _ in 0..plies {
        if board.game_over() {
            break;
        }
        let moves = generate_moves(board);
        let mv = if moves.is_empty() {
            Move::Pass
        } else {
            // xorshift-style cheap PRNG, fixed-seed for determinism.
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            moves[(*rng as usize) % moves.len()]
        };
        board.make_move(&mv);
    }
}

// ---------- Symmetry ----------

#[test]
fn heuristic_is_perspective_symmetric_after_some_moves() {
    let mut rng: u64 = 0xCAFEBABE;
    for _ in 0..20 {
        let mut b = Board::new();
        play_random(&mut rng, &mut b, 6);
        if b.game_over() {
            continue;
        }
        let v0 = eval::heuristic(&b);
        let mut bt = b.clone();
        bt.side_to_move = 1 - bt.side_to_move;
        let v1 = eval::heuristic(&bt);
        assert_eq!(
            v0 + v1,
            0,
            "eval not perspective-symmetric: {} + {} != 0 at ply {}",
            v0,
            v1,
            b.ply
        );
    }
}

#[test]
fn terminal_value_is_perspective_symmetric() {
    // Force a terminal position by passing twice on an empty-ish board.
    let mut b = Board::new();
    b.make_move(&Move::Pass);
    b.make_move(&Move::Pass);
    assert!(b.game_over());
    let v0 = eval::terminal_value(&b);
    let mut bt = b.clone();
    bt.side_to_move = 1 - bt.side_to_move;
    let v1 = eval::terminal_value(&bt);
    assert_eq!(v0 + v1, 0);
}

// ---------- Correlation with outcome ----------

#[test]
fn mid_game_eval_predicts_winner_in_self_play() {
    // Random self-play creates variance; an engine-vs-itself deterministic run
    // tends to mirror to draws. Use random play and check: at a fixed mid-game
    // ply, does the eval (from P0's perspective) agree in sign with the
    // eventual final score margin? Plan asks "meaningfully more than half".
    const N_GAMES: usize = 200;
    const MID_PLY: u32 = 12;

    let mut rng: u64 = 0xFACE_C0DE_B007_B175;
    let mut agree = 0usize;
    let mut counted = 0usize;

    for _ in 0..N_GAMES {
        let mut board = Board::new();
        let mut mid_eval_from_p0: Option<i32> = None;
        for _ply in 0..100 {
            if board.game_over() {
                break;
            }
            if board.ply == MID_PLY && mid_eval_from_p0.is_none() {
                let mut probe = board.clone();
                probe.side_to_move = 0;
                mid_eval_from_p0 = Some(eval::heuristic(&probe));
            }
            let moves = generate_moves(&board);
            let mv = if moves.is_empty() {
                Move::Pass
            } else {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                moves[(rng as usize) % moves.len()]
            };
            board.make_move(&mv);
        }
        let final_p0 = board.final_score(0) - board.final_score(1);
        if let Some(mid) = mid_eval_from_p0 {
            if mid != 0 && final_p0 != 0 {
                counted += 1;
                if (mid > 0) == (final_p0 > 0) {
                    agree += 1;
                }
            }
        }
    }

    let rate = agree as f64 / counted.max(1) as f64;
    println!(
        "[Phase 4] mid-ply {} eval predicted final-winner sign in {}/{} = {:.1}%",
        MID_PLY,
        agree,
        counted,
        rate * 100.0,
    );
    assert!(counted >= 100, "too few signed comparisons ({counted})");
    assert!(
        rate >= 0.60,
        "mid-game eval-vs-outcome agreement {:.1}% below 60%",
        rate * 100.0
    );
}

// ---------- Search-speed guard ----------

#[test]
fn real_eval_per_node_cost_not_dominated() {
    // Hardware-portable check: real-eval's per-node CPU time shouldn't be more
    // than ~4× placeholder's. This isolates "did eval get slower per node"
    // from "did total throughput change" (the latter depends on node count,
    // which differs between placeholder and real due to pruning differences).
    let mut b_placeholder = Board::new();
    let mut e_placeholder = SearchEngine::new(18);
    e_placeholder.set_weights(EvalWeights::placeholder());
    let r_placeholder = e_placeholder.search_fixed_depth(&mut b_placeholder, 3);

    let mut b_real = Board::new();
    let mut e_real = SearchEngine::new(18);
    let r_real = e_real.search_fixed_depth(&mut b_real, 3);

    let per_node = |r: &blokus_core::search::SearchResult| -> f64 {
        if r.nodes == 0 {
            f64::INFINITY
        } else {
            r.time_ms as f64 / r.nodes as f64
        }
    };
    let pn_placeholder = per_node(&r_placeholder);
    let pn_real = per_node(&r_real);
    let ratio = if pn_placeholder == 0.0 {
        f64::INFINITY
    } else {
        pn_real / pn_placeholder
    };
    println!(
        "[Phase 5] depth=3 per-node cost: placeholder {:.3} ms/node ({} nodes, {} ms), \
         real {:.3} ms/node ({} nodes, {} ms), ratio {:.2}",
        pn_placeholder, r_placeholder.nodes, r_placeholder.time_ms,
        pn_real, r_real.nodes, r_real.time_ms,
        ratio,
    );
    // 4× is generous — real eval has territory + corner_count + liability work
    // that placeholder skips. Catches catastrophic regressions only.
    assert!(
        ratio < 4.0,
        "real-eval per-node cost {:.2}× placeholder (> 4×)",
        ratio
    );
}

#[test]
fn real_eval_search_completes_in_reasonable_time() {
    // Portable bound: real-eval depth-3 from start must complete in < 2s on any
    // reasonable machine. Replaces a hardware-fragile nps-floor assertion.
    let mut b = Board::new();
    let mut e = SearchEngine::new(18);
    let r = e.search_fixed_depth(&mut b, 3);
    let nps = if r.time_ms == 0 {
        f64::INFINITY
    } else {
        r.nodes as f64 * 1000.0 / r.time_ms as f64
    };
    println!(
        "[Phase 5] real-eval depth=3 from start: nodes={}, time={}ms, nps={:.0}",
        r.nodes, r.time_ms, nps
    );
    assert!(
        r.time_ms < 2000,
        "real-eval depth-3 took {}ms, exceeds 2s portable bound",
        r.time_ms
    );
}
