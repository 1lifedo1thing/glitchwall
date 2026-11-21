"""Log parsing: calls and their outputs are paired by tool_use id and each shown once."""
import json
import tempfile
import unittest
from pathlib import Path

from glitchwall.claude_logs import ClaudeLogTailer, describe
from glitchwall.demo import SCRIPT
from glitchwall.redact import MASK, Redactor


def line(content, **kw):
    return json.dumps({"timestamp": "2026-01-01T00:00:00Z", "message": {"content": content}, **kw})


class TestTailer(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp()) / "C--code-orbit"
        self.dir.mkdir()
        self.log = self.dir / "abcd1234.jsonl"
        self.events = []
        self.t = ClaudeLogTailer(self.events.append, root=self.dir.parent)

    def write(self, *lines):
        with open(self.log, "a", encoding="utf-8") as f:
            for x in lines:
                f.write(x + "\n")

    def test_call_and_output_paired_and_deduped(self):
        use = {"type": "tool_use", "id": "toolu_01ABC", "name": "Bash", "input": {"command": "ls"}}
        res = {"type": "tool_result", "tool_use_id": "toolu_01ABC", "content": "a\nb"}
        self.write(line([use]), line([use]), line([res]), line([res]))
        self.t.scan()
        self.assertEqual([(e["kind"], e["text"]) for e in self.events], [("$", "ls"), ("out", "a\nb")])
        self.assertEqual(self.events[0]["ref"], self.events[1]["ref"])
        self.assertEqual(self.events[0]["proj"], "orbit")

    def test_results_of_hidden_tools_skipped(self):
        use = {"type": "tool_use", "id": "t2", "name": "TodoWrite", "input": {}}
        res = {"type": "tool_result", "tool_use_id": "t2", "content": "x"}
        orphan = {"type": "tool_result", "tool_use_id": "unknown", "content": "y"}
        self.write(line([use]), line([res, orphan]))
        self.t.scan()
        self.assertEqual(self.events, [])

    def test_partial_line_waits(self):
        self.write(line([{"type": "tool_use", "id": "t3", "name": "Read", "input": {"file_path": "/x"}}]))
        with open(self.log, "a", encoding="utf-8") as f:
            f.write('{"half": ')
        self.t.scan()
        self.assertEqual(len(self.events), 1)

    def test_errors_and_garbage(self):
        self.write("not json", "[]", line("text"),
                   line([{"type": "tool_use", "id": "t4", "name": "Bash", "input": {"command": "false"}}]),
                   line([{"type": "tool_result", "tool_use_id": "t4", "is_error": True, "content": [{"type": "text", "text": "boom"}]}]))
        self.t.scan()
        self.assertEqual([e["kind"] for e in self.events], ["$", "err"])

    def test_describe(self):
        self.assertEqual(describe("PowerShell", {"command": "dir"}), ("PS>", "dir"))
        self.assertEqual(describe("mcp__ccd_turn__update_plan", {})[0], None)


class TestDemoIsSafe(unittest.TestCase):
    def test_demo_secrets_masked(self):
        r = Redactor(home="", user="", mask_user=False)
        joined = r("\n".join(t + "\n" + o for _, t, o, _ in SCRIPT))
        for leak in ("live_51Hdemo", "demoDemoDemo", "hunter2"):
            self.assertNotIn(leak, joined)
        self.assertIn(MASK, joined)


if __name__ == "__main__":
    unittest.main()
