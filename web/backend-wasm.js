// WebAssembly transport: runs the engine entirely in-browser, no server.
// Exposes the same interface main.js expects from window.BACKEND. The engine
// search blocks its thread, so after the human's move we paint first (via a
// short timeout) and then run the search.

import init, { WasmGame } from "./pkg/blokus_wasm.js";

// Per-move thinking time. Higher = stronger but the tab is unresponsive for
// that long during the engine's turn (the search runs on the main thread).
const TIME_BUDGET_MS = 1500;

export async function createWasmBackend() {
  await init();
  const game = new WasmGame();
  game.setTimeBudgetMs(TIME_BUDGET_MS);

  let handlers = null;
  let thinking = false;

  function pushState() {
    handlers.onState(game.serialize());
  }

  function runEngineThenPush() {
    thinking = true;
    // Yield to the browser so the human's move and the "engine thinking…"
    // label paint before the blocking search starts.
    setTimeout(() => {
      try {
        game.playEngineUntilHumansTurn();
      } finally {
        thinking = false;
        pushState();
      }
    }, 30);
  }

  return {
    init(h) {
      handlers = h;
      handlers.onStaticMeta(game.staticMeta());
      pushState();
    },
    newGame(humanSide) {
      game.newGame(humanSide);
      pushState();
      runEngineThenPush(); // no-op if the human moves first
    },
    attemptMove(pieceId, cells) {
      if (thinking) return;
      if (!game.attemptHumanMove(pieceId, cells)) {
        handlers.onError("Not a legal placement.");
        return;
      }
      pushState();
      runEngineThenPush();
    },
    pass() {
      if (thinking) return;
      if (!game.humanPass()) {
        handlers.onError("Can't pass right now.");
        return;
      }
      pushState();
      runEngineThenPush();
    },
  };
}
