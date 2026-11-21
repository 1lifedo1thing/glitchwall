"""The looks the wallpaper knows. The page (wallpaper/index.html) holds the actual colors;
these names must match it and LivelyProperties.json (tests/test_themes.py checks)."""
from __future__ import annotations

THEMES = {
    "phosphor": "green CRT phosphor, pixel rain (the original)",
    "amber": "amber monochrome terminal",
    "cyberpunk": "magenta and cyan neon, glyph rain",
    "synthwave": "pink and teal, sunset over a rolling grid",
    "msdos": "blue DOS boxes with drop shadows, no glow",
    "bsod": "flat white on blue, a big :(",
    "ice": "cold blue glow, drifting snow",
    "paper": "light mode: ink on paper, typewriter font",
}
RAINS = {
    "theme": "whatever the theme uses",
    "pixel": "tiny bit-glyphs falling in columns",
    "code": "katakana and digits, bright heads",
    "stars": "flying through a starfield",
    "static": "dead-channel noise",
    "snow": "drifting flakes",
    "grid": "synthwave sun and perspective grid",
    "none": "plain background",
}
FONTS = {
    "theme": "whatever the theme uses",
    "departure": "Departure Mono (bundled)",
    "cascadia": "Cascadia Mono",
    "consolas": "Consolas",
    "lucida": "Lucida Console",
    "courier": "Courier New",
    "segoe": "Segoe UI",
    "mono": "the system monospace font",
}
KINDS = {"theme": THEMES, "rain": RAINS, "font": FONTS}


def look(s: dict) -> dict:
    """The theme/rain/font part of the settings, cleaned up, as sent to the page."""
    out = {}
    for k, names in KINDS.items():
        v = str(s.get(k, "")).lower()
        out[k] = v if v in names else ("phosphor" if k == "theme" else "theme")
    return out
