"""Sequential Probability Ratio Test (SPRT) for engine-vs-engine match decisions.

Hypotheses:
- H0: candidate elo difference == `elo0` (typically 0 — "no improvement").
- H1: candidate elo difference == `elo1` (typically +20 — "real improvement").

After each game we update the log-likelihood ratio. When it crosses an upper
bound we accept H1, when it crosses a lower bound we accept H0; otherwise
keep playing. Standard Type-I / Type-II error bounds (alpha, beta = 0.05).

Draws are counted as half-win + half-loss (standard convention).
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import Enum


class SprtDecision(str, Enum):
    CONTINUE = "continue"
    ACCEPT = "accept"  # candidate significantly better (H1)
    REJECT = "reject"  # candidate not better (H0)


def elo_to_winrate(elo: float) -> float:
    """Standard logistic conversion: 0 elo = 0.5, +400 elo = ~0.91, etc."""
    return 1.0 / (1.0 + 10.0 ** (-elo / 400.0))


@dataclass
class SprtState:
    """Sequential-test state. Mutate via `update(result)` after each game.

    `result` is one of {"W", "L", "D"} from the candidate's perspective.
    """

    elo0: float = 0.0
    elo1: float = 20.0
    alpha: float = 0.05
    beta: float = 0.05
    wins: int = 0
    losses: int = 0
    draws: int = 0
    log_lr: float = 0.0

    @property
    def lower_bound(self) -> float:
        return math.log(self.beta / (1.0 - self.alpha))

    @property
    def upper_bound(self) -> float:
        return math.log((1.0 - self.beta) / self.alpha)

    @property
    def n_games(self) -> int:
        return self.wins + self.losses + self.draws

    @property
    def decision(self) -> SprtDecision:
        if self.log_lr >= self.upper_bound:
            return SprtDecision.ACCEPT
        if self.log_lr <= self.lower_bound:
            return SprtDecision.REJECT
        return SprtDecision.CONTINUE

    def update(self, result: str) -> None:
        p0 = elo_to_winrate(self.elo0)
        p1 = elo_to_winrate(self.elo1)
        # Per-game log-likelihood-ratio contributions.
        # The binomial-with-draws model treats a draw as half-W + half-L.
        log_w = math.log(p1 / p0)
        log_l = math.log((1.0 - p1) / (1.0 - p0))
        if result == "W":
            self.wins += 1
            self.log_lr += log_w
        elif result == "L":
            self.losses += 1
            self.log_lr += log_l
        elif result == "D":
            self.draws += 1
            self.log_lr += 0.5 * (log_w + log_l)
        else:
            raise ValueError(f"unknown result {result!r}; expected 'W', 'L', or 'D'")

    def summary(self) -> str:
        return (
            f"SPRT({self.n_games} games: {self.wins}W/{self.losses}L/{self.draws}D, "
            f"LLR={self.log_lr:+.3f}, bounds=[{self.lower_bound:+.3f}, {self.upper_bound:+.3f}], "
            f"decision={self.decision.value})"
        )
