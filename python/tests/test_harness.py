"""Game-harness correctness gates for Phase 2.

* 1000 random-vs-random games complete with no errors and sane scores.
* GreedyPlayer beats RandomPlayer at >= 80% over 200 games.
* ASCII rendering shows the start cells correctly on an empty board.
"""

import random

import blokus
import pytest

from blokus_harness import GreedyPlayer, RandomPlayer, play_game, run_tournament


def test_ascii_empty_board_marks_start_cells():
    b = blokus.Board()
    grid = b.ascii()
    # The header row + 14 board rows. Each cell uses 2 chars (glyph + space).
    rows = grid.splitlines()
    assert len(rows) == 15, f"unexpected ascii line count: {len(rows)}"
    # Header has 14 column digits (mod 10).
    header = rows[0].split()
    assert header == [str(c % 10) for c in range(14)]
    # The board rows start with the row index in cols 0-2 then 14 cells.
    def cell_at(r: int, c: int) -> str:
        # row format: "{rr:>2}  X X X X ..." → 4-char prefix, then 2 chars per cell.
        line = rows[1 + r]
        return line[4 + 2 * c]

    # Start cells should be '+' on an empty board.
    assert cell_at(4, 4) == "+"
    assert cell_at(9, 9) == "+"
    # Other cells should be '.'.
    for r in (0, 7, 13):
        for c in (0, 7, 13):
            if (r, c) in {(4, 4), (9, 9)}:
                continue
            assert cell_at(r, c) == ".", f"cell ({r},{c}) = {cell_at(r,c)!r}"


def test_single_random_vs_random_game_finishes_with_sane_scores():
    rng = random.Random(42)
    result = play_game(RandomPlayer(rng), RandomPlayer(rng))
    # Bounds: each player's score is in [-89, +20].
    # -89 = sum of all 21 piece sizes; +20 = 15 (all placed) + 5 (mono last).
    assert -89 <= result.score0 <= 20, f"score0 out of range: {result.score0}"
    assert -89 <= result.score1 <= 20, f"score1 out of range: {result.score1}"
    assert result.plies > 0


def test_thousand_random_vs_random_games_no_errors():
    rng = random.Random(20260525)
    for game_i in range(1000):
        seed_a = rng.randrange(2**32)
        seed_b = rng.randrange(2**32)
        a = RandomPlayer(random.Random(seed_a))
        b = RandomPlayer(random.Random(seed_b))
        result = play_game(a, b)
        assert -89 <= result.score0 <= 20, (
            f"game {game_i}: score0 {result.score0} out of bounds"
        )
        assert -89 <= result.score1 <= 20, (
            f"game {game_i}: score1 {result.score1} out of bounds"
        )
        assert result.plies <= 100, f"game {game_i}: hit max_plies"


def test_greedy_beats_random_at_least_80_percent_over_200_games():
    seed_master = random.Random(0xC0FFEE)

    def greedy_factory():
        return GreedyPlayer(random.Random(seed_master.randrange(2**32)))

    def random_factory():
        return RandomPlayer(random.Random(seed_master.randrange(2**32)))

    result = run_tournament(greedy_factory, random_factory, n_games=200)
    print()
    print(result.summary("greedy", "random"))
    assert result.a_win_rate >= 0.80, (
        f"greedy win rate = {result.a_win_rate * 100:.1f}%, expected >= 80%"
    )
