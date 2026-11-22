"""The install pre-check: what it asks, what it fixes, when it stops. No downloads: the fixes are fakes."""
import argparse
import os
import tempfile
import unittest
from unittest import mock

os.environ.setdefault("GLITCHWALL_HOME", tempfile.mkdtemp(prefix="alw-test-"))

from glitchwall import cli, prereqs  # noqa: E402
from glitchwall.prereqs import Check  # noqa: E402


def run(found, answers):
    said, asked = [], []

    def ask(q):
        asked.append(q)
        return answers.pop(0)
    with mock.patch.object(prereqs, "checks", return_value=found):
        ok = prereqs.preflight("glitchwall", "live", said.append, ask)
    return ok, said, asked


class TestPreflight(unittest.TestCase):
    def test_all_there_asks_nothing(self):
        ok, said, asked = run([Check("A", True, "fine"), Check("B", True, "fine")], [])
        self.assertTrue(ok)
        self.assertEqual(asked, [])

    def test_offers_fix_and_rechecks(self):
        fixed = []
        c = Check("WebView2-ish", False, "missing", fix_prompt="Get it?", fix=lambda: fixed.append(1) or True)
        ok, said, asked = run([c], [True])
        self.assertTrue(ok)
        self.assertEqual((asked, fixed), (["Get it?"], [1]))

    def test_no_means_nothing_is_fetched(self):
        fixed = []
        c = Check("X", False, "missing", fix_prompt="Get it?", fix=lambda: fixed.append(1) or True, hint="do Y")
        ok, said, _ = run([c], [False])
        self.assertFalse(ok)
        self.assertEqual(fixed, [])
        self.assertTrue(any("do Y" in s for s in said))

    def test_optional_never_blocks(self):
        ok, _, asked = run([Check("Claude Code logs", False, "not found", required=False)], [])
        self.assertTrue(ok)
        self.assertEqual(asked, [])

    def test_failed_fix_still_missing(self):
        c = Check("X", False, "missing", fix_prompt="Get it?", fix=lambda: False)
        ok, said, _ = run([c], [True])
        self.assertFalse(ok)
        self.assertIn("  X: still missing", said)


class TestAsker(unittest.TestCase):
    def test_yes_flag(self):
        self.assertTrue(cli._asker(argparse.Namespace(yes=True))("q?"))

    def test_not_interactive_says_no(self):
        with mock.patch("sys.stdin", None):
            self.assertFalse(cli._asker(argparse.Namespace(yes=False))("q?"))


class TestRealChecks(unittest.TestCase):
    def test_lists_without_fetching(self):
        with mock.patch.object(prereqs, "download", side_effect=AssertionError("no downloads in checks")):
            found = prereqs.checks("glitchwall", "ambient")
        self.assertIn("Python", [c.name for c in found])
        self.assertNotIn("Claude Code logs", [c.name for c in found])   # ambient reads no logs


class TestContextMenu(unittest.TestCase):
    def test_add_and_remove(self):
        from glitchwall import platforms as plat
        if not plat.WIN:
            self.skipTest("Windows only")
        with mock.patch.object(plat, "MENU_KEY", r"Software\glitchwall-test-menu"):
            self.assertTrue(plat.add_context_menu())
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, plat.MENU_KEY + r"\command") as k:
                self.assertIn("settings", winreg.QueryValueEx(k, "")[0])
            self.assertTrue(plat.remove_context_menu())
            self.assertFalse(plat.remove_context_menu())


class TestEngineWatch(unittest.TestCase):
    """The built-in host notices when WebView2 has gone, but not before it ever started, and not on one blip."""
    def test_gone_twice_means_dead(self):
        from glitchwall import winhost
        h = winhost.Host(lambda n: "", on_dead=lambda: None)
        seq = iter([[], ["msedgewebview2.exe"], [], ["msedgewebview2.exe"], [], []])
        with mock.patch.object(winhost, "child_exes", lambda: next(seq)):
            self.assertEqual([h.engine_alive() for _ in range(6)], [True, True, True, True, True, False])

    def test_child_exes_sees_a_child(self):
        from glitchwall import platforms as plat, winhost
        if not plat.WIN:
            self.skipTest("Windows only")
        import subprocess
        p = subprocess.Popen(["ping", "-n", "2", "127.0.0.1"], stdout=subprocess.DEVNULL)
        try:
            self.assertIn("ping.exe", winhost.child_exes())
        finally:
            p.wait()
