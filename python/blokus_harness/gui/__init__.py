"""Browser GUI for the Blokus Duo engine.

Milestone G1 (after Phase 2): human-vs-engine play in the browser. The
engine is whichever player factory is wired in — currently `GreedyPlayer`,
to be swapped for the real search engine when Phase 3 lands.
"""

from blokus_harness.gui.server import app, GameSession, get_session

__all__ = ["app", "GameSession", "get_session"]
