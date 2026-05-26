"""Phase 7 tuning pipeline tests.

- SPRT unit tests (decision logic, draws, bounds).
- A small end-to-end match that exercises the multiprocessing worker code.
- A slow opt-in coordinate-descent run.
"""

from __future__ import annotations

import math

import pytest

from tuning import (
    DEFAULT_WEIGHTS,
    MatchConfig,
    OptimizerConfig,
    coordinate_descent,
    run_sprt_match,
)
from tuning.sprt import SprtDecision, SprtState, elo_to_winrate


# ───────────────────── SPRT unit tests ─────────────────────


def test_elo_to_winrate_basics():
    assert elo_to_winrate(0) == pytest.approx(0.5)
    assert elo_to_winrate(400) == pytest.approx(0.909, abs=0.01)
    assert elo_to_winrate(-400) == pytest.approx(0.091, abs=0.01)


def test_sprt_bounds_match_alpha_beta():
    s = SprtState(alpha=0.05, beta=0.05)
    assert s.lower_bound == pytest.approx(math.log(0.05 / 0.95))
    assert s.upper_bound == pytest.approx(math.log(0.95 / 0.05))


def test_sprt_accepts_under_dominant_wins():
    s = SprtState(elo0=0, elo1=20)
    # ~150 wins, ~50 losses for someone clearly stronger than the elo1 boundary
    # should cross the upper bound.
    for _ in range(150):
        s.update("W")
    for _ in range(50):
        s.update("L")
    assert s.decision == SprtDecision.ACCEPT, s.summary()


def test_sprt_rejects_under_dominant_losses():
    s = SprtState(elo0=0, elo1=20)
    for _ in range(150):
        s.update("L")
    for _ in range(50):
        s.update("W")
    assert s.decision == SprtDecision.REJECT, s.summary()


def test_sprt_continues_on_balanced_play():
    s = SprtState(elo0=0, elo1=20)
    for _ in range(40):
        s.update("W")
        s.update("L")
    assert s.decision == SprtDecision.CONTINUE, s.summary()


def test_sprt_draws_dont_change_log_lr_when_p1_is_50_percent():
    # When elo1==elo0==0, every result yields zero LLR.
    s = SprtState(elo0=0, elo1=0)
    for r in ("W", "L", "D"):
        s.update(r)
    assert s.log_lr == pytest.approx(0.0, abs=1e-12)


def test_sprt_rejects_unknown_result():
    s = SprtState()
    with pytest.raises(ValueError):
        s.update("X")


# ───────────────────── Match runner (multiprocessing fires) ─────────────────────


def test_sprt_match_runs_end_to_end_with_workers():
    """Identical engines should not pass SPRT in a small sample; the test
    asserts the pipeline returns a valid state and games actually played.

    A 10ms time budget keeps wallclock cost down. The point of the test is
    that the multiprocessing path (worker spawning, pickling, result return)
    works end-to-end — not strength."""
    cfg = MatchConfig(
        time_budget_ms=10,
        max_games=20,
        n_workers=2,
        seed_base=42,
    )
    result = run_sprt_match(
        candidate_weights=DEFAULT_WEIGHTS,
        champion_weights=DEFAULT_WEIGHTS,
        cfg=cfg,
    )
    assert result.sprt.n_games > 0
    # When identical engines play, with elo1=20 the SPRT should not accept;
    # it should reject or continue (most likely continue at small N).
    assert result.decision != SprtDecision.ACCEPT


# ───────────────────── Coordinate descent (slow opt-in) ─────────────────────


@pytest.mark.slow
def test_current_champion_still_beats_phase4_defaults_in_gauntlet():
    """Regression gauntlet: the tuned champion (Phase 7 v20260525_222926) must
    continue to clearly beat the Phase 4 hand-set defaults at SPRT significance.

    If any future change breaks the tuned champion's strength, this test will
    fail. Re-run `python python/run_tune.py` to re-tune from scratch if so.
    """
    from tuning import CURRENT_CHAMPION_WEIGHTS

    cfg = MatchConfig(
        time_budget_ms=20,
        max_games=200,
        n_workers=None,
        seed_base=0xDECA_FBAD,
    )
    result = run_sprt_match(
        candidate_weights=CURRENT_CHAMPION_WEIGHTS,
        champion_weights=DEFAULT_WEIGHTS,
        cfg=cfg,
    )
    print()
    print(result.summary())
    # Tuned champion should accept (SPRT) vs Phase 4 — at 200 max_games it has
    # plenty of headroom given the ~85% win-rate seen at training time.
    assert result.decision == SprtDecision.ACCEPT, (
        f"current champion no longer beats Phase 4 baseline at SPRT significance: "
        f"{result.summary()}"
    )


@pytest.mark.slow
def test_coordinate_descent_smoke_returns_valid_result():
    """Tiny coordinate-descent run: one outer iter, small SPRT, fast budget.
    Verifies the optimizer loops, calls the match runner, and returns a result.
    Does not require any specific promotion (any change is acceptable)."""
    opt_cfg = OptimizerConfig(
        initial_step=20,
        min_step=20,  # only one step size, only one outer iteration
        max_outer_iters=1,
        match=MatchConfig(
            time_budget_ms=10,
            max_games=20,
            n_workers=2,
            seed_base=12345,
        ),
    )
    result = coordinate_descent(DEFAULT_WEIGHTS, opt_cfg)
    assert result.final_weights == DEFAULT_WEIGHTS or result.accepted_steps
    assert isinstance(result.rejected_steps, list)
