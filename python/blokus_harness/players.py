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


class CenterPressurePlayer:
    """Greedy with central tiebreak — among same-size moves, prefer those whose
    centroid is closest to the board center (6.5, 6.5).

    Approximates a competent human who fights for the middle, which is the
    style that walled off the engine in the user's game. Used in diagnostics
    and in the future tuning pool so candidates can't win by huddling."""

    name = "center-pressure"

    def __init__(self, rng: random.Random | None = None):
        self.rng = rng if rng is not None else random.Random()

    def select_move(self, board, moves):
        max_size = max(m.size for m in moves)
        biggest = [m for m in moves if m.size == max_size]
        if len(biggest) == 1:
            return biggest[0]

        def dist_to_center_sq(m):
            cells = m.cells()
            cy = sum(c[0] for c in cells) / len(cells)
            cx = sum(c[1] for c in cells) / len(cells)
            return (cy - 6.5) ** 2 + (cx - 6.5) ** 2

        biggest.sort(key=dist_to_center_sq)
        # Among the moves tied for the most central centroid, random tiebreak.
        best_dist = dist_to_center_sq(biggest[0])
        tied = [m for m in biggest if dist_to_center_sq(m) == best_dist]
        return self.rng.choice(tied)


class BlockerPlayer:
    """Greedy WITH adversarial intent: for each candidate move, applies it,
    counts the opponent's available corners after the move, and picks the move
    that minimizes that count. Ties broken by maximizing own piece size, then
    by centrality.

    Approximates a stronger human strategy — actively wall the engine off
    rather than just claiming central cells. Used in diagnostics so we have
    a reproducible opponent that actually punishes positional weakness."""

    name = "blocker"

    def __init__(self, rng: random.Random | None = None):
        self.rng = rng if rng is not None else random.Random()

    @staticmethod
    def _opp_corner_count(board) -> int:
        """Count the opponent's corners (diag-of-own minus ortho-of-own minus
        occupied). board.side_to_move now refers to *whichever* side moves
        next — we count for that side, which is the opponent of whoever
        just played."""
        opp = board.side_to_move
        own_opp = [tuple(c) for c in board.cells_of(opp)]
        if not own_opp:
            return 1  # first-move special case: corners = {start_cell}
        own_self = [tuple(c) for c in board.cells_of(1 - opp)]
        occupied = set(own_opp) | set(own_self)
        diag, ortho = set(), set()
        for r, c in own_opp:
            for dr, dc in ((-1, -1), (-1, 1), (1, -1), (1, 1)):
                nr, nc = r + dr, c + dc
                if 0 <= nr < 14 and 0 <= nc < 14:
                    diag.add((nr, nc))
            for dr, dc in ((-1, 0), (1, 0), (0, -1), (0, 1)):
                nr, nc = r + dr, c + dc
                if 0 <= nr < 14 and 0 <= nc < 14:
                    ortho.add((nr, nc))
        return len(diag - ortho - occupied)

    def select_move(self, board, moves):
        center = (6.5, 6.5)
        best_score = None
        best_moves: list = []
        for m in moves:
            board.make_move(m)
            opp_corners = self._opp_corner_count(board)
            board.unmake_move()
            # Score: lower opp_corners is better. Tiebreak: bigger piece, then central.
            cells = m.cells()
            cy = sum(c[0] for c in cells) / len(cells)
            cx = sum(c[1] for c in cells) / len(cells)
            dist_sq = (cy - center[0]) ** 2 + (cx - center[1]) ** 2
            score = (opp_corners, -int(m.size), dist_sq)
            if best_score is None or score < best_score:
                best_score = score
                best_moves = [m]
            elif score == best_score:
                best_moves.append(m)
        return self.rng.choice(best_moves)


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

    # Post-Phase-7 weights after the territory feature was redesigned
    # (2026-05-27) from a halo measure to a contested-reach measure. The
    # Phase 7 SPRT-tuned `territory = -40` was bound to the OLD halo formula
    # and is invalid under the new feature — it would reward giving the
    # opponent contested cells. Starting fresh: +60 territory, comparable
    # to corner_count's 80. Will re-tune against a varied opponent pool.
    # The original Phase 7 champion JSON is preserved at
    # champions/v20260525_222926.json for history; it's no longer the
    # in-use baseline.
    CHAMPION_WEIGHTS: dict = {
        "placed_squares": 100,
        "corner_count": 80,
        "territory": 60,
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
