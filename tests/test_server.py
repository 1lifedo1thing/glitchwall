"""Key / Host / Origin checks, both as a pure function and against a real local server."""
import http.client
import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path

os.environ["GLITCHWALL_HOME"] = tempfile.mkdtemp(prefix="alw-test-")

from glitchwall import settings as cfg  # noqa: E402
from glitchwall.redact import Redactor  # noqa: E402
from glitchwall.server import EventStore, LiveSettings, check_access, make_handler, serve  # noqa: E402

KEY = "k" * 43
PORT = 47899


class TestCheckAccess(unittest.TestCase):
    def ok(self, host=f"127.0.0.1:{PORT}", origin=None, key=KEY):
        return check_access(host, origin, key, KEY, PORT)

    def test_allows_wallpaper(self):
        self.assertTrue(self.ok())
        self.assertTrue(self.ok(host=f"localhost:{PORT}"))
        self.assertTrue(self.ok(origin="null"))
        self.assertTrue(self.ok(origin="https://a1b2c3.localhost"))
        self.assertTrue(self.ok(origin="http://localhost:8080"))

    def test_wrong_or_missing_key(self):
        self.assertFalse(self.ok(key=""))
        self.assertFalse(self.ok(key="x" * 43))
        self.assertFalse(self.ok(key=KEY[:-1]))
        self.assertFalse(check_access(f"127.0.0.1:{PORT}", None, "", "", PORT))

    def test_bad_host(self):   # DNS rebinding: evil.example resolving to 127.0.0.1
        for h in ("evil.example:47899", f"127.0.0.1:{PORT + 1}", "127.0.0.1", None, "", f"0.0.0.0:{PORT}",
                  f"127.0.0.1.evil.example:{PORT}"):
            with self.subTest(h):
                self.assertFalse(self.ok(host=h))

    def test_bad_origin(self):
        for o in ("https://evil.example", "http://127.0.0.1:8000", "https://localhost.evil.example",
                  "https://evil.example/.localhost", "file://", "https://x.localhost.evil.example", ""):
            with self.subTest(o):
                self.assertFalse(self.ok(origin=o))


