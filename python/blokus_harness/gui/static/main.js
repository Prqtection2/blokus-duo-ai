// Blokus Duo GUI client.
// All rules come from the engine via legal_moves; we just render and forward.
//
// Transport-agnostic: every interaction goes through a global `BACKEND` object
// defined by a backend script loaded before this one. Two backends exist:
//   - backend-ws.js   : talks to the FastAPI server over WebSocket (native engine)
//   - backend-wasm.js : runs the engine in-browser via WebAssembly (no server)
// BACKEND must provide: init(handlers), newGame(side), attemptMove(pid, cells),
// pass(), and optionally dumpPosition(). `handlers` = {onStaticMeta, onState,
// onInfo, onError}.

const BOARD_SIZE = 14;
const CELL = 36;                  // pixels per cell on the board canvas
const TRAY_CELL = 14;             // pixels per cell for tray icons
const PLAYER_COLOR = ["#ff6a00", "#7c3aed"];
const PLAYER_SOFT = ["rgba(255, 106, 0, 0.45)", "rgba(124, 58, 237, 0.45)"];

const els = {
  board: document.getElementById("board"),
  newGame: document.getElementById("new-game"),
  humanSide: document.getElementById("human-side"),
  pass: document.getElementById("pass-btn"),
  dumpPos: document.getElementById("dump-pos-btn"),
  heatmap: document.getElementById("show-heatmap"),
  message: document.getElementById("message"),
  engineMeta: document.getElementById("engine-meta-body"),
  engineLabel: document.getElementById("engine-label"),
  scores: [document.getElementById("score-0"), document.getElementById("score-1")],
  squares: [document.getElementById("squares-0"), document.getElementById("squares-1")],
  turn: document.getElementById("turn-label"),
  ply: document.getElementById("ply-label"),
  trays: [document.getElementById("tray-0"), document.getElementById("tray-1")],
};

const ctx = els.board.getContext("2d");

// ───────── App state ─────────
let pieceBaseShapes = null;        // [free_id] -> [[r,c], ...]
let pieceNames = null;             // [free_id] -> "L5" etc.
let pieceVariants = null;          // [free_id] -> [{cells, w, h, anchorIdx}]
let currentState = null;           // last `state` message from server
let legalMoveSet = null;           // Set of "pid|r,c;r,c;..." keys
let legalByPiece = null;           // Map<free_id, Set<keyOf(cells)>>
let selectedPiece = null;          // {pieceId, variantIdx} or null
let hoverCell = null;              // [r, c] or null

// ───────── Geometry helpers ─────────
function normalize(cells) {
  let minR = Infinity, minC = Infinity;
  for (const [r, c] of cells) { if (r < minR) minR = r; if (c < minC) minC = c; }
  return cells
    .map(([r, c]) => [r - minR, c - minC])
    .sort((a, b) => (a[0] - b[0]) || (a[1] - b[1]));
}
function rotateCW(cells)  { return normalize(cells.map(([r, c]) => [c, -r])); }
function flipH(cells)     { return normalize(cells.map(([r, c]) => [r, -c])); }
function cellsKey(cells) {
  return cells.slice().sort((a, b) => (a[0] - b[0]) || (a[1] - b[1]))
    .map(([r, c]) => `${r},${c}`).join(";");
}
function variantKey(cells) { return cellsKey(cells); }
function sameCells(a, b) { return cellsKey(a) === cellsKey(b); }

function computeVariants(baseCells) {
  const seen = new Set();
  const variants = [];
  let current = normalize(baseCells);
  for (let i = 0; i < 4; i++) {
    const k = variantKey(current);
    if (!seen.has(k)) { seen.add(k); variants.push(current); }
    const reflected = flipH(current);
    const rk = variantKey(reflected);
    if (!seen.has(rk)) { seen.add(rk); variants.push(reflected); }
    current = rotateCW(current);
  }
  return variants.map((cells) => {
    const w = Math.max(...cells.map(([r, c]) => c)) + 1;
    const h = Math.max(...cells.map(([r, c]) => r)) + 1;
    return { cells, w, h };
  });
}

