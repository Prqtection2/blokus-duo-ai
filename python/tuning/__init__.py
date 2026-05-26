"""Phase 7 tuning pipeline: SPRT match runner, coordinate-descent optimizer,
champion versioning. The optimizer never promotes a candidate that hasn't
beaten the current champion at SPRT significance — no point-estimate winners
from noisy sweeps."""

# Ensure `blokus_harness` and `tuning` are importable inside multiprocessing
# workers, which on Windows spawn fresh interpreters and don't run conftest.py.
import sys as _sys
from pathlib import Path as _Path

_PYTHON_ROOT = _Path(__file__).resolve().parent.parent
if str(_PYTHON_ROOT) not in _sys.path:
    _sys.path.insert(0, str(_PYTHON_ROOT))

from tuning.sprt import SprtDecision, SprtState
from tuning.matches import MatchConfig, MatchResult, run_sprt_match
from tuning.optimizer import (
    DEFAULT_WEIGHTS,
    OptimizerConfig,
    coordinate_descent,
)
from tuning.champion import (
    CURRENT_CHAMPION_VERSION,
    CURRENT_CHAMPION_WEIGHTS,
    Champion,
    load_champion,
    save_champion,
)

__all__ = [
    "CURRENT_CHAMPION_VERSION",
    "CURRENT_CHAMPION_WEIGHTS",
    "Champion",
    "DEFAULT_WEIGHTS",
    "MatchConfig",
    "MatchResult",
    "OptimizerConfig",
    "SprtDecision",
    "SprtState",
    "coordinate_descent",
    "load_champion",
    "run_sprt_match",
    "save_champion",
]
