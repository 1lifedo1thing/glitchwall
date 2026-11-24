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
from pathlib import Path

KEY = "k" * 43
SECRET = "abc123secretvalue"


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
        for p in ("/", "/index.html", "/config.js", "/../key", "/feedx", "/settings"):
            with self.subTest(p):
                self.assertEqual(self.get(p)[0].status, 404)

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
        h = make_handler(store, cfg.load_key(), port, live, refusal_log=home / "last-refused.txt",
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

    @classmethod
    def stop(cls):
        cls.quit.set()
        cls.httpd.shutdown()
        cls.httpd.server_close()


@unittest.skipUnless(os.environ.get("GW_FEED_EXE"), "set GW_FEED_EXE to the glitchwall-render exe to check the Rust feed")
class TestRustFeed(FeedChecks, unittest.TestCase):
    PORT = 47912

    @classmethod
    def start(cls, home, port, seed):
        cls.proc = subprocess.Popen([os.environ["GW_FEED_EXE"], "feed", "--port", str(port), "--seed", str(seed)],
                                    env={**os.environ, "GLITCHWALL_HOME": str(home)})

    @classmethod
    def stop(cls):
        cls.proc.kill()
        cls.proc.wait(5)


if __name__ == "__main__":
    unittest.main()