// ───────── Rendering ─────────
function clearBoard() {
  ctx.clearRect(0, 0, els.board.width, els.board.height);
}
function drawGrid() {
  ctx.strokeStyle = "#d6d6d6";
  ctx.lineWidth = 1;
  for (let i = 0; i <= BOARD_SIZE; i++) {
    ctx.beginPath();
    ctx.moveTo(i * CELL + 0.5, 0); ctx.lineTo(i * CELL + 0.5, BOARD_SIZE * CELL);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(0, i * CELL + 0.5); ctx.lineTo(BOARD_SIZE * CELL, i * CELL + 0.5);
    ctx.stroke();
  }
}
function fillCell(r, c, fill, stroke) {
  ctx.fillStyle = fill;
  ctx.fillRect(c * CELL + 1, r * CELL + 1, CELL - 2, CELL - 2);
  if (stroke) {
    ctx.strokeStyle = stroke; ctx.lineWidth = 1.5;
    ctx.strokeRect(c * CELL + 1.5, r * CELL + 1.5, CELL - 3, CELL - 3);
  }
}
function drawStartMarkers(startCells) {
  ctx.fillStyle = "#b4b4b4";
  for (const [r, c] of startCells) {
    const cx = c * CELL + CELL / 2;
    const cy = r * CELL + CELL / 2;
    ctx.beginPath();
    ctx.arc(cx, cy, 4, 0, Math.PI * 2);
    ctx.fill();
  }
}
// Heatmap colors for the piece-coverage partition. Stones (codes 0/1) draw
// at full color separately; this table is for empty cells only.
//
// New semantics (2026-05-27, after the eval rewrite to piece-aware territory):
//   2 SAFE_P0  -> only P0 has a legal placement covering this cell (strong
//                 single-player claim). Vibrant orange.
//   3 SAFE_P1  -> only P1 has a legal placement covering this cell. Vibrant
//                 purple.
//   6 TIED     -> both players can legally cover -- whoever moves first wins
//                 it. Neutral gray.
//   7 UNREACHABLE -> neither can cover this turn. No fill.
// Codes 4/5 are no longer produced by the partition (kept for visual safety
// in case an older snapshot is loaded).
const HEATMAP_FILL = {
  2: "rgba(255, 106, 0, 0.50)",
  3: "rgba(124, 58, 237, 0.50)",
  4: "rgba(255, 106, 0, 0.50)",
  5: "rgba(124, 58, 237, 0.50)",
  6: "rgba(160, 160, 160, 0.45)",
  7: null,
};

function drawHeatmap() {
  const partition = currentState && currentState.partition;
  if (!partition || partition.length !== 14 * 14) return;
  for (let r = 0; r < BOARD_SIZE; r++) {
    for (let c = 0; c < BOARD_SIZE; c++) {
      const code = partition[r * BOARD_SIZE + c];
      // Codes 0/1 are stones — drawn at full color in a later pass.
      if (code === 0 || code === 1) continue;
      const fill = HEATMAP_FILL[code];
      if (!fill) continue;
      ctx.fillStyle = fill;
      ctx.fillRect(c * CELL + 1, r * CELL + 1, CELL - 2, CELL - 2);
    }
  }
}

function drawBoard() {
  clearBoard();
  drawGrid();
  if (!currentState) return;

  drawStartMarkers(staticMeta?.start_cells || [[4, 4], [9, 9]]);

  // Optional territory heatmap (drawn UNDER the stones).
  if (els.heatmap && els.heatmap.checked) {
    drawHeatmap();
  }

  // Player stones.
  for (let p = 0; p < 2; p++) {
    for (const [r, c] of currentState.cells[p]) {
      fillCell(r, c, PLAYER_COLOR[p], null);
    }
  }

  // Last move highlight.
  if (currentState.last_move && !currentState.last_move.passed) {
    ctx.lineWidth = 2.5;
    ctx.strokeStyle = "#fff59d";
    for (const [r, c] of currentState.last_move.cells) {
      ctx.strokeRect(c * CELL + 2, r * CELL + 2, CELL - 4, CELL - 4);
    }
  }

  // Hover preview.
  if (selectedPiece && hoverCell && isHumansTurn()) {
    const placement = previewPlacement();
    if (placement) {
      const ok = placement.inBounds && placement.legal;
      const tint = ok
        ? "rgba(56, 142, 60, 0.55)"
        : "rgba(211, 47, 47, 0.45)";
      for (const [r, c] of placement.cells) {
        if (r < 0 || r >= BOARD_SIZE || c < 0 || c >= BOARD_SIZE) continue;
        fillCell(r, c, tint, null);
      }
    }
  }
}

