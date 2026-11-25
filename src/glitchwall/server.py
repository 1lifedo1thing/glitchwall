"""The local feed: an HTTP endpoint on 127.0.0.1 that only the wallpaper page can read.

Who may read /feed:
  1. Host header must be 127.0.0.1:<port> or localhost:<port>   (blocks DNS rebinding)
  2. Origin must be absent, "null" (a local file page: Plash, Komorebi, a kiosk browser)
     or the feed's own address (glitchwall's own windows and the settings window).
     Pages from any other address, other local servers included, are refused.
  3. ?key= must equal the per-install random key, which only the wallpaper's
     config.js has. Compared in constant time.
Changing a setting also needs a JSON POST from the feed's own address (the settings window).
The socket binds 127.0.0.1 only. Nothing is sent anywhere; the page pulls.
"""
from __future__ import annotations

import hmac
import json
import mimetypes
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable
from urllib.parse import parse_qs, urlsplit

from . import settings as cfg
from . import themes
from .claude_logs import clip
from .redact import Redactor

KEEP = 300   # events kept in memory
BATCH = 60   # most events per response
SCAN = 4000  # text redacted per event: only 1600 characters show, and redacting is slower the longer the text

LOCAL_ORIGIN = "null"   # a page opened from a file
# every response: never sniffed into another type, framed only by the feed's own pages (the settings window's
# backdrop thumbnails; another site's page can't frame one, so no clickjacking), no address passed on in a Referer
SAFE_HEADERS = (("X-Content-Type-Options", "nosniff"), ("X-Frame-Options", "SAMEORIGIN"),
                ("Content-Security-Policy", "frame-ancestors 'self'"), ("Referrer-Policy", "no-referrer"))


class EventStore:
    """Redacts on the way in, so unredacted text is never kept or served.
    While hidden() is true (paused), new events are dropped, and hide_all() puts everything so far out
    of reach: resuming never replays what happened during a pause."""

    def __init__(self, redactor=None, hidden=None):
        self.redact = redactor or Redactor()
        self.hidden = hidden or (lambda: False)
        self.events: list[dict] = []
        self.lock = threading.Lock()
        self.seq = 0
        self.floor = 0   # events up to this id are never served again

    def add(self, ev: dict) -> None:
        if self.hidden():
            return
        ev = dict(ev)
        # cut at a word break first (clip): a secret cut in half would be too short to be recognized as one
        ev["text"] = self.redact(clip(str(ev.get("text", "")), SCAN))[:1600]
        ev["proj"] = self.redact(str(ev.get("proj", "")))[:60]
        with self.lock:
            self.seq += 1
            ev["id"] = self.seq
            self.events.append(ev)
            del self.events[:-KEEP]

    def hide_all(self) -> None:
        with self.lock:
            self.floor = self.seq

    def since(self, n: int, commands_only: bool = False) -> list[dict]:
        with self.lock:
            n = max(n, self.floor)
            out = [e for e in self.events if e["id"] > n]
        if commands_only:
            out = [e for e in out if e.get("kind") not in ("out", "err")]
        return out[-BATCH:]


def check_access(host: str | None, origin: str | None, given_key: str, key: str, port: int,
                 extra_origins: tuple[str, ...] = ()) -> bool:
    """True if this request may read the feed. Pure function, unit-tested."""
    if host not in (f"127.0.0.1:{port}", f"localhost:{port}"):
        return False
    if origin is not None and not (origin == LOCAL_ORIGIN or origin in extra_origins):
        return False
    if not key or not given_key:
        return False
    return hmac.compare_digest(given_key.encode(), key.encode())


class LiveSettings:
    """Settings re-read when the file changes, so `pause` / `config` act without a restart. If the file turns
    unreadable (a bad hand edit), the last good settings stay; one that's unreadable from the start counts as paused."""

    def __init__(self):
        self.mtime = None
        self.s = cfg.load_settings()

    def get(self) -> dict:
        try:
            m = cfg.settings_file().stat().st_mtime
        except OSError:
            m = None
        if m != self.mtime:
            self.mtime = m
            saved = cfg.read_saved()
            if saved is not None or not self.good:
                self.s = cfg.load_settings(saved)
                self.good = saved is not None
        return self.s

    good = False


