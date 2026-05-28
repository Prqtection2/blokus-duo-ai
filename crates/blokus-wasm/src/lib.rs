//! WebAssembly bindings for the Blokus Duo engine.
//!
//! Mirrors the server-side `GameSession` (python/blokus_harness/gui/server.py)
//! so the browser can run a full game — rules validation + engine — entirely
//! client-side, with no backend. `serialize()` and `static_meta()` produce the
//! same shapes the FastAPI server sent over WebSocket, so the existing frontend
//! rendering code works against either transport.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use blokus_core::bitboard;
use blokus_core::board::{Board, Move, START_CELLS};
use blokus_core::eval::{self, EvalWeights};
use blokus_core::movegen;
use blokus_core::pieces;
use blokus_core::search::SearchEngine;

const DEFAULT_WEIGHTS: EvalWeights = EvalWeights {
    placed_squares: 100,
    corner_count: 80,
    territory: 60,
    piece_liability: -10,
};

fn move_cells(mv: &Move) -> Vec<(usize, usize)> {
    match mv {
        Move::Pass => Vec::new(),
        Move::Place { placement, .. } => {
            placement.iter_bits().map(bitboard::from_bit_index).collect()
        }
    }
}

fn move_repr(mv: &Move) -> String {
    match mv {
        Move::Pass => "pass".to_string(),
        Move::Place { piece, placement } => {
            let name = pieces::FREE_PIECE_NAMES[*piece as usize];
            let cells: Vec<(usize, usize)> =
                placement.iter_bits().map(bitboard::from_bit_index).collect();
            format!("Move(piece={name}, cells={cells:?})")
        }
    }
}

#[derive(Clone)]
struct LastMove {
    by: u8,
    piece_id: Option<u8>,
    cells: Vec<(usize, usize)>,
    passed: bool,
}

#[derive(Clone)]
struct EngineMeta {
    eval: f64,
    depth: u32,
    nodes: u64,
    time_ms: f64,
    move_repr: String,
    passed: bool,
}

#[derive(Serialize)]
struct LegalMoveJs {
    piece_id: u8,
    cells: Vec<(usize, usize)>,
}

#[derive(Serialize)]
struct LastMoveJs {
    by: u8,
    piece_id: Option<u8>,
    cells: Vec<(usize, usize)>,
    passed: bool,
}

#[derive(Serialize)]
struct EngineMetaJs {
    eval: f64,
    depth: u32,
    nodes: u64,
    time_ms: f64,
    move_repr: String,
    passed: bool,
}

#[derive(Serialize)]
struct StateJs {
    #[serde(rename = "type")]
    typ: &'static str,
    ply: u32,
    side_to_move: u8,
    human_side: u8,
    terminal: bool,
    consecutive_passes: u8,
    scores: [i32; 2],
    squares_left: [i32; 2],
    cells: [Vec<(usize, usize)>; 2],
    pieces_left: [Vec<u8>; 2],
    legal_moves: Vec<LegalMoveJs>,
    last_move: Option<LastMoveJs>,
    partition: Vec<u8>,
    engine_meta: Option<EngineMetaJs>,
}

#[derive(Serialize)]
struct StaticMetaJs {
    #[serde(rename = "type")]
    typ: &'static str,
    piece_names: Vec<&'static str>,
    piece_shapes: Vec<Vec<(usize, usize)>>,
    start_cells: Vec<(usize, usize)>,
    engine_name: &'static str,
    engine_version: &'static str,
}

#[wasm_bindgen]
pub struct WasmGame {
    board: Board,
    human_side: u8,
    engine: SearchEngine,
    time_budget_ms: u64,
    max_depth: u32,
    last_move: Option<LastMove>,
    last_engine_meta: Option<EngineMeta>,
}

#[wasm_bindgen]
impl WasmGame {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmGame {
        console_error_panic_hook::set_once();
        let mut engine = SearchEngine::new(18);
        engine.set_weights(DEFAULT_WEIGHTS);
        WasmGame {
            board: Board::new(),
            human_side: 0,
            engine,
            time_budget_ms: 2000,
            max_depth: 16,
            last_move: None,
            last_engine_meta: None,
        }
    }

    /// Milliseconds the engine may spend per move. Higher = stronger but the
    /// search blocks its thread for that long; keep modest for browser play.
    #[wasm_bindgen(js_name = setTimeBudgetMs)]
    pub fn set_time_budget_ms(&mut self, ms: u32) {
        self.time_budget_ms = ms as u64;
    }

    #[wasm_bindgen(js_name = newGame)]
    pub fn new_game(&mut self, human_side: u8) {
        self.board = Board::new();
        self.human_side = human_side & 1;
        self.engine.clear();
        self.last_move = None;
        self.last_engine_meta = None;
    }

    fn find_legal_match(&self, piece_id: u8, cells: &[(usize, usize)]) -> Option<Move> {
        let target: std::collections::HashSet<(usize, usize)> =
            cells.iter().copied().collect();
        for mv in movegen::generate_moves(&self.board) {
            if let Move::Place { piece, placement } = mv {
                if piece == piece_id {
                    let mv_cells: std::collections::HashSet<(usize, usize)> =
                        placement.iter_bits().map(bitboard::from_bit_index).collect();
                    if mv_cells == target {
                        return Some(mv);
                    }
                }
            }
        }
        None
    }

