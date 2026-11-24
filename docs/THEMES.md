<sub>[← back to the README](../README.md)</sub>

# Themes

![nineteen themes](themes.png)

Each theme is a finished bundle: colors, two fonts, a background, a texture and its own way of bringing cards
in and taking them away. Use one as it is, or swap any single part.

| Theme | Personality | Cards arrive / leave |
| --- | --- | --- |
| `phosphor` (default) | green phosphor terminal at night: green on black glass, amber for anything live, bit-glyph rain, CRT tube | beam opens / collapses to a dot |
| `drafting` | blueprint: chalk-white linework on cobalt, shapes drafting, dimensioning and hatching themselves, a title block in the corner | ruled in / erased |
| `newsprint` · `nightprint` | cream newsprint, black ink, hot vermilion, a broadsheet page setting itself line by line under the cards; the same at night on charcoal | pressed on / lifted off |
| `neon` | near-black neon: cyan ink, hot pink, dot-matrix titles, a rainy street at night in perspective: neon signs on a brick wall and hanging off it, street lamps, a signal flashing red at the crossing, the city far off, all of it mirrored in rippling puddles | tube flickers on / off |
| `fieldops` | field-ops: khaki stock, hot orange, stencil type, a contour map with a route drawing itself from waypoint to objective | stamped / torn off |
| `duplicator` | risograph: blue and fluorescent-pink inks, halftone, the pink drum a little off true | second ink lands off true |
| `sleeve` | record sleeve: sepia paper, brown ink, mustard, a record turning in the corner | slides out of the sleeve |
| `notebook` | ruled paper, red margin, ballpoint handwriting, pencil doodles | written in / crumpled and tossed |
| `bsod` | the old blue stop screen: white VGA text, grey title chips | drawn line by line / gone |
| `cyanotype` | a sun print in Prussian blue: fern and leaf silhouettes developing pale, brushed edges to the coating | fades in / fades out |
| `oracle` | the software of an all-seeing machine, its mind laid out as one great ring of jagged black data strokes on white paper, slowly turning and always redrawing itself, one signal red (the old dark oracle as a negative, the red kept red) | opens like an iris / closes to a point |
| `orb` | the hardware oracle runs on, the same machine seen whole: a ribbed black sphere sunk in the floor of a vast, empty, dark data hall, rows of racks blinking away into the haze behind it, tiny red lights flickering and streaming along its slats, readouts pointing at its parts as they turn into view | a curved shadow sweeps off / over it |
| `scope` | a lab oscilloscope: a blue-white beam sweeping a graticule, its trace glowing yellow-green and fading slowly, the signal changing every few sweeps, readouts at the corners | swept in by the beam / fades like an afterglow |
| `chalkboard` | a blackboard smudged with old lessons: equations, plots and little flowcharts written up in chalk, then wiped with the eraser | written in a few letters at a time / smudged and wiped |
| `matrix` · `amber` · `synthwave` · `msdos` | bright-green katakana falling at three depths, an amber radar sweep, a synthwave sunset, blue DOS boxes on a text-mode desktop | glitch · beam · slides across · docks beside the last one |

**Mix and match.** Five independent choices; `theme` means "whatever the theme uses":

```sh
glitchwall themes                                   # list everything, * marks what you use now
glitchwall config --theme drafting                  # pick a bundle; the running wallpaper switches within a second
glitchwall config --palette sleeve                  # ...but with another theme's colors
glitchwall config --rain starburst --font vt323     # ...any background, any font
glitchwall config --transition neon                 # ...any way of arriving and leaving
glitchwall config --palette theme --rain theme --font theme --transition theme   # back to the bundle
glitchwall demo --open --theme notebook             # try one without touching your setup
```

| Choice | Options |
| --- | --- |
| **Background** (`--rain`) | `pixel` bit-glyph rain · `code` katakana rain · `stars` · `static` · `snow` · `grid` synthwave sun · `draft` drawing grid · `halftone` two-ink dots · `record` · `jukebox` · `starburst` atomic age · `testcard` · `print` mid-century wallpaper · `stop` stop screen · `notebook` ruled paper · `oracle` ring of data · `orb` the machine in its data hall · `cascade` katakana at three depths · `scope` oscilloscope · `radar` · `textmode` DOS desktop · `broadsheet` newspaper page · `signs` neon in the rain · `topo` field map · `sunprint` ferns and leaves · `chalk` blackboard · `none` |
| **Font** | `departure` · `plex` · `vt323` · `chakra` · `doto` · `stencil` · `serif` · `courierprime` · `dmmono` · `fira` · `overpass` · `georgia` · `cascadia` · `consolas` · `lucida` · `courier` · `segoe` · `hand` · `mono` |
| **Transition** | `glitch` · `crt` · `draft` · `press` · `neon` · `stamp` · `riso` · `sleeve` · `dos` · `write` · `iris` · `eclipse` · `trace` · `chalk` · `slide` · `fade` · `dock` beside the last card · `toast` stacked in the corner · `zoom` |
| **Palette** | any theme name |

