"""Does the engine actually use the time budget we give it?

PM's question: at 250ms with aggressive LMR pruning, the engine may be
finishing its search early and returning. If wall time stays near 250ms
when we ask for 5000ms, it's exhausting a too-narrow search; if it climbs
with the budget, depth is the right lever.
"""
from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import blokus


WEIGHTS = {
    "placed_squares": 100,
    "corner_count": 80,
    "territory": 0,
    "piece_liability": -10,
}

PLIES_TO_TEST = (0, 4, 8, 12, 16, 20)  # representative slices of a game


def play_self_to_ply(target_ply: int) -> blokus.Board:
    """Play the engine vs itself with fixed depth=2 to reach `target_ply`
    deterministically and quickly. Just to construct a position; no need
    for strong play here."""
    board = blokus.Board()
    eng = blokus.SearchEngine(tt_size_log2=16)
    eng.set_weights(**WEIGHTS)
    while board.ply < target_ply and not board.is_terminal():
        legal = board.legal_moves()
        if not legal:
            board.make_pass()
            continue
        r = eng.search_fixed_depth(board, 2)
        if r.best_move is None:
            board.make_pass()
        else:
            board.make_move(r.best_move)
    return board


def measure_at(board: blokus.Board, budget_ms: int) -> dict:
    eng = blokus.SearchEngine(tt_size_log2=18)
    eng.set_weights(**WEIGHTS)
    t0 = time.perf_counter()
    r = eng.search(board, time_budget_ms=budget_ms, max_depth=20)
    wall_ms = (time.perf_counter() - t0) * 1000.0
    return {
        "budget_ms": budget_ms,
        "wall_ms": wall_ms,
        "depth": r.depth,
        "nodes": r.nodes,
        "value": r.value,
    }


def main() -> None:
    for target_ply in PLIES_TO_TEST:
        board = play_self_to_ply(target_ply)
        actual_ply = int(board.ply)
        if board.is_terminal():
            print(f"\n=== ply {target_ply} (terminal at {actual_ply}) ===", flush=True)
            continue
        print(f"\n=== ply {actual_ply} (stm={board.side_to_move}) ===", flush=True)
        print(f"{'budget':>8} {'wall':>8} {'used %':>7} {'depth':>6} {'nodes':>10} {'value':>7}", flush=True)
        for budget in (250, 1000, 5000):
            r = measure_at(board, budget)
            used_pct = 100.0 * r["wall_ms"] / r["budget_ms"]
            print(
                f"  {r['budget_ms']:>5}ms "
                f"{r['wall_ms']:>7.0f}ms "
                f"{used_pct:>6.0f}% "
                f"{r['depth']:>6} "
                f"{r['nodes']:>10,} "
                f"{r['value']:>7}",
                flush=True,
            )


if __name__ == "__main__":
    main()
