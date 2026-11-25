"""Pieces 2d and 3b: glitchwall's desktop host runs the Rust feed (glitchwall-render feed, or host: the feed and the
windows in one process) as its child, with the Python feed as the fallback. Which exe qualifies, starting it, the asleep hand-off, restarts, ending with its parent, giving up.
The ones that run the exe need GW_FEED_EXE."""
import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import server, winhost

KEY = "k" * 43
WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"
EXE = os.environ.get("GW_FEED_EXE")


class TestWhichExe(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="gw-exe-"))
        self.new = self.dir / "new.exe"
        self.new.write_bytes(b"MZ...." + server.FEED_PROTOCOL + b"1....")
        self.old = self.dir / "old.exe"
        self.old.write_bytes(b"MZ.... a renderer without the feed ....")

    def pick(self, exe, s=None, env=None):
        with mock.patch.object(winhost, "render_exe", lambda: exe), mock.patch.object(server.os, "name", "nt"), \
                mock.patch.dict(os.environ, env or {}):
            if not env:
                os.environ.pop("GLITCHWALL_PYTHON_FEED", None)
            return server.rust_feed_exe({"renderer": "native"} if s is None else s)

    def test_only_an_exe_that_knows_the_feed(self):
        self.assertEqual(self.pick(self.new), self.new)
        self.assertIsNone(self.pick(self.old))   # would open wallpaper windows: never run to find out
        self.assertIsNone(self.pick(None))
        self.assertIsNone(self.pick(self.dir / "missing.exe"))

    def test_protocol_number(self):
        two = self.dir / "two.exe"
        two.write_bytes(b"MZ" + server.FEED_PROTOCOL + b"2\x00 more")
        self.assertEqual([server.feed_protocol(x) for x in (self.new, two, self.old, self.dir / "missing.exe")], [1, 2, 0, 0])
        self.assertEqual(self.pick(two), two)

    def test_only_with_the_native_renderer_and_no_override(self):
        self.assertIsNone(self.pick(self.new, {"renderer": "web"}))
        self.assertIsNone(self.pick(self.new, env={"GLITCHWALL_PYTHON_FEED": "1"}))

    @unittest.skipUnless(EXE, "set GW_FEED_EXE")
    def test_the_built_exe_qualifies(self):
        self.assertEqual(self.pick(Path(EXE)), Path(EXE))
        self.assertEqual(server.feed_protocol(Path(EXE)), 4)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@unittest.skipUnless(EXE and sys.platform == "win32", "set GW_FEED_EXE to the glitchwall-render exe")
