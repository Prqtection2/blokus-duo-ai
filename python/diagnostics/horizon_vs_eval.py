"""Run the PM's horizon-vs-eval experiment.

Procedure:
  1. Play the engine against a `CenterPressurePlayer` that fights for the
     center (approximates Neev's sister's blocking style). Record every move
     and the engine's per-ply eval.
  2. Save the game as a JSON fixture (`diagnostics/games/<timestamp>.json`)
     so it can be replayed later as a regression test.
  3. Identify the "committing ply" — the engine's first move whose evaluation
     drops below a hostile threshold (e.g., -200). This is the move where the
     engine walked into the box.
  4. Replay to the position just before the committing move.
  5. At that exact position, run a depth sweep (4, 6, 8) with TWO weight
     sets: the current GUI default (territory=0) and the Phase 7 tuned
     champion (territory=-40). For each, log the chosen move + its centroid
     + per-term eval breakdown.

Interpretation:
  - If at higher depth (8) the engine picks a forward/contesting move
    instead of the committing one -> **horizon problem**. The eval is OK;
    the search is too shallow at the time-budget the GUI uses.
  - If the engine picks the same (or another retreating) move at all depths
    and both weight sets -> **eval problem**. The committing position
    misvalues the right answer; we need a real new feature.

Usage:
    python python/diagnostics/horizon_vs_eval.py
"""

from __future__ import annotations

import datetime as _dt
import json
import random
import sys
import time
from pathlib import Path

_HERE = Path(__file__).resolve()
_PYTHON_ROOT = _HERE.parent.parent
if str(_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(_PYTHON_ROOT))

import blokus
from blokus_harness import BlockerPlayer, EnginePlayer


# ───────────────────────── config ─────────────────────────


TUNED_WEIGHTS = {
    "placed_squares": 100,
    "corner_count": 80,
    "territory": -40,
    "piece_liability": -10,
}

NEUTRAL_WEIGHTS = {
    "placed_squares": 100,
    "corner_count": 80,
    # The territory feature is contested-reach (BFS-based, 2026-05-27).
    # GUI default = +20 after observing +60 dominated and destabilized play.
    "territory": 60,
    "piece_liability": -10,
}

PIECE_SIZES = [1, 2, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5]

ENGINE_SIDE = 1  # engine plays as P1 (Purple), like in Neev's game
PLAY_TIME_BUDGET_MS = 1000
COMMITTING_THRESHOLD = -200
# Time-budgeted sweep instead of fixed depth — bounded wall time, reaches the
# deepest depth that fits in each budget. 1s matches in-game; 10s, 60s let the
# engine see further to test the horizon hypothesis.
TIME_BUDGET_SWEEP_MS = (1000, 10000, 60000)
SEED = 42


# ───────────────────────── helpers ─────────────────────────


