"""Single-game runner: alternate two players, handle passes, return final scores."""

from __future__ import annotations

from dataclasses import dataclass, field

import blokus


@dataclass
class GameResult:
    score0: int
    score1: int
    plies: int
    passes: int = 0
    history: list = field(default_factory=list)

    @property
    def winner(self) -> int | None:
        if self.score0 > self.score1:
            return 0
        if self.score1 > self.score0:
            return 1
        return None  # draw


def play_game(
    player0,
    player1,
    *,
    record_history: bool = False,
    render: bool = False,
    max_plies: int = 100,
) -> GameResult:
    """Play one full game between `player0` (side 0) and `player1` (side 1)."""
    board = blokus.Board()
    players = (player0, player1)
    passes = 0
    history = []

    for ply in range(max_plies):
        if board.is_terminal():
            break
        moves = board.legal_moves()
        if moves:
            mv = players[board.side_to_move].select_move(board, moves)
            board.make_move(mv)
            if record_history:
                history.append(mv)
        else:
            board.make_pass()
            passes += 1
            if record_history:
                history.append(blokus.Move.pass_move())
        if render:
            print(f"--- ply {board.ply} (stm now {board.side_to_move}) ---")
            print(board.ascii())

    return GameResult(
        score0=board.score(0),
        score1=board.score(1),
        plies=int(board.ply),
        passes=passes,
        history=history,
    )