    /// Attempt the human's move. `cells` is a JS array of [row, col] pairs.
    /// Returns true if it was legal and applied.
    #[wasm_bindgen(js_name = attemptHumanMove)]
    pub fn attempt_human_move(&mut self, piece_id: u8, cells: JsValue) -> bool {
        if self.board.game_over() || self.board.side_to_move != self.human_side {
            return false;
        }
        let cells: Vec<(usize, usize)> = match serde_wasm_bindgen::from_value(cells) {
            Ok(c) => c,
            Err(_) => return false,
        };
        match self.find_legal_match(piece_id, &cells) {
            Some(mv) => {
                self.board.make_move(&mv);
                self.last_move = Some(LastMove {
                    by: self.human_side,
                    piece_id: Some(piece_id),
                    cells,
                    passed: false,
                });
                true
            }
            None => false,
        }
    }

    #[wasm_bindgen(js_name = humanPass)]
    pub fn human_pass(&mut self) -> bool {
        if self.board.game_over() || self.board.side_to_move != self.human_side {
            return false;
        }
        if !movegen::generate_moves(&self.board).is_empty() {
            return false; // can't pass while moves exist
        }
        self.board.make_move(&Move::Pass);
        self.last_move = Some(LastMove {
            by: self.human_side,
            piece_id: None,
            cells: Vec::new(),
            passed: true,
        });
        true
    }

    fn step_engine(&mut self) {
        let legal = movegen::generate_moves(&self.board);
        let by = 1 - self.human_side;
        if legal.is_empty() {
            self.board.make_move(&Move::Pass);
            self.last_move = Some(LastMove { by, piece_id: None, cells: Vec::new(), passed: true });
            self.last_engine_meta = Some(EngineMeta {
                eval: 0.0,
                depth: 0,
                nodes: 0,
                time_ms: 0.0,
                move_repr: "pass".to_string(),
                passed: true,
            });
            return;
        }
        let result =
            self.engine
                .search_time(&mut self.board, self.time_budget_ms, self.max_depth);
        let mv = result.best_move.unwrap_or(legal[0]);
        let piece_id = match mv {
            Move::Place { piece, .. } => Some(piece),
            Move::Pass => None,
        };
        let cells = move_cells(&mv);
        let repr = move_repr(&mv);
        self.board.make_move(&mv);
        self.last_move = Some(LastMove { by, piece_id, cells, passed: false });
        self.last_engine_meta = Some(EngineMeta {
            eval: result.value as f64,
            depth: result.depth,
            nodes: result.nodes,
            time_ms: result.time_ms as f64,
            move_repr: repr,
            passed: false,
        });
    }

    /// Run engine plies until it's the human's turn or the game ends.
    #[wasm_bindgen(js_name = playEngineUntilHumansTurn)]
    pub fn play_engine_until_humans_turn(&mut self) {
        while !self.board.game_over() && self.board.side_to_move != self.human_side {
            self.step_engine();
        }
    }

    fn cells_of(&self, p: usize) -> Vec<(usize, usize)> {
        self.board.own[p].iter_bits().map(bitboard::from_bit_index).collect()
    }

    fn pieces_left_of(&self, p: usize) -> Vec<u8> {
        let mask = self.board.pieces_left[p];
        (0..pieces::NUM_FREE_PIECES as u8)
            .filter(|i| (mask >> i) & 1 != 0)
            .collect()
    }

    /// Current game state, shaped identically to the server's `serialize()`.
    pub fn serialize(&self) -> JsValue {
        let terminal = self.board.game_over();
        let legal_moves: Vec<LegalMoveJs> = if terminal {
            Vec::new()
        } else {
            movegen::generate_moves(&self.board)
                .into_iter()
                .filter_map(|m| match m {
                    Move::Place { piece, .. } => Some(LegalMoveJs {
                        piece_id: piece,
                        cells: move_cells(&m),
                    }),
                    Move::Pass => None,
                })
                .collect()
        };
        let state = StateJs {
            typ: "state",
            ply: self.board.ply,
            side_to_move: self.board.side_to_move,
            human_side: self.human_side,
            terminal,
            consecutive_passes: self.board.consecutive_passes,
            scores: [self.board.final_score(0), self.board.final_score(1)],
            squares_left: [self.board.squares_left(0), self.board.squares_left(1)],
            cells: [self.cells_of(0), self.cells_of(1)],
            pieces_left: [self.pieces_left_of(0), self.pieces_left_of(1)],
            legal_moves,
            last_move: self.last_move.as_ref().map(|m| LastMoveJs {
                by: m.by,
                piece_id: m.piece_id,
                cells: m.cells.clone(),
                passed: m.passed,
            }),
            partition: eval::contested_partition(&self.board),
            engine_meta: self.last_engine_meta.as_ref().map(|m| EngineMetaJs {
                eval: m.eval,
                depth: m.depth,
                nodes: m.nodes,
                time_ms: m.time_ms,
                move_repr: m.move_repr.clone(),
                passed: m.passed,
            }),
        };
        serde_wasm_bindgen::to_value(&state).unwrap()
    }

    /// Static metadata (piece names/shapes, start cells, version).
    #[wasm_bindgen(js_name = staticMeta)]
    pub fn static_meta(&self) -> JsValue {
        let mut piece_shapes: Vec<Vec<(usize, usize)>> =
            vec![Vec::new(); pieces::NUM_FREE_PIECES];
        for op in pieces::oriented_pieces() {
            let slot = &mut piece_shapes[op.free_id as usize];
            if slot.is_empty() {
                *slot = op
                    .cells
                    .iter()
                    .map(|&(r, c)| (r as usize, c as usize))
                    .collect();
            }
        }
        let meta = StaticMetaJs {
            typ: "static_meta",
            piece_names: pieces::FREE_PIECE_NAMES.to_vec(),
            piece_shapes,
            start_cells: START_CELLS.to_vec(),
            engine_name: "engine",
            engine_version: blokus_core::version(),
        };
        serde_wasm_bindgen::to_value(&meta).unwrap()
    }
}

impl Default for WasmGame {
    fn default() -> Self {
        Self::new()
    }
}