class TestLiveServer(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.store = EventStore(Redactor(home="/home/octo", user="octo"))
        cls.store.add({"kind": "$", "ref": "a1", "text": "export API_TOKEN=" + "abc123secretvalue", "proj": "p"})
        cls.store.add({"kind": "out", "ref": "a1", "text": "ok", "proj": "p"})
        cls.refused = Path(os.environ["GLITCHWALL_HOME"]) / "last-refused.txt"
        cls.httpd = serve(make_handler(cls.store, KEY, PORT, refusal_log=cls.refused), PORT)
        threading.Thread(target=cls.httpd.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()

    def get(self, path, headers=None, port=PORT):
        c = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
        c.putrequest("GET", path, skip_host=True)
        for k, v in (headers or {"Host": f"127.0.0.1:{port}"}).items():
            c.putheader(k, v)
        c.endheaders()
        r = c.getresponse()
        body = r.read()
        c.close()
        return r, body

    def test_good_request(self):
        r, body = self.get(f"/feed?key={KEY}")
        self.assertEqual(r.status, 200)
        data = json.loads(body)
        self.assertEqual([e["kind"] for e in data["events"]], ["$", "out"])
        self.assertNotIn("abc123secretvalue", body.decode())

    def test_cors_echo_only_for_allowed_origin(self):
        r, _ = self.get(f"/feed?key={KEY}", {"Host": f"127.0.0.1:{PORT}", "Origin": "https://ab12.localhost"})
        self.assertEqual(r.status, 200)
        self.assertEqual(r.getheader("Access-Control-Allow-Origin"), "https://ab12.localhost")

    def test_refusals(self):
        cases = {
            "no key": ("/feed", {"Host": f"127.0.0.1:{PORT}"}),
            "bad key": ("/feed?key=nope", {"Host": f"127.0.0.1:{PORT}"}),
            "rebinding host": (f"/feed?key={KEY}", {"Host": f"evil.example:{PORT}"}),
            "web origin": (f"/feed?key={KEY}", {"Host": f"127.0.0.1:{PORT}", "Origin": "https://evil.example"}),
        }
        for name, (path, headers) in cases.items():
            with self.subTest(name):
                r, body = self.get(path, headers)
                self.assertEqual(r.status, 403)
                self.assertEqual(body, b"")
                self.assertIsNone(r.getheader("Access-Control-Allow-Origin"))
        self.assertNotIn(KEY, self.refused.read_text(encoding="utf-8"))

    def test_unknown_path_404_and_no_static(self):
        for p in ("/", "/index.html", "/config.js", "/../key"):
            r, _ = self.get(p)
            self.assertEqual(r.status, 404, p)

    def test_commands_only_and_pause(self):
        s = cfg.load_settings()
        httpd2 = serve(make_handler(self.store, KEY, PORT + 1, LiveSettings()), PORT + 1)
        threading.Thread(target=httpd2.serve_forever, daemon=True).start()
        try:
            def fetch():
                return json.loads(self.get(f"/feed?key={KEY}", port=PORT + 1)[1])
            s["commands_only"] = True
            cfg.save_settings(s)
            os.utime(cfg.settings_file(), (time.time() + 1, time.time() + 1))
            self.assertEqual([e["kind"] for e in fetch()["events"]], ["$"])
            s["paused"] = True
            cfg.save_settings(s)
            os.utime(cfg.settings_file(), (time.time() + 2, time.time() + 2))
            d = fetch()
            self.assertEqual(d["events"], [])
            self.assertTrue(d["paused"])
            self.assertEqual(d["look"], {"theme": "phosphor", "palette": "theme", "rain": "theme", "font": "theme", "transition": "theme"})
            s.update(theme="synthwave", rain="stars", font="bogus")     # `glitchwall config --theme ...`
            cfg.save_settings(s)
            os.utime(cfg.settings_file(), (time.time() + 3, time.time() + 3))
            self.assertEqual(fetch()["look"], {"theme": "synthwave", "palette": "theme", "rain": "stars", "font": "theme", "transition": "theme"})
        finally:
            httpd2.shutdown()
            httpd2.server_close()
            cfg.save_settings(cfg.DEFAULTS)
            self.store.floor = 0   # the pause hid the shared store; other tests read it from the start

    def test_resume_never_replays_the_pause(self):
        paused = {"on": False}
        store = EventStore(Redactor(home="", user=""), hidden=lambda: paused["on"])
        store.add({"kind": "$", "text": "seen before the pause"})
        store.add({"kind": "$", "text": "queued, not yet shown"})
        s = dict(cfg.DEFAULTS)
        live = type("L", (), {"get": lambda self: s})()
        httpd2 = serve(make_handler(store, KEY, PORT + 2, live), PORT + 2)
        threading.Thread(target=httpd2.serve_forever, daemon=True).start()
        try:
            def fetch(since):
                return json.loads(self.get(f"/feed?key={KEY}&since={since}", port=PORT + 2)[1])
            s["paused"] = paused["on"] = True
            self.assertEqual(fetch(1)["events"], [])
            store.add({"kind": "$", "text": "typed while paused"})
            s["paused"] = paused["on"] = False
            store.add({"kind": "$", "text": "after resume"})
            self.assertEqual([e["text"] for e in fetch(1)["events"]], ["after resume"])
            s["mode"] = "art"                         # art mode clears the screen the same way
            store.add({"kind": "$", "text": "during art"})
            fetch(0)
            s["mode"] = "live"
            self.assertEqual(fetch(0)["events"], [])
        finally:
            httpd2.shutdown()
            httpd2.server_close()

    def test_sleep_flag_per_screen(self):
        # glitchwall's own host: a screen covered by a fullscreen app is told to stop animating
        asleep = {2}
        httpd3 = serve(make_handler(self.store, KEY, PORT + 3, sleeping=lambda scr: scr.isdigit() and int(scr) in asleep), PORT + 3)
        threading.Thread(target=httpd3.serve_forever, daemon=True).start()
        try:
            def sleep(q):
                return json.loads(self.get(f"/feed?key={KEY}{q}", port=PORT + 3)[1])["sleep"]
            self.assertFalse(sleep("&screen=1"))
            self.assertTrue(sleep("&screen=2"))
            self.assertFalse(sleep(""))
            self.assertFalse(sleep("&screen=x"))
        finally:
            httpd3.shutdown()
            httpd3.server_close()

    def test_host_setting(self):
        self.assertEqual(cfg.host({}), "lively")              # upgrades keep what they had until install picks
        self.assertEqual(cfg.host({"host": "glitchwall"}), "glitchwall")
        self.assertEqual(cfg.host({"host": "bogus"}), "lively")

    def test_tune_is_clamped(self):
        t = cfg.tune({"linger": 99, "cards": 0, "glass": "x", "intensity": 3})
        self.assertEqual((t["linger"], t["cards"], t["glass"], t["rain"]), (30, 1, 58, 3))

    def test_binds_loopback_only(self):
        self.assertEqual(self.httpd.server_address[0], "127.0.0.1")


class TestKey(unittest.TestCase):
    def test_key_is_random_and_stable(self):
        k1 = cfg.load_key()
        self.assertGreaterEqual(len(k1), 40)
        self.assertEqual(cfg.load_key(), k1)
        self.assertIn(k1, cfg.config_js(1, k1, "x"))


if __name__ == "__main__":
    unittest.main()


class TestSettingsWindow(unittest.TestCase):
    """The settings window's API: reads and changes, behind the same checks as the feed, plus JSON + Origin."""

    P = PORT + 4
    ME = f"http://127.0.0.1:{PORT + 4}"

    @classmethod
    def setUpClass(cls):
        cls.changed = []
        h = make_handler(EventStore(), KEY, cls.P, LiveSettings(), extra_origins=(cls.ME,), editable=True,
                         on_change=cls.changed.append)
        cls.httpd = serve(h, cls.P)
        threading.Thread(target=cls.httpd.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()
        cfg.save_settings(dict(cfg.DEFAULTS))

    def req(self, method="GET", key=KEY, body=None, origin=ME, ctype="application/json", host=None):
        c = http.client.HTTPConnection("127.0.0.1", self.P, timeout=5)
        hd = {"Host": host or f"127.0.0.1:{self.P}"}
        if origin:
            hd["Origin"] = origin
        if body is not None:
            hd["Content-Type"] = ctype
            body = body if isinstance(body, bytes) else json.dumps(body).encode()
        c.request(method, f"/settings?key={key}", body=body, headers=hd)
        r = c.getresponse()
        data = r.read()
        c.close()
        return r.status, (json.loads(data) if data else None)

    def test_apply_changes_validates(self):
        s, refused = cfg.apply_changes(dict(cfg.DEFAULTS), {
            "theme": "phosphor", "mode": "ambient", "paused": True, "cards": 3, "label": "hi",
            "glass": 5, "cards_x": 1, "port": 1, "host": "lively", "screens": "primary", "theme_bad": 1,
            "mask_user": "yes", "linger": True, "label2": "x"})
        self.assertEqual((s["mode"], s["paused"], s["cards"], s["label"]), ("ambient", True, 3, "hi"))
        self.assertEqual(s["glass"], cfg.DEFAULTS["glass"])
        self.assertEqual(sorted(refused), sorted(["glass", "cards_x", "port", "host", "screens", "theme_bad",
                                                  "mask_user", "linger", "label2"]))
        self.assertIn("label", cfg.apply_changes({}, {"label": "a\nb"})[1])
        self.assertIn("theme", cfg.apply_changes({}, {"theme": "no-such-theme"})[1])

    def test_read(self):
        st, d = self.req(origin=None)
        self.assertEqual(st, 200)
        self.assertIn("phosphor", d["choices"]["theme"])
        self.assertEqual(d["ranges"]["cards"], [1, 6])
        self.assertNotIn("key", json.dumps(d["settings"]).lower().replace("mask", ""))

    def test_change_saves_and_tells_the_feed(self):
        st, d = self.req("POST", body={"cards": 2, "mask_email": False, "port": 1})
        self.assertEqual(st, 200)
        self.assertEqual((d["settings"]["cards"], d["settings"]["mask_email"]), (2, False))
        self.assertEqual(d["refused"], ["port"])
        self.assertEqual(cfg.load_settings()["cards"], 2)
        self.assertEqual(cfg.load_settings()["port"], cfg.DEFAULT_PORT)
        self.assertFalse(self.changed[-1]["mask_email"])

    def test_refusals(self):
        cases = {
            "wrong key": dict(method="POST", key="x" * 43, body={"paused": True}),
            "no origin": dict(method="POST", origin=None, body={"paused": True}),
            "other site": dict(method="POST", origin="https://evil.example", body={"paused": True}),
            "form post": dict(method="POST", body=b"paused=true", ctype="application/x-www-form-urlencoded"),
            "text post": dict(method="POST", body=b'{"paused": true}', ctype="text/plain"),
            "rebinding": dict(method="POST", body={"paused": True}, host=f"evil.example:{self.P}"),
        }
        for name, kw in cases.items():
            with self.subTest(name):
                self.assertEqual(self.req(**kw)[0], 403)
        self.assertEqual(self.req("POST", body=b"{nope")[0], 400)
        self.assertFalse(cfg.load_settings()["paused"])

    def test_off_unless_editable(self):
        h = make_handler(EventStore(), KEY, self.P + 1)
        httpd = serve(h, self.P + 1)
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        try:
            c = http.client.HTTPConnection("127.0.0.1", self.P + 1, timeout=5)
            c.request("GET", f"/settings?key={KEY}", headers={"Host": f"127.0.0.1:{self.P + 1}"})
            self.assertEqual(c.getresponse().status, 404)
        finally:
            httpd.shutdown()
            httpd.server_close()
