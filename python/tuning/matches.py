"""Parallel match runner with SPRT-driven early stopping.

A worker process plays one game between (candidate, champion) with sides
alternating per game_id. Results are streamed back to the main process which
updates an SprtState and stops submitting new games once the test decides.
"""

from __future__ import annotations

import os
import random
import sys
import time
from concurrent.futures import FIRST_COMPLETED, Future, ProcessPoolExecutor, wait
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

# Make sibling packages importable inside spawned worker processes.
_PYTHON_ROOT = Path(__file__).resolve().parent.parent
if str(_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(_PYTHON_ROOT))

from blokus_harness import EnginePlayer, play_game

from tuning.sprt import SprtDecision, SprtState


# ──────────────────────── Worker-side: single game ────────────────────────


@dataclass(frozen=True)
class GameTask:
    """Configuration to play one game in a worker process. Must be picklable."""

    game_id: int
    seed_candidate: int
    seed_champion: int
    candidate_weights: dict
    champion_weights: dict
    time_budget_ms: int
    candidate_side: int  # 0 if candidate plays as P0, else 1
    random_opening_plies: int = 2


@dataclass(frozen=True)
class GameOutcome:
    """Worker → main result. Scores are from the absolute player's POV."""

    game_id: int
    candidate_score: int
    champion_score: int
    candidate_side: int

    @property
    def result_for_candidate(self) -> str:
        if self.candidate_score > self.champion_score:
            return "W"
        if self.candidate_score < self.champion_score:
            return "L"
        return "D"


def play_match_game(task: GameTask) -> GameOutcome:
    """Worker entry point — must be a top-level module function for pickling."""
    candidate = EnginePlayer(
        time_budget_ms=task.time_budget_ms,
        weights=task.candidate_weights,
        random_opening_plies=task.random_opening_plies,
        rng=random.Random(task.seed_candidate),
    )
    champion = EnginePlayer(
        time_budget_ms=task.time_budget_ms,
        weights=task.champion_weights,
        random_opening_plies=task.random_opening_plies,
        rng=random.Random(task.seed_champion),
    )
    if task.candidate_side == 0:
        game = play_game(candidate, champion)
        cand_s, champ_s = game.score0, game.score1
    else:
        game = play_game(champion, candidate)
        cand_s, champ_s = game.score1, game.score0
    return GameOutcome(
        game_id=task.game_id,
        candidate_score=cand_s,
        champion_score=champ_s,
        candidate_side=task.candidate_side,
    )


# ──────────────────────── Main-side: SPRT match driver ────────────────────────


@dataclass
class MatchConfig:
    time_budget_ms: int = 50
    max_games: int = 400
    elo0: float = 0.0
    elo1: float = 20.0
    alpha: float = 0.05
    beta: float = 0.05
    n_workers: Optional[int] = None
    seed_base: int = 0xBA0BA0
    random_opening_plies: int = 2
    verbose: bool = False


@dataclass
class MatchResult:
    sprt: SprtState
    margins: list[int] = field(default_factory=list)
    wall_seconds: float = 0.0

    @property
    def decision(self) -> SprtDecision:
        return self.sprt.decision

    def summary(self) -> str:
        avg = sum(self.margins) / max(1, len(self.margins))
        return (
            f"{self.sprt.summary()}  avg margin {avg:+.2f}  "
            f"wall {self.wall_seconds:.1f}s"
        )


def _build_task(
    game_id: int, candidate_weights: dict, champion_weights: dict, cfg: MatchConfig
) -> GameTask:
    return GameTask(
        game_id=game_id,
        seed_candidate=cfg.seed_base + 2 * game_id,
        seed_champion=cfg.seed_base + 2 * game_id + 1,
        candidate_weights=dict(candidate_weights),
        champion_weights=dict(champion_weights),
        time_budget_ms=cfg.time_budget_ms,
        candidate_side=game_id % 2,
        random_opening_plies=cfg.random_opening_plies,
    )


def run_sprt_match(
    candidate_weights: dict,
    champion_weights: dict,
    cfg: MatchConfig | None = None,
) -> MatchResult:
    """Play a SPRT match between candidate and champion. Returns once SPRT
    decides or max_games is reached. Cancels pending tasks when the decision
    is made; in-flight tasks finish but their results are still folded in."""

    if cfg is None:
        cfg = MatchConfig()
    state = SprtState(
        elo0=cfg.elo0, elo1=cfg.elo1, alpha=cfg.alpha, beta=cfg.beta
    )
    result = MatchResult(sprt=state)
    n_workers = cfg.n_workers or max(1, (os.cpu_count() or 2) - 1)

    t0 = time.perf_counter()

    with ProcessPoolExecutor(max_workers=n_workers) as executor:
        # Submit games in a small over-subscription pool: worker count × 2 in
        # flight at any time. This keeps cancellation cheap once SPRT decides.
        in_flight: set[Future] = set()
        next_id = 0
        target_in_flight = n_workers * 2

        def submit_next() -> None:
            nonlocal next_id
            if next_id >= cfg.max_games:
                return
            task = _build_task(next_id, candidate_weights, champion_weights, cfg)
            in_flight.add(executor.submit(play_match_game, task))
            next_id += 1

        for _ in range(target_in_flight):
            submit_next()

        while in_flight and state.decision == SprtDecision.CONTINUE:
            done, _pending = wait(in_flight, return_when=FIRST_COMPLETED)
            for fut in done:
                in_flight.discard(fut)
                outcome: GameOutcome = fut.result()
                state.update(outcome.result_for_candidate)
                result.margins.append(outcome.candidate_score - outcome.champion_score)
                if cfg.verbose:
                    print(
                        f"  game {outcome.game_id:>4}: "
                        f"cand={outcome.candidate_score:>4} champ={outcome.champion_score:>4} "
                        f"({outcome.result_for_candidate})  LLR={state.log_lr:+.3f}"
                    )
                if state.decision != SprtDecision.CONTINUE:
                    break
                submit_next()
            if state.decision != SprtDecision.CONTINUE:
                # Stop submitting new games. Cancel anything still queued
                # (running tasks will keep going to completion, which is fine —
                # they don't block the decision).
                for fut in list(in_flight):
                    fut.cancel()
                break

    result.wall_seconds = time.perf_counter() - t0
    return result
