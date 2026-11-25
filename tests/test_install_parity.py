"""Piece 4c: `glitchwall-render install|uninstall` does what cli.cmd_install / cmd_uninstall do, except that autostart
runs the exe's `run` straight away. Nothing real is touched: each side gets its own GLITCHWALL_HOME, its own APPDATA
(the Start menu shortcut) and its own throwaway HKCU key (GLITCHWALL_REG_ROOT), deleted afterwards; the Rust side
gets --no-start and the Python side a stand-in for starting. Needs GW_FEED_EXE."""
import contextlib
import ctypes
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
from glitchwall import platforms as plat

EXE = os.environ.get("GW_FEED_EXE")
WALLPAPER = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"
RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"
ARGS = ["install", "--port", "47901", "--commands-only", "--mode", "ambient", "--screens", "primary"]


def reg_value(root: str, sub: str, name: str):
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, rf"{root}\{sub}") as k:
            return winreg.QueryValueEx(k, name)[0]
    except FileNotFoundError:
        return None


def lnk(path: Path) -> dict:
    ps = (f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{path}');"
          "@{t=$s.TargetPath;a=$s.Arguments;d=$s.Description;w=$s.WorkingDirectory;i=$s.IconLocation} | ConvertTo-Json")
    return json.loads(subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", ps],
                                     capture_output=True, text=True, timeout=60).stdout)


