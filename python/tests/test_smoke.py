"""Smoke test: confirm the maturin-built `blokus` extension is importable
and exposes a non-empty version string."""

import blokus


def test_version_non_empty():
    v = blokus.version()
    assert isinstance(v, str) and v, f"unexpected version: {v!r}"


if __name__ == "__main__":
    test_version_non_empty()
    print(f"OK: blokus.version() = {blokus.version()!r}")
