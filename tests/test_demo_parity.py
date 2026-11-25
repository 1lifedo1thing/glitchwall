"""`glitchwall demo` against both feeds: the Python one (demo.py's run_demo) and the Rust one (glitchwall-render demo).

Piece 2c of docs/native-plan.md. Each runs in live (the scripted session), ambient and art mode, with a fresh state
folder whose settings.json changes the theme and a slider: the demo takes the look from the settings plus what it is
given (--rain stars) and everything else from the defaults. The Rust one runs only with GW_FEED_EXE set.
"""
import http.client
import json
import os
import re
import subprocess
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"
KINDS = {"$", "PS>", "read", "edit", "write", "find", "web", "agent"}
SAVED = {"theme": "amber", "cards": 2, "palette": "neon"}
FAKE_SECRETS = ("live_51HdemoNotARealKey", "demoDemoDemoDemoDemo", "hunter2")


class DemoChecks:
    MODE = "live"
    PORT = 0

    @classmethod
    def setUpClass(cls):
        cls.old_home = os.environ.get("GLITCHWALL_HOME")
        cls.home = Path(tempfile.mkdtemp(prefix="gw-demo-"))
        os.environ["GLITCHWALL_HOME"] = str(cls.home)
        (cls.home / "settings.json").write_text(json.dumps(SAVED), encoding="utf-8")
        cls.start()
        for _ in range(100):
            try:
                st, body = cls.get("/config.js")
                if st == 200:
                    break
            except OSError:
                pass
            time.sleep(0.05)
        cls.config = body.decode()
        cls.key = json.loads(re.fullmatch(r"window\.FEED = (\{.*\});\n", cls.config).group(1))["key"]

    @classmethod
    def tearDownClass(cls):
        cls.stop()
        if cls.old_home is None:
            os.environ.pop("GLITCHWALL_HOME", None)
        else:
            os.environ["GLITCHWALL_HOME"] = cls.old_home

    @classmethod
    def get(cls, path, headers=None, method="GET", body=None):
        c = http.client.HTTPConnection("127.0.0.1", cls.PORT, timeout=5)
        c.putrequest(method, path, skip_host=True)
        for k, v in (headers or {"Host": f"127.0.0.1:{cls.PORT}"}).items():
            c.putheader(k, v)
        if body is not None:
            c.putheader("Content-Length", str(len(body)))
        c.endheaders(body)
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, data

    def feed(self):
        st, body = self.get(f"/feed?key={self.key}")
        self.assertEqual(st, 200)
        return json.loads(body), body.decode()

    def label(self):
        from glitchwall import settings as cfg
        return cfg.display_label({**cfg.load_settings(), "mode": self.MODE})

    def test_config_js(self):
        d = json.loads(re.fullmatch(r"window\.FEED = (\{.*\});\n", self.config).group(1))
        self.assertEqual(list(d), ["port", "key", "label"])
        self.assertEqual((d["port"], d["label"]), (self.PORT, self.label() + "  [demo]"))
        self.assertRegex(d["key"], r"^[A-Za-z0-9_-]{32}$")   # random per run

    def test_feed(self):
        from glitchwall import ambient, demo, themes
        from glitchwall import settings as cfg
        d, raw = self.feed()
        self.assertEqual(d["mode"], self.MODE)
        self.assertEqual(d["label"], self.label() + "  [demo]" if self.label() else "")
        self.assertEqual(d["look"], themes.look({**cfg.load_settings(), "rain": "stars"}))
        self.assertEqual((d["tune"], d["opts"], d["paused"]), (cfg.tune(cfg.DEFAULTS), cfg.opts(cfg.DEFAULTS), False))
        evs = []
        for _ in range(60):
            time.sleep(0.05)
            d, raw = self.feed()
            evs = d["events"]
            if len(evs) >= 12 or self.MODE == "art":
                break
        if self.MODE == "art":
            self.assertEqual(evs, [])
            return
        self.assertGreaterEqual(len(evs), 4)
        for e in evs:
            self.assertIn(e["kind"], KINDS | {"out", "err"})
            self.assertIn(e["proj"], ambient.PROJECTS if self.MODE == "ambient" else demo.PROJECTS)
        if self.MODE == "live":
            script = {t for _, text, out, _ in demo.SCRIPT for t in (text, out)}
            for e in evs:
                if "••••" not in e["text"]:
                    self.assertIn(e["text"], script)
            self.assertEqual([e["text"] for e in evs[:2]], [demo.SCRIPT[0][1], demo.SCRIPT[0][2]])
        for s in FAKE_SECRETS:
            self.assertNotIn(s, raw)

    def test_page_and_no_settings_window(self):
        st, body = self.get("/")
        self.assertEqual((st, body), (200, (WALLPAPER / "index.html").read_bytes()))
        self.assertEqual(self.get(f"/settings?key={self.key}")[0], 404)
        hd = {"Host": f"127.0.0.1:{self.PORT}", "Origin": f"http://127.0.0.1:{self.PORT}", "Content-Type": "application/json"}
        self.assertEqual(self.get(f"/settings?key={self.key}", hd, "POST", b'{"paused": true}')[0], 404)
        self.assertEqual(json.loads((self.home / "settings.json").read_text(encoding="utf-8")), SAVED)   # left alone

    def test_refusals(self):
        p = self.PORT
        for path, headers in ((f"/feed?key={'x' * 32}", {"Host": f"127.0.0.1:{p}"}),
                              (f"/feed?key={self.key}", {"Host": f"127.0.0.1:{p}", "Origin": "https://evil.example"}),
                              (f"/feed?key={self.key}", {"Host": f"evil.example:{p}"}),
                              ("/index.html", {"Host": f"evil.example:{p}"})):
            with self.subTest(path=path, headers=headers):
                self.assertEqual(self.get(path, headers), (403, b""))


