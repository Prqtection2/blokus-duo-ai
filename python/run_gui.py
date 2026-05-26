"""Entry point for the Blokus Duo browser GUI (Phase 2.5 G1).

Run from the repo root:
    python python/run_gui.py
then open http://127.0.0.1:8765/ in your browser.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Make `blokus_harness` importable when launching by file path.
PROJECT_PYTHON_ROOT = Path(__file__).resolve().parent
if str(PROJECT_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_PYTHON_ROOT))

import uvicorn

from blokus_harness.gui.server import app


def main() -> None:
    uvicorn.run(app, host="127.0.0.1", port=8765, log_level="info")


if __name__ == "__main__":
    main()