Fonts are bundled or already on your system; nothing is downloaded at run time. The sliders are
`glitchwall config --type-speed 1-20 --linger 2-30 --cards 1-6 --intensity 0-10 --glass 20-95`, and they
apply within a second. In a browser, URL parameters (`?theme=sleeve&rain=jukebox&transition=stamp`) win over
your settings, and `?cycle=10` steps through every theme. The MOTION slider sets how busy any background is (0 stills it).

### How the themes are built

A few rules every theme follows:

- **One whole look, not a skin.** Colors, two fonts (one for titles, one for the typing), a background, a
  texture over everything, film grain, and the way cards arrive and leave are chosen together. You can swap any
  one part; the bundle is the starting point.
- **Borrowed from real things, copied from none.** Each look starts from a real object or medium (a blueprint, a
  risograph, an oscilloscope, a chalkboard) and is drawn from scratch: nothing is a copy of a game, a film or a
  show. The first eight follow the palettes and personalities of SUB/WAVE's themes (see License & credits).
- **The cards come first.** The background is a backdrop for the tool calls, so every background stays darker or
  quieter than the cards, and nothing in it competes with the text you're meant to read.
- **Motion only where it means something.** A background moves as fast as its subject does and no faster: a
  record turns, a page sets line by line, a sun print develops slowly. That's why the frame rates below differ.
- **Fonts ship with glitchwall.** Every font is bundled under the SIL Open Font License, so a theme looks the same
  on every PC and nothing is downloaded at run time.
- **Safe to stream.** No real names or accounts in any background, and every number on screen is made up.
- **Four options per background.** Each background has exactly four settings in the settings window: enough to
  make it yours, and few enough to fit in one row and stay polished.

#### Theme specs

The frame rate is how often the background redraws. Cards are separate: they redraw at 60 fps only while one
arrives, leaves or slides (about half a second), and otherwise only when text changes. When nothing moves,
nothing is drawn at all.

| Theme | Background | Titles / typing fonts | Texture, grain | Background fps | Why that rate |
| --- | --- | --- | --- | --- | --- |
| `phosphor` | `pixel` bit-glyph rain | VT323 / IBM Plex Mono | CRT tube, 0.06 | 30 | falling glyphs need smooth motion to read as rain |
| `drafting` | `draft` drawing grid | Chakra Petch / IBM Plex Mono | none, 0.08 | 30 | there's always a line being ruled; slower looks jerky |
| `newsprint` · `nightprint` | `broadsheet` | Gelasio / DM Mono | none, 0.4 · 0.25 | 15 while setting, 1 while the page holds | type setting is steady, not fast; a finished page only needs its clock |
| `neon` | `signs` in the rain | Doto / Fira Code | scan lines, 0.12 | 24 | rain and tube flicker; film rate is enough for both |
| `fieldops` | `topo` field map | Saira Stencil One / Overpass Mono | none, 0.08 | 24 | the route draws itself slowly |
| `duplicator` | `halftone` | DM Mono / DM Mono | halftone, 0.08 | 12 (0 when the drift is still) | the field breathes; more frames add nothing |
| `sleeve` | `record` | Instrument Serif / Courier Prime | none, 0.4 | 30 (0 when the record rests) | a turning label shows every dropped frame |
| `matrix` | `cascade` katakana at three depths | Departure Mono | scan lines, none | 30 | fast falling columns |
| `amber` | `radar` | Departure Mono | scan lines, none | 30 | the sweep is the whole point |
| `synthwave` | `grid` | Departure Mono | scan lines, none | 30 | the grid rolls toward you |
| `msdos` | `textmode` DOS desktop | VT323 | scan lines, none | 12 while the mouse moves, 1 otherwise | only the pointer and the clock change |
| `bsod` | `stop` screen | VT323 | scan lines, none | about 2 (the cursor blink), 0 with no blink | a still screen with a blinking cursor |
| `cyanotype` | `sunprint` ferns and leaves | Instrument Serif / IBM Plex Mono | none, 0.35 | 15 | a print develops slowly |
| `notebook` | `notebook` ruled paper | Caveat | none, 0.22 | 30 while a doodle is drawn, 0 after | a pencil stroke needs smooth motion; a finished doodle needs none |
| `oracle` | `oracle` ring of data | Chakra Petch / DM Mono | none, 0.05 | 20 | strokes redraw constantly but slowly |
| `orb` | `orb` the machine in its hall | Chakra Petch / DM Mono | none, 0.07 | 20 | the busiest scene; lights flicker and the sphere turns slowly |
| `scope` | `scope` oscilloscope | Overpass Mono / IBM Plex Mono | CRT tube, 0.05 | 30 | a sweeping beam must look continuous |
| `chalkboard` | `chalk` blackboard | Caveat | none, 0.18 | 20 while writing or wiping | handwriting pace |

