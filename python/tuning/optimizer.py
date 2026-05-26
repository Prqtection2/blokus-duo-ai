"""Coordinate-descent optimizer over EvalWeights.

For each weight in turn, try `value ± step`. If SPRT accepts (candidate beats
current champion at statistical significance), promote and restart the sweep
with the new champion. When no perturbation passes a full sweep, halve the
step and try again. Stop when the step shrinks below `min_step`.

A candidate is *only* promoted on SPRT acceptance — never on a higher point
estimate in an inconclusive match, per the Phase-6 lesson learned.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Callable, Iterable, Optional

from tuning.matches import MatchConfig, MatchResult, run_sprt_match
from tuning.sprt import SprtDecision

# Phase 4 hand-set defaults; the optimizer's starting point unless overridden.
DEFAULT_WEIGHTS: dict = {
    "placed_squares": 100,
    "corner_count": 80,
    "territory": 20,
    "piece_liability": -10,
}

WEIGHT_KEYS = ("placed_squares", "corner_count", "territory", "piece_liability")


@dataclass
class OptimizerConfig:
    initial_step: int = 20
    min_step: int = 5
    max_outer_iters: int = 6
    match: MatchConfig = field(default_factory=MatchConfig)
    keys: tuple[str, ...] = WEIGHT_KEYS
    on_event: Optional[Callable[[str], None]] = None

    def emit(self, msg: str) -> None:
        if self.on_event is not None:
            self.on_event(msg)


@dataclass
class CoordinateDescentResult:
    initial_weights: dict
    final_weights: dict
    accepted_steps: list[dict] = field(default_factory=list)
    rejected_steps: list[dict] = field(default_factory=list)
    wall_seconds: float = 0.0


def coordinate_descent(
    starting_weights: dict | None = None,
    config: OptimizerConfig | None = None,
) -> CoordinateDescentResult:
    """Run coordinate descent. Returns the (possibly identical) final weights.

    Each promotion is an SPRT-accepted improvement vs the then-current champion."""

    cfg = config or OptimizerConfig()
    champion = dict(starting_weights or DEFAULT_WEIGHTS)
    out = CoordinateDescentResult(
        initial_weights=dict(champion),
        final_weights=dict(champion),
    )
    step = cfg.initial_step
    t0 = time.perf_counter()
    iter_n = 0

    cfg.emit(f"[opt] starting weights: {champion}, step={step}")

    while step >= cfg.min_step and iter_n < cfg.max_outer_iters:
        iter_n += 1
        cfg.emit(f"[opt] iter {iter_n}: step={step}, champion={champion}")
        improved_this_pass = False
        for key in cfg.keys:
            for direction in (+1, -1):
                candidate = dict(champion)
                candidate[key] = candidate[key] + direction * step
                cfg.emit(
                    f"[opt] try {key}{direction:+d}*{step} -> {candidate[key]}"
                )
                match: MatchResult = run_sprt_match(
                    candidate_weights=candidate,
                    champion_weights=champion,
                    cfg=cfg.match,
                )
                cfg.emit(f"[opt]   {match.summary()}")
                step_record = {
                    "key": key,
                    "direction": direction,
                    "step": step,
                    "candidate": dict(candidate),
                    "champion_before": dict(champion),
                    "match_summary": match.summary(),
                    "decision": match.decision.value,
                }
                if match.decision == SprtDecision.ACCEPT:
                    out.accepted_steps.append(step_record)
                    champion = candidate
                    improved_this_pass = True
                    cfg.emit(f"[opt]   PROMOTED -> {champion}")
                    break  # next outer pass with the new champion
                else:
                    out.rejected_steps.append(step_record)
            if improved_this_pass:
                break
        if not improved_this_pass:
            step //= 2
            cfg.emit(f"[opt] no improvement at this step; halving to {step}")

    out.final_weights = dict(champion)
    out.wall_seconds = time.perf_counter() - t0
    return out
