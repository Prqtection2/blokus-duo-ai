//! PyO3 bindings: thin wrappers around `blokus_core::board::Board` and `Move`.

use std::fmt::Write;

use pyo3::prelude::*;

use blokus_core::bitboard::{self, Bitboard};
use blokus_core::board as core_board;
use blokus_core::eval as core_eval;
use blokus_core::movegen;
use blokus_core::pieces;
use blokus_core::search;

/// Engine version, re-exported from `blokus_core`.
#[pyfunction]
fn version() -> &'static str {
    blokus_core::version()
}

#[pyclass(name = "Move")]
#[derive(Clone)]
struct PyMove(core_board::Move);

#[pymethods]
impl PyMove {
    #[staticmethod]
    fn pass_move() -> Self {
        PyMove(core_board::Move::Pass)
    }

    #[getter]
    fn is_pass(&self) -> bool {
        matches!(self.0, core_board::Move::Pass)
    }

    #[getter]
    fn piece_id(&self) -> Option<u8> {
        match self.0 {
            core_board::Move::Pass => None,
            core_board::Move::Place { piece, .. } => Some(piece),
        }
    }

    #[getter]
    fn size(&self) -> u32 {
        match self.0 {
            core_board::Move::Pass => 0,
            core_board::Move::Place { placement, .. } => placement.count_ones(),
        }
    }

    /// List of (row, col) cells covered by this move; empty for a pass.
    fn cells(&self) -> Vec<(usize, usize)> {
        match self.0 {
            core_board::Move::Pass => Vec::new(),
            core_board::Move::Place { placement, .. } => placement
                .iter_bits()
                .map(bitboard::from_bit_index)
                .collect(),
        }
    }

    fn __repr__(&self) -> String {
        match self.0 {
            core_board::Move::Pass => "Move(pass)".to_string(),
            core_board::Move::Place { piece, placement } => {
                let name = pieces::FREE_PIECE_NAMES[piece as usize];
                let cells: Vec<(usize, usize)> = placement
                    .iter_bits()
                    .map(bitboard::from_bit_index)
                    .collect();
                format!("Move(piece={name}, cells={cells:?})")
            }
        }
    }
}

#[pyclass(name = "Board")]
struct PyBoard(core_board::Board);

#[pymethods]
impl PyBoard {
    #[new]
    fn new() -> Self {
        PyBoard(core_board::Board::new())
    }

    fn legal_moves(&self) -> Vec<PyMove> {
        movegen::generate_moves(&self.0)
            .into_iter()
            .map(PyMove)
            .collect()
    }

    fn make_move(&mut self, mv: &PyMove) {
        self.0.make_move(&mv.0);
    }

    fn make_pass(&mut self) {
        self.0.make_move(&core_board::Move::Pass);
    }

    fn unmake_move(&mut self) {
        self.0.unmake_move();
    }

    fn is_terminal(&self) -> bool {
        self.0.game_over()
    }

    fn score(&self, player: usize) -> i32 {
        self.0.final_score(player)
    }

    fn squares_left(&self, player: usize) -> i32 {
        self.0.squares_left(player)
    }

    #[getter]
    fn side_to_move(&self) -> u8 {
        self.0.side_to_move
    }

    #[getter]
    fn ply(&self) -> u32 {
        self.0.ply
    }

    #[getter]
    fn zobrist(&self) -> u64 {
        self.0.zobrist
    }

    fn pieces_left(&self, player: usize) -> Vec<u8> {
        let mask = self.0.pieces_left[player];
        (0..pieces::NUM_FREE_PIECES as u8)
            .filter(|i| (mask >> i) & 1 != 0)
            .collect()
    }

    /// Cells (row, col) occupied by `player`.
    fn cells_of(&self, player: usize) -> Vec<(usize, usize)> {
        self.0.own[player]
            .iter_bits()
            .map(bitboard::from_bit_index)
            .collect()
    }

    fn last_placed(&self, player: usize) -> Option<u8> {
        self.0.last_placed[player]
    }

    fn consecutive_passes(&self) -> u8 {
        self.0.consecutive_passes
    }

    /// Render the board as a 14x14 ASCII grid. `X` = side 0, `O` = side 1,
    /// `+` = empty start cell, `.` = empty other cell.
    fn ascii(&self) -> String {
        ascii_render(&self.0)
    }

    fn __repr__(&self) -> String {
        format!(
            "Board(ply={}, stm={}, terminal={})",
            self.0.ply,
            self.0.side_to_move,
            self.0.game_over()
        )
    }
}

fn ascii_render(b: &core_board::Board) -> String {
    let mut s = String::new();
    s.push_str("    ");
    for c in 0..bitboard::PLAY_COLS {
        let _ = write!(s, "{} ", c % 10);
    }
    s.push('\n');
    for r in 0..bitboard::PLAY_ROWS {
        let _ = write!(s, "{:>2}  ", r);
        for c in 0..bitboard::PLAY_COLS {
            let idx = bitboard::bit_index(r, c);
            let ch = if b.own[0].get_bit(idx) {
                'X'
            } else if b.own[1].get_bit(idx) {
                'O'
            } else if (r, c) == core_board::START_CELLS[0]
                || (r, c) == core_board::START_CELLS[1]
            {
                '+'
            } else {
                '.'
            };
            s.push(ch);
            s.push(' ');
        }
        s.push('\n');
    }
    s
}

/// Number of free pieces (21).
#[pyfunction]
fn num_free_pieces() -> usize {
    pieces::NUM_FREE_PIECES
}

/// Free-piece names, indexed by piece_id.
#[pyfunction]
fn piece_names() -> Vec<&'static str> {
    pieces::FREE_PIECE_NAMES.to_vec()
}