def make_handler(store: EventStore, key: str, port: int, live: LiveSettings | None = None,
                 static_dir: Path | None = None, config_js: str | None = None,
                 extra_origins: tuple[str, ...] = (), refusal_log: Path | None = None,
                 look: dict | None = None, mode: str | None = None, label: str | None = None,
                 sleeping: Callable[[str], bool] | None = None, editable: bool = False,
                 on_change: Callable[[dict], None] | None = None, on_reset: Callable[[], None] | None = None):
    class Handler(BaseHTTPRequestHandler):
        server_version = "glitchwall"
        sys_version = ""

        def end_headers(self):
            for k, v in SAFE_HEADERS:
                self.send_header(k, v)
            super().end_headers()

        def refuse(self, code: int) -> None:
            if code == 403 and refusal_log:   # note who was turned away (never the key) to help debug a new host
                try:
                    refusal_log.write_text(
                        f"{time.ctime()} host={self.headers.get('Host')} origin={self.headers.get('Origin')} "
                        f"referer={(self.headers.get('Referer') or '').split('?')[0]}\n", encoding="utf-8")
                except OSError:
                    pass
            self.send_response(code)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_GET(self):
            u = urlsplit(self.path)
            if u.path == "/feed":
                return self.feed(u)
            if u.path == "/settings":
                return self.settings_api(u, None)
            if static_dir is not None:   # the page itself (demo mode, glitchwall's own host, the settings window)
                return self.static(u.path)
            return self.refuse(404)

        def feed(self, u):
            q = parse_qs(u.query)
            origin = self.headers.get("Origin")
            if not check_access(self.headers.get("Host"), origin, q.get("key", [""])[0], key, port, extra_origins):
                return self.refuse(403)
            try:
                since = int(q.get("since", ["0"])[0])
            except ValueError:
                since = 0
            s = live.get() if live else cfg.DEFAULTS
            m = mode or cfg.mode(s)
            hidden = bool(s.get("paused")) or m == "art"
            if hidden:   # the page clears the screen; what it hadn't shown yet must not come back later
                store.hide_all()
            out = [] if hidden else store.since(since, bool(s.get("commands_only")))
            body = json.dumps({"events": out, "paused": bool(s.get("paused")), "last": store.seq,
                               "look": look or themes.look(s), "mode": m,
                               "label": cfg.display_label(s) if label is None else label, "tune": cfg.tune(s), "opts": cfg.opts(s),
                               "corner": cfg.corner(s),   # where the hot-corner icon sits
                               "masked": cfg.masked(s),   # how masked secrets show
                               # the built-in host: nobody can see this screen (covered, locked), so stop animating
                               "sleep": bool(sleeping and sleeping(q.get("screen", [""])[0]))}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            if origin:
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Vary", "Origin")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            u = urlsplit(self.path)
            try:
                n = int(self.headers.get("Content-Length") or 0)
            except ValueError:
                n = -1
            if u.path != "/settings" or not 0 < n <= 8192:
                return self.refuse(404 if u.path != "/settings" else 400)
            return self.settings_api(u, self.rfile.read(n))

        def settings_api(self, u, body: bytes | None):
            """The settings window: GET reads, POST changes. Same key/Host/Origin checks as the feed; a change
            must also be a JSON POST from the feed's own address, where the settings window lives (a plain form
            can't be one, and a file page or a sandboxed frame, which send Origin: null, can't make one)."""
            if not editable:
                return self.refuse(404)
            q, origin = parse_qs(u.query), self.headers.get("Origin")
            if not check_access(self.headers.get("Host"), origin, q.get("key", [""])[0], key, port, extra_origins):
                return self.refuse(403)
            refused, reset = [], False
            if body is not None:
                if origin not in extra_origins or self.headers.get("Content-Type", "").split(";")[0].strip() != "application/json":
                    return self.refuse(403)
                try:
                    changes = json.loads(body)
                except ValueError:
                    return self.refuse(400)
                changes = changes if isinstance(changes, dict) else {}
                reset = changes.pop("reset", None) is True   # RESET ALL: defaults, then a fresh wallpaper
                s = cfg.reset_settings(cfg.load_settings()) if reset else cfg.load_settings()
                s, refused = cfg.apply_changes(s, changes)
                cfg.save_settings(s)
                if on_change:
                    on_change(s)
            s = cfg.load_settings()
            body_out = json.dumps({
                "settings": {k: s.get(k, v) for k, v in cfg.DEFAULTS.items()}, "refused": refused,
                "defaults": {k: v for k, v in cfg.DEFAULTS.items() if k not in ("port", "host", "screens", "renderer")},
                "choices": {**themes.KINDS, **themes.OPTIONS, "mode": cfg.MODES, "corner": cfg.CORNERS, "masked": cfg.MASKED},
                "options": themes.OPTIONS_OF, "theme_rain": themes.THEME_RAIN,
                "ranges": {k: [lo, hi] for k, (_, lo, hi) in cfg.TUNE.items()},
                "running": True, "restarting": bool(reset and on_reset),
            }).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body_out)))
            if origin:
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Vary", "Origin")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body_out)
            if reset and on_reset:   # after the answer is out: the page knows to wait for the restart
                threading.Timer(0.5, on_reset).start()

        def static(self, path):
            if self.headers.get("Host") not in (f"127.0.0.1:{port}", f"localhost:{port}"):
                return self.refuse(403)
            name = path.lstrip("/") or "index.html"
            if name == "config.js" and config_js is not None:
                data, ctype = config_js.encode(), "text/javascript"
            else:
                f = (static_dir / name).resolve()
                if static_dir.resolve() not in f.parents or not f.is_file():
                    return self.refuse(404)
                data = f.read_bytes()
                ctype = mimetypes.guess_type(f.name)[0] or "application/octet-stream"
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *a):
            pass

    return Handler


