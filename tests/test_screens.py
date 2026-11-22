"""Several monitors: install shows glitchwall on each screen and notes what each showed; uninstall puts
each screen's own wallpaper back. Lively is faked: these tests never touch a real install."""
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

os.environ.setdefault("GLITCHWALL_HOME", tempfile.mkdtemp(prefix="alw-test-"))

from glitchwall import platforms as plat  # noqa: E402


class FakeLively:
    """Two screens, each with its own wallpaper; records the commands it was given."""

    def __init__(self, dest, screens=(1, 2), arrangement="per"):
        self.dest, self.nums, self.arrangement, self.calls = str(dest), list(screens), arrangement, []
        self.tmp = Path(tempfile.mkdtemp())
        self.shown = {}
        for n in self.nums:
            (self.tmp / f"old{n}").mkdir()
            self.shown[n] = str(self.tmp / f"old{n}")

    def lively(self, *args):
        self.calls.append(args)
        cmd, a = args[0], dict(zip(args[1::2], args[2::2]))
        targets = [int(a["--monitor"])] if a.get("--monitor", "-1") != "-1" else self.nums
        if cmd == "setwp" and "--monitor" not in a:
            targets = self.nums if self.arrangement != "per" else self.nums[:1]
        for n in targets:
            if cmd == "setwp":
                self.shown[n] = a["--file"]
            else:
                self.shown.pop(n, None)
        return True

    def layout(self):
        return [{"index": n, "device": f"\\\\.\\DISPLAY{n}", "path": p} for n, p in sorted(self.shown.items())]

    def patches(self):
        return [mock.patch.object(plat, "lively", self.lively), mock.patch.object(plat, "lively_layout", self.layout),
                mock.patch.object(plat, "screens", lambda: self.nums),
                mock.patch.object(plat, "lively_arrangement", lambda: self.arrangement),
                mock.patch.object(plat.time, "sleep", lambda s: None)]


class TestScreens(unittest.TestCase):
    def setUp(self):
        self.dest = Path(tempfile.mkdtemp()) / "glitchwall"
        plat.previous_file().unlink(missing_ok=True)

    def run_with(self, fake, fn, *a):
        ps = fake.patches()
        for p in ps:
            p.start()
        try:
            return fn(*a)
        finally:
            for p in ps:
                p.stop()

    def install(self, fake, which="all"):
        self.run_with(fake, lambda: plat.remember_previous(plat.lively_layout(), self.dest))
        return self.run_with(fake, plat.show_in_lively, self.dest, which)

    def test_install_and_uninstall_on_two_screens(self):
        fake = FakeLively(self.dest)
        before = dict(fake.shown)
        msgs = self.install(fake)
        self.assertEqual(set(fake.shown.values()), {str(self.dest)}, msgs)
        self.assertIn("all 2 screens", msgs[0])
        self.run_with(fake, plat.restore_in_lively, self.dest)
        self.assertEqual(fake.shown, before)     # each screen got its own wallpaper back

    def test_reinstall_keeps_the_original_wallpapers(self):
        fake = FakeLively(self.dest)
        before = dict(fake.shown)
        self.install(fake)
        self.install(fake)                       # glitchwall already showing: the first note stands
        self.run_with(fake, plat.restore_in_lively, self.dest)
        self.assertEqual(fake.shown, before)

    def test_primary_only(self):
        fake = FakeLively(self.dest)
        self.install(fake, "primary")
        self.assertEqual(fake.shown[1], str(self.dest))
        self.assertNotEqual(fake.shown[2], str(self.dest))

    def test_span_is_one_call(self):
        fake = FakeLively(self.dest, arrangement="span")
        msgs = self.install(fake)
        self.assertEqual([c for c in fake.calls if c[0] == "setwp"], [("setwp", "--file", str(self.dest))])
        self.assertIn("spanning", msgs[0])

    def test_screen_without_a_note_is_closed(self):
        fake = FakeLively(self.dest)
        self.install(fake)
        plat.previous_file().write_text('[{"index": 1, "device": "\\\\\\\\.\\\\DISPLAY1", "path": "%s"}]'
                                        % fake.tmp.joinpath("old1").as_posix(), encoding="utf-8")
        self.run_with(fake, plat.restore_in_lively, self.dest)
        self.assertEqual(fake.shown, {1: fake.tmp.joinpath("old1").as_posix()})   # screen 2 closed, not left broken

    def test_old_screenless_note_still_restores(self):
        fake = FakeLively(self.dest, screens=(1,))
        old = str(fake.tmp / "old1")
        plat.previous_file().write_text(f'["{Path(old).as_posix()}"]', encoding="utf-8")   # how 0.4 saved it
        self.run_with(fake, plat.show_in_lively, self.dest, "all")
        self.run_with(fake, plat.restore_in_lively, self.dest)
        self.assertEqual(fake.shown, {1: Path(old).as_posix()})


if __name__ == "__main__":
    unittest.main()
