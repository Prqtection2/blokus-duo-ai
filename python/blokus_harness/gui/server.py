"""FastAPI + WebSocket server backing the G1 GUI.

The browser does no rules — every attempted move is validated against
``board.legal_moves()`` here. The server keeps a single ``GameSession`` and
broadcasts state changes to all connected clients.
"""

from __future__ import annotations

import asyncio
import random
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

import blokus
from blokus_harness import EnginePlayer, GreedyPlayer

STATIC_DIR = Path(__file__).parent / "static"
HUMAN_DEFAULT_SIDE = 0


@dataclass
class EngineMeta:
    eval: float = 0.0
    depth: int = 0
    nodes: int = 0
    time_ms: float = 0.0
    move_repr: str = ""
    passed: bool = False


@dataclass
class LastMove:
    by: int
    piece_id: int | None = None
    cells: list[list[int]] = field(default_factory=list)
    passed: bool = False


def _default_engine_factory() -> object:
    # 1000ms per move: budget sweep showed 200ms -> 1000ms is worth ~250 elo,
    # while 1000ms -> 5000ms gains nothing measurable. Override via
    # configure_session if you want a baseline for comparison.
    #
    # We override the Phase-7-tuned champion's `territory = -40` to 0 here.
    # The -40 weight was a real SPRT-validated improvement in self-play, but
    # it overfit to the Phase 4 hand-set baseline: it codes for "play
    # compactly, don't expand your halo," which against humans translates to
    # passive corner-hugging that gets choked off easily (the diagnostic
    # confirmed this — see feedback_phase7_territory_finding). For human
    # play, neutralizing the feature gives more engaging, forward play.
    # A real fix (a proper forward-mobility feature + gauntlet pool that
    # includes an aggressive blocker) is the next eval-iteration task.
    human_play_weights = {
        "placed_squares": 100,
        "corner_count": 80,
        "territory": 0,
        "piece_liability": -10,
    }
    return EnginePlayer(
        time_budget_ms=1000,
        weights=human_play_weights,
        max_depth=16,
    )


class GameSession:
    """Holds the active game and exposes the operations the WS handler needs."""

    def __init__(
        self,
        engine_factory: Callable[[], object] = _default_engine_factory,
    ):
        self.engine_factory = engine_factory
        self.lock = asyncio.Lock()
        self.board = blokus.Board()
        self.human_side = HUMAN_DEFAULT_SIDE
        self.engine = engine_factory()
        self.last_move: LastMove | None = None
        self.last_engine_meta: EngineMeta | None = None

    def new_game(self, *, human_side: int) -> None:
        self.board = blokus.Board()
        self.human_side = int(human_side) & 1
        self.engine = self.engine_factory()
        self.last_move = None
        self.last_engine_meta = None

    def _find_legal_match(self, piece_id: int, cells: list[list[int]]):
        target = frozenset((int(r), int(c)) for r, c in cells)
        for mv in self.board.legal_moves():
            if mv.piece_id == piece_id and frozenset(mv.cells()) == target:
                return mv
        return None

    def attempt_human_move(
        self, piece_id: int, cells: list[list[int]]
    ) -> tuple[bool, str | None]:
        if self.board.is_terminal():
            return False, "game is over"
        if self.board.side_to_move != self.human_side:
            return False, "not your turn"
        mv = self._find_legal_match(piece_id, cells)
        if mv is None:
            return False, "not a legal placement"
        self.board.make_move(mv)
        self.last_move = LastMove(
            by=self.human_side,
            piece_id=piece_id,
            cells=[list(c) for c in cells],
        )
        return True, None

    def human_pass(self) -> tuple[bool, str | None]:
        if self.board.is_terminal():
            return False, "game is over"
        if self.board.side_to_move != self.human_side:
            return False, "not your turn"
        if self.board.legal_moves():
            return False, "cannot pass while legal moves are available"
        self.board.make_pass()
        self.last_move = LastMove(by=self.human_side, passed=True)
        return True, None

    def _step_engine(self) -> None:
        """Single engine ply. Updates last_move and last_engine_meta."""
        legal = self.board.legal_moves()
        t0 = time.perf_counter()
        if legal:
            mv = self.engine.select_move(self.board, legal)
            self.board.make_move(mv)
            wall_ms = round((time.perf_counter() - t0) * 1000.0, 2)
            self.last_move = LastMove(
                by=1 - self.human_side,
                piece_id=int(mv.piece_id),
                cells=[list(c) for c in mv.cells()],
            )
            # If the player exposes a richer SearchResult (EnginePlayer),
            # report its depth/nodes. Use WALL time, not sr.time_ms — the
            # latter records "elapsed at the last completed iteration" so
            # it badly under-reports when depth N+1 aborts mid-search.
            sr = getattr(self.engine, "last_result", None)
            if sr is not None:
                self.last_engine_meta = EngineMeta(
                    eval=float(sr.value),
                    depth=int(sr.depth),
                    nodes=int(sr.nodes),
                    time_ms=wall_ms,
                    move_repr=repr(mv),
                )
            else:
                self.last_engine_meta = EngineMeta(
                    eval=float(mv.size),
                    depth=0,
                    nodes=len(legal),
                    time_ms=wall_ms,
                    move_repr=repr(mv),
                )
        else:
            self.board.make_pass()
            wall_ms = round((time.perf_counter() - t0) * 1000.0, 2)
            self.last_move = LastMove(by=1 - self.human_side, passed=True)
            self.last_engine_meta = EngineMeta(
                eval=0.0,
                depth=0,
                nodes=0,
                time_ms=wall_ms,
                move_repr="pass",
                passed=True,
            )

    def play_engine_until_humans_turn(self) -> None:
        """Run engine plies until it's the human's turn or the game ends."""
        while (
            not self.board.is_terminal()
            and self.board.side_to_move != self.human_side
        ):
            self._step_engine()

    def serialize(self) -> dict[str, Any]:
        board = self.board
        terminal = bool(board.is_terminal())
        legal = [] if terminal else board.legal_moves()
        return {
            "type": "state",
            "ply": int(board.ply),
            "side_to_move": int(board.side_to_move),
            "human_side": int(self.human_side),
            "terminal": terminal,
            "consecutive_passes": int(board.consecutive_passes()),
            "scores": [int(board.score(0)), int(board.score(1))],
            "squares_left": [
                int(board.squares_left(0)),
                int(board.squares_left(1)),
            ],
            "cells": [
                [list(c) for c in board.cells_of(0)],
                [list(c) for c in board.cells_of(1)],
            ],
            "pieces_left": [
                list(board.pieces_left(0)),
                list(board.pieces_left(1)),
            ],
            "legal_moves": [
                {"piece_id": int(m.piece_id), "cells": [list(c) for c in m.cells()]}
                for m in legal
            ],
            "last_move": (
                {
                    "by": self.last_move.by,
                    "piece_id": self.last_move.piece_id,
                    "cells": self.last_move.cells,
                    "passed": self.last_move.passed,
                }
                if self.last_move
                else None
            ),
            "engine_meta": (
                {
                    "eval": self.last_engine_meta.eval,
                    "depth": self.last_engine_meta.depth,
                    "nodes": self.last_engine_meta.nodes,
                    "time_ms": self.last_engine_meta.time_ms,
                    "move_repr": self.last_engine_meta.move_repr,
                    "passed": self.last_engine_meta.passed,
                }
                if self.last_engine_meta
                else None
            ),
        }


