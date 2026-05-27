//! Negamax alpha-beta with iterative deepening, transposition table, killer
//! and history move-ordering heuristics.

use std::time::{Duration, Instant};

use crate::board::{Board, Move};
use crate::eval::{self, EvalWeights};
use crate::movegen;
use crate::pieces::NUM_FREE_PIECES;
use crate::tt::{TranspositionTable, TtFlag};

/// Hard cap on search depth (also bounds killer-move array size).
pub const MAX_DEPTH: u32 = 64;
/// Search value sentinel. Kept well below i32::MAX so negation can't overflow.
const INF: i32 = 1_000_000_000;
/// Initial aspiration window around the previous iteration's value.
/// Sized to comfortably contain typical iteration-to-iteration eval swings
/// in the real-eval engine (placed_squares ~100/piece × small move-count
/// differences = a few hundred is the typical scale).
const ASPIRATION_WINDOW: i32 = 200;

#[derive(Default, Clone, Copy, Debug)]
pub struct SearchStats {
    pub nodes: u64,
    pub tt_hits: u64,
    pub tt_cutoffs: u64,
    pub beta_cutoffs: u64,
}

#[derive(Default, Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub value: i32,
    pub depth: u32,
    pub nodes: u64,
    pub tt_hits: u64,
    pub time_ms: u64,
}

struct KillerMoves {
    slots: Vec<[Option<Move>; 2]>,
}
impl KillerMoves {
    fn new(max_depth: u32) -> Self {
        Self { slots: vec![[None, None]; max_depth as usize + 1] }
    }
    fn get(&self, depth: u32) -> [Option<Move>; 2] {
        let d = depth as usize;
        if d < self.slots.len() { self.slots[d] } else { [None, None] }
    }
    fn update(&mut self, depth: u32, mv: Move) {
        let d = depth as usize;
        if d >= self.slots.len() { return; }
        let s = &mut self.slots[d];
        if s[0] != Some(mv) {
            s[1] = s[0];
            s[0] = Some(mv);
        }
    }
    fn clear(&mut self) {
        for s in self.slots.iter_mut() { *s = [None, None]; }
    }
}

struct History {
    table: [[u32; NUM_FREE_PIECES]; 2],
}
impl History {
    fn new() -> Self { Self { table: [[0u32; NUM_FREE_PIECES]; 2] } }
    fn update(&mut self, player: usize, piece: u8, depth: u32) {
        let bonus = (depth as u32).saturating_mul(depth as u32);
        let slot = &mut self.table[player][piece as usize];
        *slot = slot.saturating_add(bonus);
    }
    fn score(&self, player: usize, piece: u8) -> u32 {
        self.table[player][piece as usize]
    }
    fn clear(&mut self) {
        self.table = [[0u32; NUM_FREE_PIECES]; 2];
    }
}

pub struct SearchEngine {
    tt: TranspositionTable,
    killers: KillerMoves,
    history: History,
    stats: SearchStats,
    deadline: Option<Instant>,
    aborted: bool,
    use_tt: bool,
    weights: EvalWeights,
    /// Reused per-ply move buffers. Taken out via `mem::take` during search so
    /// negamax can split-borrow it; restored after the search returns.
    move_scratch: Vec<Vec<Move>>,
    /// Endgame solver activates when combined `pieces_left.count_ones()` for
    /// both players ≤ this threshold. In that mode, negamax ignores depth-0
    /// heuristic cutoffs and only returns at terminal (exact `final_score`).
    /// Set to 0 to disable entirely.
    endgame_threshold: u32,
    /// Late-move reductions: search moves at index >= 3 (past TT-move +
    /// killers) at depth-1 with a null window, then re-search at full depth
    /// only if they improve alpha. Cuts nodes substantially at the cost of
    /// being technically unsound — the AB=MM tests disable this flag.
    lmr_enabled: bool,
}

