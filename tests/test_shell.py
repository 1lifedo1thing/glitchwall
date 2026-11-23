"""Your own PowerShell commands and their output: the hook's files followed, the hook put in and taken out cleanly."""
import json
import os
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

from glitchwall import shell_history as sh


class TestShellTailer(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.events, self.on = [], True
        self.t = sh.ShellTailer(self.events.append, lambda: self.on, root=self.dir)

    def add(self, name, *recs):
        with open(self.dir / name, "a", encoding="utf-8", newline="\n") as f:
            for r in recs:
                f.write(json.dumps(r) + "\n")

    def test_command_then_its_output(self):
        self.add("100.jsonl", {"t": "cmd", "id": 1, "text": "ping -n 1 x"})
        self.t.scan()
        self.add("100.jsonl", {"t": "out", "id": 1, "ok": True, "text": "Reply from x\n"},
                 {"t": "cmd", "id": 2, "text": "bad"}, {"t": "out", "id": 2, "ok": False, "text": "not found"})
        self.t.scan()
        self.assertEqual([(e["kind"], e["text"], e["ref"]) for e in self.events],
                         [("PS>", "ping -n 1 x", "sh100-1"), ("out", "Reply from x", "sh100-1"),
                          ("PS>", "bad", "sh100-2"), ("err", "not found", "sh100-2")])
        self.assertEqual({e["proj"] for e in self.events}, {"terminal"})

    def test_wsl_commands(self):   # shell-hook.bash marks its records "sh": "bash"
        self.add("wsl-12.jsonl", {"sh": "bash", "t": "cmd", "id": 4, "text": "ls /x"},
                 {"sh": "bash", "t": "out", "id": 4, "ok": False, "text": "exit 2"})
        self.t.scan()
        self.assertEqual([(e["kind"], e["proj"], e["text"], e["ref"]) for e in self.events],
                         [("$", "wsl", "ls /x", "shwsl-12-4"), ("err", "wsl", "exit 2", "shwsl-12-4")])

    def test_empty_output_says_so(self):
        self.add("1.jsonl", {"t": "cmd", "id": 1, "text": "cls"}, {"t": "out", "id": 1, "ok": True, "text": ""})
        self.t.scan()
        self.assertEqual(self.events[-1]["text"], "(no output)")

    def test_older_shell_read_from_its_end(self):
        f = self.dir / "7.jsonl"
        self.add("7.jsonl", {"t": "cmd", "id": 1, "text": "old"})
        os.utime(f, (time.time() - 60, time.time() - 60))
        self.t.started = time.time() + 5   # as if the feed started after this shell wrote
        self.t.scan()
        self.add("7.jsonl", {"t": "cmd", "id": 2, "text": "new"})
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["new"])

    def test_off_shows_nothing_and_saves_nothing_up(self):
        self.on = False
        self.add("2.jsonl", {"t": "cmd", "id": 1, "text": "hidden"})
        self.t.scan()
        self.on = True
        self.add("2.jsonl", {"t": "cmd", "id": 2, "text": "shown"})
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["shown"])

    def test_absurdly_nested_line_is_skipped(self):   # json.loads raises RecursionError, not ValueError
        with open(self.dir / "4.jsonl", "a", encoding="utf-8") as f:
            f.write("[" * 200_000 + '\n{"t": "cmd", "id": 1, "text": "after"}\n')
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["after"])

    def test_partial_line_and_garbage(self):
        with open(self.dir / "3.jsonl", "a", encoding="utf-8") as f:
            f.write('not json\n{"t": "cmd", "id": 1, "te')
        self.t.scan()
        self.assertEqual(self.events, [])
        with open(self.dir / "3.jsonl", "a", encoding="utf-8") as f:
            f.write('xt": "done"}\n')
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["done"])

    def test_stale_files_cleared(self):
        self.add("9.jsonl", {"t": "cmd", "id": 1, "text": "x"})
        old = time.time() - sh.STALE - 10
        os.utime(self.dir / "9.jsonl", (old, old))
        self.t.scan()
        self.assertFalse((self.dir / "9.jsonl").exists())


class TestHookInstall(unittest.TestCase):
    def test_on_then_off_leaves_profiles_as_they_were(self):
        home = Path(tempfile.mkdtemp())
        p7, p5 = home / "PowerShell" / "profile.ps1", home / "WindowsPowerShell" / "profile.ps1"
        p7.parent.mkdir()
        p7.write_text("Set-Alias ll ls\n", encoding="utf-8")
        with mock.patch.dict(os.environ, {"GLITCHWALL_HOME": str(home / "state")}), \
             mock.patch.object(sh, "profiles", return_value=[p7, p5]):
            self.assertEqual(sh.install_hook(), [p7, p5])
            self.assertEqual(sh.install_hook(), [])   # twice changes nothing
            self.assertTrue(sh.hook_installed())
            self.assertIn("Set-Alias ll ls", p7.read_text(encoding="utf-8-sig"))
            self.assertTrue((home / "state" / "shell-hook.ps1").is_file())
            sh.remove_hook()
            self.assertFalse(sh.hook_installed())
            self.assertEqual(p7.read_text(encoding="utf-8-sig"), "Set-Alias ll ls\n")
            self.assertFalse((home / "state" / "shell-hook.ps1").exists())


class TestWslHook(unittest.TestCase):
    def test_on_then_off(self):
        home = Path(tempfile.mkdtemp())
        calls = []
        with mock.patch.dict(os.environ, {"GLITCHWALL_HOME": str(home)}), \
             mock.patch.object(sh, "_wsl", side_effect=lambda script, *a: calls.append(a) or 0):
            self.assertTrue(sh.install_wsl_hook())
            hook = home / "shell-hook.bash"
            self.assertNotIn(b"\r", hook.read_bytes())   # bash wants LF
            self.assertEqual(calls[-1], (str(hook), str(home / "shell"), sh.MARK))
            self.assertTrue(sh.remove_wsl_hook())
            self.assertFalse(hook.exists())
            self.assertEqual(calls[-1], (sh.MARK,))

    def test_no_wsl(self):
        with mock.patch.object(sh, "_wsl", return_value=None), \
             mock.patch.dict(os.environ, {"GLITCHWALL_HOME": tempfile.mkdtemp()}):
            self.assertFalse(sh.install_wsl_hook())
            self.assertFalse(sh.remove_wsl_hook())


if __name__ == "__main__":
    unittest.main()
