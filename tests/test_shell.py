"""Your own PowerShell commands: followed from PSReadLine's history, new ones only, multi-line ones whole."""
import tempfile
import unittest
from pathlib import Path

from glitchwall.shell_history import ShellHistoryTailer, commands


class TestShellHistory(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.file = self.dir / "ConsoleHost_history.txt"
        self.file.write_text("old command\n", encoding="utf-8")
        self.events, self.on = [], True
        self.t = ShellHistoryTailer(self.events.append, lambda: self.on, root=self.dir)

    def add(self, text):
        with open(self.file, "a", encoding="utf-8", newline="") as f:
            f.write(text)

    def test_only_new_commands(self):
        self.t.scan()
        self.assertEqual(self.events, [])   # what was there before isn't shown
        self.add("git status\r\nls -la\n")
        self.t.scan()
        self.assertEqual([(e["kind"], e["text"], e["proj"], e["session"]) for e in self.events],
                         [("PS>", "git status", "terminal", "you"), ("PS>", "ls -la", "terminal", "you")])
        self.assertNotEqual(self.events[0]["ref"], self.events[1]["ref"])

    def test_partial_line_waits(self):
        self.t.scan()
        self.add("half typed")
        self.t.scan()
        self.assertEqual(self.events, [])
        self.add(" now done\n")
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["half typed now done"])

    def test_off_shows_nothing_and_saves_nothing_up(self):
        self.t.scan()
        self.on = False
        self.add("secret stuff\n")
        self.t.scan()
        self.on = True
        self.add("visible\n")
        self.t.scan()
        self.assertEqual([e["text"] for e in self.events], ["visible"])

    def test_multi_line(self):
        self.assertEqual(commands("Get-Thing `\n  -Name x\nnext\n\n"), ["Get-Thing \n  -Name x", "next"])

    def test_no_history_folder(self):
        ShellHistoryTailer(self.events.append, root=self.dir / "missing").scan()
        self.assertEqual(self.events, [])


if __name__ == "__main__":
    unittest.main()
