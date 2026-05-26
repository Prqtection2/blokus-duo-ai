"""Phase 7 tuning entry point.

Usage:
    python python/run_tune.py [--time-budget-ms 50] [--max-games 400] [--seed 42]

Runs coordinate descent starting from the Phase 4 hand-set weights, saves the
resulting champion to `champions/v<timestamp>.json`, and prints a gauntlet
match summary against Phase 4 defaults.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import sys
from pathlib import Path

PROJECT_PYTHON_ROOT = Path(__file__).resolve().parent
if str(PROJECT_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_PYTHON_ROOT))

from tuning import (
    DEFAULT_WEIGHTS,
    Champion,
    MatchConfig,
    OptimizerConfig,
    coordinate_descent,
    run_sprt_match,
    save_champion,
)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--time-budget-ms", type=int, default=50)
    ap.add_argument("--max-games", type=int, default=400,
                    help="Max games per SPRT inside coordinate descent")
    ap.add_argument("--gauntlet-games", type=int, default=600,
                    help="Max games for the final gauntlet match")
    ap.add_argument("--seed", type=int, default=0xBA0BA0)
    ap.add_argument("--initial-step", type=int, default=20)
    ap.add_argument("--min-step", type=int, default=5)
    ap.add_argument("--workers", type=int, default=None)
    ap.add_argument("--output-dir", type=Path,
                    default=PROJECT_PYTHON_ROOT.parent / "champions")
    args = ap.parse_args()

    match_cfg = MatchConfig(
        time_budget_ms=args.time_budget_ms,
        max_games=args.max_games,
        n_workers=args.workers,
        seed_base=args.seed,
    )
    opt_cfg = OptimizerConfig(
        initial_step=args.initial_step,
        min_step=args.min_step,
        match=match_cfg,
        on_event=print,
    )

    print(f"Starting weights: {DEFAULT_WEIGHTS}")
    result = coordinate_descent(DEFAULT_WEIGHTS, opt_cfg)
    print()
    print(f"Final weights:   {result.final_weights}")
    print(f"Accepted steps:  {len(result.accepted_steps)}")
    print(f"Rejected steps:  {len(result.rejected_steps)}")
    print(f"Tuning wall:     {result.wall_seconds:.1f}s")

    # Gauntlet: tuned vs Phase 4 defaults, larger sample.
    print()
    print("=== Gauntlet: tuned vs Phase 4 defaults ===")
    gauntlet_cfg = MatchConfig(
        time_budget_ms=args.time_budget_ms,
        max_games=args.gauntlet_games,
        n_workers=args.workers,
        seed_base=args.seed ^ 0xDEAD_BEEF,
    )
    gauntlet = run_sprt_match(
        candidate_weights=result.final_weights,
        champion_weights=DEFAULT_WEIGHTS,
        cfg=gauntlet_cfg,
    )
    print(gauntlet.summary())

    version = _dt.datetime.now().strftime("v%Y%m%d_%H%M%S")
    champ = Champion(
        version=version,
        weights=result.final_weights,
        parent="phase4_defaults",
        promotion_notes=(
            f"Coordinate descent over {len(result.accepted_steps)} promoted steps "
            f"({len(result.rejected_steps)} rejected). Gauntlet: {gauntlet.summary()}"
        ),
        sprt_log_lr=gauntlet.sprt.log_lr,
        sprt_games={
            "wins": gauntlet.sprt.wins,
            "losses": gauntlet.sprt.losses,
            "draws": gauntlet.sprt.draws,
        },
    )
    out_path = args.output_dir / f"{version}.json"
    save_champion(champ, out_path)
    print(f"\nChampion saved -> {out_path}")


if __name__ == "__main__":
    main()
