# Plan: a native renderer for glitchwall on Windows

Goal: glitchwall's own wallpaper windows on Windows draw with Direct2D from a small Rust program instead of WebView2.
The aim is about **5% CPU and 140 MB** in total (renderer plus the Python feed), down from about 20% and 400 MB, with the
same look.

## What a theme is today (wallpaper/index.html)

A theme is already mostly data. The `THEMES` table gives each theme:

- a palette: bg, surface, ink, muted, faint, acc, acc2, err, dim, hi, plus optional overrides;
- two fonts (from `FONTS`), grain, a texture, glow or misregistration, and card solidity;
- a transition (`THEME_ANIM`) and a background (`rain`).

Only three kinds of thing are code:

| Part | How many | What it is today | Native equivalent |
|---|---|---|---|
| Card | 1, with parameters | CSS: glass blur, edge, shadow, glow, misprint, ruled lines, oracle brackets | One Rust card drawer reading the same parameters |
| Transitions | 11 | CSS keyframes: clip inset or circle, translate, scale, skew, rotate, opacity, brightness, hue-rotate, blur, RGB split | **Data.** The same keyframes in themes.json, run by a small keyframe player |
| Overlays | 4 | grain, scan, tube, halftone, plus a flicker every 6 s | Bitmaps made once, as in the prototype |
| Backgrounds | 16 (+ sphere coming) | Canvas JavaScript, 10 to 130 lines each | **Code.** One Rust module each, on a small canvas-like layer |

So a new theme that only picks colors, fonts, texture, transition and an existing background needs no code in either
renderer. Only a **new background** is real work twice.

## The boundary already exists

The page knows nothing but `GET /feed`. That one JSON reply carries events, look, tune, sleep, paused, mode, label and
corner. A native renderer polls the same endpoint and is a drop-in replacement for the page. The Python feed, the
redaction, settings, the CLI and the sleep check stay as they are. winhost.py starts `glitchwall-render.exe` per screen
instead of a WebView2 window.

## Milestones

**M0. One theme file for both renderers** (small, worth doing anyway)
- Move `THEMES`, `FONTS`, `ANIMS` and `THEME_ANIM` out of index.html into `wallpaper/themes.json`. The page loads it,
  `themes.py` reads it (replacing the name-match test), and the native renderer reads it.
- Write the 11 transitions as keyframe data in the same file. The page turns them into CSS `@keyframes` at load, so
  there is one definition.

  *Done 2026-11-22 as `wallpaper/looks.js`.* The page loads it as a script, so exported folders opened from disk still
  work; themes.py and Rust read the JSON after `window.LOOKS =`. Applied looks are verified identical for all 361
  theme/palette pairs, every transition, font and background. **The keyframes are not data yet**; that moves to M1,
  where the native keyframe player decides the format.

**M1. Renderer skeleton, opt-in** (the big one)
- D3D11, a flip-model DXGI swap chain and a Direct2D 1.1 device context in a child window of Progman behind the icons.
  1.1 is needed for the blur, shadow (glow) and hue-rotate effects. **Check this first, day one**: the prototype
  used the older HwndRenderTarget.
- Poll `/feed` every second; handle sleep, pause, art mode, label, corner and tune.
- Bundled fonts through a DirectWrite in-memory font set; no system fallbacks.
- Cards: placement (the same best-of-80 rule), typing, output under its command, linger, and the 11 transitions from
  data. Glass blur samples our own background bitmap under the card, which is cheaper than Chromium's backdrop-filter.
- Overlays, plus backgrounds `pixel` and `none`.
- **Draw only when something changed.** A still background with no typing presents nothing. record, stop, a stilled
  oracle and newsprint then cost close to zero.
- Turned on with `glitchwall config --renderer native`; WebView2 stays the default. Measure against today.

**M2. Port the backgrounds**, in three batches
1. Simple: stars, snow, static, code, grid, print, stop, testcard.
2. Drawn: draft, notebook, halftone, record, jukebox, starburst.
3. oracle, and sphere when it lands.

Each port is checked against the page side by side in `?still` mode with the same seed.

**M3. Native becomes the default on Windows.** WebView2 stays as the fallback (`--renderer web`) and as the theme lab.

**M4 (decide after M3). Port the feed to Rust** for one process and about 50 MB. The redaction is the risky part.
It would be ported with the existing tests turned into shared test vectors.

## How new themes get made afterwards

- **Palette, font, texture or transition only:** edit themes.json. Both renderers pick it up, and the native one
  reloads it live.
- **New background:** draft it in the page, where the canvas is fast to iterate, then port it to a Rust module. The
  canvas-like layer keeps that close to a line-by-line translation.

## Decisions (taken 2026-11-22)

- **Keep both renderers.** The web page stays the reference and the theme lab; each new background is ported to Rust.
- **The exe ships inside a Windows wheel**, built on Kahoy (maturin bin mode).
- **Stay on the `windows` crate 0.58** with the GNU toolchain; move to llvm-mingw only when forced.
- **Each background sets its frame rate** in themes.json, and a still frame presents nothing.
- **60 fps only while a card transition plays** (about half a second each), then back to the background's own rate.
  Never the monitor's refresh rate: on a 200 Hz screen that would cost about as much as WebView2 does now.

## The options as they were put

1. **Two renderers for good.** The page has to stay for OBS browser sources, Wallpaper Engine, macOS and Linux. So
   every new background is written twice. The other option is native-only themes, which wouldn't show on stream
   through OBS. *Recommendation:* accept two renderers and keep the web one the lab and the reference.
2. **Shipping the exe.** glitchwall installs with `uv tool` from Python. The options:
   - put the Rust exe in a Windows wheel (maturin's bin mode), built on Kahoy;
   - build it from source at install, which needs Rust on every machine.
   *Recommendation:* the wheel.
3. **Toolchain.** Today it builds with the GNU toolchain on `windows` 0.58, the last version that needs nothing
   else. Newer versions need either llvm-mingw (FOSS, about 150 MB) or Microsoft's Build Tools (not FOSS, several
   GB). *Recommendation:* stay on 0.58 until something forces a move, then llvm-mingw.
4. **Frame rate.** 30 fps everywhere today. Calm backgrounds (print, record, halftone, stop) could run at 10-15 fps,
   with change-only drawing. *Recommendation:* a per-background rate in themes.json.

## Risks

- Flip-model swap chains as a child of Progman. The prototype's blt-model target works there; flip-model has to be
  tested.
- Transition and glow fidelity. CSS filters such as hue-rotate and backdrop blur have direct Direct2D effects, but small
  differences will show side by side.
- Multi-monitor and DPI. winhost.py handles these today; the renderer gets one window per screen from it.
