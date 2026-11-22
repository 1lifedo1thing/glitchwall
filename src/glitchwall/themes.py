"""The looks the wallpaper knows. They all live in wallpaper/looks.js, shared with the page and the
native renderer: one place to add a theme, background, font or transition.

The first eight themes follow the palettes and personalities of SUB/WAVE's themes
(https://github.com/perminder-klair/subwave, MIT)."""
from __future__ import annotations

import json
from pathlib import Path

LOOKS_FILE = Path(__file__).resolve().parent / "wallpaper" / "looks.js"


def load(path: Path = LOOKS_FILE) -> dict:
    """looks.js is a script for the page, but what it assigns to window.LOOKS is plain JSON."""
    text = path.read_text(encoding="utf-8")
    start = text.rindex("window.LOOKS =") + len("window.LOOKS =")
    return json.loads(text[start:text.rindex(";")])


LOOKS = load()
THEMES = {k: t["about"] for k, t in LOOKS["themes"].items()}
RAINS = {"theme": "whatever the theme uses", **{k: b["about"] for k, b in LOOKS["backgrounds"].items()}}
FONTS = {"theme": "the theme's own pair (title + text)", **{k: f["about"] for k, f in LOOKS["fonts"].items()}}
PALETTE_NOTES = {k: t["palette"] for k, t in LOOKS["themes"].items()}   # for borrowing a theme's colors under another
PALETTES = {"theme": "the theme's own colors", **PALETTE_NOTES}
TRANSITIONS = {"theme": "whatever the theme uses", **{k: a["about"] for k, a in LOOKS["transitions"].items()}}# a theme is a bundle; every part of it can be swapped on its own
KINDS = {"theme": THEMES, "palette": PALETTES, "rain": RAINS, "font": FONTS, "transition": TRANSITIONS}


def look(s: dict) -> dict:
    """The theme/rain/font part of the settings, cleaned up, as sent to the page."""
    out = {}
    for k, names in KINDS.items():
        v = str(s.get(k, "")).lower()
        out[k] = v if v in names else ("phosphor" if k == "theme" else "theme")
    return out