function drawPieceOnCanvas(canvas, cells, color, cell = TRAY_CELL) {
  const w = Math.max(...cells.map(([r, c]) => c)) + 1;
  const h = Math.max(...cells.map(([r, c]) => r)) + 1;
  canvas.width = w * cell + 2;
  canvas.height = h * cell + 2;
  const c2 = canvas.getContext("2d");
  c2.fillStyle = color;
  for (const [r, c] of cells) {
    c2.fillRect(c * cell + 1, r * cell + 1, cell - 1, cell - 1);
  }
}

function renderTrays() {
  if (!currentState) return;
  for (let p = 0; p < 2; p++) {
    els.trays[p].innerHTML = "";
    const left = new Set(currentState.pieces_left[p]);
    for (let pid = 0; pid < pieceBaseShapes.length; pid++) {
      const card = document.createElement("div");
      card.className = "piece-card";
      if (!left.has(pid)) card.classList.add("used");
      const variants = pieceVariants[pid];
      const isSelected = selectedPiece
        && selectedPiece.pieceId === pid
        && selectedPiece.playerSide === p;
      if (isSelected) card.classList.add("selected");

      const cells = isSelected
        ? variants[selectedPiece.variantIdx].cells
        : variants[0].cells;
      const canvas = document.createElement("canvas");
      canvas.className = "piece-canvas";
      drawPieceOnCanvas(canvas, cells, PLAYER_COLOR[p]);
      card.appendChild(canvas);
      const label = document.createElement("div");
      label.className = "piece-name";
      label.textContent = pieceNames[pid];
      card.appendChild(label);

      if (left.has(pid) && p === currentState.human_side) {
        card.addEventListener("click", () => selectPiece(pid));
      }
      els.trays[p].appendChild(card);
    }
  }
}

// ───────── Move logic ─────────
function isHumansTurn() {
  return currentState
    && !currentState.terminal
    && currentState.side_to_move === currentState.human_side;
}

function previewPlacement() {
  if (!selectedPiece || !hoverCell) return null;
  const variant = pieceVariants[selectedPiece.pieceId][selectedPiece.variantIdx];
  const [hr, hc] = hoverCell;
  // Anchor convention: hovered cell = the piece's bounding-box top-left.
  // Variant cells are normalized so min row == min col == 0.
  const cells = variant.cells.map(([r, c]) => [r + hr, c + hc]);
  const inBounds = cells.every(
    ([r, c]) => r >= 0 && r < BOARD_SIZE && c >= 0 && c < BOARD_SIZE
  );
  let legal = false;
  if (inBounds && legalByPiece) {
    const key = cellsKey(cells);
    const set = legalByPiece.get(selectedPiece.pieceId);
    legal = !!(set && set.has(key));
  }
  return { cells, inBounds, legal };
}

function selectPiece(pieceId) {
  selectedPiece = {
    pieceId,
    variantIdx: 0,
    playerSide: currentState.human_side,
  };
  drawBoard();
  renderTrays();
}

function rotateOrFlip(transform) {
  if (!selectedPiece) return;
  const pid = selectedPiece.pieceId;
  const variants = pieceVariants[pid];
  const current = variants[selectedPiece.variantIdx].cells;
  const transformed = transform(current);
  const targetKey = variantKey(transformed);
  const newIdx = variants.findIndex(v => variantKey(v.cells) === targetKey);
  if (newIdx >= 0) {
    selectedPiece.variantIdx = newIdx;
    drawBoard();
    renderTrays();
  }
}

function tryPlace() {
  const placement = previewPlacement();
  if (!placement || !placement.inBounds || !placement.legal) {
    flashMessage("Not a legal placement.", true);
    return;
  }
  BACKEND.attemptMove(selectedPiece.pieceId, placement.cells);
  selectedPiece = null;
}

// ───────── UI events ─────────
els.board.addEventListener("mousemove", (e) => {
  const rect = els.board.getBoundingClientRect();
  const x = e.clientX - rect.left;
  const y = e.clientY - rect.top;
  const c = Math.floor(x / CELL);
  const r = Math.floor(y / CELL);
  if (r < 0 || r >= BOARD_SIZE || c < 0 || c >= BOARD_SIZE) {
    if (hoverCell) { hoverCell = null; drawBoard(); }
    return;
  }
  if (!hoverCell || hoverCell[0] !== r || hoverCell[1] !== c) {
    hoverCell = [r, c];
    drawBoard();
  }
});
els.board.addEventListener("mouseleave", () => { hoverCell = null; drawBoard(); });
els.board.addEventListener("click", () => {
  if (!isHumansTurn()) {
    flashMessage("Wait for your turn.", true);
    return;
  }
  if (!selectedPiece) {
    flashMessage("Pick a piece from your tray first.", true);
    return;
  }
  tryPlace();
});

