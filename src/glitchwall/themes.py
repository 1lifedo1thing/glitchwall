"""The looks the wallpaper knows. The page (wallpaper/index.html) holds the actual colors;
these names must match it and LivelyProperties.json (tests/test_themes.py checks).

The first eight follow the palettes and personalities of SUB/WAVE's themes
(https://github.com/perminder-klair/subwave, MIT)."""
from __future__ import annotations

THEMES = {
    "phosphor": "green phosphor terminal at night: green on black glass, amber for anything live",
    "drafting": "blueprint: chalk-white linework on cobalt over a drawing grid",
    "newsprint": "cream newsprint, black ink, hot vermilion, a page setting itself under the cards",
    "nightprint": "charcoal newsprint, warm off-white ink, the same vermilion and the same page",
    "neon": "near-black neon: cyan ink, hot pink, dot-matrix titles, signs buzzing in the rain",
    "fieldops": "field-ops: khaki stock, hot orange, stencil type, a map with a route drawing itself",
    "duplicator": "risograph: blue and fluorescent pink drums, halftone, printed off true",
    "sleeve": "record sleeve: sepia paper, brown ink, mustard, a turning record",
    "matrix": "extra: bright green CRT, katakana falling at three depths",
    "amber": "extra: amber monochrome, a radar sweep finding contacts",
    "synthwave": "extra: pink and teal, sunset over a rolling grid",
    "msdos": "extra: blue DOS boxes with drop shadows on a text-mode desktop",
    "bsod": "extra: the old blue stop screen, white VGA text, grey title chip",
    "cyanotype": "extra: a sun print in Prussian blue, fern and leaf silhouettes developing",
    "notebook": "extra: ruled notebook paper, ballpoint handwriting, pencil doodles",
    "oracle": "extra: an all-seeing machine's ring of jagged data on black, one signal red",
    "orb": "extra: the machine itself, a ribbed black sphere studded with red lights, sunk in the floor of a vast, empty data hall",
    "scope": "extra: a lab oscilloscope, a blue-white beam sweeping a graticule, yellow-green afterglow",
    "chalkboard": "extra: a blackboard, chalk handwriting, working and diagrams written up and wiped",
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
    "orb": "a ribbed black sphere in a vast, dark data hall, rows of racks blinking into the haze, readouts on its parts",
    "cascade": "katakana falling at three depths, white-hot heads, glyphs changing as they fall",
    "scope": "an oscilloscope graticule, one beam sweeping it, the trace fading slowly; the signal changes every few sweeps",
    "radar": "an amber radar screen: a sweep turning, land clutter and slow contacts lit as it passes",
    "textmode": "a text-mode desktop: shaded backdrop, menu bar with a clock, a block mouse cursor",
    "broadsheet": "a newspaper page ghosted under the cards, setting itself line by line",
    "signs": "neon signs buzzing on a wet brick wall in the rain, a letter that won't stay lit",
    "topo": "a field map: contour lines, grid references, a route drawing itself to an objective",
    "sunprint": "fern and leaf silhouettes developing pale in a sun print",
    "chalk": "a blackboard: equations, plots and flowcharts written up in chalk, then wiped",
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
PALETTES = {"theme": "the theme's own colors", **{k: f"the colors of {k}" for k in THEMES}}
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
    "eclipse": "a curved shadow sweeps off it; another sweeps over it",
    "trace": "swept in left to right by the beam; fades out like an afterglow",
    "chalk": "written in a few letters at a time; smudged sideways and wiped",
    "slide": "slides in from the nearer side of the screen; out across to the other",
    "fade": "a soft fade in from a blur; out the same way",
    "dock": "docks beside the last card instead of anywhere; shrinks away",
    "toast": "slides in from the right onto a pile in the corner, the others moving up",
    "zoom": "comes up out of the distance; rushes past on the way out",
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
