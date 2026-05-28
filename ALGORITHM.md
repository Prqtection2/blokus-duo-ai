# How the Engine Picks a Move

This document explains, in detail but from the ground up, how the Blokus Duo
engine decides what to play. It assumes no prior knowledge of game AI.

If you only read one paragraph: the engine imagines playing a move, then imagines
every reply the opponent could make, then every reply to that, and so on as deep
as time allows. At the bottom of this imagined tree it scores each position with a
formula. Assuming both players always pick their best option, it works backward to
find the move that leads to the best reachable score. Everything else in this
document is about doing that fast enough to be useful.

---

## 1. The Game Tree

From any position, you have a set of legal moves. Each move leads to a new
position, from which the opponent has their own legal moves, and so on. Drawn out,
this forms a tree:

```
                 current position (my turn)
        ┌──────────────┼──────────────┐
     my move A      my move B      my move C
        │              │              │
   opp reply…      opp reply…     opp reply…
     ┌──┴──┐        ┌──┴──┐
   ...     ...    ...     ...
```

- A **node** is a position.
- An **edge** is a move.
- A **leaf** is where we stop looking (either the game ended, or we hit our depth
  limit).
- **Depth** is how many moves deep we look. Depth 3 means "my move, opponent's
  reply, my reply."
- **Branching factor** is how many legal moves exist at a node. In Blokus this is
  large early (300-600 in the opening) and small late (single digits).

The tree is far too big to explore fully. A full-width search of depth `d` with
branching `b` visits about `b^d` nodes. At `b = 380`, depth 4 is ~20 billion
nodes. The whole game of all the techniques below is to get a good answer without
visiting all of them.

---

## 2. Scoring a Leaf: the Evaluation Function

When we stop at a leaf, we need a number saying how good that position is. That's
the **evaluation function** ([eval.rs](crates/blokus-core/src/eval.rs)). It's a
weighted sum of four features, each computed as **(mine − the opponent's)** from
the perspective of whoever is to move:

| Feature | What it measures | Weight |
|---|---|---|
| `placed_squares` | total board cells I've covered with pieces | +100 |
| `corner_count` | my number of **live corners** (corners that can still grow a piece) | +80 |
| `territory` | cells **only I** can legally cover next, minus cells only the opponent can | +60 |
| `piece_liability` | sum of (piece size)² for pieces still in my hand (a debt — big unplayed pieces are bad) | −10 |

So a position's score is:

```
score = 100·(my_squares − opp_squares)
      +  80·(my_live_corners − opp_live_corners)
      +  60·(cells_only_I_cover − cells_only_opp_covers)
      −  10·(my_piece_debt − opp_piece_debt)
```

A positive score means the side to move is doing well.

### Why these features

- **placed_squares** — Blokus scoring is about how many squares you place, so this
  is the most direct proxy for winning.
- **corner_count (live corners)** — corners are where future pieces attach. More
  live corners = more options = more flexibility. A corner walled in by your own
  stones doesn't count; only corners with room to grow.
- **territory (piece-aware)** — measures board control. A cell counts as "mine"
  only if I have an actual piece + orientation + position that can legally land on
  it, and the opponent does not. Cells both of us could reach are a toss-up and
  count for nobody. (An earlier version of this feature used walking distance from
  corners and over-claimed far-away cells the engine had no real plan to reach;
  the piece-aware version fixed that.)
- **piece_liability** — squaring the size makes the unplayed 5-cell pieces hurt
  much more than the 1-cell piece, nudging the engine to off-load big pieces
  early rather than getting stuck holding them.

### Terminal positions

If the game is actually over at a node, we skip the heuristic and use the **exact**
final score difference, including Blokus's end bonuses (+15 for placing everything,
+5 if your last piece was the monomino). That's `terminal_value` in eval.rs.

The weights live in `_default_engine_factory()` in
[gui/server.py](python/blokus_harness/gui/server.py) for GUI play. They were
chosen by self-play tuning (see §10).

---

## 3. Minimax and Negamax

Both players are assumed to play optimally. On my turn I pick the move that
**maximizes** my score; on the opponent's turn they pick the move that
**minimizes** my score (= maximizes theirs). Propagating these choices up from the
leaves is **minimax**.