document.addEventListener("keydown", (e) => {
  if (e.key === "r" || e.key === "R") { rotateOrFlip(rotateCW); }
  else if (e.key === "f" || e.key === "F") { rotateOrFlip(flipH); }
  else if (e.key === "Escape") { selectedPiece = null; drawBoard(); renderTrays(); }
});

els.newGame.addEventListener("click", () => {
  const humanSide = parseInt(els.humanSide.value, 10);
  selectedPiece = null;
  hoverCell = null;
  BACKEND.newGame(humanSide);
});
els.pass.addEventListener("click", () => {
  BACKEND.pass();
});
if (els.dumpPos) {
  els.dumpPos.addEventListener("click", () => {
    if (BACKEND.dumpPosition) BACKEND.dumpPosition();
  });
}
if (els.heatmap) {
  els.heatmap.addEventListener("change", () => drawBoard());
}

function flashMessage(msg, isError = false) {
  els.message.textContent = msg;
  els.message.style.color = isError ? "#c62828" : "";
  els.message.classList.remove("muted");
  if (!isError) {
    setTimeout(() => {
      els.message.classList.add("muted");
    }, 1200);
  }
}

// ───────── State handling ─────────
let staticMeta = null;

function onStaticMeta(msg) {
  staticMeta = msg;
  pieceNames = msg.piece_names;
  pieceBaseShapes = msg.piece_shapes;
  pieceVariants = pieceBaseShapes.map(computeVariants);
  els.engineLabel.textContent = `· vs ${msg.engine_name} (engine ${msg.engine_version})`;
}

function buildLegalIndex(legal_moves) {
  legalByPiece = new Map();
  for (const m of legal_moves) {
    if (!legalByPiece.has(m.piece_id)) legalByPiece.set(m.piece_id, new Set());
    legalByPiece.get(m.piece_id).add(cellsKey(m.cells));
  }
}

function onState(msg) {
  currentState = msg;
  buildLegalIndex(msg.legal_moves);
  // Clear selection if it's not the human's turn anymore.
  if (!isHumansTurn() && selectedPiece) selectedPiece = null;
  // Update HUD.
  for (let p = 0; p < 2; p++) {
    els.scores[p].textContent = msg.scores[p];
    els.squares[p].textContent = `${msg.squares_left[p]} squares left`;
  }
  els.ply.textContent = `Ply: ${msg.ply}`;
  if (msg.terminal) {
    const winner = msg.scores[0] > msg.scores[1] ? 0
      : msg.scores[1] > msg.scores[0] ? 1 : -1;
    const label = winner === -1 ? "Draw" : `Player ${winner} wins`;
    els.turn.textContent = `Game over — ${label}`;
  } else {
    const stm = msg.side_to_move;
    const youOrEngine = stm === msg.human_side ? "your move" : "engine thinking…";
    els.turn.textContent = `Turn: ${stm === 0 ? "Orange" : "Purple"} (${youOrEngine})`;
  }

  // Engine meta panel.
  if (msg.engine_meta) {
    const m = msg.engine_meta;
    const lines = [
      `move: ${m.passed ? "(pass)" : m.move_repr}`,
      `eval: ${m.eval.toFixed(2)}`,
      `depth: ${m.depth}`,
      `nodes: ${m.nodes}`,
      `time: ${m.time_ms.toFixed(1)} ms`,
    ];
    els.engineMeta.textContent = lines.join("\n");
    els.engineMeta.classList.remove("muted");
  } else {
    els.engineMeta.textContent = "— no move yet";
    els.engineMeta.classList.add("muted");
  }

  // Pass-button enabling: only legal when it's human's turn AND no legal moves.
  els.pass.disabled = !(isHumansTurn() && msg.legal_moves.length === 0);

  renderTrays();
  drawBoard();
}

BACKEND.init({
  onStaticMeta,
  onState,
  onInfo: (m) => flashMessage(m, false),
  onError: (m) => flashMessage(m, true),
});
drawBoard();
