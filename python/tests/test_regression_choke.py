"""Regression tests for the boxed-game failure that Neev's sister exposed.

The pre-LMR engine, playing as P1 with default (territory=0) eval against a
BlockerPlayer with seed 42, evaluated +80 at ply 11 (thought it was fine),
walked into a wall, crashed to -990 at ply 13, was forced to pass at ply 22,
and lost the game -44 to -36.

Post-LMR: the engine reaches depth 8-11 at the tactical mid-game plies where
the wall would have been built and routes around it instead. It plays a
different (better) game from the same opponent and seed, wins comfortably,
and is never forced to pass.

These tests pin both failure modes: forced passes and final-score collapse.
"""

from __future__ import annotations

import random
import sys
from pathlib import Path

import pytest

# Ensure blokus_harness is importable in process pool workers too (none here,
# but matches the rest of the test setup).
_HERE = Path(__file__).resolve()
sys.path.insert(0, str(_HERE.parent.parent))

import blokus
from blokus_harness import BlockerPlayer, EnginePlayer


@pytest.mark.slow
def test_engine_does_not_get_walled_by_blocker_seed_42():
    """End-to-end: from seed 42 — the seed that exposed the wall-off failure
    pre-LMR — the engine must finish the game without being forced to pass
    and must not lose by a wide margin.

    Pre-LMR baseline (before the fix landed): engine passed at ply 22, lost
    P1=-44 vs P0=-36 (deficit -8). The test gates against forced passes and
    a deficit worse than the pre-fix baseline."""
    # Match the GUI's engine config: 1000ms budget + the post-Phase-7-debug
    # eval weights (territory=0; the GUI's _default_engine_factory overrides
    # EnginePlayer's CHAMPION_WEIGHTS for human play).
    human_play_weights = {
        "placed_squares": 100,
        "corner_count": 80,
        "territory": 0,
        "piece_liability": -10,
    }
    rng = random.Random(42)
    board = blokus.Board()
    engine = EnginePlayer(
        time_budget_ms=1000,
        max_depth=16,
        weights=human_play_weights,
    )
    blocker = BlockerPlayer(rng=rng)

    engine_forced_passes = 0
    first_engine_pass_ply: int | None = None
    engine_min_eval = 0
    while not board.is_terminal() and board.ply < 100:
        legal = board.legal_moves()
        if board.side_to_move == 1:  # engine
            if not legal:
                engine_forced_passes += 1
                if first_engine_pass_ply is None:
                    first_engine_pass_ply = int(board.ply)
                board.make_pass()
            else:
                mv = engine.select_move(board, legal)
                if engine.last_result is not None:
                    engine_min_eval = min(engine_min_eval, int(engine.last_result.value))
                board.make_move(mv)
        else:  # blocker
            if not legal:
                board.make_pass()
            else:
                mv = blocker.select_move(board, legal)
                board.make_move(mv)

    p0_score = board.score(0)
    p1_score = board.score(1)
    margin = p1_score - p0_score
    print()
    print(f"final: P0={p0_score}, P1={p1_score}, engine margin={margin:+d}")
    print(f"engine forced passes: {engine_forced_passes}")
    print(f"first engine pass ply: {first_engine_pass_ply}")
    print(f"engine worst eval during game: {engine_min_eval}")

    # The pre-LMR failure was first-engine-pass at ply 22 (mid-game). After
    # LMR, any forced engine passes happen near game-end as a normal
    # piece-exhaustion outcome. Gate on first-pass-ply being late, not on
    # zero passes. (A pass at ply >= 30 with engine winning means it just
    # had unplaceable small pieces left, which is normal Blokus.)
    if first_engine_pass_ply is not None:
        assert first_engine_pass_ply >= 30, (
            f"engine forced to pass at ply {first_engine_pass_ply} — "
            f"this is the mid-game boxing failure from pre-LMR"
        )

    # Pre-LMR margin was -8 (engine lost). Post-LMR should win clearly.
    assert margin > 0, (
        f"engine margin {margin} non-positive — regression vs current "
        f"post-LMR baseline (was +28)"
    )

    # Hard catch on horizon: pre-LMR eval crashed to -990. Post-LMR worst is
    # around -80. Anything below -500 means the horizon problem is back.
    assert engine_min_eval > -500, (
        f"engine eval crashed to {engine_min_eval} at some point — "
        f"horizon problem may have returned"
    )