@unittest.skipUnless(EXE and sys.platform == "win32", "set GW_FEED_EXE to the glitchwall-render exe")
class TestInstallParity(unittest.TestCase):
    def setUp(self):
        self.sides = {}
        for side in ("py", "rs"):
            t = Path(tempfile.mkdtemp(prefix=f"gw-install-{side}-"))
            self.sides[side] = {"GLITCHWALL_HOME": str(t / "home"), "APPDATA": str(t / "appdata"),
                                "GLITCHWALL_REG_ROOT": rf"Software\glitchwall-test-{os.getpid()}-{side}"}

    def tearDown(self):
        for env in self.sides.values():
            ctypes.windll.advapi32.RegDeleteTreeW(ctypes.c_void_p(0x80000001), env["GLITCHWALL_REG_ROOT"])

    def python(self, args):
        out = io.StringIO()
        from glitchwall import prereqs, winhost
        with mock.patch.dict(os.environ, self.sides["py"]), contextlib.redirect_stdout(out), \
                mock.patch.object(cli, "cmd_start", lambda a: 0), mock.patch.object(plat, "stop_background", lambda: False), \
                mock.patch.object(prereqs, "preflight", lambda *a: True), mock.patch.object(cli.time, "sleep", lambda s: None), \
                mock.patch.object(winhost, "shown_screens", lambda: [(0, 0, 1, 1)]), \
                mock.patch.object(winhost, "monitors", lambda: [(0, 0, 1, 1)]):
            code = cli.main(args)
        return code, out.getvalue().replace(self.sides["py"]["GLITCHWALL_HOME"], "<HOME>")

    def rust(self, args):
        extra = ["--no-start", "--glitchwall", json.dumps(plat.live_command())] if args[0] == "install" else []
        p = subprocess.run([EXE, *args, *extra, "--wallpaper", str(WALLPAPER)], capture_output=True, timeout=60,
                           env={**os.environ, **self.sides["rs"]})
        out = p.stdout.decode("utf-8").replace(self.sides["rs"]["GLITCHWALL_HOME"], "<HOME>")
        if args[0] == "install":   # the prerequisite check (piece 4d; test_doctor_parity), a stand-in on the Python side
            self.assertIn("checking what glitchwall needs:\n", out)
            out = "".join(l for l in out.splitlines(True) if not (l.startswith("  ") or l.startswith("checking what")))
        return p.returncode, out

    def test_install_then_uninstall(self):
        (pc, po), (rc, ro) = self.python(ARGS), self.rust(ARGS)
        self.assertEqual((pc, rc), (0, 0), ro)
        # the same first lines: mode, state dir, autostart, Start menu, desktop menu
        self.assertEqual(po.splitlines()[:5], ro.splitlines()[:5])
        self.assertEqual(ro.splitlines()[5:], ["copied glitchwall-render.exe to <HOME>\\bin", "copied wallpaper to <HOME>\\bin"])
        hp, hr = (Path(self.sides[s]["GLITCHWALL_HOME"]) for s in ("py", "rs"))
        self.assertEqual(*(json.loads((h / "settings.json").read_text(encoding="utf-8")) for h in (hp, hr)))
        self.assertGreaterEqual(len((hr / "key").read_text(encoding="utf-8").strip()), 32)
        self.assertTrue((hr / "bin" / "glitchwall-render.exe").is_file() and (hr / "bin" / "wallpaper" / "looks.js").is_file())

        # autostart: Python's runs pythonw -m glitchwall run, Rust's runs the exe's own `run` (supervise.rs)
        rp, rr = (self.sides[s]["GLITCHWALL_REG_ROOT"] for s in ("py", "rs"))
        self.assertEqual(reg_value(rp, RUN, "glitchwall"), subprocess.list2cmdline(plat.feed_command()))
        self.assertEqual(reg_value(rr, RUN, "glitchwall"), subprocess.list2cmdline(
            [str(hr / "bin" / "glitchwall-render.exe"), "run", "--which", "primary", "--glitchwall",
             json.dumps(plat.live_command()), "--wallpaper", str(hr / "bin" / "wallpaper")]))
        # the desktop right-click entry and the Start menu shortcut: the same on both
        for sub, name in [(plat.MENU_KEY, ""), (plat.MENU_KEY, "MUIVerb"), (plat.MENU_KEY, "Icon"), (plat.MENU_KEY, "Position"),
                          (plat.MENU_KEY + r"\command", "")]:
            self.assertEqual(reg_value(rp, sub, name), reg_value(rr, sub, name), (sub, name))
        self.assertIsNotNone(reg_value(rr, plat.MENU_KEY + r"\command", ""))
        links = [Path(self.sides[s]["APPDATA"]) / "Microsoft/Windows/Start Menu/Programs/glitchwall settings.lnk" for s in ("py", "rs")]
        self.assertEqual(lnk(links[0]), lnk(links[1]))
        # nothing landed in the real places
        self.assertNotIn(str(hr), plat.autostart_entry() or "")

        (pc, po), (rc, ro) = self.python(["uninstall"]), self.rust(["uninstall"])
        self.assertEqual((pc, rc), (0, 0))
        self.assertEqual(po.splitlines()[:3], ro.splitlines()[:3])
        self.assertEqual(ro.splitlines()[3:], ["deleted <HOME> (key, settings, logs, programs)", "Your agents' logs were never modified."])
        for root in (rp, rr):
            self.assertIsNone(reg_value(root, RUN, "glitchwall"))
            self.assertIsNone(reg_value(root, plat.MENU_KEY, "MUIVerb"))
        self.assertFalse(any(f.exists() for f in links) or hp.exists() or hr.exists())

    def test_uninstall_with_nothing_there(self):
        (pc, po), (rc, ro) = self.python(["uninstall"]), self.rust(["uninstall"])
        self.assertEqual(po.splitlines()[0], ro.splitlines()[0])
        self.assertEqual(ro.splitlines(), ["nothing to remove", "Your agents' logs were never modified."])

    def test_no_autostart(self):
        (_, po), (_, ro) = self.python([*ARGS, "--no-autostart"]), self.rust([*ARGS, "--no-autostart"])
        self.assertEqual(po.splitlines()[:5], ro.splitlines()[:5])
        self.assertIsNone(reg_value(self.sides["rs"]["GLITCHWALL_REG_ROOT"], RUN, "glitchwall"))


if __name__ == "__main__":
    unittest.main()