_SESSION: GameSession | None = None


def get_session() -> GameSession:
    global _SESSION
    if _SESSION is None:
        _SESSION = GameSession()
    return _SESSION


def configure_session(engine_factory: Callable[[], object]) -> GameSession:
    """Used by tests / Phase 3 to install a different engine."""
    global _SESSION
    _SESSION = GameSession(engine_factory=engine_factory)
    return _SESSION


_CONNECTIONS: set[WebSocket] = set()


async def _broadcast(message: dict[str, Any]) -> None:
    dead = []
    for ws in list(_CONNECTIONS):
        try:
            await ws.send_json(message)
        except Exception:
            dead.append(ws)
    for ws in dead:
        _CONNECTIONS.discard(ws)


def _static_meta() -> dict[str, Any]:
    return {
        "type": "static_meta",
        "piece_names": blokus.piece_names(),
        "piece_shapes": [
            [list(c) for c in shape] for shape in blokus.piece_base_shapes()
        ],
        "start_cells": [list(c) for c in blokus.start_cells()],
        "engine_name": getattr(get_session().engine, "name", "engine"),
        "engine_version": blokus.version(),
    }


app = FastAPI(title="Blokus Duo GUI (G1)")
app.mount("/static", StaticFiles(directory=STATIC_DIR), name="static")


@app.get("/")
async def root() -> FileResponse:
    return FileResponse(STATIC_DIR / "index.html")


@app.websocket("/ws")
async def ws_endpoint(ws: WebSocket) -> None:
    await ws.accept()
    _CONNECTIONS.add(ws)
    session = get_session()
    try:
        await ws.send_json(_static_meta())
        await ws.send_json(session.serialize())
        while True:
            message = await ws.receive_json()
            await _handle_message(message)
    except WebSocketDisconnect:
        pass
    except Exception as exc:  # noqa: BLE001 - we want to log and continue
        try:
            await ws.send_json({"type": "error", "message": str(exc)})
        except Exception:
            pass
    finally:
        _CONNECTIONS.discard(ws)


async def _handle_message(message: dict[str, Any]) -> None:
    session = get_session()
    kind = message.get("type")
    async with session.lock:
        if kind == "new_game":
            session.new_game(human_side=int(message.get("human_side", 0)))
            session.play_engine_until_humans_turn()
            await _broadcast(session.serialize())
        elif kind == "attempt_move":
            piece_id = int(message["piece_id"])
            cells = message["cells"]
            ok, err = session.attempt_human_move(piece_id, cells)
            if not ok:
                await _broadcast({"type": "rejected", "reason": err})
                return
            await _broadcast(session.serialize())
            session.play_engine_until_humans_turn()
            await _broadcast(session.serialize())
        elif kind == "pass":
            ok, err = session.human_pass()
            if not ok:
                await _broadcast({"type": "rejected", "reason": err})
                return
            await _broadcast(session.serialize())
            session.play_engine_until_humans_turn()
            await _broadcast(session.serialize())
        elif kind == "request_state":
            await _broadcast(session.serialize())
        else:
            await _broadcast({"type": "rejected", "reason": f"unknown message {kind!r}"})
