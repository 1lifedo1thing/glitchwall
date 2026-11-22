"""The looks the wallpaper knows. The page (wallpaper/index.html) holds the actual colors;
these names must match it (tests/test_themes.py checks).

The first eight follow the palettes and personalities of SUB/WAVE's themes
(https://github.com/perminder-klair/subwave, MIT)."""
from __future__ import annotations

THEMES = {
    "phosphor": "lab oscilloscope at night: green phosphor on black glass, amber for anything live",
    "drafting": "blueprint: chalk-white linework on cobalt over a drawing grid",
    "newsprint": "cream newsprint, black ink, hot vermilion",
    "nightprint": "charcoal newsprint, warm off-white ink, the same vermilion",
    "neon": "near-black neon: cyan ink, hot pink, dot-matrix titles",
    "fieldops": "field-ops poster: khaki stock, hot-orange grid, stencil type",
    "duplicator": "risograph: blue and fluorescent pink drums, halftone, printed off true",
    "sleeve": "record sleeve: sepia paper, brown ink, mustard, a turning record",
    "matrix": "extra: bright green CRT, the original glitchwall look",
    "amber": "extra: amber monochrome terminal",
    "synthwave": "extra: pink and teal, sunset over a rolling grid",
    "msdos": "extra: blue DOS boxes with drop shadows",
    "bsod": "extra: the old blue stop screen, white VGA text, grey title chip",
    "cyanotype": "extra: a sun print in Prussian blue, paper grain, mid-century print",
    "notebook": "extra: ruled notebook paper, ballpoint handwriting, pencil doodles",
    "oracle": "extra: an all-seeing machine's ring of jagged data on black, one signal red",
}
RAINS = {
    "theme": "whatever the theme uses",
    "pixel": "tiny bit-glyphs falling in columns",
    "code": "katakana and digits, bright heads",
    "stars": "flying through a starfield",
    "static": "dead-channel noise",
    "snow": "drifting flakes",
    "grid": "synthwave sun and perspective grid",
    "draft": "drawing grid, shapes drafting and dimensioning themselves",
    "halftone": "a breathing two-color halftone field",
    "record": "a record turning in the corner",
    "jukebox": "a 50s jukebox arch, glowing bands, bubble tubes",
    "starburst": "atomic-age starbursts, boomerangs and orbits",
    "testcard": "an old broadcast test card with a slow roll",
    "print": "mid-century wallpaper: diamonds, dots, stars",
    "stop": "the old stop screen, faint, with a blinking cursor",
    "notebook": "ruled paper and red margin, pencil doodles sketching themselves",
    "oracle": "a great ring of jagged data strokes, slowly turning, always redrawing itself",
    "none": "plain background",
}
FONTS = {
    "theme": "the theme's own pair (title + text)",
    "departure": "Departure Mono (bundled)",
    "plex": "IBM Plex Mono (bundled)",
    "vt323": "VT323 (bundled)",
    "chakra": "Chakra Petch (bundled)",
    "doto": "Doto (bundled)",
    "stencil": "Saira Stencil One (bundled)",
    "serif": "Instrument Serif (bundled)",
    "georgia": "Gelasio (bundled; drawn to match Georgia)",
    "courierprime": "Courier Prime (bundled)",
    "dmmono": "DM Mono (bundled)",
    "fira": "Fira Code (bundled)",
    "overpass": "Overpass Mono (bundled)",
    "cascadia": "Cascadia Mono (bundled)",
    "consolas": "Inconsolata (bundled; close to Consolas)",
    "lucida": "Lucida Console (system font, not bundled)",
    "courier": "Courier Prime (bundled; same as courierprime)",
    "segoe": "Segoe UI (system font, not bundled)",
    "hand": "Caveat handwriting (bundled)",
    "mono": "the system monospace font",
}
PALETTE_NOTES = {   # what each theme's colors are, for borrowing them under another theme
    "phosphor": "phosphor green on black glass, amber for anything live",
    "drafting": "chalk white on cobalt blue",
    "newsprint": "black ink on cream paper, vermilion accents",
    "nightprint": "warm off-white on charcoal, vermilion accents",
    "neon": "cyan and hot pink on near-black",
    "fieldops": "khaki stock, dark ink, hot-orange accents",
    "duplicator": "riso blue and fluorescent pink on off-white paper",
    "sleeve": "brown ink and mustard on sepia paper",
    "matrix": "bright CRT green on black",
    "amber": "amber on black, one color only",
    "synthwave": "hot pink and teal on deep purple",
    "msdos": "white and yellow on DOS blue, grey shadows",
    "bsod": "white on stop-screen blue, a grey title chip",
    "cyanotype": "pale lines on Prussian blue",
    "notebook": "blue ballpoint and a red margin on ruled white paper",
    "oracle": "white data on black, one signal red",
    "orb": "black and graphite, studded with red lights",
    "scope": "a blue-white beam with yellow-green afterglow on black",
    "chalkboard": "chalk white on a dark board",
}
PALETTES = {"theme": "the theme's own colors", **{k: PALETTE_NOTES.get(k, f"{k}'s colors") for k in THEMES}}
TRANSITIONS = {
    "theme": "whatever the theme uses",
    "glitch": "torn slices and RGB split in, CRT collapse out",
    "crt": "the beam opens to a line, then the frame; collapses to a dot",
    "draft": "ruled in edge first; erased left to right",
    "press": "set onto the page; lifted off",
    "neon": "a tube flickering on; a last flicker and dark",
    "stamp": "stamped down crooked; torn off sideways",
    "riso": "a second ink lands off true, then settles",
    "sleeve": "slides up out of a sleeve; back down",
    "dos": "drawn a few lines at a time; gone at once",
    "write": "written in left to right; crumpled and tossed",
    "iris": "opens out from the centre like an aperture; closes to a point",
}
# a theme is a bundle; every part of it can be swapped on its own
KINDS = {"theme": THEMES, "palette": PALETTES, "rain": RAINS, "font": FONTS, "transition": TRANSITIONS}


def look(s: dict) -> dict:
    """The theme/rain/font part of the settings, cleaned up, as sent to the page."""
    out = {}
    for k, names in KINDS.items():
        v = str(s.get(k, "")).lower()
        out[k] = v if v in names else ("phosphor" if k == "theme" else "theme")
    return out
