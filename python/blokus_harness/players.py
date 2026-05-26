"""Players for the Blokus Duo engine.

Each player exposes `select_move(board, moves) -> Move`, where `moves` is the
non-empty list returned by `board.legal_moves()`. The harness only calls into
the player when there is at least one legal move; passes are handled at the
harness level.
"""

from __future__ import annotations

import random
from typing import Protocol

import blokus


class Player(Protocol):
    name: str

    def select_move(self, board, moves):  # pragma: no cover - interface only
        ...


class RandomPlayer:
    """Uniform-random over the legal moves."""

    name = "random"

    def __init__(self, rng: random.Random | None = None):
        self.rng = rng if rng is not None else random.Random()

    def select_move(self, board, moves):
        return self.rng.choice(moves)


class GreedyPlayer:
    """One-ply greedy: maximize piece size (= squares placed); break ties
    uniformly at random."""

    name = "greedy"

    def __init__(self, rng: random.Random | None = None):
        self.rng = rng if rng is not None else random.Random()

    def select_move(self, board, moves):
        max_size = max(m.size for m in moves)
        best = [m for m in moves if m.size == max_size]
        return self.rng.choice(best)


class EnginePlayer:
    """Search engine wrapped as a Player.

    Each call to `select_move` runs iterative-deepening alpha-beta with the
    given time budget and depth cap. The last `SearchResult` is exposed via
    `.last_result` so the GUI can render eval/depth/nodes/time.

    `weights` is an optional dict overriding the eval weights. Keys accepted:
    `placed_squares`, `corner_count`, `territory`, `piece_liability`. When
    `weights` is None and `placeholder` is False, the Phase 7 tuned champion
    weights are applied (see `tuning.CURRENT_CHAMPION_WEIGHTS`). Pass
    `placeholder=True` to use the Phase 3 baseline eval (squares-diff only)
    for A/B comparisons.
    """

    name = "engine"

    # Tuned champion (Phase 7 v20260525_222926). Beats Phase 4 hand-set
    # weights 66W/12L/0D = 84.6% over 78 games (SPRT-accepted, LLR +2.98).
    CHAMPION_WEIGHTS: dict = {
        "placed_squares": 100,
        "corner_count": 80,
        "territory": -40,
        "piece_liability": -10,
    }

    def __init__(
        self,
        *,
        time_budget_ms: int = 200,
        max_depth: int = 16,
        tt_size_log2: int = 18,
        weights: dict | None = None,
        placeholder: bool = False,
        random_opening_plies: int = 0,
        rng: random.Random | None = None,
        endgame_threshold: int | None = None,
    ):
        self.time_budget_ms = time_budget_ms
        self.max_depth = max_depth
        self.engine = blokus.SearchEngine(tt_size_log2)
        self.random_opening_plies = int(random_opening_plies)
        self.rng = rng if rng is not None else random.Random()
        if placeholder:
            self.engine.use_placeholder_eval()
        else:
            # When weights aren't explicit, install the tuned champion so the
            # GUI and harness use the strongest known engine by default.
            effective_weights = weights if weights is not None else self.CHAMPION_WEIGHTS
            current = self.engine.weights()
            keys = ("placed_squares", "corner_count", "territory", "piece_liability")
            merged = {k: int(effective_weights.get(k, current[i])) for i, k in enumerate(keys)}
            self.engine.set_weights(**merged)
        if endgame_threshold is not None:
            self.engine.set_endgame_threshold(int(endgame_threshold))
        self.last_result = None

    def select_move(self, board, moves):
        # Optional randomized opening to defeat alpha-beta's determinism and
        # actually sample many positions in tournament comparisons.
        if board.ply < self.random_opening_plies:
            mv = self.rng.choice(moves)
            self.last_result = None
            return mv
        result = self.engine.search(
            board,
            time_budget_ms=self.time_budget_ms,
            max_depth=self.max_depth,
        )
        self.last_result = result
        mv = result.best_move
        # Robustness: if search couldn't return a move (shouldn't happen since
        # `moves` is non-empty), fall back to the first legal move.
        return mv if mv is not None else moves[0]
