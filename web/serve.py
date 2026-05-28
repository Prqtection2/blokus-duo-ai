"""Tiny static server for local testing of the WASM site.

Serves the `web/` directory with the correct MIME types for ES modules
(`.js`) and streaming WebAssembly (`.wasm`) — the stdlib defaults can be wrong
on Windows, which breaks module loading. Production (GitHub Pages) sets these
correctly on its own; this is only for local testing.

    python web/serve.py        # then open http://localhost:8080
"""

from __future__ import annotations

import http.server
import socketserver
from pathlib import Path

PORT = 8080
DIRECTORY = str(Path(__file__).resolve().parent)


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIRECTORY, **kwargs)

    def end_headers(self):
        # No caching, so rebuilds show up immediately during development.
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


Handler.extensions_map = {
    **http.server.SimpleHTTPRequestHandler.extensions_map,
    ".js": "text/javascript",
    ".mjs": "text/javascript",
    ".wasm": "application/wasm",
}


def main() -> None:
    with socketserver.TCPServer(("127.0.0.1", PORT), Handler) as httpd:
        print(f"Serving {DIRECTORY} at http://localhost:{PORT}")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
