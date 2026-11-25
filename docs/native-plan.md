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
- **The exe ships inside a Windows wheel**, built on the dev PC (maturin bin mode).
- **Stay on the `windows` crate 0.58** with the GNU toolchain; move to llvm-mingw only when forced.
- **Each background sets its frame rate** in themes.json, and a still frame presents nothing.
- **60 fps only while a card transition plays** (about half a second each), then back to the background's own rate.
  Never the monitor's refresh rate: on a 200 Hz screen that would cost about as much as WebView2 does now.

## The options as they were put

1. **Two renderers for good.** The page has to stay for OBS browser sources, Wallpaper Engine, macOS and Linux. So
   every new background is written twice. The other option is native-only themes, which wouldn't show on stream
   through OBS. *Recommendation:* accept two renderers and keep the web one the lab and the reference.
2. **Shipping the exe.** glitchwall installs with `uv tool` from Python. The options:
   - put the Rust exe in a Windows wheel (maturin's bin mode), built on the dev PC;
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

## M4: Rust everything (planned 2026-11-23)

Goal: one `glitchwall.exe`, no Python, no `uv tool install`. The web page stays (OBS browser source, Wallpaper
Engine, the no-install demo, the README image tools) and is served by the Rust feed exactly as today.

What is still Python: about 3,300 lines (cli 519, winhost 513, server 355, platforms 314, redact 263, settingswin 235,
settings 224, ambient 203, prereqs 185, winexe 178, claude_logs 174, demo 97, themes 41) and 1,080 lines of tests.

Pieces, in order; each one is built, tested against the Python version and committed on its own:

1. **Redaction and log following** (redact, claude_logs): a Rust library. The riskiest part, so it goes first
   with parity tests: the Python tests' inputs and outputs are dumped to JSON fixtures, and Rust must match every
   one. Medium.
2. **The feed** (server, settings, ambient, themes, demo): same HTTP API and access checks, so the renderer and the
   page don't change. Parity tests from test_server, test_settings, test_modes. Large.
3. **The desktop host** (winhost): screens, behind the icons, sleep and fullscreen checks, restarts, moved into the
   renderer's process. Medium to large.
   Split (2026-11-25), each built, checked on the real desktop and committed on its own:
   - **3a. Sleep in the renderer.** Port covered_screens, work_areas and session_hidden (winhost) so the renderer
     knows by itself which of its screens nobody can see; the feed's ?screen= answer (asleep.json) stays for the
     web page hosts. Tests: winhost's geometry cases as Rust unit tests.
     Done (2026-11-25): desk.rs work_areas, covered (EnumWindows, DWM frame, cloaked/click-through/faded windows
     skipped), session_hidden (screen saver, locked input desktop) and asleep(); main.rs looks twice a second and
     the scene sleeps when the feed or its own look says so. On the real desktop a renderer on the demo feed (which
     never says sleep) went from 0.4% of a core to 0% under a maximized window and back.
   - **3b. One process.** `glitchwall-render host`: the feed (server.rs) on threads inside the renderer's process,
     no child processes, a screen change rebuilt in place instead of a restart. run_feed starts it when the exe
     knows it (a new FEED_PROTOCOL), as 2d did, Python staying the fallback. In parts:
     - 3b-1 (done 2026-11-25): the `host` command. One process serves the feed and draws (78 MB, against 20 + 89
       for feed and renderer apart), refuses a wrong key, starts no children; `--peek` works for trying it by hand.
       The in-place rebuild on a screen change builds and closes the old windows quietly (desk::close), but is not
       yet tried with a real monitor change.
     - 3b-2: try the rebuild with a monitor plugged in or out, and a scaling change.
     - 3b-3 (done 2026-11-25): run_feed hands over to `host` when the exe says FEED_PROTOCOL 2 (feed_protocol reads
       the number; 1 still gets the 2d feed child). Python keeps only the tray and the hot corner and watches the
       child on the main thread; after 3 starts in a minute the feed and the windows come back as before. The
       run_feed test uses stand-ins for the hosting path, so no wallpaper windows open while the tests run.
   - **3c. Hot corner.** watch_corner and bring_settings_forward in Rust; it still opens the Python settings window
     until piece 5. Done 2026-11-25 (corner.rs): FEED_PROTOCOL 3, glitchwall passes `--open-settings` (its own
     command for the settings, as a JSON list) and stops watching the corner itself; if host is given up on, the
     corner comes back to Python with the feed. On the real desktop the corner brought a minimized settings window
     back to the front, without a second copy. Opening a fresh window uses the same command Python used.
   - **3d. Tray.** The notification-area icon and its menu (mode, theme, corner, masked, agents, show, pause, quit),
     writing settings.json as tray.py does. Done 2026-11-25 (tray.rs, tray_menu and Feed::tray_change in server.rs):
     FEED_PROTOCOL 4, `--glitchwall` (glitchwall's own command, + settings / stop) replaces 3c's --open-settings;
     the corner and the tray come back to Python together if host is given up on. Saves go through the feed's
     one-at-a-time lock, like the settings window's; a new theme resets its look and motion tweaks. On the real
     desktop: one tray icon, owned by host; a left click restored the minimized settings; a right click opened
     the 11-entry menu. (A settings window left open across feed restarts and outside minimize/restore went
     black; a fresh one was fine.)
4. **The CLI and install** (cli, platforms, prereqs, winexe): `glitchwall` subcommands, autostart, the desktop
   menu entry. Medium. Split (2026-11-25):
   - **4a (done 2026-11-25). Nothing in Python left running.** `glitchwall-render run` (supervise.rs, FEED_PROTOCOL
     5): notes its pid in feed.pid (so stop, status and the tray's Quit work unchanged), keeps `host` as its child,
     starts it again when it closes, and after 3 starts in a minute starts `glitchwall run` with
     GLITCHWALL_PYTHON_FEED=1 and steps aside. cli.cmd_run hands over when started in the background (start,
     autostart) and the exe knows `run`; at a terminal it stays Python. Live: run 9 MB + host 130 MB, the 33 MB
     Python feed gone; stop ends both. Autostart still goes through pythonw for a moment (4c can point it at the
     exe).
   - **4b (done 2026-11-25).** `glitchwall-render config|themes|reset|export` (cli.rs), output and saved settings
     matched to cli.py by tests/test_cli_parity.py. Not what `glitchwall` runs yet: that stays the Python entry
     point until one exe ships. Deliberate differences: config checks values through the schema (refused: exit 2;
     Python saves whatever argparse let through), on|off in any case; restarting reuses the options `run` noted in
     run-args.json; the exe attaches to the terminal it was typed in.
   - **4c (done 2026-11-25).** `glitchwall-render install|uninstall` (install.rs): the HKCU Run value, the Start
     menu shortcut (IShellLinkW instead of a PowerShell call), the desktop right-click entry and, for a standalone
     exe, a copy of itself and the wallpaper folder in <state>\bin. tests/test_install_parity.py runs both sides
     with their own GLITCHWALL_HOME, APPDATA and a throwaway HKCU key (GLITCHWALL_REG_ROOT, which platforms.py now
     honours too), so the real registry and Start menu are never touched. FEED_PROTOCOL unchanged: Python doesn't
     call it. Deliberate differences: autostart runs `glitchwall-render.exe run --which S [--glitchwall JSON]
     --wallpaper DIR` straight away (no pythonw first; no --port, so host reads the port from the settings each
     logon); glitchwall's own command comes from --glitchwall, else the Python glitchwall the exe ships in
     (`<tool>\Scripts\pythonw.exe -m glitchwall`); with neither, the Start menu and right-click entries are skipped
     (the settings window is still Python until piece 5) and the tray and corner are off. Copying happens whenever
     the exe isn't inside a Python glitchwall (Python copies only when frozen). No prerequisite check yet (4d adds
     doctor). `--no-start` (tests, packagers) stops after the files are in place. The Python `glitchwall install`
     is unchanged and still what Lester runs.
   - **4d (done 2026-11-25).** `glitchwall-render doctor [--yes]` (doctor.rs): prereqs.py's Windows build, .NET
     Framework, WebView2 version and Claude logs checks with the same lines; `install` runs the same check first
     (4c's missing piece) and stops when something required is missing. `demo` now prints to the terminal it was
     typed in, refuses a port in use as cli.cmd_demo does, and `--open` opens the page in the browser after 0.8 s
     (its port, pace, mode and look options were already wired in 2c). tests/test_doctor_parity.py compares both
     sides on this machine (read-only). Deliberate differences: no Python or pywebview lines (the exe needs
     neither); the Windows build is read from the registry (CurrentBuildNumber) rather than GetVersionEx; a windowed
     exe can't ask at the keyboard, so without --yes the download-page offers always say "skipped", as Python's do
     when no one is at the keyboard.
5. **The settings window** (settingswin): the same settings.html in WebView2, opened from Rust instead of
   pywebview. Small to medium. Planned 2026-11-25 (not started; after the weekly reset):
   - **Why:** today an open settings window is an extra ~108 MB Python process (pywebview + .NET via pythonnet).
     In Rust it is WebView2 only, and once the switch is done .NET leaves doctor's list of needs.
   - **The crate question, first.** WebView2's COM interfaces are not in the `windows` crate, so one crate must
     come from crates.io (a download: ask Lester first). Two candidates: `webview2-com` alone (just the WebView2
     bindings; we already have our own windows, message loop and DPI code in desk.rs), or `wry` (webview2-com plus
     its own window layer, more than we need). Leaning webview2-com. The risk: we stay on `windows` 0.58 with the
     GNU toolchain, so it must be a version built on windows 0.58 (newer ones need 0.59+, which needs dlltool
     and an assembler rustup doesn't ship), and its WebView2Loader must link under GNU (or ship
     WebView2Loader.dll beside the exe). Check both before writing anything else.
   - **5a. Spike.** Add the crate, build on GNU, open a plain window showing settings.html from the running feed.
     Measure memory against the Python window. If it won't build on 0.58/GNU, stop and bring the options to
     Lester (another crate version, the loader DLL, or a toolchain change).
   - **5b. The window.** `glitchwall-render settings`: settingswin.Settings.open's rules (560x1050 logical px,
     shrunk to fit a short screen, centred on the screen under the mouse, not resizable, background #010401),
     the title "glitchwall settings" (bring_settings_forward finds it by title), the icon, its own taskbar
     button (AppUserModelID glitchwall.settings), a private profile in <state>\webview-settings, quiet_webview's
     browser arguments, one window at a time, the feed started first when it isn't running (cmd_settings).
   - **5c. What the page may ask.** settings.html calls window.pywebview.api.fade / keep / feedback. A small
     script injected at page load supplies that same object over WebView2's message channel, so settings.html
     doesn't change and the Python window keeps working. fade: layered-window alpha; keep: don't slide away;
     feedback: the fixed issues URL in the browser, nothing else. Navigation is locked to the feed's own origin
     and new windows are refused.
   - **5d. Docking.** settingswin's _dock_loop (dock at the left, right or top edge, slide away after 3 s
     leaving a 6 px strip, back on touch, drag off to undock). In our own window procedure a drag is known from
     WM_ENTERSIZEMOVE/WM_EXITSIZEMOVE, so nothing outside the window is read.
   - **5e. Switch over.** The hot corner, the tray's "settings" and `glitchwall settings` open the Rust window
     when the exe knows it (a FEED_PROTOCOL bump), Python's window staying the fallback. A window left open across
     a feed restart reloads its page instead of going black (the 3d note).
   - **Tests:** the injected script's calls and refusals as Rust unit tests; a parity test that the window
     opens with the right title and size and loads the page from the feed with the key (run on a throwaway
     GLITCHWALL_HOME and port, closed by the test); docking by a scripted drag, as the security pass did.

Then: package one exe (zip or installer), update the README's install steps and the measurements.

### Piece 2, the feed: how it is split (2026-11-23)

Piece 1 is done: `renderer/src/redact.rs` (c0af200) and `renderer/src/logs.rs` (9a3bd95), matched to the Python by
fixtures. The feed comes in parts, each built, tested and committed on its own:

- **2a. HTTP and events.** `glitchwall-render feed --port N` runs a Rust feed: std::net, hand-written HTTP/1.1 (local
  only, no crate), a thread per connection. `/feed` with the same Host/Origin/key checks (check_access), the safe
  headers on every reply, CORS echoed only for allowed origins, 403 with an empty body and the refusal log without
  the key, 404 for everything else when there is no static dir. The event store: redact on add, KEEP, hide_all and
  the floor, since(n, commands_only), paused. The live sources from piece 1 feed it (logs.rs Tailer), plus the
  terminal hook's files (shell_history.ShellTailer, to port).
  Parity: tests/test_feed_parity.py runs one set of HTTP checks against both feeds, each with a temp GLITCHWALL_HOME
  and events appended to a seed file it follows (the exe's test-only `--seed FILE`); the Rust one only with
  GW_FEED_EXE set. TestCheckAccess's cases are also a Rust unit test (server.rs).
  Done: server.rs (HTTP feed, event store) with its sources: logs.rs's Tailer in live mode and shell.rs (a port of
  ShellTailer) while the terminal setting is on; mask_user/mask_email follow settings.json. On real logs the Rust and
  Python feeds served identical text for every call both held. Ambient mode's generator moves to 2c.
- **2b. Settings.** settings.json load/save with the same defaults and validation (apply_changes, opts, tune, masked,
  corner, mode), LiveSettings re-read on change, `/settings` GET/POST and reset, the static files (index.html,
  settings.html, looks.js) served with `#key=`. Parity: TestSettingsWindow and test_settings.py against either feed.
  Done: settings.rs (DEFAULTS from looks.js like themes.py, apply_changes, reset, tune/opts/look/masked/corner/mode,
  display_label/live_label, the temp-file save written as json.dumps(indent=2) writes it, Live = LiveSettings) and
  server.rs (/settings GET/POST/reset, the static files from `--wallpaper DIR`, default the folder next to the exe,
  /config.js without the key, the settings fields in /feed). Parity: test_feed_parity.py now runs both feeds set up
  as run_feed has it (page served, settings editable) and checks the static files and traversal refusals, the feed
  fields for odd settings, a broken file keeping the last good settings, and the settings window's answers, saved
  file and refusals, all against what settings.py computes; settings.rs has test_settings.py's and the settings
  window's unit cases. Where Rust differs on purpose: a whole number sent as 3.0 counts as an int (the JSON reader
  can't tell), NaN/Infinity make settings.json unreadable (paused) where Python reads them, list or object values
  where Python would raise are refused or defaulted, a failed save answers 500, content types come from a fixed
  table (Python asks the registry), and the connection is closed gracefully (unread input drained) so a refused
  body never resets it. Also fixed in 2a's code: `;` no longer splits the query (parse_qs since 3.9.2), and `//x`
  is read as `/x` (http.server). Not here: the restart after RESET ALL and the per-screen sleep flag (piece 3).
- **2c. Modes.** ambient (made-up sessions) and art, follow_mode switching, the demo script. Parity: test_modes.py.
  Done: ambient.rs (ambient.py's eleven episodes and demo.py's script, the generators with pace and a prompt stop,
  random numbers seeded from BCryptGenRandom, the demo key from it too), server.rs's Sources (follow_mode: the
  mode's own source on a change, one reader per agent turned on, each stopped on its own) and `glitchwall-render
  demo [--port N] [--pace X] [--mode M] [--theme/--palette/--rain/--font/--transition NAME] [--wallpaper DIR]`
  (run_demo: random key in /config.js, settings window off, look from the settings plus the options, the rest from
  the defaults, no home or user name to mask). With --seed the seed file stands in for the agents' logs but the
  modes still follow the settings. Parity: test_feed_parity.py switches the running feed to ambient and checks what
  it makes; test_demo_parity.py runs the demo in live, ambient and art on both; TestFollowMode and TestAmbient's
  cases are Rust unit tests. The made-up text is random on both sides, so parity is in shape (kinds, projects,
  ids, timestamps, the script's own lines), not in the words drawn.
- **2d. Switch over.** `glitchwall start` runs the Rust feed when the exe is there; the Python one stays the fallback
  until pieces 3-4 replace the rest. The tray and hot corner stay in Python until piece 3 moves the desktop host.
  Done: run_feed hands the feed to `glitchwall-render feed` (server.py's RustFeed) when the native renderer draws the
  windows and the exe holds FEED_PROTOCOL ("glitchwall-feed-protocol:1"; the file is searched, never run, since an
  older exe would open windows). The Python process keeps the windows, the tray and the hot corner and starts none of
  its own sources. The Rust feed gets `--parent` (it exits within 2 s of glitchwall) and `--asleep FILE` (the host
  writes which screens are covered to <state>/asleep.json; /feed?screen=N answers sleep from it). It is started again
  if it stops; after 3 starts in a minute the Python feed takes over. GLITCHWALL_PYTHON_FEED=1 keeps everything in
  Python; renderer = web keeps it too (the WebView2 host's restart-on-reset needs the Python feed until piece 3).
  With the Rust feed RESET ALL doesn't restart the wallpaper: the native renderer takes the new look on its next poll.
  Tests: tests/test_rust_feed_host.py (which exe qualifies, the start, the asleep hand-off, a restart after a
  crash, ending with the parent, giving up to Python, and run_feed choosing each way with the windows left out).
