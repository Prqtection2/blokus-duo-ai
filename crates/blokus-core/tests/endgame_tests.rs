//! Phase 6 endgame solver gates.
//!
//! We don't construct a hand-built late position with a known optimum — that
//! is fragile against any future change to board internals. Instead:
//!
//! * Force the solver always-on and compare its value to a deep alpha-beta
//!   search with the solver off. From a sufficiently late state they must
//!   agree (alpha-beta to a depth exceeding the remaining plies reaches the
//!   same exact terminal value).
//! * Verify the opening-time penalty of enabling the solver is small (the
//!   activation check is cheap and never fires at low ply).

use blokus_core::board::{Board, Move};
use blokus_core::search::SearchEngine;

fn drive_to_late_state(plies: u32) -> Board {
    let mut b = Board::new();
    let mut engine = SearchEngine::new(16);
    for _ in 0..plies {
        if b.game_over() {
            break;
        }
        let r = engine.search_fixed_depth(&mut b, 1);
        let mv = r.best_move.unwrap_or(Move::Pass);
        b.make_move(&mv);
    }
    b
}

#[test]
fn endgame_solver_value_matches_deep_alpha_beta_on_late_position() {
    // 30 plies of depth-1 self-play leaves each side with ~6 pieces remaining,
    // which keeps the deep alpha-beta search feasible while exercising the
    // solver's "search to terminal" path through several real plies.
    let late = drive_to_late_state(30);
    if late.game_over() {
        // If self-play already ended, the test is trivially satisfied; both
        // searches would return `terminal_value` at root.
        return;
    }

    // Reference: solver disabled, alpha-beta to depth 30 (well past any plausible
    // remaining game length from a state this late).
    let mut e_off = SearchEngine::new(16);
    e_off.set_endgame_threshold(0);
    let r_off = e_off.search_fixed_depth(&mut late.clone(), 30);

    // Solver: always active (huge threshold), depth 30 — exact same depth, but
    // negamax now ignores the depth-0 heuristic cutoff.
    let mut e_solver = SearchEngine::new(16);
    e_solver.set_endgame_threshold(u32::MAX);
    let r_solver = e_solver.search_fixed_depth(&mut late.clone(), 30);

    println!(
        "[Phase 6] late state (ply {}): solver-off value={} ({} nodes, {} ms), \
         solver-on value={} ({} nodes, {} ms)",
        late.ply, r_off.value, r_off.nodes, r_off.time_ms,
        r_solver.value, r_solver.nodes, r_solver.time_ms,
    );
    assert_eq!(
        r_off.value, r_solver.value,
        "solver value diverges from deep alpha-beta on a late position"
    );
}

#[test]
fn endgame_solver_does_not_slow_opening() {
    // The activation check is `2 × count_ones` per node — must not measurably
    // slow the opening, where it always returns false.
    let mut b_disabled = Board::new();
    let mut e_disabled = SearchEngine::new(18);
    e_disabled.set_endgame_threshold(0);
    let r_disabled = e_disabled.search_fixed_depth(&mut b_disabled, 3);

    let mut b_enabled = Board::new();
    let e_enabled = SearchEngine::new(18); // default threshold 10, never fires at ply 0
    let mut e_enabled = e_enabled;
    let r_enabled = e_enabled.search_fixed_depth(&mut b_enabled, 3);

    assert_eq!(r_disabled.nodes, r_enabled.nodes,
        "enabling endgame check changed node count at opening");
    println!(
        "[Phase 6] opening cost with vs without endgame check: \
         {} ms vs {} ms ({} nodes)",
        r_enabled.time_ms, r_disabled.time_ms, r_disabled.nodes,
    );
    let slower_ms = r_enabled.time_ms.max(1);
    let faster_ms = r_disabled.time_ms.max(1);
    // Generous bound: a cheap activation check shouldn't cost more than ~30%.
    let ratio = slower_ms as f64 / faster_ms as f64;
    assert!(
        ratio < 1.4,
        "endgame check added {:.0}% to opening search time",
        (ratio - 1.0) * 100.0
    );
}

#[test]
fn endgame_solver_value_independent_of_input_depth() {
    // Solver always extends past depth 0 to terminal. So calling
    // search_fixed_depth at depth 1 vs depth 6 must produce the same value
    // (both explore the same full game-end tree). If the solver were broken
    // and silently returned a heuristic at depth 0, the two depths would
    // expand different subtrees of leaves and likely return different values.
    //
    // Driving to ply 28 keeps the remaining game tree small enough that
    // running the search at both depths is fast.
    let late = drive_to_late_state(28);
    if late.game_over() {
        return;
    }

    let mut e_shallow = SearchEngine::new(16);
    e_shallow.set_endgame_threshold(u32::MAX);
    let r_shallow = e_shallow.search_fixed_depth(&mut late.clone(), 1);

    let mut e_deep = SearchEngine::new(16);
    e_deep.set_endgame_threshold(u32::MAX);
    let r_deep = e_deep.search_fixed_depth(&mut late.clone(), 6);

    println!(
        "[Phase 6] solver depth-independence at ply {}: \
         d1 value={} ({} nodes, {} ms), \
         d6 value={} ({} nodes, {} ms)",
        late.ply, r_shallow.value, r_shallow.nodes, r_shallow.time_ms,
        r_deep.value, r_deep.nodes, r_deep.time_ms,
    );
    assert_eq!(
        r_shallow.value, r_deep.value,
        "solver value differs across input depths — solver is not extending to terminal"
    );
}

#[test]
fn endgame_solver_threshold_changes_take_effect() {
    let mut e = SearchEngine::new(16);
    assert_eq!(e.endgame_threshold(), 6);
    e.set_endgame_threshold(0);
    assert_eq!(e.endgame_threshold(), 0);
    e.set_endgame_threshold(42);
    assert_eq!(e.endgame_threshold(), 42);
}
