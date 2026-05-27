"""Per-term eval breakdown + contested partition for a position.

Replaced the original BFS-based contested-reach diagnostic on 2026-05-27 with
piece-coverage-based metric (see eval.rs `piece_aware_territory_diff`). All
computation is delegated to the Rust extension via PyO3 so the Python view
matches what the engine actually scores.
"""

from __future__ import annotations

import sys
from pathlib import Path

_HERE = Path(__file__).resolve()
sys.path.insert(0, str(_HERE.parent.parent))

import blokus


CHAMPION_WEIGHTS = {
    "placed_squares": 100,
    "corner_count": 80,
    "territory": 60,
    "piece_liability": -10,
}

PIECE_SIZES = [1, 2, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5]
SEEDS_TO_BOARD_CENTER = (6.5, 6.5)

# Partition codes from eval::partition (kept in sync with eval.rs).
OWN_P0, OWN_P1 = 0, 1
SAFE_P0, SAFE_P1 = 2, 3
TIED = 6
UNREACHABLE = 7


def live_corners_set(board: blokus.Board, p: int) -> set:
    """Live corners for player p — corners with at least one ortho-adjacent
    extension-eligible cell. Replicates eval.rs live_corners() in Python."""
    own = {tuple(c) for c in board.cells_of(p)}
    if not own:
        return {tuple(blokus.start_cells()[p])}
    opp = {tuple(c) for c in board.cells_of(1 - p)}
    occupied = own | opp
    diag = set()
    ortho = set()
    for r, c in own:
        for dr, dc in ((-1, -1), (-1, 1), (1, -1), (1, 1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < 14 and 0 <= nc < 14:
                diag.add((nr, nc))
        for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < 14 and 0 <= nc < 14:
                ortho.add((nr, nc))
    corners = diag - (own | ortho) - occupied
    forbidden = own | ortho
    ext = set()
    for r in range(14):
        for c in range(14):
            if (r, c) in occupied or (r, c) in forbidden:
                continue
            ext.add((r, c))
    live = set()
    for r, c in corners:
        for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            if (r + dr, c + dc) in ext:
                live.add((r, c))
                break
    return live


def render_partition(board: blokus.Board, partition: list[int]) -> str:
    """ASCII grid for the piece-coverage partition.

    Glyphs:
      X / O  = P0 / P1 stones
      0 / 1  = SAFE_P0 / SAFE_P1 (only that player can cover this turn)
      =      = TIED (both can cover — race)
      .      = UNREACHABLE (neither can cover this turn)
    """
    glyph = {
        OWN_P0: "X",
        OWN_P1: "O",
        SAFE_P0: "0",
        SAFE_P1: "1",
        TIED: "=",
        UNREACHABLE: ".",
    }
    rows = ["    " + " ".join(f"{c % 10}" for c in range(14))]
    for r in range(14):
        row = f"{r:>2}  "
        for c in range(14):
            row += glyph.get(partition[r * 14 + c], "?") + " "
        rows.append(row.rstrip())
    return "\n".join(rows)


def per_term_breakdown(board: blokus.Board, weights: dict) -> dict:
    """Compute per-term eval from stm's perspective using Rust primitives."""
    stm = board.side_to_move
    other = 1 - stm
    placed_diff = (89 - board.squares_left(stm)) - (89 - board.squares_left(other))
    stm_live = len(live_corners_set(board, stm))
    other_live = len(live_corners_set(board, other))
    corner_diff = stm_live - other_live

    cov_stm = {tuple(c) for c in blokus.coverable_cells(board, stm)}
    cov_oth = {tuple(c) for c in blokus.coverable_cells(board, other)}
    only_stm = len(cov_stm - cov_oth)
    only_oth = len(cov_oth - cov_stm)
    tied = len(cov_stm & cov_oth)
    territory_diff = only_stm - only_oth

    lia_stm = sum(PIECE_SIZES[i] ** 2 for i in board.pieces_left(stm))
    lia_oth = sum(PIECE_SIZES[i] ** 2 for i in board.pieces_left(other))
    liability_diff = lia_stm - lia_oth

    return {
        "placed_squares": (placed_diff, weights["placed_squares"], weights["placed_squares"] * placed_diff),
        "corner_count": (corner_diff, weights["corner_count"], weights["corner_count"] * corner_diff),
        "territory": (territory_diff, weights["territory"], weights["territory"] * territory_diff),
        "piece_liability": (liability_diff, weights["piece_liability"], weights["piece_liability"] * liability_diff),
        "_stm": stm,
        "_only_stm": only_stm,
        "_only_oth": only_oth,
        "_tied": tied,
    }


def fmt_move(mv) -> str:
    if mv is None or mv.is_pass:
        return "(pass)"
    cells = sorted(mv.cells())
    name = blokus.piece_names()[mv.piece_id]
    cy = sum(c[0] for c in cells) / len(cells)
    cx = sum(c[1] for c in cells) / len(cells)
    return f"{name} centroid=({cy:.1f},{cx:.1f}) cells={cells}"


def central_distance(mv) -> float:
    cells = mv.cells()
    cy = sum(c[0] for c in cells) / len(cells)
    cx = sum(c[1] for c in cells) / len(cells)
    return (cy - SEEDS_TO_BOARD_CENTER[0]) ** 2 + (cx - SEEDS_TO_BOARD_CENTER[1]) ** 2


def print_for_move(label: str, board: blokus.Board, mv, weights: dict) -> None:
    print(f"\n{'='*78}", flush=True)
    print(f"{label}: {fmt_move(mv)}", flush=True)
    print('=' * 78, flush=True)
    board.make_move(mv)
    bk = per_term_breakdown(board, weights)
    stm = bk["_stm"]
    print(f"After this move, STM = P{stm} (the other player).")
    print(f"Per-term eval from P{stm}'s perspective (engine view = negate):")
    print(f"  {'term':<18} {'raw':>6} {'weight':>8} {'contribution':>14}")
    total = 0
    for term in ("placed_squares", "corner_count", "territory", "piece_liability"):
        raw, w, contrib = bk[term]
        total += contrib
        print(f"  {term:<18} {raw:>6} {w:>8} {contrib:>14}")
    print(f"  {'TOTAL':<18} {'':>6} {'':>8} {total:>14}")
    print(
        f"  coverable cells -- only-P{stm}: {bk['_only_stm']}, "
        f"only-P{1-stm}: {bk['_only_oth']}, tied: {bk['_tied']}"
    )
    print()
    print("Piece-coverage partition (X/O = stones, 0/1 = only-that-player can cover,")
    print("                         = tied (both can cover), . unreachable):")
    print(render_partition(board, blokus.contested_partition(board)))
    board.unmake_move()
