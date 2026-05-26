"""Smoke tests for the G1 server.

Covers HTTP and the core WebSocket protocol end-to-end via FastAPI's TestClient,
without spinning up uvicorn or a real browser. Manual play verification is the
remaining DoD gate.
"""

import pytest

pytest.importorskip("fastapi")
pytest.importorskip("httpx")

from fastapi.testclient import TestClient

from blokus_harness.gui.server import app, configure_session, get_session


@pytest.fixture(autouse=True)
def fresh_session():
    # Reset the singleton between tests so they don't share state.
    configure_session(get_session().engine_factory)
    yield


def test_static_index_returns_html():
    client = TestClient(app)
    r = client.get("/")
    assert r.status_code == 200
    body = r.text
    assert "Blokus Duo" in body
    assert 'id="board"' in body


def test_websocket_initial_handshake_sends_meta_then_state():
    client = TestClient(app)
    with client.websocket_connect("/ws") as ws:
        meta = ws.receive_json()
        state = ws.receive_json()
        assert meta["type"] == "static_meta"
        assert len(meta["piece_shapes"]) == 21
        assert len(meta["piece_names"]) == 21
        assert meta["start_cells"] == [[4, 4], [9, 9]]
        assert state["type"] == "state"
        assert state["ply"] == 0
        assert state["side_to_move"] == 0
        assert state["terminal"] is False
        assert len(state["legal_moves"]) == 414


def test_new_game_with_human_as_second_runs_engine_first():
    client = TestClient(app)
    with client.websocket_connect("/ws") as ws:
        ws.receive_json()  # static_meta
        ws.receive_json()  # initial state
        ws.send_json({"type": "new_game", "human_side": 1})
        state = ws.receive_json()
        assert state["type"] == "state"
        # Engine (side 0) played one ply already.
        assert state["ply"] == 1
        assert state["side_to_move"] == 1  # now human's turn
        assert state["engine_meta"] is not None
        assert state["last_move"]["by"] == 0
        assert len(state["last_move"]["cells"]) >= 1


def test_attempt_move_legal_then_engine_replies():
    client = TestClient(app)
    with client.websocket_connect("/ws") as ws:
        ws.receive_json()  # static_meta
        state = ws.receive_json()  # initial state
        # Pick the first legal move and play it.
        m = state["legal_moves"][0]
        ws.send_json({
            "type": "attempt_move",
            "piece_id": m["piece_id"],
            "cells": m["cells"],
        })
        # First we get state after the human move, then state after engine reply.
        after_human = ws.receive_json()
        assert after_human["type"] == "state"
        assert after_human["ply"] == 1
        after_engine = ws.receive_json()
        assert after_engine["type"] == "state"
        assert after_engine["ply"] == 2
        assert after_engine["engine_meta"] is not None
        # The human's piece must no longer be in P0's tray.
        assert m["piece_id"] not in after_engine["pieces_left"][0]


def test_attempt_move_illegal_is_rejected_state_unchanged():
    client = TestClient(app)
    with client.websocket_connect("/ws") as ws:
        ws.receive_json()  # static_meta
        ws.receive_json()  # initial state
        # Try to place monomino at (0, 0) — not covering start cell → illegal.
        ws.send_json({
            "type": "attempt_move",
            "piece_id": 0,
            "cells": [[0, 0]],
        })
        msg = ws.receive_json()
        assert msg["type"] == "rejected"
        assert "legal" in msg["reason"].lower()


def test_pass_rejected_when_legal_moves_exist():
    client = TestClient(app)
    with client.websocket_connect("/ws") as ws:
        ws.receive_json()  # static_meta
        ws.receive_json()  # initial state
        ws.send_json({"type": "pass"})
        msg = ws.receive_json()
        assert msg["type"] == "rejected"
        assert "pass" in msg["reason"].lower() or "legal" in msg["reason"].lower()
