"""Match runner: N games between two players, alternating starting side.

Returns a TournamentResult with win/loss/draw counts, mean score margin, and
both a Wilson 95% interval on win-rate and a t-interval on the mean margin.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Callable

from blokus_harness.harness import play_game


@dataclass
class TournamentResult:
    a_wins: int = 0
    b_wins: int = 0
    draws: int = 0
    margins: list[int] = field(default_factory=list)  # from A's perspective

    @property
    def n_games(self) -> int:
        return self.a_wins + self.b_wins + self.draws

    @property
    def a_win_rate(self) -> float:
        """A's win rate, counting draws as half-wins."""
        if self.n_games == 0:
            return 0.0
        return (self.a_wins + 0.5 * self.draws) / self.n_games

    def wilson_ci(self, z: float = 1.96) -> tuple[float, float]:
        """Wilson-score CI on A's win rate (draws-as-half)."""
        n = self.n_games
        if n == 0:
            return (0.0, 1.0)
        p = self.a_win_rate
        denom = 1.0 + z * z / n
        center = (p + z * z / (2.0 * n)) / denom
        radius = (z / denom) * math.sqrt(p * (1.0 - p) / n + z * z / (4.0 * n * n))
        return (center - radius, center + radius)

    def margin_mean_ci(self, z: float = 1.96) -> tuple[float, float, float]:
        """(mean, lo, hi) — normal-approximation CI for mean of A's margins."""
        n = len(self.margins)
        if n == 0:
            return (0.0, 0.0, 0.0)
        mean = sum(self.margins) / n
        if n < 2:
            return (mean, mean, mean)
        var = sum((m - mean) ** 2 for m in self.margins) / (n - 1)
        se = math.sqrt(var / n)
        return (mean, mean - z * se, mean + z * se)

    def summary(self, label_a: str = "A", label_b: str = "B") -> str:
        lo, hi = self.wilson_ci()
        mean, mlo, mhi = self.margin_mean_ci()
        return (
            f"{label_a} vs {label_b}: {self.a_wins}W/{self.b_wins}L/{self.draws}D "
            f"over {self.n_games} games\n"
            f"  {label_a} win rate: {self.a_win_rate * 100:5.1f}% "
            f"(95% CI [{lo * 100:.1f}%, {hi * 100:.1f}%])\n"
            f"  mean margin ({label_a} - {label_b}): {mean:+.2f} "
            f"(95% CI [{mlo:+.2f}, {mhi:+.2f}])"
        )


def run_tournament(
    a_factory: Callable[[], object],
    b_factory: Callable[[], object],
    n_games: int,
    *,
    verbose: bool = False,
) -> TournamentResult:
    """Play `n_games` between players from `a_factory()` and `b_factory()`.

    The starting side alternates: on even-indexed games A plays as side 0,
    on odd-indexed games A plays as side 1. Score margins are always reported
    from A's perspective.
    """
    result = TournamentResult()
    for i in range(n_games):
        a, b = a_factory(), b_factory()
        if i % 2 == 0:
            game = play_game(a, b)
            a_score, b_score = game.score0, game.score1
        else:
            game = play_game(b, a)
            a_score, b_score = game.score1, game.score0

        margin = a_score - b_score
        result.margins.append(margin)
        if margin > 0:
            result.a_wins += 1
        elif margin < 0:
            result.b_wins += 1
        else:
            result.draws += 1

        if verbose:
            print(f"Game {i + 1:>4}: A={a_score:>4}  B={b_score:>4}  (margin {margin:+})")

    return result