impl SearchEngine {
    pub fn new(tt_size_log2: u32) -> Self {
        let scratch = (0..=MAX_DEPTH as usize)
            .map(|_| Vec::with_capacity(256))
            .collect();
        Self {
            tt: TranspositionTable::new(tt_size_log2),
            killers: KillerMoves::new(MAX_DEPTH),
            history: History::new(),
            stats: SearchStats::default(),
            deadline: None,
            aborted: false,
            use_tt: true,
            weights: EvalWeights::default(),
            move_scratch: scratch,
            // 6 ≈ 3 pieces remaining per side on average. Empirically the
            // sweet spot from calibration: low enough that the solver doesn't
            // blow the time budget on interior nodes, high enough to actually
            // fire in late games. At this engine's current strength the
            // benefit is small but the cost is invisible.
            endgame_threshold: 6,
            lmr_enabled: true,
        }
    }

    pub fn lmr_enabled(&self) -> bool { self.lmr_enabled }

    /// Toggle late-move reductions. With LMR off, alpha-beta returns the
    /// exact min-max value (modulo TT) — needed by AB=MM tests.
    pub fn set_lmr_enabled(&mut self, enabled: bool) {
        if enabled != self.lmr_enabled {
            // Reductions affect TT values; clearing avoids reading entries
            // computed under a different reduction policy.
            self.tt.clear();
        }
        self.lmr_enabled = enabled;
    }

    pub fn endgame_threshold(&self) -> u32 {
        self.endgame_threshold
    }

    /// Set the endgame solver activation threshold (combined `pieces_left`
    /// count across both players). Set to 0 to disable.
    pub fn set_endgame_threshold(&mut self, threshold: u32) {
        if threshold != self.endgame_threshold {
            // Endgame mode changes eval semantics (no heuristic at depth 0).
            // Invalidate any TT entries computed under the previous policy.
            self.tt.clear();
        }
        self.endgame_threshold = threshold;
    }

    #[inline]
    fn endgame_active(&self, board: &Board) -> bool {
        if self.endgame_threshold == 0 {
            return false;
        }
        let remaining = board.pieces_left[0].count_ones()
            + board.pieces_left[1].count_ones();
        remaining <= self.endgame_threshold
    }

    pub fn set_tt_enabled(&mut self, enabled: bool) { self.use_tt = enabled; }
    pub fn tt_enabled(&self) -> bool { self.use_tt }
    pub fn tt_capacity(&self) -> usize { self.tt.capacity() }

    pub fn weights(&self) -> EvalWeights { self.weights }
    /// Set evaluation weights. Invalidates the TT (entries' values were
    /// computed under the previous weight vector).
    pub fn set_weights(&mut self, weights: EvalWeights) {
        if weights != self.weights {
            self.tt.clear();
        }
        self.weights = weights;
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.killers.clear();
        self.history.clear();
        self.stats = SearchStats::default();
    }

    /// Fixed-depth search. No deadline; always completes.
    pub fn search_fixed_depth(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        let start = Instant::now();
        self.stats = SearchStats::default();
        self.aborted = false;
        self.deadline = None;
        let mut scratch = std::mem::take(&mut self.move_scratch);
        let (value, best_move) = self.negamax(board, depth, -INF, INF, &mut scratch);
        self.move_scratch = scratch;
        let time_ms = start.elapsed().as_millis() as u64;
        SearchResult {
            best_move,
            value,
            depth,
            nodes: self.stats.nodes,
            tt_hits: self.stats.tt_hits,
            time_ms,
        }
    }

