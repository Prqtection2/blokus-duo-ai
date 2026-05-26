"""Versioned champion weight storage. Each champion is one JSON file with
its weights, when it was created, what beat what got it here, and the SPRT
result that promoted it."""

from __future__ import annotations

import datetime as _dt
import json
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Optional


# Phase 7 tuned champion (coordinate descent, SPRT-gated promotions, gauntlet
# 66W/12L/0D vs Phase 4 defaults — LLR +2.98, avg margin +10.63 over 78 games).
# Saved as champions/v20260525_222926.json. Any future change must beat this
# champion at SPRT significance to become the new baseline.
CURRENT_CHAMPION_WEIGHTS: dict = {
    "placed_squares": 100,
    "corner_count": 80,
    "territory": -40,  # tuned from +20; territory was a spurious-signal feature
    "piece_liability": -10,
}
CURRENT_CHAMPION_VERSION = "v20260525_222926"


@dataclass
class Champion:
    version: str
    weights: dict
    parent: Optional[str] = None
    created_at: str = field(
        default_factory=lambda: _dt.datetime.now(_dt.timezone.utc).isoformat()
    )
    promotion_notes: str = ""
    sprt_log_lr: Optional[float] = None
    sprt_games: Optional[dict] = None  # {"wins": W, "losses": L, "draws": D}


def save_champion(champ: Champion, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = asdict(champ)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True))


def load_champion(path: Path) -> Champion:
    payload = json.loads(path.read_text())
    return Champion(**payload)
