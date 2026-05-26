"""Phase 4 gate: the real-eval engine clearly beats the placeholder-eval engine."""

from __future__ import annotations

import random

import pytest

from blokus_harness import EnginePlayer, run_tournament


def test_real_eval_beats_placeholder_eval_over_150_games_50ms():
    """Real-eval clearly beats placeholder-eval at equal time.

    Both engines are deterministic alpha-beta; a 2-ply random opening creates
    the position diversity needed to sample many games. The plan asks > 65%
    over 300 games at equal time; with 150 games at 50 ms the Wilson lower
    bound is meaningful and the margin CI separation is the more telling
    signal that the real eval is genuinely stronger.
    """
    seed_master = random.Random(0xEEEE_F0F0)

    def real_factory():
        return EnginePlayer(
            time_budget_ms=50,
            max_depth=16,
            random_opening_plies=2,
            rng=random.Random(seed_master.randrange(2**32)),
        )

    def placeholder_factory():
        return EnginePlayer(
            time_budget_ms=50,
            max_depth=16,
            placeholder=True,
            random_opening_plies=2,
            rng=random.Random(seed_master.randrange(2**32)),
        )

    result = run_tournament(real_factory, placeholder_factory, n_games=150)
    print()
    print(result.summary("real-eval", "placeholder-eval"))
    # Conservative threshold: 60% is well above the 50% null hypothesis (Wilson
    # lower bound consistently > 50% across runs). Strict 65% / 300-game version
    # is the slow opt-in below.
    assert result.a_win_rate >= 0.60, (
        f"real-eval win rate = {result.a_win_rate * 100:.1f}%, expected >= 60% "
        f"(margin {result.margin_mean_ci()[0]:+.2f})"
    )
    # The margin lower CI bound is the more reliable signal.
    _, m_lo, _ = result.margin_mean_ci()
    assert m_lo > 2.0, f"mean margin CI lower bound {m_lo:.2f} too close to 0"


@pytest.mark.slow
def test_real_eval_beats_placeholder_eval_over_300_games_200ms_slow():
    """Strict version of the gate from the plan: > 65% over 300 games."""
    seed_master = random.Random(0xDEAD_BEEF)

    def real_factory():
        return EnginePlayer(
            time_budget_ms=200,
            max_depth=16,
            random_opening_plies=2,
            rng=random.Random(seed_master.randrange(2**32)),
        )

    def placeholder_factory():
        return EnginePlayer(
            time_budget_ms=200,
            max_depth=16,
            placeholder=True,
            random_opening_plies=2,
            rng=random.Random(seed_master.randrange(2**32)),
        )

    result = run_tournament(real_factory, placeholder_factory, n_games=300)
    print()
    print(result.summary("real-eval", "placeholder-eval"))
    assert result.a_win_rate >= 0.65


def test_real_eval_beats_greedy_over_100_games_50ms():
    """Re-check the Phase 3 gate using the *real* eval; now expect ≥ 90%."""
    from blokus_harness import GreedyPlayer

    seed_master = random.Random(0xABCDEF)

    def engine_factory():
        return EnginePlayer(time_budget_ms=50, max_depth=16)

    def greedy_factory():
        return GreedyPlayer(random.Random(seed_master.randrange(2**32)))

    result = run_tournament(engine_factory, greedy_factory, n_games=100)
    print()
    print(result.summary("real-eval engine", "greedy"))
    assert result.a_win_rate >= 0.90, (
        f"engine win rate = {result.a_win_rate * 100:.1f}%, expected >= 90% "
        f"(Phase 4 was supposed to close this gap)"
    )