/// Start cells [(row, col), (row, col)] for each player (0-indexed).
#[pyfunction]
fn start_cells() -> Vec<(usize, usize)> {
    core_board::START_CELLS.to_vec()
}

#[pyclass(name = "SearchResult")]
#[derive(Clone)]
struct PySearchResult {
    #[pyo3(get)]
    value: i32,
    #[pyo3(get)]
    depth: u32,
    #[pyo3(get)]
    nodes: u64,
    #[pyo3(get)]
    tt_hits: u64,
    #[pyo3(get)]
    time_ms: u64,
    inner_move: Option<core_board::Move>,
}

#[pymethods]
impl PySearchResult {
    #[getter]
    fn best_move(&self) -> Option<PyMove> {
        self.inner_move.map(PyMove)
    }

    #[getter]
    fn nodes_per_second(&self) -> f64 {
        if self.time_ms == 0 {
            self.nodes as f64
        } else {
            self.nodes as f64 * 1000.0 / self.time_ms as f64
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SearchResult(value={}, depth={}, nodes={}, time_ms={}, nps={:.0})",
            self.value, self.depth, self.nodes, self.time_ms,
            self.nodes_per_second()
        )
    }
}

#[pyclass(name = "SearchEngine")]
struct PySearchEngine {
    inner: search::SearchEngine,
}

#[pymethods]
impl PySearchEngine {
    #[new]
    #[pyo3(signature = (tt_size_log2 = 18))]
    fn new(tt_size_log2: u32) -> Self {
        Self {
            inner: search::SearchEngine::new(tt_size_log2),
        }
    }

    fn clear(&mut self) {
        self.inner.clear();
    }

    #[setter]
    fn set_tt_enabled(&mut self, enabled: bool) {
        self.inner.set_tt_enabled(enabled);
    }

    #[getter]
    fn tt_enabled(&self) -> bool {
        self.inner.tt_enabled()
    }

    #[getter]
    fn tt_capacity(&self) -> usize {
        self.inner.tt_capacity()
    }

    #[pyo3(signature = (board, time_budget_ms = 200, max_depth = 16))]
    fn search(
        &mut self,
        board: &mut PyBoard,
        time_budget_ms: u64,
        max_depth: u32,
    ) -> PySearchResult {
        let r = self.inner.search_time(&mut board.0, time_budget_ms, max_depth);
        PySearchResult {
            value: r.value,
            depth: r.depth,
            nodes: r.nodes,
            tt_hits: r.tt_hits,
            time_ms: r.time_ms,
            inner_move: r.best_move,
        }
    }

    fn search_fixed_depth(
        &mut self,
        board: &mut PyBoard,
        depth: u32,
    ) -> PySearchResult {
        let r = self.inner.search_fixed_depth(&mut board.0, depth);
        PySearchResult {
            value: r.value,
            depth: r.depth,
            nodes: r.nodes,
            tt_hits: r.tt_hits,
            time_ms: r.time_ms,
            inner_move: r.best_move,
        }
    }

    /// Replace evaluation weights. Clears the TT.
    fn set_weights(
        &mut self,
        placed_squares: i32,
        corner_count: i32,
        territory: i32,
        piece_liability: i32,
    ) {
        self.inner.set_weights(core_eval::EvalWeights {
            placed_squares,
            corner_count,
            territory,
            piece_liability,
        });
    }

    /// Set placeholder (Phase 3) weights: only placed_squares.
    fn use_placeholder_eval(&mut self) {
        self.inner.set_weights(core_eval::EvalWeights::placeholder());
    }

    /// Evaluate the current position using current weights (no search).
    fn eval(&self, board: &PyBoard) -> i32 {
        core_eval::heuristic_with(&board.0, &self.inner.weights())
    }

    fn weights(&self) -> (i32, i32, i32, i32) {
        let w = self.inner.weights();
        (w.placed_squares, w.corner_count, w.territory, w.piece_liability)
    }

    #[getter]
    fn endgame_threshold(&self) -> u32 {
        self.inner.endgame_threshold()
    }

    /// Set the endgame solver activation threshold (combined pieces_left
    /// count). Set to 0 to disable the solver entirely.
    fn set_endgame_threshold(&mut self, threshold: u32) {
        self.inner.set_endgame_threshold(threshold);
    }
}

/// Canonical cell shapes for each of the 21 free pieces (indexed by piece_id).
/// One orientation per piece — the UI is responsible for rotation/reflection.
#[pyfunction]
fn piece_base_shapes() -> Vec<Vec<(usize, usize)>> {
    let mut result: Vec<Vec<(usize, usize)>> =
        vec![Vec::new(); pieces::NUM_FREE_PIECES];
    for op in pieces::oriented_pieces() {
        let slot = &mut result[op.free_id as usize];
        if slot.is_empty() {
            *slot = op
                .cells
                .iter()
                .map(|&(r, c)| (r as usize, c as usize))
                .collect();
        }
    }
    result
}

// Suppress unused-import warning — Bitboard is needed indirectly via Board fields.
#[allow(dead_code)]
fn _unused_marker(_: Bitboard) {}

#[pymodule]
fn blokus(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(num_free_pieces, m)?)?;
    m.add_function(wrap_pyfunction!(piece_names, m)?)?;
    m.add_function(wrap_pyfunction!(start_cells, m)?)?;
    m.add_function(wrap_pyfunction!(piece_base_shapes, m)?)?;
    m.add_class::<PyMove>()?;
    m.add_class::<PyBoard>()?;
    m.add_class::<PySearchResult>()?;
    m.add_class::<PySearchEngine>()?;
    Ok(())
}