def serve(handler, port: int) -> ThreadingHTTPServer:
    httpd = ThreadingHTTPServer(("127.0.0.1", port), handler)
    httpd.daemon_threads = True
    return httpd


RESTARTS = "GLITCHWALL_RESTARTS"


def restart_self(httpd, manual: bool = False) -> None:
    """Free the port and start this same command again, fresh: a new WebView2 and a new page. Used when WebView2
    went away under the wallpaper (gives up after a few quick restarts in a row, so a broken WebView2 can't make
    it loop forever) and for RESET ALL in the settings window (`manual`: always restarts)."""
    import subprocess
    import sys
    n, since = (os.environ.get(RESTARTS, "0:0").split(":") + ["0"])[:2]
    n = int(n) + 1 if not manual and time.time() - float(since or 0) < 300 else 1
    httpd.socket.close()
    if n <= 3:
        args = [sys.executable] + (sys.argv[1:] if getattr(sys, "frozen", False) else ["-m", "glitchwall"] + sys.argv[1:])
        env = {**os.environ, RESTARTS: f"{n}:{time.time() if n == 1 else since}"}
        subprocess.Popen(args, env=env, close_fds=True, creationflags=getattr(subprocess, "DETACHED_PROCESS", 0)
                         | getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0))
    os._exit(0)


FEED_PROTOCOL = b"glitchwall-feed-protocol:"   # renderer/src/server.rs: what an exe with the Rust feed holds, then
# its number: 1 knows `feed` (piece 2d), 2 also `host`, the feed and the windows in one process (piece 3b)


def feed_protocol(exe: Path) -> int:
    """The protocol number in the exe, 0 when it has none (or can't be read). Searched for, never run."""
    try:
        data = exe.read_bytes()
    except OSError:
        return 0
    i = data.find(FEED_PROTOCOL)
    digits = data[i + len(FEED_PROTOCOL):i + len(FEED_PROTOCOL) + 4] if i >= 0 else b""
    n = len(digits) - len(digits.lstrip(b"0123456789"))
    return int(digits[:n]) if n else 0