Every background drops to 0 fps when the MOTION slider is at 0. The eight extra backgrounds belong to no
theme, for mixing your own: `code` and `stars` (30 fps), `snow` (30), `jukebox` and `starburst` (20), `static`,
`testcard` and `print` (15).

### Up close

Full-size shots (click one to open it). They come from ambient mode, so every session in them is made up.

<table>
<tr><td>

[![the phosphor look](shots/phosphor.jpg)](shots/phosphor.jpg)<br><sub>phosphor</sub>

</td><td>

[![the drafting look](shots/drafting.jpg)](shots/drafting.jpg)<br><sub>drafting</sub>

</td></tr>
<tr><td>

[![the notebook look](shots/notebook.jpg)](shots/notebook.jpg)<br><sub>notebook</sub>

</td><td>

[![the sleeve look](shots/sleeve.jpg)](shots/sleeve.jpg)<br><sub>sleeve</sub>

</td></tr>
<tr><td>

[![the duplicator look](shots/duplicator.jpg)](shots/duplicator.jpg)<br><sub>duplicator</sub>

</td><td>

[![the newsprint look](shots/newsprint.jpg)](shots/newsprint.jpg)<br><sub>newsprint</sub>

</td></tr>
<tr><td>

[![the oracle look](shots/oracle.jpg)](shots/oracle.jpg)<br><sub>oracle</sub>

</td><td>

[![the oracle, art mode look](shots/art-oracle.jpg)](shots/art-oracle.jpg)<br><sub>oracle, art mode</sub>

</td></tr>
<tr><td>

[![the orb look](shots/orb.jpg)](shots/orb.jpg)<br><sub>orb</sub>

</td><td>

[![the orb, art mode look](shots/art-orb.jpg)](shots/art-orb.jpg)<br><sub>orb, art mode</sub>

</td></tr>
<tr><td>

[![the scope look](shots/scope.jpg)](shots/scope.jpg)<br><sub>scope</sub>

</td><td>

[![the scope, art mode look](shots/art-scope.jpg)](shots/art-scope.jpg)<br><sub>scope, art mode</sub>

</td></tr>
<tr><td>

[![the chalkboard look](shots/chalkboard.jpg)](shots/chalkboard.jpg)<br><sub>chalkboard</sub>

</td><td>

[![the chalkboard, art mode look](shots/art-chalkboard.jpg)](shots/art-chalkboard.jpg)<br><sub>chalkboard, art mode</sub>

</td></tr>
<tr><td>

[![the neon look](shots/neon.jpg)](shots/neon.jpg)<br><sub>neon</sub>

</td><td>

[![the bsod look](shots/bsod.jpg)](shots/bsod.jpg)<br><sub>bsod</sub>

</td></tr>
<tr><td>

[![the synthwave, art mode look](shots/art-synthwave.jpg)](shots/art-synthwave.jpg)<br><sub>synthwave, art mode</sub>

</td><td>

[![the duplicator, art mode look](shots/art-halftone.jpg)](shots/art-halftone.jpg)<br><sub>duplicator, art mode</sub>

</td></tr>
<tr><td>

[![the sleeve + starburst, art mode look](shots/art-starburst.jpg)](shots/art-starburst.jpg)<br><sub>sleeve + starburst, art mode</sub>

</td><td>

[![the fieldops + testcard, art mode look](shots/art-testcard.jpg)](shots/art-testcard.jpg)<br><sub>fieldops + testcard, art mode</sub>

</td></tr>
</table>


### Make your own

Every theme, background, font and transition is defined once in [`looks.js`](../src/glitchwall/wallpaper/looks.js),
which both the page and the Rust renderer read. A new theme that reuses existing parts is one entry there. A new
background is a canvas scene in [`index.html`](../src/glitchwall/wallpaper/index.html) plus its port in [`bg.rs`](../renderer/src/bg.rs),
checked side by side. See [CONTRIBUTING.md](../CONTRIBUTING.md).
