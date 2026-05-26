"""Make `blokus_harness` (sibling to the tests/ directory) importable."""

import sys
from pathlib import Path

PROJECT_PYTHON_ROOT = Path(__file__).resolve().parent.parent
if str(PROJECT_PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_PYTHON_ROOT))
