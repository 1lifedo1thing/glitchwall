"""The same HTTP checks against both feeds: the Python one (server.py) and the Rust one (glitchwall-render feed).

The Rust feed runs when GW_FEED_EXE names the exe (piece 2a of docs/native-plan.md); otherwise only the
Python one does. Each runs on its own port with a fresh state folder (key, settings.json), fed events through a
seed file it follows, so both get them the same way: appended lines, paused or not.
"""
import http.client
import json
import os
import subprocess
import tempfile
import threading
import time
import unittest
from datetime import datetime
from pathlib import Path
from unittest import mock

AMBIENT_KINDS = {"$", "PS>", "read", "edit", "write", "find", "web", "agent"}
KEY = "k" * 43
SECRET = "abc123secretvalue"
WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"   # the page, looks.js, the fonts


class FeedChecks:
    """Mixed into one TestCase per feed. start(home, port, seed) runs it; stop() ends it."""
    PORT = 0

    @classmethod
    def setUpClass(cls):
        cls.old_home = os.environ.get("GLITCHWALL_HOME")
        cls.home = Path(tempfile.mkdtemp(prefix="gw-parity-"))
        os.environ["GLITCHWALL_HOME"] = str(cls.home)
        (cls.home / "key").write_text(KEY, encoding="utf-8")
        cls.seed = cls.home / "seed.jsonl"
        cls.seed.write_text("", encoding="utf-8")
        cls.settings({})
        cls.start(cls.home, cls.PORT, cls.seed)
        for _ in range(100):   # until it answers
            try:
                cls.fetch_cls()
                break
            except OSError:
                time.sleep(0.05)

    @classmethod
    def tearDownClass(cls):
        cls.stop()
        if cls.old_home is None:
            os.environ.pop("GLITCHWALL_HOME", None)
        else:
            os.environ["GLITCHWALL_HOME"] = cls.old_home

    # ---- helpers
    @classmethod
    def settings(cls, s: dict):
        f = cls.home / "settings.json"
        f.write_text(json.dumps(s), encoding="utf-8")
        t = time.time() + getattr(cls, "_bump", 0)   # a new mtime every time, however fast the writes come
        cls._bump = getattr(cls, "_bump", 0) + 1
        os.utime(f, (t, t))

    @classmethod
    def get_cls(cls, path, headers=None):
        c = http.client.HTTPConnection("127.0.0.1", cls.PORT, timeout=5)
        c.putrequest("GET", path, skip_host=True)
        for k, v in (headers or {"Host": f"127.0.0.1:{cls.PORT}"}).items():
            c.putheader(k, v)
        c.endheaders()
        r = c.getresponse()
        body = r.read()
        c.close()
        return r, body

    @classmethod
    def fetch_cls(cls, since=0):
        return json.loads(cls.get_cls(f"/feed?key={KEY}&since={since}")[1])

    def get(self, path, headers=None):
        return self.get_cls(path, headers)

    def fetch(self, since=0):
        return self.fetch_cls(since)

    def add(self, *evs):
        with open(self.seed, "a", encoding="utf-8", newline="\n") as f:
            for e in evs:
                f.write(json.dumps(e) + "\n")

    def settle(self):   # the seed file is followed every 50 ms
        time.sleep(0.4)

    def new_events(self, since):
        return [e["text"] for e in self.fetch(since)["events"]]

    # ---- the checks (each starts from where the last left the store: they look only at what they add)
    def test_good_request_redacts(self):
        last = self.fetch()["last"]
        self.add({"kind": "$", "ref": "a1", "text": "export API_TOKEN=" + SECRET, "proj": "p"},
                 {"kind": "out", "ref": "a1", "text": "ok", "proj": "p"})
        self.settle()
        r, body = self.get(f"/feed?key={KEY}&since={last}")
        self.assertEqual(r.status, 200)
        self.assertEqual(r.getheader("Content-Type"), "application/json")
        self.assertEqual(r.getheader("Cache-Control"), "no-store")
        d = json.loads(body)
        self.assertEqual([e["kind"] for e in d["events"]], ["$", "out"])
        self.assertEqual([e["id"] for e in d["events"]], [last + 1, last + 2])
        self.assertEqual(d["last"], last + 2)
        self.assertNotIn(SECRET, body.decode())
        self.assertFalse(d["paused"])
        self.assertEqual(d["mode"], "live")

    def test_cors_echo_only_for_allowed_origin(self):
        for origin in ("null", f"http://127.0.0.1:{self.PORT}", f"http://localhost:{self.PORT}"):
            with self.subTest(origin):
                r, _ = self.get(f"/feed?key={KEY}", {"Host": f"127.0.0.1:{self.PORT}", "Origin": origin})
                self.assertEqual(r.status, 200)
                self.assertEqual(r.getheader("Access-Control-Allow-Origin"), origin)
                self.assertEqual(r.getheader("Vary"), "Origin")
        r, _ = self.get(f"/feed?key={KEY}")
        self.assertIsNone(r.getheader("Access-Control-Allow-Origin"))

    def test_safe_headers_everywhere(self):
        for path in (f"/feed?key={KEY}", "/feed", "/nope"):
            with self.subTest(path):
                r, _ = self.get(path)
                self.assertEqual(r.getheader("X-Content-Type-Options"), "nosniff")
                self.assertEqual(r.getheader("X-Frame-Options"), "SAMEORIGIN")
                self.assertEqual(r.getheader("Content-Security-Policy"), "frame-ancestors 'self'")
                self.assertEqual(r.getheader("Referrer-Policy"), "no-referrer")

    def test_refusals(self):
        p = self.PORT
        cases = {
            "no key": ("/feed", {"Host": f"127.0.0.1:{p}"}),
            "bad key": ("/feed?key=nope", {"Host": f"127.0.0.1:{p}"}),
            "short key": (f"/feed?key={KEY[:-1]}", {"Host": f"127.0.0.1:{p}"}),
            "no host": (f"/feed?key={KEY}", {}),
            "rebinding host": (f"/feed?key={KEY}", {"Host": f"evil.example:{p}"}),
            "web origin": (f"/feed?key={KEY}", {"Host": f"127.0.0.1:{p}", "Origin": "https://evil.example"}),
            "other local server": (f"/feed?key={KEY}", {"Host": f"127.0.0.1:{p}", "Origin": "http://localhost:3000"}),
        }
        for name, (path, headers) in cases.items():
            with self.subTest(name):
                r, body = self.get(path, headers or {"X-None": "1"})
                self.assertEqual(r.status, 403)
                self.assertEqual(body, b"")
                self.assertIsNone(r.getheader("Access-Control-Allow-Origin"))
        log = (self.home / "last-refused.txt").read_text(encoding="utf-8")
        self.assertIn("origin=http://localhost:3000", log)   # the last one turned away
        self.assertNotIn(KEY, log)

    def test_unknown_path_404(self):
        for p in ("/../key", "/feedx", "/nope.js", "/fonts", "/fonts/", "/fonts/../../settings.py", "/..%2f..%2fsettings.py",
                  "/%2e%2e/settings.py", "/..\\settings.py", "/C:/Windows/win.ini", "/fonts/licenses/../../../key"):
            with self.subTest(p):
                r, body = self.get(p)
                self.assertEqual((r.status, body), (404, b""))

    # ---- piece 2b: the page, the settings window, the settings in the feed
    def test_static_files(self):
        for p, name in (("/", "index.html"), ("/index.html", "index.html"), ("/settings.html", "settings.html"),
                        ("/looks.js", "looks.js"), ("/fonts/VT323-Regular.ttf", "fonts/VT323-Regular.ttf"),
                        ("/index.html?screen=1", "index.html")):
            with self.subTest(p):
                r, body = self.get(p)
                self.assertEqual(r.status, 200)
                self.assertEqual(body, (WALLPAPER / name).read_bytes())
                self.assertEqual(r.getheader("Cache-Control"), "no-store")
                self.assertIsNone(r.getheader("Access-Control-Allow-Origin"))
        self.assertEqual(self.get("/index.html")[0].getheader("Content-Type"), "text/html")
        self.assertIn("javascript", self.get("/looks.js")[0].getheader("Content-Type"))
        r, body = self.get("/config.js")   # the key stays in the page address (#key=), out of every file
        self.assertEqual((r.status, body), (200, b"// the key is in the page address\n"))
        for headers in ({"Host": f"evil.example:{self.PORT}"}, {"X-None": "1"}):
            with self.subTest(headers):
                r, body = self.get("/index.html", headers)
                self.assertEqual((r.status, body), (403, b""))

    def expected_feed(self):
        from glitchwall import settings as cfg
        from glitchwall import themes
        s = cfg.load_settings()
        return {"look": themes.look(s), "mode": cfg.mode(s), "label": cfg.display_label(s), "tune": cfg.tune(s),
                "opts": cfg.opts(s), "corner": cfg.corner(s), "masked": cfg.masked(s), "paused": bool(s.get("paused"))}

    def test_feed_fields_from_settings(self):
        cases = [{}, {"theme": "NEON", "palette": "nope", "font": "doto", "linger": 99, "glass": "x", "cards": " 3 ",
                      "intensity": True, "grain": 2.7, "corner": "Bottom-Right", "masked": "words", "orb_spin": "nope"},
                 {"agent_claude": False, "agent_gemini": True, "agent_codex": True},
                 {"agent_claude": False, "agent_gemini": "yes"}, {"label": "MINE ☃"}, {"label": ""}, {"label": 5},
                 {"mode": "ambient"}, {"mode": "ART", "corner": "off"}, {"masked": "Words", "corner": 7, "theme": None}]
        try:
            for s in cases:
                with self.subTest(s):
                    self.settings(s)
                    d = self.fetch()
                    self.assertEqual({k: d[k] for k in self.expected_feed()}, self.expected_feed())
        finally:
            self.settings({})

    def test_broken_file_keeps_last_good(self):
        f = self.home / "settings.json"
        try:
            self.settings({"theme": "neon", "paused": False})
            self.assertEqual(self.fetch()["look"]["theme"], "neon")
            f.write_text("{oops", encoding="utf-8")
            os.utime(f, (time.time() + 99, time.time() + 99))
            d = self.fetch()
            self.assertEqual((d["look"]["theme"], d["paused"]), ("neon", False))
            st, w = self.settings_req()   # the window reads the file itself: unreadable counts as paused
            self.assertTrue(w["settings"]["paused"])
            f.write_text('\ufeff{"theme": "neon", "paused": true}', encoding="utf-8")   # PowerShell's BOM is fine
            os.utime(f, (time.time() + 199, time.time() + 199))
            self.assertTrue(self.fetch()["paused"])
        finally:
            self.settings({})

    # ---- the settings window's API (test_server.py's TestSettingsWindow, against either feed)
    def settings_req(self, method="GET", key=KEY, body=None, origin="me", ctype="application/json", host=None):
        me = f"http://127.0.0.1:{self.PORT}"
        c = http.client.HTTPConnection("127.0.0.1", self.PORT, timeout=5)
        hd = {"Host": host or f"127.0.0.1:{self.PORT}"}
        if origin:
            hd["Origin"] = me if origin == "me" else origin
        if body is not None:
            hd["Content-Type"] = ctype
            body = body if isinstance(body, bytes) else json.dumps(body).encode()
        c.request(method, f"/settings?key={key}", body=body, headers=hd)
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, (json.loads(data) if data else None)

    def expected_window(self, refused=()):
        from glitchwall import settings as cfg
        from glitchwall import themes
        s = cfg.load_settings()
        return {"settings": {k: s.get(k, v) for k, v in cfg.DEFAULTS.items()}, "refused": list(refused),
                "defaults": {k: v for k, v in cfg.DEFAULTS.items() if k not in ("port", "host", "screens", "renderer")},
                "choices": {**themes.KINDS, **themes.OPTIONS, "mode": cfg.MODES, "corner": cfg.CORNERS, "masked": cfg.MASKED},
                "options": themes.OPTIONS_OF, "theme_rain": themes.THEME_RAIN,
                "ranges": {k: [lo, hi] for k, (_, lo, hi) in cfg.TUNE.items()}, "running": True, "restarting": False}

    def saved_text(self):   # what save_settings writes for what's on disk now
        from glitchwall import settings as cfg
        s = cfg.load_settings()
        return json.dumps({k: s[k] for k in cfg.DEFAULTS if k in s}, indent=2) + "\n"

    def test_settings_read(self):
        try:
            for s in ({}, {"theme": "neon", "cards": "x", "orb_spin": "nope", "extra": 1, "label": "é"}):
                with self.subTest(s):
                    self.settings(s)
                    for origin in (None, "me", "null"):
                        st, d = self.settings_req(origin=origin)
                        self.assertEqual(st, 200)
                        self.assertEqual(d, self.expected_window())
                    self.assertNotIn("key", json.dumps(d["settings"]).lower().replace("mask", ""))
        finally:
            self.settings({})

    def test_settings_change_saves(self):
        f = self.home / "settings.json"
        try:
            st, d = self.settings_req("POST", body={"cards": 2, "mask_email": False, "port": 1, "label": "héllo ☃",
                                                    "glass": 5, "theme": "neon", "orb_spin": "nope", "linger": True})
            self.assertEqual(st, 200)
            self.assertEqual(d, self.expected_window(["port", "glass", "orb_spin", "linger"]))
            self.assertEqual((d["settings"]["cards"], d["settings"]["mask_email"], d["settings"]["label"]), (2, False, "héllo ☃"))
            self.assertEqual(f.read_text(encoding="utf-8"), self.saved_text())   # byte for byte as Python writes it
            self.assertFalse([p for p in self.home.iterdir() if p.name.endswith(".tmp")])
            d = self.fetch()
            self.assertEqual((d["tune"]["cards"], d["look"]["theme"], d["label"]), (2, "neon", "héllo ☃"))
            st, d = self.settings_req("POST", body=[1, 2])   # not an object: no changes, saved as it is
            self.assertEqual((st, d["refused"]), (200, []))
        finally:
            self.settings({})

    def test_settings_reset_all(self):
        try:
            self.settings_req("POST", body={"theme": "neon", "cards": 1, "font": "doto", "mask_user": False,
                                            "mode": "ambient", "paused": True})
            st, d = self.settings_req("POST", body={"reset": True, "glass": 30, "linger": 0})
            self.assertEqual(st, 200)
            got = {k: d["settings"][k] for k in ("theme", "cards", "font", "mask_user", "mode", "paused", "glass")}
            self.assertEqual(got, {"theme": "phosphor", "cards": 4, "font": "theme", "mask_user": True,
                                   "mode": "ambient", "paused": True, "glass": 30})   # mode and pause are kept
            self.assertEqual(d, self.expected_window(["linger"]))
            self.assertFalse(d["restarting"])   # no built-in host here, so nothing to restart
            st, d = self.settings_req("POST", body={"reset": "yes", "cards": 2})   # only true resets
            self.assertEqual((d["settings"]["cards"], d["settings"]["glass"]), (2, 30))
        finally:
            self.settings({})

    def test_settings_refusals(self):
        try:
            self.settings({})
            cases = {
                "wrong key": dict(method="POST", key="x" * 43, body={"paused": True}),
                "no origin": dict(method="POST", origin=None, body={"paused": True}),
                "other site": dict(method="POST", origin="https://evil.example", body={"paused": True}),
                "file page": dict(method="POST", origin="null", body={"paused": True}),   # or a sandboxed frame
                "other local server": dict(method="POST", origin="http://localhost:3000", body={"paused": True}),
                "form post": dict(method="POST", body=b"paused=true", ctype="application/x-www-form-urlencoded"),
                "text post": dict(method="POST", body=b'{"paused": true}', ctype="text/plain"),
                "json-ish post": dict(method="POST", body=b'{"paused": true}', ctype="application/json-x"),
                "rebinding": dict(method="POST", body={"paused": True}, host=f"evil.example:{self.PORT}"),
                "read, wrong key": dict(key="x" * 43),
                "read, other site": dict(origin="https://evil.example"),
                "read, rebinding": dict(host=f"evil.example:{self.PORT}"),
            }
            for name, kw in cases.items():
                with self.subTest(name):
                    self.assertEqual(self.settings_req(**kw), (403, None))
            self.assertEqual(self.settings_req("POST", body={"paused": True}, ctype="application/json; charset=utf-8")[0], 200)
            self.settings({})
            self.assertEqual(self.settings_req("POST", body=b"{nope")[0], 400)
            self.assertEqual(self.settings_req("POST", body=b'\xff{"paused": true}')[0], 400)
            self.assertEqual(self.settings_req("POST", body=b"x" * 8193)[0], 400)   # too long
            for length in ("abc", "0", "-1", "8193"):   # a Content-Length that won't do
                with self.subTest(length):
                    c = http.client.HTTPConnection("127.0.0.1", self.PORT, timeout=5)
                    c.putrequest("POST", f"/settings?key={KEY}", skip_host=True)
                    for k, v in {"Host": f"127.0.0.1:{self.PORT}", "Origin": f"http://127.0.0.1:{self.PORT}",
                                 "Content-Type": "application/json", "Content-Length": length}.items():
                        c.putheader(k, v)
                    c.endheaders()
                    self.assertEqual(c.getresponse().status, 400)
                    c.close()
            self.assertFalse(self.fetch()["paused"])
        finally:
            self.settings({})

    def test_commands_only_and_pause(self):
        try:
            last = self.fetch()["last"]
            self.add({"kind": "$", "text": "cmd"}, {"kind": "out", "text": "its output"}, {"kind": "err", "text": "e"})
            self.settle()
            self.settings({"commands_only": True})
            self.assertEqual(self.new_events(last), ["cmd"])
            self.settings({"paused": True})
            d = self.fetch(last)
            self.assertEqual((d["events"], d["paused"]), ([], True))
        finally:
            self.settings({})

    def test_resume_never_replays_the_pause(self):
        try:
            last = self.fetch()["last"]
            self.add({"kind": "$", "text": "queued, not yet shown"})
            self.settle()
            self.settings({"paused": True})
            self.assertEqual(self.fetch(last)["events"], [])
            self.add({"kind": "$", "text": "typed while paused"})
            self.settle()
            self.settings({})
            self.add({"kind": "$", "text": "after resume"})
            self.settle()
            self.assertEqual(self.new_events(last), ["after resume"])
            last = self.fetch()["last"]
            self.settings({"mode": "art"})   # art mode clears the screen the same way
            self.add({"kind": "$", "text": "during art"})
            self.settle()
            d = self.fetch(last)
            self.assertEqual((d["events"], d["mode"]), ([], "art"))
            self.settings({"mode": "live"})
            self.assertEqual(self.new_events(last), [])
        finally:
            self.settings({})

    def test_batch_and_keep(self):
        last = self.fetch()["last"]
        self.add(*({"kind": "$", "text": f"n{i}"} for i in range(70)))
        time.sleep(0.8)
        d = self.fetch(last)
        self.assertEqual(len(d["events"]), 60)                 # BATCH: the newest 60
        self.assertEqual(d["events"][-1]["text"], "n69")
        self.assertEqual(self.new_events("junk")[-1], "n69")   # a since that isn't a number reads as 0

    # ---- piece 2c: ambient mode follows the setting, no restart
    def test_ambient_mode(self):
        from datetime import timedelta
        from glitchwall import ambient
        from glitchwall import settings as cfg
        try:
            last = self.fetch()["last"]
            self.settings({"mode": "ambient"})
            evs = []
            for _ in range(40):   # the mode is looked at once a second; an episode's first call comes at once
                time.sleep(0.1)
                evs = self.fetch(last)["events"]
                if evs:
                    break
            self.assertTrue(evs)
            d = self.fetch(last)
            self.assertEqual((d["mode"], d["label"]), ("ambient", cfg.AMBIENT_LABEL))
            for e in evs:
                self.assertIn(e["kind"], AMBIENT_KINDS | {"out", "err"})
                self.assertIn(e["proj"], ambient.PROJECTS)
                self.assertRegex(e["session"], r"^[0-9a-f]{4}$")
                self.assertRegex(e["ref"], r"^[0-9a-f]{12}$")
                self.assertEqual(e["side"], e["kind"] == "agent")
                self.assertEqual(datetime.fromisoformat(e["ts"]).utcoffset(), timedelta(0))
                self.assertEqual(set(e), {"proj", "session", "side", "ts", "kind", "ref", "text", "id"})
        finally:
            self.settings({})
            time.sleep(1.5)   # back in live mode: the made-up sessions stop before the next check counts events

    def test_long_text_clipped(self):
        last = self.fetch()["last"]
        self.add({"kind": "out", "text": "é" * 2000, "proj": "p" * 100})
        self.settle()
        e = self.fetch(last)["events"][0]
        self.assertEqual((len(e["text"]), len(e["proj"])), (1600, 60))