class TestRustFeedChild(unittest.TestCase):
    def setUp(self):
        self.home = Path(tempfile.mkdtemp(prefix="gw-child-"))
        (self.home / "key").write_text(KEY, encoding="utf-8")
        self.env = mock.patch.dict(os.environ, {"GLITCHWALL_HOME": str(self.home)})
        self.env.start()
        self.port = free_port()
        self.host = mock.Mock(asleep=set())
        self.feed = server.RustFeed(Path(EXE), self.port, self.home / "asleep.json", self.host, WALLPAPER)
        self.stop = threading.Event()

    def tearDown(self):
        self.stop.set()
        self.feed.end()
        self.env.stop()

    def fetch(self, screen=""):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        c.request("GET", f"/feed?key={KEY}&screen={screen}", headers={"Host": f"127.0.0.1:{self.port}"})
        r = c.getresponse()
        d = json.loads(r.read())
        c.close()
        return d

    def test_starts_tells_sleep_and_comes_back(self):
        self.assertTrue(self.feed.start())
        self.assertEqual(self.fetch()["mode"], "live")
        gave_up = []
        threading.Thread(target=self.feed.watch, args=(self.stop, lambda: gave_up.append(1)), daemon=True).start()
        self.host.asleep = {1}
        time.sleep(1.2)
        self.assertEqual((self.fetch("1")["sleep"], self.fetch("2")["sleep"]), (True, False))
        self.host.asleep = set()
        time.sleep(1.2)
        self.assertFalse(self.fetch("1")["sleep"])
        first = self.feed.proc.pid
        self.feed.proc.kill()   # it stops: started again
        for _ in range(80):
            time.sleep(0.1)
            if self.feed.proc.pid != first and self.feed.proc.poll() is None:
                try:
                    self.fetch()
                    break
                except OSError:
                    pass
        self.assertNotEqual(self.feed.proc.pid, first)
        self.assertEqual(self.fetch()["mode"], "live")
        self.assertEqual(gave_up, [])

    def test_ends_with_its_parent(self):
        parent = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        child = subprocess.Popen([EXE, "feed", "--port", str(self.port), "--parent", str(parent.pid), "--wallpaper", str(WALLPAPER)])
        try:
            for _ in range(50):
                try:
                    self.fetch()
                    break
                except OSError:
                    time.sleep(0.1)
            self.assertIsNone(child.poll())
            parent.kill()
            child.wait(6)   # it looks every 2 s
        finally:
            parent.kill()
            child.kill()

    def test_host_serves_and_ends_with_its_parent(self):
        """`host`: the feed and the windows in one process, no children. --peek: a small window, not the desktop."""
        parent = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        child = subprocess.Popen([EXE, "host", "--port", str(self.port), "--parent", str(parent.pid), "--peek",
                                  "--wallpaper", str(WALLPAPER)])
        try:
            for _ in range(50):
                try:
                    self.assertEqual(self.fetch()["mode"], "live")
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                self.fail("host never answered")
            c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
            c.request("GET", "/feed?key=wrong", headers={"Host": f"127.0.0.1:{self.port}"})
            self.assertEqual(c.getresponse().status, 403)
            c.close()
            self.assertIsNone(child.poll())
            parent.kill()
            child.wait(6)   # it looks every 2 s
        finally:
            parent.kill()
            child.kill()

    def test_gives_up_when_it_cannot_run(self):
        taken = socket.socket()
        taken.bind(("127.0.0.1", self.port))   # bound, not listening: the exe can't have the port and exits
        try:
            began = time.monotonic()
            self.assertFalse(self.feed.start())
            self.assertEqual(len(self.feed.starts), 3)
            self.assertLess(time.monotonic() - began, 20)
        finally:
            taken.close()

    def test_falls_back_after_repeated_stops(self):
        self.assertTrue(self.feed.start())
        gave_up = threading.Event()
        self.feed.starts = [time.monotonic()] * 3   # as if it had just been started three times
        threading.Thread(target=self.feed.watch, args=(self.stop, gave_up.set), daemon=True).start()
        taken = None
        self.feed.proc.kill()
        self.feed.proc.wait(5)
        try:
            taken = socket.socket()
            taken.bind(("127.0.0.1", self.port))
            self.assertTrue(gave_up.wait(10))   # the feed comes back to Python
        finally:
            if taken:
                taken.close()