def corners_of(board, p):
    """Replicate the maintained `corners[p]` mask in Python from raw cells."""
    own = {tuple(c) for c in board.cells_of(p)}
    start_cells = blokus.start_cells()
    if not own:
        return {tuple(start_cells[p])}
    other = {tuple(c) for c in board.cells_of(1 - p)}
    occupied = own | other
    diag, ortho = set(), set()
    for r, c in own:
        for dr, dc in ((-1, -1), (-1, 1), (1, -1), (1, 1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < 14 and 0 <= nc < 14:
                diag.add((nr, nc))
        for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < 14 and 0 <= nc < 14:
                ortho.add((nr, nc))
    return diag - ortho - occupied


def one_step_influence(own):
    result = set(own)
    for r, c in own:
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                if dr == 0 and dc == 0:
                    continue
                nr, nc = r + dr, c + dc
                if 0 <= nr < 14 and 0 <= nc < 14:
                    result.add((nr, nc))
    return result


def per_term_breakdown(board, weights):
    """Compute each eval term's value AND weighted contribution at this
    position, from the side-to-move's perspective. Replicates the Rust
    `heuristic_with` arithmetic in Python so we can see which term is
    swinging the engine's decision."""
    stm = board.side_to_move
    other = 1 - stm

    # placed_squares
    placed_diff = (89 - board.squares_left(stm)) - (89 - board.squares_left(other))

    # corner_count
    c_stm = corners_of(board, stm)
    c_oth = corners_of(board, other)
    corner_diff = len(c_stm) - len(c_oth)

    # territory (1-step Chebyshev influence)
    own_stm = {tuple(c) for c in board.cells_of(stm)}
    own_oth = {tuple(c) for c in board.cells_of(other)}
    occupied = own_stm | own_oth
    empty = {(r, c) for r in range(14) for c in range(14)} - occupied
    me_reach = one_step_influence(own_stm)
    opp_reach = one_step_influence(own_oth)
    mine = (me_reach & empty) - opp_reach
    theirs = (opp_reach & empty) - me_reach
    territory_diff = len(mine) - len(theirs)

    # piece_liability (size^2 sum)
    lia_stm = sum(PIECE_SIZES[i] ** 2 for i in board.pieces_left(stm))
    lia_oth = sum(PIECE_SIZES[i] ** 2 for i in board.pieces_left(other))
    liability_diff = lia_stm - lia_oth

    raw = {
        "placed_diff": placed_diff,
        "corner_diff": corner_diff,
        "territory_diff": territory_diff,
        "liability_diff": liability_diff,
    }
    weighted = {
        "placed_squares": weights["placed_squares"] * placed_diff,
        "corner_count": weights["corner_count"] * corner_diff,
        "territory": weights["territory"] * territory_diff,
        "piece_liability": weights["piece_liability"] * liability_diff,
    }
    weighted["TOTAL"] = sum(weighted.values())
    return raw, weighted


def fmt_move(mv) -> str:
    if mv is None or mv.is_pass:
        return "(pass)"
    name = blokus.piece_names()[mv.piece_id]
    cells = sorted(mv.cells())
    cy = sum(c[0] for c in cells) / len(cells)
    cx = sum(c[1] for c in cells) / len(cells)
    return f"{name} centroid=({cy:.1f},{cx:.1f}) cells={cells}"


def centroid(cells):
    if not cells:
        return None
    cy = sum(c[0] for c in cells) / len(cells)
    cx = sum(c[1] for c in cells) / len(cells)
    return (cy, cx)


# ───────────────────────── play + record ─────────────────────────


def play_and_record(engine_weights):
    """Play one game with engine vs BlockerPlayer. Engine plays
    `ENGINE_SIDE`. Return (moves, engine_eval_log, final_board)."""

    rng = random.Random(SEED)
    board = blokus.Board()
    engine = EnginePlayer(
        time_budget_ms=PLAY_TIME_BUDGET_MS,
        weights=engine_weights,
        max_depth=16,
    )
    human = BlockerPlayer(rng=rng)

    moves: list[dict] = []
    engine_eval_log: list[dict] = []

    while not board.is_terminal() and board.ply < 100:
        legal = board.legal_moves()
        if not legal:
            board.make_pass()
            moves.append({"ply": board.ply - 1, "side": 1 - board.side_to_move, "pass": True})
            continue
        side = board.side_to_move
        if side == ENGINE_SIDE:
            mv = engine.select_move(board, legal)
            sr = engine.last_result
            engine_eval_log.append({
                "ply": int(board.ply),
                "value": int(sr.value),
                "depth": int(sr.depth),
                "nodes": int(sr.nodes),
                "move": repr(mv),
            })
        else:
            mv = human.select_move(board, legal)
        moves.append({
            "ply": int(board.ply),
            "side": int(side),
            "piece_id": int(mv.piece_id),
            "cells": [list(c) for c in mv.cells()],
        })
        board.make_move(mv)

    return moves, engine_eval_log, board


def replay_to_ply(moves, target_ply):
    """Replay the recorded game up to (but not including) `target_ply`."""
    board = blokus.Board()
    for m in moves:
        if m["ply"] >= target_ply:
            break
        if m.get("pass"):
            board.make_pass()
            continue
        piece_id = m["piece_id"]
        target_cells = frozenset(tuple(c) for c in m["cells"])
        for legal in board.legal_moves():
            if legal.piece_id == piece_id and frozenset(legal.cells()) == target_cells:
                board.make_move(legal)
                break
        else:
            raise RuntimeError(f"Could not replay move at ply {m['ply']}")
    return board


def find_committing_ply(eval_log, threshold=COMMITTING_THRESHOLD):
    """The committing ply is the engine's turn IMMEDIATELY BEFORE its eval
    first crashes below `threshold`. That's where the engine still thought
    the position was OK and made the move that walked into trouble. If we
    re-search FROM that position, we want to see whether a deeper search
    would have rejected the move it actually picked."""
    prev = None
    for entry in eval_log:
        if entry["value"] <= threshold and (prev is None or prev["value"] > threshold):
            return prev["ply"] if prev else entry["ply"]
        prev = entry
    return None


def time_budget_sweep(board, weights_options, budgets_ms):
    """For each (label, weights) x time-budget, run an iterative-deepening
    search from `board`. Captures the depth actually reached + the chosen
    move. Bounded wall time (no risk of hanging on deep searches)."""
    out = []
    for label, weights in weights_options:
        for ms in budgets_ms:
            eng = blokus.SearchEngine(tt_size_log2=18)
            eng.set_weights(**weights)
            t0 = time.perf_counter()
            r = eng.search(board, time_budget_ms=ms, max_depth=16)
            wall = time.perf_counter() - t0
            cells = sorted(r.best_move.cells()) if r.best_move and not r.best_move.is_pass else []
            cent = centroid(cells)
            out.append({
                "weights_label": label,
                "budget_ms": ms,
                "depth": r.depth,
                "value": r.value,
                "nodes": r.nodes,
                "wall_s": round(wall, 2),
                "move_piece_id": r.best_move.piece_id if r.best_move else None,
                "move_cells": cells,
                "centroid": cent,
            })
            print(
                f"    {label:<14} budget={ms:>5}ms  "
                f"depth_reached={r.depth:<3}  "
                f"move={fmt_move(r.best_move):<60}  "
                f"value={r.value:>6}  nodes={r.nodes:>9,}  wall={wall:5.2f}s",
                flush=True,
            )
    return out


# ───────────────────────── main ─────────────────────────


def main() -> None:
    games_dir = _PYTHON_ROOT.parent / "diagnostics" / "games"
    games_dir.mkdir(parents=True, exist_ok=True)

    # If we already have a saved game from a previous run, reuse it instead of
    # spending 15s replaying. (Pass --replay to force a fresh game.)
    force_replay = "--replay" in sys.argv
    existing = sorted(games_dir.glob("horizon_vs_eval_*.json"))
    if existing and not force_replay:
        latest = existing[-1]
        print(f"Reusing saved game: {latest.name} (pass --replay to play fresh)", flush=True)
        payload = json.loads(latest.read_text())
        moves = payload["moves"]
        engine_eval_log = payload["engine_eval_log"]
        game_path = latest
    else:
        print("=" * 78, flush=True)
        print(f"Step 1: play engine (P1, territory=0) vs BlockerPlayer (P0)", flush=True)
        print(f"Engine: time={PLAY_TIME_BUDGET_MS}ms, weights={NEUTRAL_WEIGHTS}", flush=True)
        print("=" * 78, flush=True)
        timestamp = _dt.datetime.now().strftime("%Y%m%d_%H%M%S")
        t0 = time.perf_counter()
        moves, engine_eval_log, final_board = play_and_record(NEUTRAL_WEIGHTS)
        elapsed = time.perf_counter() - t0
        print()
        print(f"Game finished at ply {final_board.ply}: "
              f"P0={final_board.score(0)}, P1={final_board.score(1)}", flush=True)
        print(f"Played in {elapsed:.1f}s", flush=True)
        game_path = games_dir / f"horizon_vs_eval_{timestamp}.json"
        game_path.write_text(json.dumps({
            "version": 1,
            "timestamp": timestamp,
            "engine_weights": NEUTRAL_WEIGHTS,
            "engine_side": ENGINE_SIDE,
            "engine_time_budget_ms": PLAY_TIME_BUDGET_MS,
            "opponent": "BlockerPlayer",
            "seed": SEED,
            "moves": moves,
            "engine_eval_log": engine_eval_log,
            "final_score_p0": int(final_board.score(0)),
            "final_score_p1": int(final_board.score(1)),
        }, indent=2))
        print(f"\nGame saved -> {game_path}", flush=True)

    print()
    print("Engine eval over time (P1 perspective):", flush=True)
    for entry in engine_eval_log:
        print(f"  ply {entry['ply']:>3}: value={entry['value']:>6}  "
              f"depth={entry['depth']}  nodes={entry['nodes']:>8,}", flush=True)

    # Find committing ply
    committing_ply = find_committing_ply(engine_eval_log)
    if committing_ply is None:
        print("\nNo ply hit the committing threshold; nothing more to analyze.")
        return

    print()
    print("=" * 78)
    print(f"Step 2: committing ply = {committing_ply} "
          f"(first eval <= {COMMITTING_THRESHOLD})")
    print("=" * 78)
    # Replay to the engine's position just BEFORE that move
    pre_board = replay_to_ply(moves, committing_ply)
    print(pre_board.ascii())

    # Per-term breakdown at this position
    print()
    print("Per-term eval breakdown at this position (engine's perspective):")
    print(f"{'term':<18} {'raw diff':>10} {'  weight':>10} {'contribution':>14}")
    for label, weights in (("neutral (terr=0)", NEUTRAL_WEIGHTS),
                            ("tuned (terr=-40)", TUNED_WEIGHTS)):
        print(f"  --- weights: {label} ---")
        raw, weighted = per_term_breakdown(pre_board, weights)
        for k in ("placed_squares", "corner_count", "territory", "piece_liability"):
            raw_key = {"placed_squares": "placed_diff", "corner_count": "corner_diff",
                       "territory": "territory_diff", "piece_liability": "liability_diff"}[k]
            print(f"  {k:<18} {raw[raw_key]:>10} {weights[k]:>10} {weighted[k]:>14}")
        print(f"  {'TOTAL':<18} {'':>10} {'':>10} {weighted['TOTAL']:>14}")

    # Depth sweep
    print()
    print("=" * 78)
    print(f"Step 3: time-budget sweep at committing position ({TIME_BUDGET_SWEEP_MS} ms)", flush=True)
    print("  -> If longer budgets switch to a forward/contesting move, horizon problem", flush=True)
    print("  -> If both weight sets pick the same move at the longest budget, eval problem", flush=True)
    print("=" * 78, flush=True)
    sweep_results = time_budget_sweep(
        pre_board,
        weights_options=[
            ("neutral terr=0", NEUTRAL_WEIGHTS),
            ("tuned terr=-40", TUNED_WEIGHTS),
        ],
        budgets_ms=TIME_BUDGET_SWEEP_MS,
    )

    print()
    print("=" * 78)
    print("Interpretation", flush=True)
    print("=" * 78)
    # Compare moves at shallow (1s) vs longest budget — did the choice change?
    neutral_by_budget = {r["budget_ms"]: r for r in sweep_results if r["weights_label"] == "neutral terr=0"}
    tuned_by_budget = {r["budget_ms"]: r for r in sweep_results if r["weights_label"] == "tuned terr=-40"}

    short = TIME_BUDGET_SWEEP_MS[0]
    long = TIME_BUDGET_SWEEP_MS[-1]
    sn = neutral_by_budget.get(short)
    dn = neutral_by_budget.get(long)
    if sn and dn:
        same = sn["move_cells"] == dn["move_cells"]
        print(f"neutral (terr=0):  {short}ms move == {long}ms move? {same}", flush=True)
        print(f"   short: depth={sn['depth']} value={sn['value']} cells={sn['move_cells']}", flush=True)
        print(f"   long:  depth={dn['depth']} value={dn['value']} cells={dn['move_cells']}", flush=True)
    st = tuned_by_budget.get(short)
    dt = tuned_by_budget.get(long)
    if st and dt:
        same = st["move_cells"] == dt["move_cells"]
        print(f"tuned (terr=-40):  {short}ms move == {long}ms move? {same}", flush=True)
        print(f"   short: depth={st['depth']} value={st['value']} cells={st['move_cells']}", flush=True)
        print(f"   long:  depth={dt['depth']} value={dt['value']} cells={dt['move_cells']}", flush=True)

    print()
    print(f"Game JSON: {game_path}")


if __name__ == "__main__":
    main()
