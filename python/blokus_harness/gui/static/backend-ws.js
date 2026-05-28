// WebSocket transport for the FastAPI server (native engine).
// Defines window.BACKEND consumed by main.js. Loaded as a classic script
// before main.js, so window.BACKEND exists by the time main.js runs.

(function () {
  let ws = null;
  let handlers = null;

  function route(ev) {
    const msg = JSON.parse(ev.data);
    if (msg.type === "static_meta") handlers.onStaticMeta(msg);
    else if (msg.type === "state") handlers.onState(msg);
    else if (msg.type === "rejected") handlers.onError(`Rejected: ${msg.reason}`);
    else if (msg.type === "error") handlers.onError(`Error: ${msg.message}`);
    else if (msg.type === "info") handlers.onInfo(msg.message);
  }

  function connect() {
    const wsUrl = `ws://${location.host}/ws`;
    ws = new WebSocket(wsUrl);
    ws.addEventListener("open", () => handlers.onInfo("Connected."));
    ws.addEventListener("close", () => {
      handlers.onError("Disconnected — reconnecting…");
      setTimeout(connect, 1500);
    });
    ws.addEventListener("message", route);
  }

  function send(obj) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(obj));
    }
  }

  window.BACKEND = {
    init(h) {
      handlers = h;
      connect();
    },
    newGame(humanSide) {
      send({ type: "new_game", human_side: humanSide });
    },
    attemptMove(pieceId, cells) {
      send({ type: "attempt_move", piece_id: pieceId, cells });
    },
    pass() {
      send({ type: "pass" });
    },
    dumpPosition() {
      send({ type: "dump_position" });
    },
  };
})();