class TestPythonFeed(FeedChecks, unittest.TestCase):
    PORT = 47911

    @classmethod
    def start(cls, home, port, seed):
        from glitchwall import settings as cfg
        from glitchwall.redact import Redactor
        from glitchwall.server import EventStore, LiveSettings, make_handler, serve
        live = LiveSettings()
        store = EventStore(Redactor(), hidden=lambda: bool(live.get().get("paused")))
        # as run_feed sets it up: it serves the page and the settings window too
        h = make_handler(store, cfg.load_key(), port, live, refusal_log=home / "last-refused.txt",
                         static_dir=WALLPAPER, config_js="// the key is in the page address\n", editable=True,
                         extra_origins=(f"http://127.0.0.1:{port}", f"http://localhost:{port}"))
        cls.httpd = serve(h, port)
        threading.Thread(target=cls.httpd.serve_forever, daemon=True).start()
        cls.quit = threading.Event()

        def follow():   # what --seed does in the Rust feed
            pos = 0
            while not cls.quit.is_set():
                data = seed.read_bytes()
                nl = data.rfind(b"\n")
                if nl >= pos:
                    for line in data[pos:nl].split(b"\n"):
                        if line.strip():
                            store.add(json.loads(line))
                    pos = nl + 1
                time.sleep(0.05)
        threading.Thread(target=follow, daemon=True).start()
        # the modes' own sources follow the settings, as in run_feed; the seed file stands in for the agents' logs
        from glitchwall import server
        cls.no_readers = mock.patch.object(server, "start_reader", lambda agent, store: threading.Event())
        cls.no_readers.start()
        threading.Thread(target=server.follow_mode, args=(store, live), kwargs={"quit": cls.quit}, daemon=True).start()

    @classmethod
    def stop(cls):
        cls.quit.set()
        cls.no_readers.stop()
        cls.httpd.shutdown()
        cls.httpd.server_close()


@unittest.skipUnless(os.environ.get("GW_FEED_EXE"), "set GW_FEED_EXE to the glitchwall-render exe to check the Rust feed")
class TestRustFeed(FeedChecks, unittest.TestCase):
    PORT = 47912

    @classmethod
    def start(cls, home, port, seed):
        cls.proc = subprocess.Popen([os.environ["GW_FEED_EXE"], "feed", "--port", str(port), "--seed", str(seed),
                                     "--wallpaper", str(WALLPAPER)],
                                    env={**os.environ, "GLITCHWALL_HOME": str(home)})

    @classmethod
    def stop(cls):
        cls.proc.kill()
        cls.proc.wait(5)


if __name__ == "__main__":
    unittest.main()
