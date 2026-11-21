"""Where state lives. On Windows it must not be under AppData (MSIX redirects it)."""
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

from agent_live_wallpaper import settings as cfg


class TestStateDir(unittest.TestCase):
    def test_override(self):
        with mock.patch.dict(os.environ, {"AGENT_WALLPAPER_HOME": "/tmp/x"}):
            self.assertEqual(cfg.state_dir(), Path("/tmp/x"))
            self.assertEqual(cfg.legacy_state_dirs(), [])

    @unittest.skipUnless(sys.platform == "win32", "Windows layout")
    def test_windows_not_in_appdata(self):
        env = {k: v for k, v in os.environ.items() if k != "AGENT_WALLPAPER_HOME"}
        with mock.patch.dict(os.environ, env, clear=True):
            d = cfg.state_dir()
            self.assertEqual(d, Path.home() / ".agent-live-wallpaper")
            self.assertNotIn("appdata", str(d).lower())
            self.assertEqual(cfg.legacy_state_dirs(), [Path(os.environ["LOCALAPPDATA"]) / "agent-live-wallpaper"])


if __name__ == "__main__":
    unittest.main()
