"""live / ambient / art: the label, the ambient generator, and switching source without a restart."""
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

os.environ.setdefault("GLITCHWALL_HOME", tempfile.mkdtemp(prefix="alw-test-"))

from glitchwall import ambient, server  # noqa: E402
from glitchwall import settings as cfg  # noqa: E402
from glitchwall.redact import Redactor  # noqa: E402
from glitchwall.server import EventStore  # noqa: E402

KINDS = {"$", "PS>", "read", "edit", "write", "find", "web", "agent"}


class TestLabel(unittest.TestCase):
    def test_stock_label_follows_mode(self):
        self.assertEqual(cfg.display_label({}), cfg.DEFAULTS["label"])
        self.assertEqual(cfg.display_label({"mode": "ambient"}), cfg.AMBIENT_LABEL)
        self.assertEqual(cfg.display_label({"mode": "art"}), "")
        self.assertEqual(cfg.mode({"mode": "bogus"}), "live")

    def test_own_label_wins(self):
        self.assertEqual(cfg.display_label({"mode": "ambient", "label": "hi"}), "hi")


class TestAmbient(unittest.TestCase):
    def test_every_episode_is_well_formed(self):
        for ep in ambient.EPISODES:
            for _ in range(20):
                for kind, text, out, err in ep("orbit"):
                    with self.subTest(ep=ep.__name__, text=text[:40]):
                        self.assertIn(kind, KINDS)
                        self.assertTrue(text.strip())
                        self.assertIsInstance(out, str)
                        self.assertIsInstance(err, bool)

    def test_generator_feeds_the_store(self):
        store, stop = EventStore(Redactor(home="", user="", mask_user=False)), threading.Event()
        t = threading.Thread(target=ambient.generate, args=(store, stop, 0.005))
        t.start()
        time.sleep(0.4)
        stop.set()
        t.join(2)
        self.assertFalse(t.is_alive())
        self.assertGreater(store.seq, 2)
        self.assertTrue(all(e["kind"] in KINDS | {"out", "err"} for e in store.since(0)))


class TestFollowMode(unittest.TestCase):
    def test_switches_source_when_mode_changes(self):
        modes = iter(["live", "live", "ambient", "art", "art"])
        started = []

        def fake_start(m, store):
            started.append(m)
            return threading.Event()

        class Live:
            def get(self):
                try:
                    return {"mode": next(modes)}
                except StopIteration:
                    quit.set()
                    return {"mode": "art"}

        quit = threading.Event()
        with mock.patch.object(server, "start_source", fake_start),                 mock.patch.object(server, "start_reader", lambda a, store: threading.Event()):
            server.follow_mode(EventStore(Redactor(home="", user="")), Live(), poll=0, quit=quit)
        self.assertEqual(started, ["live", "ambient", "art"])

    def test_agents_start_and_stop_one_by_one(self):
        # Gemini and Codex stay off until turned on; turning one on or off leaves the others' readers running
        states = iter([{}, {"agent_gemini": True}, {"agent_gemini": True, "agent_claude": False},
                       {"agent_gemini": "yes"}, {"mode": "art", "agent_codex": True}])
        started, stops = [], {}

        def fake_reader(a, store):
            started.append(a)
            stops.setdefault(a, []).append(threading.Event())
            return stops[a][-1]

        class Live:
            def get(self):
                try:
                    return next(states)
                except StopIteration:
                    quit.set()
                    return {"mode": "art"}

        quit = threading.Event()
        with mock.patch.object(server, "start_reader", fake_reader):
            server.follow_mode(EventStore(Redactor(home="", user="")), Live(), poll=0, quit=quit)
        # claude; + gemini; claude off; gemini "yes" is not on, claude back; art: everything stops
        self.assertEqual(started, ["agent_claude", "agent_gemini", "agent_claude"])
        self.assertTrue(all(e.is_set() for es in stops.values() for e in es))
        self.assertNotIn("agent_codex", started)

    def test_art_starts_nothing(self):
        stop = server.start_source("art", EventStore(Redactor(home="", user="")))
        self.assertIsInstance(stop, threading.Event)


class TestHealConfig(unittest.TestCase):
    def test_stale_key_in_wallpaper_is_rewritten(self):
        # the key file went missing and the feed made a new one: the wallpaper's copy must follow
        from glitchwall.cli import sync_config
        dest = Path(tempfile.mkdtemp())
        (dest / "config.js").write_text(cfg.config_js(1, "old-key", "x"), encoding="utf-8")
        s = cfg.load_settings()
        self.assertTrue(sync_config(dest, s))
        self.assertIn(cfg.load_key(), (dest / "config.js").read_text(encoding="utf-8"))
        self.assertFalse(sync_config(dest, s))                   # already right: left alone
        self.assertFalse(sync_config(dest / "missing", s))       # nothing installed: nothing written


if __name__ == "__main__":
    unittest.main()