    /// Iterative-deepening search with a wall-clock budget. Depth 1 always
    /// completes; deeper iterations are cut off if the budget expires.
    pub fn search_time(
        &mut self,
        board: &mut Board,
        time_budget_ms: u64,
        max_depth: u32,
    ) -> SearchResult {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(time_budget_ms);

        // Depth 1 always runs (no deadline check).
        self.stats = SearchStats::default();
        self.aborted = false;
        self.deadline = None;
        let mut scratch = std::mem::take(&mut self.move_scratch);
        let (v1, mv1) = self.negamax(board, 1, -INF, INF, &mut scratch);
        let mut last = SearchResult {
            best_move: mv1,
            value: v1,
            depth: 1,
            nodes: self.stats.nodes,
            tt_hits: self.stats.tt_hits,
            time_ms: start.elapsed().as_millis() as u64,
        };
        let mut total_nodes = self.stats.nodes;
        let mut total_tt_hits = self.stats.tt_hits;

        self.deadline = Some(deadline);
        let cap = max_depth.min(MAX_DEPTH);
        let mut prev_value = v1;
        for d in 2..=cap {
            if Instant::now() >= deadline {
                break;
            }
            self.stats = SearchStats::default();
            self.aborted = false;

            // Aspiration window: search a narrow band around the previous
            // iteration's value. If the result falls outside the window
            // ("fail-low" if ≤ alpha, "fail-high" if ≥ beta), the value isn't
            // exact — re-search with a full window. Window size grows on
            // re-search rather than jumping straight to ±INF.
            let mut alpha = prev_value.saturating_sub(ASPIRATION_WINDOW);
            let mut beta = prev_value.saturating_add(ASPIRATION_WINDOW);
            let (mut v, mut mv) = self.negamax(board, d, alpha, beta, &mut scratch);
            while !self.aborted && (v <= alpha || v >= beta) {
                // Failed; widen and retry. Doubling each round caps at INF.
                let widened = (beta - alpha).saturating_mul(2).max(ASPIRATION_WINDOW * 2);
                if v <= alpha {
                    alpha = prev_value.saturating_sub(widened);
                } else {
                    beta = prev_value.saturating_add(widened);
                }
                if widened >= INF {
                    alpha = -INF;
                    beta = INF;
                }
                let (v2, mv2) = self.negamax(board, d, alpha, beta, &mut scratch);
                v = v2;
                mv = mv2;
                if alpha == -INF && beta == INF {
                    break;
                }
            }

            total_nodes += self.stats.nodes;
            total_tt_hits += self.stats.tt_hits;
            if self.aborted {
                break;
            }
            prev_value = v;
            last = SearchResult {
                best_move: mv,
                value: v,
                depth: d,
                nodes: total_nodes,
                tt_hits: total_tt_hits,
                time_ms: start.elapsed().as_millis() as u64,
            };
        }
        self.move_scratch = scratch;
        last
    }

