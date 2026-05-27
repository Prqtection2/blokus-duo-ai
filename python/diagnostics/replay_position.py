"""Replay a saved GUI position and show why the engine chose what it did.

Usage:
  # Diagnose the most recent saved position
  python python/diagnostics/replay_position.py

  # Specific dump
  python python/diagnostics/replay_position.py python/diagnostics/positions/pos_ply008_*.json

  # Compare engine's choice with the biggest move that covers a specific cell
  python python/diagnostics/replay_position.py --alt 6,5

By default we replay history minus the *last* entry — so if you clicked
"Save position" right after the engine moved, we re-derive the engine's
choice at the position it was facing, and show the per-term breakdown +
contested partition.
"""

from __future__ import annotations

import argparse
import json
import sys
import time as _time
from pathlib import Path

_HERE = Path(__file__).resolve()
sys.path.insert(0, str(_HERE.parent.parent))

import blokus
from diagnostics.move_choice_breakdown import (
    CHAMPION_WEIGHTS,
    fmt_move,
    print_for_move,
)


POSITIONS_DIR = _HERE.parent / "positions"


def latest_dump() -> Path:
    dumps = sorted(POSITIONS_DIR.glob("pos_*.json"))
    if not dumps:
        raise FileNotFoundError(
            f"No saved positions in {POSITIONS_DIR}. "
            "Click 'Save position' in the GUI first."
        )
    return dumps[-1]


def replay_history(history: list) -> blokus.Board:
    board = blokus.Board()
    for entry in history:
        if entry.get("passed"):
            board.make_pass()
            continue
        piece_id = entry["piece_id"]
        target = frozenset(tuple(c) for c in entry["cells"])
        for legal in board.legal_moves():
            if legal.piece_id == piece_id and frozenset(legal.cells()) == target:
                board.make_move(legal)
                break
        else:
            raise RuntimeError(f"Cannot replay history entry: {entry}")
    return board


def fmt_history_entry(entry: dict) -> str:
    if entry.get("passed"):
        return f"P{entry['by']} pass"
    name = blokus.piece_names()[entry["piece_id"]]
    return f"P{entry['by']} {name} cells={entry['cells']}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "path",
        nargs="?",
        default=None,
        help="Path to dump JSON (default: latest in positions/)",
    )
    parser.add_argument(
        "--budget-ms",
        type=int,
        default=3000,
        help="Search time budget for the replayed search (default 3000).",
    )
    parser.add_argument(
        "--from-current",
        action="store_true",
        help="Replay the full history (don't trim last move). Use when you "
             "want to see what the engine WOULD play next from the current "
             "displayed position, rather than re-deriving the last move.",
    )
    parser.add_argument(
        "--alt",
        type=str,
        default=None,
        help='"r,c" — also show breakdown for the largest legal move that '
             "covers cell (r,c). Useful for asking 'why didn't it play here?'",
    )
    args = parser.parse_args()

    dump_path = Path(args.path) if args.path else latest_dump()
    print(f"Loading: {dump_path}", flush=True)
    payload = json.loads(dump_path.read_text())
    history = payload.get("history", [])
    weights = payload.get("engine_weights") or CHAMPION_WEIGHTS

    if not history:
        print("Empty history -- nothing to diagnose.")
        return

    if args.from_current:
        board = replay_history(history)
        trimmed = None
    else:
        trimmed = history[-1]
        board = replay_history(history[:-1])

    print(f"Replayed to board.ply = {board.ply}, P{board.side_to_move} to move.")
    if trimmed is not None:
        print(f"(Trimmed last history entry: {fmt_history_entry(trimmed)})")
    print(f"Weights: {weights}")
    print()
    print(board.ascii())

    eng = blokus.SearchEngine(tt_size_log2=18)
    eng.set_weights(**weights)
    t0 = _time.perf_counter()
    r = eng.search(board, time_budget_ms=args.budget_ms, max_depth=16)
    wall_ms = round((_time.perf_counter() - t0) * 1000.0, 1)
    engine_choice = r.best_move
    print(
        f"\nEngine search ({args.budget_ms}ms budget): "
        f"value={r.value} depth={r.depth} nodes={r.nodes} "
        f"time_ms={r.time_ms} (last completed iter) wall_ms={wall_ms} "
        f"nps={r.nodes_per_second:.0f}"
    )
    print(f"Engine's chosen move: {fmt_move(engine_choice)}")

    print_for_move("ENGINE'S CHOICE", board, engine_choice, weights)

    if args.alt:
        try:
            ar, ac = (int(x) for x in args.alt.split(","))
        except ValueError:
            print(f"\nInvalid --alt format: {args.alt!r} (expected 'r,c')")
            return
        legal = board.legal_moves()
        candidates = [
            m for m in legal
            if (ar, ac) in [tuple(c) for c in m.cells()]
        ]
        if not candidates:
            print(f"\nNo legal move covers cell ({ar}, {ac}).")
            return
        candidates.sort(key=lambda m: -m.size)
        alt = candidates[0]
        print_for_move(
            f"ALTERNATIVE (largest covering ({ar},{ac}))",
            board, alt, weights,
        )


if __name__ == "__main__":
    main()
