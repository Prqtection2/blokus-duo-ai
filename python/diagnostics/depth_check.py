"""Quick: is the engine actually reaching the depth it should at mid-game?

The GUI just reported depth=3, nodes=6557 at ply 8 with a ~1.5s budget.
That's ~4k nps; pre-regression we were seeing 100k+ nps and depth 8-11
in this range. This script reproduces the conditions without the GUI.
"""

import random
import time

import blokus


def main() -> None:
    seed = 0
    rng = random.Random(seed)
    b = blokus.Board()
    for ply_target in [4, 8, 12, 16, 20, 24, 28]:
        while b.ply < ply_target:
            legal = b.legal_moves()
            if legal:
                b.make_move(rng.choice(legal))
            else:
                b.make_pass()

        eng = blokus.SearchEngine()
        eng.set_weights(100, 80, 60, -10)
        n_legal = len(b.legal_moves())
        t0 = time.perf_counter()
        r = eng.search(b, time_budget_ms=1000, max_depth=16)
        wall_ms = round((time.perf_counter() - t0) * 1000.0, 1)
        nps = r.nodes_per_second
        print(
            f"ply={b.ply:>2} legal_root={n_legal:>4} | "
            f"depth={r.depth} nodes={r.nodes:>9} nps={nps:>10.0f} "
            f"time_ms={r.time_ms} wall_ms={wall_ms}"
        )


if __name__ == "__main__":
    main()