Because Blokus is symmetric (what's good for me is exactly as bad for you), we use
the tidier **negamax** formulation: always score from the side-to-move's
perspective, and negate when we look at the child position.

```
function negamax(position, depth):
    if game over:        return exact_score(position)
    if depth == 0:       return evaluate(position)
    best = -infinity
    for each legal move:
        make the move
        value = -negamax(position, depth - 1)   # negate: child is opponent's view
        undo the move
        best = max(best, value)
    return best
```

The actual code is `negamax(...)` in
[search.rs](crates/blokus-core/src/search.rs); the unoptimized reference version is
`plain_minimax` (used only to verify the fast one in tests).

---

## 4. Alpha-Beta Pruning

Minimax visits the whole tree. Most of it is wasted: once you know a move is worse
than something you can already guarantee, you don't need to know *how much* worse.
**Alpha-beta** tracks two bounds as it searches:

- **alpha** — the best score I've already secured somewhere. I'll never accept less.
- **beta** — the best the opponent will allow. If a line gets better than beta for
  me, the opponent had a better choice earlier and will never let me reach here.

When a move's value reaches `beta`, we stop searching the remaining moves at that
node — a **beta cutoff** — because the opponent won't permit this line anyway.

```
function negamax(position, depth, alpha, beta):
    ...
    for each legal move:
        value = -negamax(child, depth-1, -beta, -alpha)
        if value >= beta:  return beta          # prune the rest
        if value > alpha:  alpha = value         # raise our floor
    return alpha
```

Alpha-beta returns the exact same move as plain minimax, but can skip huge parts of
the tree. The catch: it only prunes well if good moves are tried first. That makes
**move ordering** (§7) critical.

---

## 5. Iterative Deepening

We don't search straight to a fixed depth. Instead we search depth 1, then depth 2,
then 3, … until the clock runs out. This sounds wasteful (re-searching shallow
trees) but it's a net win:

- We always have a complete, returnable best move from the last finished depth, so
  we can stop anytime the time budget expires.
- The shallow searches fill the transposition table and move-ordering tables, which
  make the deeper searches prune far more — so the redo cost is small.

`search_time(board, time_budget_ms, max_depth)` runs this loop. Depth 1 always
completes (no deadline check), so we never return without a move. After each depth
finishes we store that result; when time runs out mid-depth, we discard the
unfinished depth (its move ordering is biased toward whatever it looked at first)
and return the last completed one.

This is why, in the opening, you'll see the engine report `depth 3` even with a
10-second budget: the branching factor is so high that depth 4 doesn't finish in
time, so it returns the best depth-3 result.

---

## 6. The Speed Tricks

These don't change *what* answer we get; they make the search reach deeper in the
same time.

### 6.1 Aspiration windows

After depth N reports a value V, depth N+1 is usually close to V. So instead of
searching with the full window `[-∞, +∞]`, we search a narrow band `[V−200, V+200]`
(the constant `ASPIRATION_WINDOW = 200`). A narrow window prunes more. If the true
value falls outside the band (a "fail"), we widen and re-search. Most of the time it
lands inside, and we win.

### 6.2 PVS (Principal Variation Search)

At each node, the first move (our best guess) is searched with the full window. For
every other move we first do a cheap **null-window** search — a window of width 1,
`[alpha, alpha+1]` — which only answers "is this better than alpha or not?" without
computing the exact value. If the answer is "not better" (the common case), we're
done with that move cheaply. If it *is* better, we re-search it with the full window
to get the real value. Since good ordering means most moves aren't better than the
first, most moves get the cheap treatment.

### 6.3 Late-Move Reductions (LMR)

After good move ordering, moves late in the list are probably bad. So for moves at
index ≥ 3, when depth ≥ 3 (and the move isn't a pass), we search them **one ply
shallower** than normal. If a reduced search surprisingly beats alpha, we re-search
it at full depth to confirm. This skips depth on the moves least likely to matter.
Controlled by the `lmr_enabled` flag.

The combined PVS + LMR logic is the per-move block in `negamax`: move 0 gets
full-window/full-depth; moves 1+ get null-window, with index-≥3/depth-≥3 moves also
reduced a ply, and a re-search only when they improve alpha.

### 6.4 Transposition Table (TT)

Different move orders can reach the **same position**. Without memory we'd re-search
it every time. The TT is a big hash table that caches, per position:

- the value found, the depth it was searched to, and the best move;
- a **flag** saying whether that value is exact, a lower bound, or an upper bound
  (bounds happen because alpha-beta sometimes only proves "≥ X" or "≤ X").

Before searching a node we probe the TT by the position's hash. If we have an entry
searched at least as deep as we need, we either return it (exact) or tighten alpha/
beta with the bound — sometimes producing an immediate cutoff. Even when we can't
reuse the value, the stored best move is tried first, which sharpens ordering.

**Zobrist hashing** makes the position key cheap: every (cell, owner) and turn-state
detail is assigned a fixed random 64-bit number at startup; the position's hash is
the XOR of all the numbers currently "on." Making a move XORs out what changed and
XORs in what's new — a couple of operations, no rescanning the board.

The TT is cleared whenever the weights or the endgame threshold change, because old
cached values were computed under different rules.

### 6.5 Move ordering

`order_moves` sorts each node's moves by priority before searching:

1. **TT move** (the best move found here previously) — highest priority.
2. **Killer moves** — two moves per depth that caused a beta cutoff at the same
   depth elsewhere; cutoffs tend to repeat, so these are tried early.
3. **Everything else** — ranked by piece size (bigger first, since `placed_squares`
   dominates the eval), with a tiebreak by the **history heuristic**: a per-(player,
   piece) counter that accumulates `depth²` whenever that piece caused a cutoff.
4. **Pass** — always last.

Good ordering is what makes alpha-beta, PVS, and LMR pay off.

---

## 7. The Endgame Solver

Heuristics are approximations. Near the end of the game the tree is small enough to
search **exactly** to the finish. When the combined number of unplayed pieces across
both players drops to the `endgame_threshold` (default **6**), the search switches
mode: at depth 0 it does **not** stop and call the heuristic — it keeps recursing
until the game is actually over, scoring with the exact `final_score`. This yields
perfect endgame play at negligible cost (the tree is tiny by then). Set the
threshold to 0 to disable it.

---

## 8. Time Management

`search_time` takes a wall-clock budget in milliseconds. Inside `negamax`, every
16,384 nodes we check whether the deadline has passed; if so we set an `aborted`
flag and unwind immediately. The iterative-deepening loop notices the abort, drops
the unfinished iteration, and returns the best result from the last completed depth.
Depth 1 is exempt from the deadline so we always have a legal move to return.

The reported `time_ms` is the elapsed time at the **last completed** iteration, not
total wall time — so if depth N+1 aborts mid-flight, the number reflects when depth
N finished.

---

## 9. The Full Flow, Start to Finish

When it's the engine's turn:

```
select_move(board):
    result = search_time(board, time_budget_ms = 10000, max_depth = 16)
    return result.best_move

search_time(board, budget, max_depth):
    run depth 1 (no deadline) -> remember best move
    for d in 2 .. max_depth:
        if past deadline: stop
        value = negamax(board, d, alpha = V−200, beta = V+200)   # aspiration
        if value fell outside the window: widen and re-search
        remember (best_move, value, d)
    return last completed result

negamax(board, depth, alpha, beta):
    if game over:                    return exact final score
    if depth == 0 and not endgame:   return evaluate(board)        # the 4-feature sum
    probe transposition table; maybe return early or tighten alpha/beta
    generate legal moves; order them (TT move, killers, size+history, pass last)
    for i, move in moves:
        make move
        if i == 0:        full-window, full-depth search
        else:             null-window (+ LMR reduction if i>=3, depth>=3),
                          re-search at full depth/window only if it beats alpha
        unmake move
        track best; if value >= beta: record killer/history, beta cutoff (stop)
    store result in transposition table
    return best
```

The number that comes out of `evaluate` at the leaves is exactly the weighted
feature sum from §2. Everything above it — alpha-beta, iterative deepening,
aspiration, PVS, LMR, the TT, move ordering, the endgame solver — exists purely to
search as deep as possible within the time budget so that the move chosen at the
root is backed by the furthest, sharpest lookahead we can afford.

---

## 10. How the Weights Were Chosen

The four eval weights aren't guessed. They were tuned by **self-play**: the engine
plays many games against versions of itself with slightly different weights, and a
statistical test (**SPRT** — Sequential Probability Ratio Test) decides whether a
change is a real improvement or just noise, stopping as soon as the evidence is
conclusive either way. A coordinate-descent loop bumps one weight at a time and
keeps only SPRT-verified gains. The tuning code lives in
[python/tuning/](python/tuning/). The `territory` feature itself was later
redesigned (from walking-distance to piece-aware coverage) after diagnostics showed
the old metric over-claimed territory the engine couldn't actually take — see
[diagnostics/replay_position.py](python/diagnostics/replay_position.py) for the tool
that pins down why a given move was chosen.