@unittest.skipUnless(EXE and sys.platform == "win32", "set GW_FEED_EXE to the glitchwall-render exe")
class TestRunFeedChoosesTheFeed(unittest.TestCase):
    """run_feed as `glitchwall start` runs it, with the windows, the tray and the hot corner left out."""

    def run_feed(self, exe, protocol=None, launch=None, watch=None):
        home = Path(tempfile.mkdtemp(prefix="gw-runfeed-"))
        (home / "key").write_text(KEY, encoding="utf-8")
        port, seen, started, feeds = free_port(), {}, [], []
        self.corners, self.trays = [], []
        real_start = server.RustFeed.start

        def keep(rf):
            feeds.append(rf)
            return real_start(rf)

        def fake_run(host, stop):   # instead of the windows: look at what serves the port, then end
            c = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
            c.request("GET", f"/feed?key={KEY}", headers={"Host": f"127.0.0.1:{port}"})
            seen["mode"] = json.loads(c.getresponse().read())["mode"]
            c.close()
        with mock.patch.dict(os.environ, {"GLITCHWALL_HOME": str(home), "GLITCHWALL_RENDER_EXE": str(exe)}), \
                mock.patch.object(winhost.NativeHost, "run", fake_run), \
                mock.patch.object(winhost, "watch_corner", lambda *a: self.corners.append(1)), \
                mock.patch.object(server, "start_tray", lambda *a, **k: self.trays.append(1)), \
                mock.patch.object(server, "follow_mode", lambda *a, **k: started.append(1)), \
                mock.patch.object(server.RustFeed, "start", keep), \
                mock.patch.object(server, "feed_protocol", protocol or server.feed_protocol), \
                mock.patch.object(server.RustFeed, "launch", launch or server.RustFeed.launch), \
                mock.patch.object(server.RustFeed, "watch", watch or server.RustFeed.watch):
            os.environ.pop("GLITCHWALL_PYTHON_FEED", None)
            try:
                server.run_feed(port)
            finally:
                for f in feeds:
                    f.end()
        return seen, started, feeds

    def test_rust_feed_when_the_exe_has_it(self):   # protocol 1 (2d): the Rust feed, the windows as before
        seen, started, feeds = self.run_feed(Path(EXE), protocol=lambda exe: 1)
        self.assertEqual(seen, {"mode": "live"})
        self.assertEqual(started, [])   # Python's own sources never started: nothing is read twice
        self.assertEqual(len(feeds), 1)
        self.assertIsNotNone(feeds[0].proc)
        self.assertEqual((self.corners, self.trays), ([1], [1]))   # protocol 1: the corner and tray stay here

    def test_host_with_protocol_4_and_back_to_python(self):
        """Protocol 4: one `host` child draws, serves, watches the hot corner and shows the tray; nothing else here. When it can't be kept running, the feed
        and the windows come back as before. Stand-ins, so no real wallpaper windows open during the tests."""
        launched, watched = [], []

        def launch(rf):
            launched.append(rf.hosting)
            return True

        def watch(rf, stop, on_give_up):
            watched.append(1)
            on_give_up()   # as if it kept stopping
        seen, started, feeds = self.run_feed(Path(EXE), launch=launch, watch=watch)
        self.assertEqual((launched, watched), ([True], [1]))
        self.assertEqual((seen, started), ({"mode": "live"}, [1]))   # the Python feed took over
        self.assertEqual((self.corners, self.trays), ([1], [1]))   # and the corner and tray, once: not while host had them

    def test_host_command_line(self):
        rf = server.RustFeed(Path("x.exe"), 47999, Path("a.json"), mock.Mock(which="primary"), WALLPAPER, hosting=True)
        with mock.patch("subprocess.Popen") as popen, mock.patch("glitchwall.platforms.port_open", lambda p: True):
            popen.return_value.poll.return_value = None
            self.assertTrue(rf.launch())
        args = popen.call_args[0][0]
        self.assertEqual(args[:2], ["x.exe", "host"])
        self.assertIn("--parent", args)
        self.assertEqual(args[args.index("--which") + 1], "primary")
        self.assertNotIn("--asleep", args)   # host knows for itself which screens nobody can see
        self.assertNotIn("--glitchwall", args)
        rf.glitchwall = ["glitchwall-feed.exe", "-m", "glitchwall"]
        with mock.patch("subprocess.Popen") as popen, mock.patch("glitchwall.platforms.port_open", lambda p: True):
            popen.return_value.poll.return_value = None
            rf.launch()
        args = popen.call_args[0][0]
        self.assertEqual(json.loads(args[args.index("--glitchwall") + 1]), rf.glitchwall)

    def test_python_feed_with_an_older_exe(self):
        old = Path(tempfile.mkdtemp(prefix="gw-old-")) / "glitchwall-render.exe"
        old.write_bytes(b"MZ an older renderer")
        seen, started, feeds = self.run_feed(old)
        self.assertEqual((seen, started, feeds), ({"mode": "live"}, [1], []))


if __name__ == "__main__":
    unittest.main()
