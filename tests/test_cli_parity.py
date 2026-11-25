"""Piece 4b: glitchwall's typed commands in Rust (config, themes, reset, export) print what the Python ones print
and leave the same settings behind. Each side gets its own fresh state folder. Needs GW_FEED_EXE."""
import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import cli

EXE = os.environ.get("GW_FEED_EXE")
WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"
KEY = "k" * 43


def home(settings: dict | None = None) -> Path:
    h = Path(tempfile.mkdtemp(prefix="gw-cli-"))
    (h / "key").write_text(KEY, encoding="utf-8")
    if settings is not None:
        (h / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    return h


@unittest.skipUnless(EXE and sys.platform == "win32", "set GW_FEED_EXE to the glitchwall-render exe")
class TestCliParity(unittest.TestCase):
    def python(self, args, h):
        out = io.StringIO()
        with mock.patch.dict(os.environ, {"GLITCHWALL_HOME": str(h)}), contextlib.redirect_stdout(out), \
                mock.patch.object(cli, "cmd_start", lambda a: 0):
            code = cli.main(args)
        return code, out.getvalue()

    def rust(self, args, h):
        p = subprocess.run([EXE, *args, "--wallpaper", str(WALLPAPER)], capture_output=True, timeout=60,
                           env={**os.environ, "GLITCHWALL_HOME": str(h)})
        return p.returncode, p.stdout.decode("utf-8")

    def both(self, args, settings=None):
        hp, hr = home(settings), home(settings)
        p, r = self.python(args, hp), self.rust(args, hr)
        saved = [json.loads(f.read_text(encoding="utf-8")) if f.exists() else None for f in (hp / "settings.json", hr / "settings.json")]
        return p, r, saved, (hp, hr)

    def test_config_lists_every_setting(self):
        p, r, saved, _ = self.both(["config"])
        self.assertEqual(p, r)
        self.assertEqual(saved[0], saved[1])

    def test_config_changes(self):
        args = ["config", "--mode", "ambient", "--theme", "amber", "--linger", "9", "--claude", "off", "--gemini", "on",
                "--label", "hi there", "--commands-only", "yes", "--mask-email", "off"]
        p, r, saved, _ = self.both(args, {"theme": "phosphor", "cards": 2})
        self.assertEqual(p, r)
        self.assertEqual(saved[0], saved[1])
        self.assertEqual((saved[1]["mode"], saved[1]["linger"], saved[1]["agent_gemini"]), ("ambient", 9, True))

    def test_themes(self):
        p, r, _, _ = self.both(["themes"], {"theme": "amber"})
        self.assertEqual(p, r)
        self.assertIn("* amber", r[1])

    def test_reset_settings(self):
        start = {"theme": "amber", "linger": 20, "mode": "ambient", "paused": True, "mask_user": False}
        hp, hr = home(start), home(start)
        self.python(["reset"], hp)
        code, out = self.rust(["reset"], hr)
        self.assertTrue(out.startswith("settings back to the defaults (mode, pause and install choices kept)\n"))
        self.assertEqual(json.loads((hp / "settings.json").read_text()), json.loads((hr / "settings.json").read_text()))

    def test_export(self):
        hp, hr = home({"label": "on the wall"}), home({"label": "on the wall"})
        dp, dr = hp / "out", hr / "out"
        p = self.python(["export", str(dp)], hp)
        r = self.rust(["export", str(dr)], hr)
        self.assertEqual(p[0], r[0])
        self.assertEqual(p[1].replace(str(hp.resolve()), "H").replace(hp.resolve().as_uri(), "U"),
                         r[1].replace(str(hr.resolve()), "H").replace(hr.resolve().as_uri(), "U"))
        files = lambda d: sorted(str(f.relative_to(d)) for f in d.rglob("*"))
        self.assertEqual(files(dp), files(dr))
        self.assertEqual((dp / "config.js").read_text(encoding="utf-8"), (dr / "config.js").read_text(encoding="utf-8"))

    def test_refuses_nonsense(self):
        code, _ = self.rust(["config", "--mode", "loud"], home())
        self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
