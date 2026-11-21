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
            self.assertEqual(d["look"], {"theme": "phosphor", "rain": "theme", "font": "theme"})
            s.update(theme="synthwave", rain="stars", font="bogus")     # `glitchwall config --theme ...`
            cfg.save_settings(s)
            os.utime(cfg.settings_file(), (time.time() + 3, time.time() + 3))
            self.assertEqual(fetch()["look"], {"theme": "synthwave", "rain": "stars", "font": "theme"})
        finally:
            httpd2.shutdown()
            httpd2.server_close()
            cfg.save_settings(cfg.DEFAULTS)

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
