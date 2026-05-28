# Blokus Duo AI

A Blokus Duo engine with a browser GUI. The performance-critical engine
(board, move generation, search, evaluation) is written in Rust; orchestration,
the web GUI, tournaments, and tuning are in Python. The two are bridged with
[PyO3](https://pyo3.rs) + [maturin](https://www.maturin.rs), so Python can
`import blokus` and call the Rust engine directly.

## What it is

- **Game:** Blokus Duo — two players, 14×14 board, 21 polyomino pieces each,
  fixed start cells (orange (4,4), purple (9,9)).
- **Engine:** iterative-deepening negamax alpha-beta with PVS, aspiration
  windows, late-move reductions, a transposition table, and an exact endgame
  solver. Evaluation is a weighted sum of placed squares, live corners,
  piece-aware territory, and piece liability. For a full ground-up explanation
  of how moves are chosen, see **[ALGORITHM.md](ALGORITHM.md)**.
- **GUI:** play against the engine in a browser. Includes a "territory" heatmap
  showing which cells each side can legally claim, and a "Save position" button
  for offline diagnosis.

## Layout

```
crates/
  blokus-core/        Pure-Rust engine
    src/bitboard.rs    256-bit bitboard ([u64; 4]) over a padded 16×16 grid
    src/board.rs       Board state + Move; make/unmake; Zobrist hashing
    src/pieces.rs      21 free pieces, 91 oriented variants, precomputed placements
    src/movegen.rs     Corner-anchored legal move generation; coverable_cells
    src/eval.rs        Weighted-feature evaluation + territory partition
    src/search.rs      Alpha-beta search and all its optimizations
  blokus-py/
    src/lib.rs         PyO3 bindings exposing the engine as the `blokus` module
  blokus-wasm/
    src/lib.rs         wasm-bindgen bindings: run the engine in a browser (WasmGame)
web/                   Static site for the WebAssembly build (GitHub Pages)
  index.html           Boots the WASM backend, then loads the shared renderer
  backend-wasm.js      In-browser transport (runs the engine via WASM)
  serve.py             Local test server with correct MIME types
python/
  blokus_harness/
    players.py         EnginePlayer, GreedyPlayer, BlockerPlayer, ...
    harness.py         Play a full game between two players
    tournament.py      Run many games between two engines
    gui/server.py      FastAPI + WebSocket server
    gui/static/        Browser frontend (HTML5 canvas, no framework)
  tuning/              SPRT, parallel match runner, coordinate-descent tuner
  diagnostics/         Position replay, per-term eval breakdown, depth benchmarks
  tests/               pytest suite
  run_gui.py           Launch the GUI
  run_tune.py          Launch weight tuning
```

## Prerequisites (Windows)

- Rust (rustup, MSVC toolchain) — `winget install Rustlang.Rustup`
- Visual Studio 2022 Build Tools with the C++ workload + Windows 11 SDK
- A real Python venv at `.venv` (do **not** use the Microsoft Store Python —
  PyO3/maturin can't introspect through the WindowsApps shim)
- `maturin` installed in the venv

## Build

The Rust engine must be compiled and installed into the venv before Python can
import it. From the repo root (PowerShell):

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cmd /c "`"$vcvars`" && set `"VIRTUAL_ENV=C:\code\blokus-ai\.venv`" && set `"PATH=C:\code\blokus-ai\.venv\Scripts;%PATH%`" && maturin develop --release"
```

Re-run this whenever you change a `.rs` file. Pure-Python changes need no rebuild.

> If the rebuild fails with "could not overwrite the installed extension module
> because it is in use," stop any running Python process that imported `blokus`
> (e.g. the GUI server), delete `.venv\Lib\site-packages\~-okus` if present, and
> retry.

## Run the GUI

```powershell
.\.venv\Scripts\python.exe python\run_gui.py
```

Open <http://127.0.0.1:8765/>. Pick your side, click a piece in your tray
(**R** to rotate, **F** to flip), and click the board to place. Toggle **Show
territory** to see the coverage heatmap:

- **Orange / purple fill** — only that player can legally cover the cell this turn.
- **Gray** — both players can cover it (a race; whoever moves first wins it).
- **No fill** — neither can cover it this turn.

Engine strength is configured in `_default_engine_factory()` in
[gui/server.py](python/blokus_harness/gui/server.py) (`time_budget_ms` and the
eval `weights`).

## Play in the browser (no install) — WebAssembly build

The engine also compiles to WebAssembly so it can run entirely in a browser with
no server. The static site lives in [web/](web/); a GitHub Actions workflow builds
the WASM and publishes it to GitHub Pages on every push to `main`.

**One-time setup:** in the repo's **Settings → Pages**, set **Source** to
**GitHub Actions**. After the next push (or a manual run of the "Deploy WASM site
to GitHub Pages" workflow), the site is live at
`https://<user>.github.io/<repo>/`.

**Build and test it locally:**

```powershell
# 1. Build the engine to WASM and generate JS bindings
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
cargo build --release --target wasm32-unknown-unknown -p blokus-wasm
wasm-bindgen target\wasm32-unknown-unknown\release\blokus_wasm.wasm --out-dir web\pkg --target web

# 2. Copy the shared frontend into web/ (CI does this automatically)
Copy-Item python\blokus_harness\gui\static\main.js  web\main.js
Copy-Item python\blokus_harness\gui\static\style.css web\style.css

# 3. Serve it (correct MIME types for ES modules + WASM)
.\.venv\Scripts\python.exe web\serve.py
```

Then open <http://localhost:8080>. Prerequisites: `rustup target add
wasm32-unknown-unknown` and `cargo install wasm-bindgen-cli --version 0.2.100`.

The browser build runs the search on the main thread, so the tab is briefly
unresponsive (~1.5s, configurable in [web/backend-wasm.js](web/backend-wasm.js))
during the engine's turn. The local FastAPI GUI (above) uses the faster native
engine and is the better tool for development and diagnostics.

## Tests

```powershell
$vcvars = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
# Rust
cmd /c "`"$vcvars`" && set `"PYO3_PYTHON=C:\code\blokus-ai\.venv\Scripts\python.exe`" && cargo test --workspace"
# Python (after a maturin build)
$env:PYTHONPATH = "c:\code\blokus-ai\python"; .\.venv\Scripts\python.exe -m pytest python\tests
```

## Diagnosing a move you disagree with

1. In the GUI, click **Save position** right after the engine moves. It writes a
   JSON snapshot to `python/diagnostics/positions/`.
2. Replay it offline, optionally comparing against the move you'd have played at
   cell `R,C`:

```powershell
$env:PYTHONPATH = "c:\code\blokus-ai\python"
.\.venv\Scripts\python.exe python\diagnostics\replay_position.py --alt R,C
```

This prints the engine's choice, the per-term eval breakdown (which feature drove
the decision), and an ASCII territory partition.

## How the engine picks a move

For the position in front of it, the engine explores the game tree: play a move,
consider every reply, consider every counter-reply, and so on, as deep as the time
budget allows. At the leaves it scores the position with the evaluation function
(a weighted sum of features). Assuming both sides play their best at every level
(alpha-beta minimax), it picks the move leading to the best outcome. Bitboards,
the transposition table, and the search reductions exist to make this fast enough
to do millions of times per move.

For the complete, ground-up walkthrough — game trees, negamax, alpha-beta,
iterative deepening, aspiration windows, PVS, late-move reductions, the
transposition table, move ordering, the endgame solver, and how the eval weights
were tuned — read **[ALGORITHM.md](ALGORITHM.md)**.
