"""Piece 4d: `glitchwall-render doctor` prints prereqs.py's checks (less its Python and pywebview lines: the exe needs
neither) and `glitchwall-render demo` refuses a port in use as cli.cmd_demo does. Read-only: the checks only read the
registry and look for the Claude logs folder. Needs GW_FEED_EXE."""
import contextlib
import io
import json
import os
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import cli

EXE = os.environ.get("GW_FEED_EXE")
WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"


@unittest.skipUnless(EXE and sys.platform == "win32", "set GW_FEED_EXE to the glitchwall-render exe")
class TestDoctorParity(unittest.TestCase):
    def run_both(self, args, settings=None, env=None):
        h = Path(tempfile.mkdtemp(prefix="gw-doctor-"))
        if settings:
            (h / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
        env = {**(env or {}), "GLITCHWALL_HOME": str(h)}
        out = io.StringIO()
        with mock.patch.dict(os.environ, env), contextlib.redirect_stdout(out):
            pc = cli.main(args)
        p = subprocess.run([EXE, *args, "--wallpaper", str(WALLPAPER)], capture_output=True, timeout=60, env={**os.environ, **env})
        py = [l for l in out.getvalue().splitlines() if not l.startswith(("  ok  Python:", "  ok  pywebview:"))]
        return (pc, py), (p.returncode, p.stdout.decode("utf-8").splitlines())

    def test_doctor_live(self):
        p, r = self.run_both(["doctor"])
        self.assertEqual(p, r)
        self.assertEqual(r[1][0], "checking what glitchwall needs:")
        self.assertTrue(any(l.startswith("  ok  WebView2 runtime: ") for l in r[1]))

    def test_doctor_no_claude_logs(self):
        empty = tempfile.mkdtemp(prefix="gw-noclaude-")
        p, r = self.run_both(["doctor"], env={"CLAUDE_CONFIG_DIR": empty})
        self.assertEqual(p, r)
        self.assertTrue(any(l.startswith("  --  Claude Code logs: not found") for l in r[1]))

    def test_doctor_ambient_skips_the_logs(self):
        p, r = self.run_both(["doctor"], {"mode": "ambient"})
        self.assertEqual(p, r)
        self.assertFalse(any("Claude Code logs" in l for l in r[1]))

    def test_demo_port_in_use(self):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            s.listen()
            port = str(s.getsockname()[1])
            p, r = self.run_both(["demo", "--port", port])
        self.assertEqual(p, r)
        self.assertEqual(r, (1, [f"port {port} is in use; try --port N"]))


if __name__ == "__main__":
    unittest.main()
