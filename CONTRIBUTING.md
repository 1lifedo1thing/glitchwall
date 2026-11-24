# Contributing

Bug reports, theme ideas and pull requests are welcome. Security problems: please use the private route in
[SECURITY.md](SECURITY.md), not a public issue.

## Build and test

Needs [uv](https://docs.astral.sh/uv/) and, for the renderer, Rust with the GNU toolchain (`windows` crate 0.58).

```
uv run --with pytest pytest -q                     # Python tests (set GW_FEED_EXE to the renderer exe to run the feed parity tests too)
cd renderer && cargo test --release                # Rust tests
cd renderer && cargo build --release               # the renderer: renderer/target/release/glitchwall-render.exe
uv run python tools/build_release.py               # the release wheel: renderer inside, labelled Windows-only
```

Installing from a plain checkout (`uv tool install .`) gives a wheel without the renderer; glitchwall then falls
back to drawing in WebView2. Use `tools/build_release.py` for anything you hand to someone else.

## How it fits together

Two programs: a Python **feed** that follows the logs, masks them and serves them on `127.0.0.1`, and a Rust
**renderer** that draws every screen behind the desktop icons with Direct2D. The same looks also run as a web page
([`index.html`](src/glitchwall/wallpaper/index.html)) for OBS, Wallpaper Engine and the demo.

| Path | Role |
| --- | --- |
| `src/glitchwall/claude_logs.py` | follow the logs, pair each call with its output |
| `src/glitchwall/redact.py` | all masking |
| `src/glitchwall/server.py` | the local feed and its access checks |
| `src/glitchwall/settings.py` | settings, their defaults and limits |
| `src/glitchwall/winhost.py` | the windows behind the desktop icons, one per screen |
| `src/glitchwall/settingswin.py` | the settings window (pywebview + WebView2) |
| `src/glitchwall/tray.py` | the notification-area icon (plain Win32) |
| `src/glitchwall/prereqs.py` | what `install` and `doctor` check for |
| `src/glitchwall/wallpaper/looks.js` | every theme, background, font and transition, read by the page and the renderer |
| `src/glitchwall/wallpaper/index.html` | the page (`?still` renders without animation) |
| `src/glitchwall/wallpaper/settings.html` | the settings window's page |
| `renderer/src/` | the Rust renderer: `bg.rs` backgrounds, `scene.rs` cards, `server.rs` + `redact.rs` + `logs.rs` a Rust feed (not used yet) |

## Rules the looks follow

Read [docs/THEMES.md](docs/THEMES.md) first: every theme is a whole look, borrowed from real things and copied
from none, quieter than the cards, safe to stream, with fonts bundled and exactly four options per background.
A new background is written twice, as a canvas scene in the page and in `bg.rs`, and checked side by side.

## Recording the docs

The tools in `tools/` record the web page with made-up data (`--with pillow --with websockets`):
`record_demo.py` (docs/demo.gif), `theme_gallery.py` (docs/themes.png), `screenshots.py` (docs/shots/),
`showcase.py` (docs/gifs/), `make_icon.py` (the app icon).
