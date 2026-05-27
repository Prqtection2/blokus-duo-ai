"""Diagnostics: scripts to investigate engine behavior on specific positions.

Used to answer questions like "is the engine getting boxed in because the
search is too shallow (horizon) or because the eval misjudges the position?"
Output gets saved under `diagnostics/games/` so individual failures can be
turned into permanent regression tests.
"""
