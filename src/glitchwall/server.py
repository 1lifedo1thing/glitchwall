"""The local feed: an HTTP endpoint on 127.0.0.1 that only the wallpaper page can read.

Who may read /feed:
  1. Host header must be 127.0.0.1:<port> or localhost:<port>   (blocks DNS rebinding)
  2. Origin must be absent, "null" (a local file page) or http(s)://*.localhost
     (Lively serves web wallpapers as https://<hash>.localhost; browsers pin
     *.localhost to this machine, so no internet site can send it)
  3. ?key= must equal the per-install random key, which only the wallpaper's
     config.js has. Compared in constant time.
The socket binds 127.0.0.1 only. Nothing is sent anywhere; the page pulls.
"""
from __future__ import annotations

import hmac
import json
import mimetypes
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from . import settings as cfg
from . import themes
from .redact import Redactor

KEEP = 300   # events kept in memory
BATCH = 60   # most events per response

LOCAL_ORIGIN = re.compile(r"^(null|https?://([a-z0-9-]+\.)*localhost(:\d{1,5})?)$")


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
        ev["text"] = self.redact(str(ev.get("text", "")))[:1600]
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
    if origin is not None and not (LOCAL_ORIGIN.match(origin) or origin in extra_origins):
        return False
    if not key or not given_key:
        return False
    return hmac.compare_digest(given_key.encode(), key.encode())


class LiveSettings:
    """Settings re-read when the file changes, so `pause` / `config` act without a restart."""

    def __init__(self):
        self.mtime = None
        self.s = cfg.load_settings()

    def get(self) -> dict:
        try:
            m = cfg.settings_file().stat().st_mtime
        except OSError:
            m = None
        if m != self.mtime:
            self.mtime, self.s = m, cfg.load_settings()
        return self.s


def make_handler(store: EventStore, key: str, port: int, live: LiveSettings | None = None,
                 static_dir: Path | None = None, config_js: str | None = None,
                 extra_origins: tuple[str, ...] = (), refusal_log: Path | None = None,
                 look: dict | None = None, mode: str | None = None, label: str | None = None):
    class Handler(BaseHTTPRequestHandler):
        server_version = "glitchwall"
        sys_version = ""

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
            if static_dir is not None:   # demo mode only: serve the wallpaper page itself
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
                               "label": cfg.display_label(s) if label is None else label}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            if origin:
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Vary", "Origin")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

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


def run_feed(port: int | None = None) -> None:
    """Run the feed in the foreground (what `start` launches in the background).
    The source follows the `mode` setting and switches when it changes, no restart needed:
    live = tail Claude Code's logs, ambient = made-up sessions (reads nothing), art = nothing."""
    live = LiveSettings()
    s = live.get()
    port = port or int(s["port"])
    store = EventStore(Redactor(mask_user=s.get("mask_user", True), mask_email=s.get("mask_email", True)),
                       hidden=lambda: bool(live.get().get("paused")))
    threading.Thread(target=follow_mode, args=(store, live), daemon=True).start()
    handler = make_handler(store, cfg.load_key(), port, live, refusal_log=cfg.state_dir() / "last-refused.txt")
    serve(handler, port).serve_forever()


def start_source(mode: str, store: EventStore) -> threading.Event:
    """Start the event source for a mode; set the returned event to stop it."""
    stop = threading.Event()
    if mode == "live":
        from .claude_logs import ClaudeLogTailer
        threading.Thread(target=ClaudeLogTailer(store.add).run, args=(stop,), daemon=True).start()
    elif mode == "ambient":
        from . import ambient
        threading.Thread(target=ambient.generate, args=(store, stop), daemon=True).start()
    return stop


def follow_mode(store: EventStore, live: LiveSettings, poll: float = 1.0, quit: threading.Event | None = None) -> None:
    mode, stop = None, None
    while not (quit and quit.is_set()):
        m = cfg.mode(live.get())
        if m != mode:
            if stop:
                stop.set()
            mode, stop = m, start_source(m, store)
        time.sleep(poll)
    if stop:
        stop.set()
