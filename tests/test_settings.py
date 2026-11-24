"""Where state lives. On Windows it must not be under AppData (MSIX redirects it)."""
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import settings as cfg


class TestStateDir(unittest.TestCase):
    def test_override(self):
        with mock.patch.dict(os.environ, {"GLITCHWALL_HOME": "/tmp/x"}):
            self.assertEqual(cfg.state_dir(), Path("/tmp/x"))

    @unittest.skipUnless(sys.platform == "win32", "Windows layout")
    def test_windows_not_in_appdata(self):
        env = {k: v for k, v in os.environ.items() if k != "GLITCHWALL_HOME"}
        with mock.patch.dict(os.environ, env, clear=True):
            d = cfg.state_dir()
            self.assertEqual(d, Path.home() / ".glitchwall")
            self.assertNotIn("appdata", str(d).lower())


class TestBrokenFile(unittest.TestCase):
    """A settings.json that can't be read (a bad hand edit) must never turn a pause off."""

    def setUp(self):
        import tempfile
        self.dir = tempfile.TemporaryDirectory()
        self.env = mock.patch.dict(os.environ, {"GLITCHWALL_HOME": self.dir.name})
        self.env.start()
        self.file = cfg.settings_file()

    def tearDown(self):
        self.env.stop()
        self.dir.cleanup()

    def test_missing_is_defaults(self):
        self.assertFalse(cfg.load_settings()["paused"])

    def test_unreadable_counts_as_paused(self):
        for bad in ("{not json", "5", "[1, 2]"):
            self.file.parent.mkdir(parents=True, exist_ok=True)
            self.file.write_text(bad, encoding="utf-8")
            self.assertTrue(cfg.load_settings()["paused"], bad)

    def test_bom_is_fine(self):   # PowerShell's Set-Content writes one
        self.file.parent.mkdir(parents=True, exist_ok=True)
        self.file.write_text('﻿{"theme": "neon"}', encoding="utf-8")
        self.assertEqual(cfg.load_settings()["theme"], "neon")
        self.assertFalse(cfg.load_settings()["paused"])

    def test_running_feed_keeps_last_good(self):
        from glitchwall.server import LiveSettings
        cfg.save_settings({**cfg.DEFAULTS, "paused": True, "theme": "neon"})
        live = LiveSettings()
        self.assertTrue(live.get()["paused"])
        self.file.write_text("{oops", encoding="utf-8")
        os.utime(self.file, (1, 1))   # a new mtime, so it's read again
        self.assertTrue(live.get()["paused"])
        self.assertEqual(live.get()["theme"], "neon")
        cfg.save_settings({**cfg.DEFAULTS, "paused": False})
        self.assertFalse(live.get()["paused"])


if __name__ == "__main__":
    unittest.main()