    fn negamax(
        &mut self,
        board: &mut Board,
        depth: u32,
        mut alpha: i32,
        mut beta: i32,
        scratch: &mut [Vec<Move>],
    ) -> (i32, Option<Move>) {
        if self.aborted {
            return (alpha, None);
        }
        self.stats.nodes += 1;
        // Periodic deadline check.
        if self.stats.nodes & 0x3FFF == 0 {
            if let Some(d) = self.deadline {
                if Instant::now() >= d {
                    self.aborted = true;
                    return (alpha, None);
                }
            }
        }

        if board.game_over() {
            return (eval::terminal_value(board), None);
        }
        let in_endgame = self.endgame_active(board);
        if depth == 0 && !in_endgame {
            return (eval::heuristic_with(board, &self.weights), None);
        }

        let orig_alpha = alpha;
        let key = board.zobrist;
        let mut tt_move: Option<Move> = None;

        if self.use_tt {
            if let Some(e) = self.tt.probe(key) {
                self.stats.tt_hits += 1;
                if e.depth >= depth as u8 {
                    match e.flag {
                        TtFlag::Exact => return (e.value, e.best_move),
                        TtFlag::Lower => alpha = alpha.max(e.value),
                        TtFlag::Upper => beta = beta.min(e.value),
                        TtFlag::Empty => unreachable!(),
                    }
                    if alpha >= beta {
                        self.stats.tt_cutoffs += 1;
                        return (e.value, e.best_move);
                    }
                }
                tt_move = e.best_move;
            }
        }

        // Split this ply's buffer from deeper plies' buffers.
        let (buf, rest) = scratch.split_first_mut().expect("scratch underrun");
        movegen::generate_moves_into(board, buf);
        if buf.is_empty() {
            buf.push(Move::Pass);
        }
        order_moves(buf, tt_move, depth, board, &self.killers, &self.history);

        let mut best_value = -INF;
        let mut best_move: Option<Move> = None;

        // Index-loop so we don't hold an iterator borrow over the recursive call.
        // `Move` is Copy so per-iteration `buf[i]` is a cheap value read.
        // `saturating_sub(1)` lets endgame mode keep recursing past depth=0;
        // termination is then bounded by `board.game_over()`.
        let next_depth = depth.saturating_sub(1);
        let n = buf.len();
        for i in 0..n {
            let mv = buf[i];
            board.make_move(&mv);

            // PVS + LMR:
            //  - Move 0 (the principal-variation candidate): full window,
            //    full depth.
            //  - Moves 1+: null-window search. With LMR, late moves (i >= 3)
            //    at depth >= 3 get an additional depth reduction; if the
            //    reduced result improves alpha we re-search at full depth
            //    (still null window) and then full window inside (alpha, beta).
            let child_value: i32 = if i == 0 {
                -self.negamax(board, next_depth, -beta, -alpha, rest).0
            } else {
                let do_lmr = self.lmr_enabled
                    && i >= 3
                    && depth >= 3
                    && !matches!(mv, Move::Pass);
                let reduction: u32 = if do_lmr { 1 } else { 0 };
                let reduced_depth = next_depth.saturating_sub(reduction);

                let v_reduced = -self
                    .negamax(board, reduced_depth, -(alpha + 1), -alpha, rest)
                    .0;

                if self.aborted {
                    board.unmake_move();
                    return (alpha, best_move);
                }

                if v_reduced > alpha {
                    // Promising — re-search at full depth (null window) if
                    // we reduced, then full window if it lands in (alpha, beta).
                    let v_full = if reduction > 0 {
                        -self
                            .negamax(board, next_depth, -(alpha + 1), -alpha, rest)
                            .0
                    } else {
                        v_reduced
                    };
                    if self.aborted {
                        board.unmake_move();
                        return (alpha, best_move);
                    }
                    if v_full > alpha && v_full < beta {
                        -self.negamax(board, next_depth, -beta, -alpha, rest).0
                    } else {
                        v_full
                    }
                } else {
                    v_reduced
                }
            };

            board.unmake_move();
            if self.aborted {
                return (alpha, best_move);
            }
            let value = child_value;
            if value > best_value {
                best_value = value;
                best_move = Some(mv);
            }
            if value > alpha {
                alpha = value;
            }
            if alpha >= beta {
                self.stats.beta_cutoffs += 1;
                if let Move::Place { piece, .. } = mv {
                    self.killers.update(depth, mv);
                    self.history
                        .update(board.side_to_move as usize, piece, depth);
                }
                break;
            }
        }

        if self.use_tt && !self.aborted {
            let flag = if best_value <= orig_alpha {
                TtFlag::Upper
            } else if best_value >= beta {
                TtFlag::Lower
            } else {
                TtFlag::Exact
            };
            self.tt
                .store(key, depth as u8, best_value, flag, best_move);
        }

        (best_value, best_move)
    }
}

fn order_moves(
    moves: &mut Vec<Move>,
    tt_move: Option<Move>,
    depth: u32,
    board: &Board,
    killers: &KillerMoves,
    history: &History,
) {
    let killer_pair = killers.get(depth);
    let stm = board.side_to_move as usize;
    moves.sort_by_key(|m| {
        let score: i32 = if Some(*m) == tt_move {
            1_000_000_000
        } else if Some(*m) == killer_pair[0] {
            500_000_000
        } else if Some(*m) == killer_pair[1] {
            490_000_000
        } else {
            match m {
                Move::Place { piece, placement } => {
                    let size = placement.count_ones() as i32;
                    (size << 20) + history.score(stm, *piece) as i32
                }
                Move::Pass => -1_000_000_000,
            }
        };
        std::cmp::Reverse(score)
    });
}

/// Reference unpruned negamax used by tests. No TT, no ordering, no cutoffs.
pub fn plain_minimax(board: &mut Board, depth: u32) -> (i32, u64) {
    if board.game_over() {
        return (eval::terminal_value(board), 1);
    }
    if depth == 0 {
        return (eval::heuristic(board), 1);
    }
    let mut moves = movegen::generate_moves(board);
    if moves.is_empty() {
        moves.push(Move::Pass);
    }
    let mut best = -INF;
    let mut nodes: u64 = 1;
    for mv in &moves {
        board.make_move(mv);
        let (v, n) = plain_minimax(board, depth - 1);
        board.unmake_move();
        nodes += n;
        let v = -v;
        if v > best {
            best = v;
        }
    }
    (best, nodes)
}