def rust_feed_exe(s: dict) -> Path | None:
    """glitchwall-render.exe when it can run the feed (piece 2d of docs/native-plan.md): Windows, the native renderer
    drawing the windows (the WebView2 host's restart-on-reset still needs this process's own feed), and an exe new
    enough to know `feed`. An older one would open wallpaper windows instead, so the file is searched for the
    protocol text, never run to find out. GLITCHWALL_PYTHON_FEED=1 keeps the feed in Python."""
    if os.name != "nt" or os.environ.get("GLITCHWALL_PYTHON_FEED") or s.get("renderer") != "native":
        return None
    from . import winhost
    exe = winhost.render_exe()
    return exe if exe and feed_protocol(exe) >= 1 else None


class RustFeed:
    """The feed run by glitchwall-render (server.rs) as a child of this process, which keeps the windows, the tray
    and the hot corner. It ends with this process (it checks every 2 s), is started again if it stops, and gives up
    after 3 starts in a minute, when the feed comes back here (`on_give_up`). Which screens nobody can see goes to it
    through a small file (`asleep`), as this process is the one that knows.
    With `hosting` (protocol 2) the child is `glitchwall-render host` instead: the feed and the wallpaper windows in
    one process, which works out for itself which screens nobody can see, so there is no file to write."""

    def __init__(self, exe: Path, port: int, asleep: Path, host=None, wallpaper: Path | None = None,
                 hosting: bool = False):
        self.exe, self.port, self.asleep, self.host, self.wallpaper = exe, port, asleep, host, wallpaper
        self.hosting = hosting
        self.proc = None
        self.starts: list[float] = []

    def launch(self) -> bool:
        """Start it and wait (up to 5 s) for it to answer on the port. False if it didn't."""
        import subprocess
        from .platforms import port_open
        now = time.monotonic()
        self.starts = [t for t in self.starts if now - t < 60] + [now]
        if self.hosting:
            args = [str(self.exe), "host", "--port", str(self.port), "--parent", str(os.getpid()),
                    "--which", getattr(self.host, "which", "all")]
        else:
            args = [str(self.exe), "feed", "--port", str(self.port), "--parent", str(os.getpid()),
                    "--asleep", str(self.asleep)]
        if self.wallpaper:
            args += ["--wallpaper", str(self.wallpaper)]
        try:
            self.proc = subprocess.Popen(args, close_fds=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        except OSError:
            return False
        for _ in range(50):
            if self.proc.poll() is not None:
                return False
            if port_open(self.port):
                return True
            time.sleep(0.1)
        self.end()
        return False

    def start(self) -> bool:
        """Up, or False after 3 tries in a minute."""
        while len(self.starts) < 3 or time.monotonic() - self.starts[-3] >= 60:
            if self.launch():
                return True
        return False

    def write_asleep(self, screens: list[int]) -> None:
        try:
            cfg._private_write(self.asleep, json.dumps(screens))
        except OSError:
            pass

    def watch(self, stop: threading.Event, on_give_up: Callable[[], None]) -> None:
        last = None
        while not stop.is_set():
            now = sorted(getattr(self.host, "asleep", ()) or ())
            if now != last and not self.hosting:
                self.write_asleep(now)
                last = now
            if self.proc is not None and self.proc.poll() is not None and not self.start():
                self.end()
                on_give_up()
                return
            stop.wait(0.5)

    def end(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()


def run_feed(port: int | None = None) -> None:
    """Run the feed in the foreground (what `start` launches in the background).
    The source follows the `mode` setting and switches when it changes, no restart needed:
    live = tail the agents' logs that are turned on, ambient = made-up sessions (reads nothing), art = nothing."""
    live = LiveSettings()
    s = live.get()
    port = port or int(s["port"])
    store = EventStore(Redactor(mask_user=s.get("mask_user", True), mask_email=s.get("mask_email", True)),
                       hidden=lambda: bool(live.get().get("paused")))
    key = cfg.load_key()
    host = None
    if os.name == "nt":
        from . import winhost
        exe = winhost.render_exe() if s.get("renderer") == "native" else None
        if exe:   # glitchwall-render draws the windows; it polls this feed like the page does
            from .platforms import wallpaper_source
            host = winhost.NativeHost(exe, port, key, s.get("screens", "all"), wallpaper_source())
        elif winhost.available():
            # the page is served from here and finds its key in the address (#key=, never sent over the wire)
            host = winhost.Host(lambda n: f"http://127.0.0.1:{port}/?screen={n}#key={key}",
                                s.get("screens", "all"), cfg.state_dir() / "webview",
                                on_dead=lambda: restart_self(httpd))
    from .platforms import wallpaper_source

    def on_change(new: dict) -> None:   # the settings window changed the masking: applies to what comes next
        store.redact = Redactor(mask_user=new.get("mask_user", True), mask_email=new.get("mask_email", True))

    # the feed also serves the page (glitchwall's own host) and the settings window, both from its own origin
    handler = make_handler(store, key, port, live, refusal_log=cfg.state_dir() / "last-refused.txt",
                           static_dir=wallpaper_source(), config_js="// the key is in the page address\n",
                           extra_origins=(f"http://127.0.0.1:{port}", f"http://localhost:{port}"),
                           sleeping=(lambda scr: scr.isdigit() and int(scr) in host.asleep) if host else None,
                           editable=True, on_change=on_change,
                           on_reset=(lambda: restart_self(httpd, manual=True)) if host else None)
    httpd = None

    def python_feed() -> None:   # the feed from this process: the default, and the fallback when the Rust one can't run
        nonlocal httpd
        httpd = serve(handler, port)
        threading.Thread(target=follow_mode, args=(store, live), daemon=True).start()
        if host:
            threading.Thread(target=httpd.serve_forever, daemon=True).start()

    exe = rust_feed_exe(s) if host and isinstance(host, winhost.NativeHost) else None
    rust = RustFeed(exe, port, cfg.state_dir() / "asleep.json", host, wallpaper_source(),
                    hosting=feed_protocol(exe) >= 2) if exe else None
    hosted = False   # glitchwall-render host draws the windows too: this process keeps only the tray and the corner
    if rust and rust.start():
        import atexit
        atexit.register(rust.end)
        hosted = rust.hosting
        if not hosted:
            threading.Thread(target=rust.watch, args=(threading.Event(), python_feed), daemon=True).start()
    else:
        python_feed()
    if os.name == "nt":   # the hot corner: rest the mouse there and the settings open
        from . import winhost as wh
        from .platforms import live_command

        def open_settings() -> None:
            if not wh.bring_settings_forward():
                import subprocess
                subprocess.Popen(live_command("settings"), close_fds=True)
        threading.Thread(target=wh.watch_corner, args=(lambda: cfg.corner(live.get()), open_settings, threading.Event()),
                         daemon=True).start()
        if live.get().get("tray", True):   # glitchwall in the notification area, for as long as the feed runs
            start_tray(live, open_settings, on_change)
    if not host:
        httpd.serve_forever()
        return
    if hosted:   # until it can't be kept running: then the feed comes back here and the windows as before
        rust.watch(threading.Event(), python_feed)
    host.run(threading.Event())   # the windows need this (the main) thread; runs until the process ends


def start_tray(live: "LiveSettings", open_settings, on_change=None) -> None:
    """The notification-area icon: a click opens the settings. Its menu has the everyday settings, each current one
    ticked (mode, theme, hot corner, how masked secrets show, what shows), plus pause (stream-safe) and quit."""
    import subprocess
    from . import themes
    from .platforms import live_command, wallpaper_source
    from .tray import Tray

    def change(k, v) -> None:
        s = cfg.load_settings()
        new, refused = cfg.apply_changes(s, {k: v})
        if refused:
            return
        if k == "theme" and v != s.get("theme"):   # as the settings window does: a new theme starts clean
            for x in ("palette", "rain", "font", "transition", *cfg.TUNE):   # its look and motion tweaks
                new[x] = cfg.DEFAULTS[x]
        cfg.save_settings(new)
        if on_change and k in ("mask_user", "mask_email"):
            on_change(new)

    def quit_all() -> None:
        tray.hide()   # take the icon away first, or it lingers in the tray until the mouse passes over it
        subprocess.Popen(live_command("stop"), close_fds=True)

    def pick(k, choices, name=lambda v: v.replace("-", " ").capitalize()) -> list:
        cur = live.get().get(k, cfg.DEFAULTS.get(k))
        return [(name(v), (lambda v=v: change(k, v)), v == cur) for v in choices]

    def flip(k, label) -> tuple:
        on = bool(live.get().get(k, cfg.DEFAULTS.get(k)))
        return (label, lambda: change(k, not on), on)

    masked = {"dots": "Dots  ••••", "words": "Words  [REDACTED]", "blocks": "Blocks  ████", "stars": "Stars  ****",
              "noise": "Gibberish  x7#q"}

    def menu() -> list:
        s = live.get()
        p = bool(s.get("paused"))
        return [("Open settings", open_settings),
                ("Resume" if p else "Pause (stream-safe)", lambda: change("paused", not p)),
                None,
                ("Mode", pick("mode", cfg.MODES)),
                ("Theme", pick("theme", themes.THEMES)),
                ("Hot corner", pick("corner", cfg.CORNERS)),
                ("Masked secrets", pick("masked", masked, masked.get)),
                ("Read logs of", [flip(k, name) for k, (name, _) in cfg.AGENTS.items()]),
                ("Show", [flip("commands_only", "Calls only, no output"),
                          None, flip("mask_user", "Mask my user name"), flip("mask_email", "Mask emails")]),
                None,
                ("Quit glitchwall", quit_all)]

    tray = Tray(str(wallpaper_source() / "glitchwall.ico"), "glitchwall", menu)
    tray.show()


def agents_on(s: dict) -> tuple[str, ...]:
    """The agents whose logs live mode reads: only those turned on (Claude Code by default, the others off)."""
    return tuple(k for k in cfg.AGENTS if s.get(k, cfg.DEFAULTS[k]) is True)


def start_source(mode: str, store: EventStore) -> threading.Event:
    """Start the event source for a mode; set the returned event to stop it. Live mode's sources are the agents'
    log readers, started one by one in follow_mode (start_reader)."""
    stop = threading.Event()
    if mode == "ambient":
        from . import ambient
        threading.Thread(target=ambient.generate, args=(store, stop), daemon=True).start()
    return stop


def start_reader(agent: str, store: EventStore) -> threading.Event:
    """Follow one agent's logs (a key of settings.AGENTS); set the returned event to stop it."""
    from .claude_logs import ClaudeLogTailer
    from .codex_logs import CodexLogTailer
    from .gemini_logs import GeminiLogTailer
    reader = {"agent_claude": ClaudeLogTailer, "agent_gemini": GeminiLogTailer, "agent_codex": CodexLogTailer}[agent]
    stop = threading.Event()
    threading.Thread(target=reader(store.add).run, args=(stop,), daemon=True).start()
    return stop


def follow_mode(store: EventStore, live: LiveSettings, poll: float = 1.0, quit: threading.Event | None = None) -> None:
    """Keep the sources in step with the settings: the mode's own, and in live mode one reader per agent turned
    on. Turning an agent on or off starts or stops just that reader; an agent that is off has its folder left alone."""
    mode, stop = None, None
    readers: dict[str, threading.Event] = {}
    while not (quit and quit.is_set()):
        s = live.get()
        m = cfg.mode(s)
        if m != mode:
            if stop:
                stop.set()
            mode, stop = m, start_source(m, store)
        want = set(agents_on(s)) if m == "live" else set()
        for a in [a for a in readers if a not in want]:
            readers.pop(a).set()
        for a in cfg.AGENTS:
            if a in want and a not in readers:
                readers[a] = start_reader(a, store)
        time.sleep(poll)
    for r in [stop, *readers.values()]:
        if r:
            r.set()