class PythonDemo(DemoChecks):
    @classmethod
    def start(cls):
        from glitchwall import demo
        cls.servers = []
        serve = demo.serve

        def keep(h, port):
            httpd = serve(h, port)
            cls.servers.append(httpd)
            return httpd
        cls.patch = mock.patch.object(demo, "serve", keep)
        cls.patch.start()
        with mock.patch("builtins.print"):
            threading.Thread(target=demo.run_demo, args=(cls.PORT, 0.02, {"rain": "stars"}, cls.MODE), daemon=True).start()
            for _ in range(100):
                if cls.servers:
                    break
                time.sleep(0.02)

    @classmethod
    def stop(cls):
        for s in cls.servers:
            s.shutdown()
            s.server_close()
        cls.patch.stop()


class RustDemo(DemoChecks):
    @classmethod
    def start(cls):
        cls.proc = subprocess.Popen([os.environ["GW_FEED_EXE"], "demo", "--port", str(cls.PORT), "--pace", "0.02",
                                     "--mode", cls.MODE, "--rain", "stars", "--wallpaper", str(WALLPAPER)],
                                    env={**os.environ, "GLITCHWALL_HOME": str(cls.home)},
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    @classmethod
    def stop(cls):
        cls.proc.kill()
        cls.proc.wait(5)


rust = unittest.skipUnless(os.environ.get("GW_FEED_EXE"), "set GW_FEED_EXE to the glitchwall-render exe to check the Rust demo")


class TestPythonDemoLive(PythonDemo, unittest.TestCase):
    PORT, MODE = 47921, "live"


class TestPythonDemoAmbient(PythonDemo, unittest.TestCase):
    PORT, MODE = 47922, "ambient"


class TestPythonDemoArt(PythonDemo, unittest.TestCase):
    PORT, MODE = 47923, "art"


@rust
class TestRustDemoLive(RustDemo, unittest.TestCase):
    PORT, MODE = 47924, "live"


@rust
class TestRustDemoAmbient(RustDemo, unittest.TestCase):
    PORT, MODE = 47925, "ambient"


@rust
class TestRustDemoArt(RustDemo, unittest.TestCase):
    PORT, MODE = 47926, "art"


if __name__ == "__main__":
    unittest.main()
