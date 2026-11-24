"""Codex CLI log reader: made-up rollout lines in the shapes codex writes (see docs/other-agents.md)."""
import json
import os
import tempfile
import time
import unittest
from pathlib import Path

from glitchwall import codex_logs as cx

TS = "2026-11-24T10:00:00.000Z"
F = Path("sessions") / "2026" / "11" / "24" / "rollout-2026-11-24T10-00-00-0199a213-bbab2a035a53.jsonl"


def item(payload, ts=TS):
    return json.dumps({"timestamp": ts, "type": "response_item", "payload": payload})


def fc(cid, name, args, ts=TS):
    return item({"type": "function_call", "name": name, "arguments": json.dumps(args), "call_id": cid}, ts)


def out(cid, output):
    return item({"type": "function_call_output", "call_id": cid, "output": output})


class Feed:
    def __init__(self):
        self.events = []
        self.t = cx.CodexLogTailer(self.events.append, root=Path("."))

    def __call__(self, *lines):
        for line in lines:
            self.t.parse_line(line.encode("utf-8"), F)
        return [(e["kind"], e["text"]) for e in self.events]


class TestDescribe(unittest.TestCase):
    def test_shells(self):
        self.assertEqual(cx.describe("exec_command", {"cmd": "ls"}, ""), ("$", "ls"))
        self.assertEqual(cx.describe("shell", {"command": ["bash", "-lc", "make test"]}, ""), ("$", "make test"))
        self.assertEqual(cx.describe("shell", {"command": ["rg", "x"]}, ""), ("$", "rg x"))
        self.assertEqual(cx.describe("shell_command", {"command": "npm test"}, ""), ("$", "npm test"))

    def test_others(self):
        self.assertEqual(cx.describe("view_image", {"path": "a.png"}, ""), ("read", "a.png"))
        self.assertEqual(cx.describe("update_plan", {}, ""), (None, ""))
        self.assertEqual(cx.describe("mcp_x", {"q": "é"}, '{"q": "\\u00e9"}'), ("tool", 'mcp_x {"q": "é"}'))
        self.assertEqual(cx.describe("odd", {}, "raw"), ("tool", "odd raw"))

    def test_patch(self):
        p = "*** Begin Patch\n*** Update File: a.py\n@@\n-x\n+y\n*** Add File: b.md\n+hi\n*** End Patch"
        self.assertEqual(cx.patch_text(p), "a.py\n@@\n-x\n+y\nnew: b.md\n+hi")


class TestOutput(unittest.TestCase):
    def test_exec_header(self):
        head = "Chunk ID: 1\nWall time: 0.1 seconds\nProcess exited with code {}\nOriginal token count: 3\nOutput:\n{}"
        self.assertEqual(cx.output_text(head.format(0, "ok")), ("ok", False))
        self.assertEqual(cx.output_text(head.format(2, "boom")), ("boom", True))

    def test_old_json_and_items(self):
        self.assertEqual(cx.output_text(json.dumps({"output": "x", "metadata": {"exit_code": 1}})), ("x", True))
        self.assertEqual(cx.output_text([{"type": "input_text", "text": "a"}, {"type": "input_image"}]), ("a", False))
        self.assertEqual(cx.output_text(None), ("(no output)", False))

    def test_plain_text_is_kept(self):
        self.assertEqual(cx.output_text("Output: not a header"), ("Output: not a header", False))


class TestParse(unittest.TestCase):
    def test_call_and_result(self):
        self.assertEqual(Feed()(fc("c1", "exec_command", {"cmd": "ls"}), out("c1", "a\nb")), [("$", "ls"), ("out", "a\nb")])

    def test_orphans_and_repeats(self):
        line = fc("c2", "exec_command", {"cmd": "pwd"})
        self.assertEqual(Feed()(out("zz", "orphan"), line, line, out("c2", "/"), out("c2", "/")), [("$", "pwd"), ("out", "/")])

    def test_apply_patch_custom_call(self):
        feed = Feed()
        res = feed(item({"type": "custom_tool_call", "call_id": "p1", "name": "apply_patch",
                         "input": "*** Begin Patch\n*** Update File: a.py\n-x\n+y\n*** End Patch"}),
                   item({"type": "custom_tool_call_output", "call_id": "p1",
                         "output": json.dumps({"output": "Success.", "metadata": {"exit_code": 0}})}))
        self.assertEqual(res, [("edit", "a.py\n-x\n+y"), ("out", "Success.")])

    def test_project_and_subagent(self):
        feed = Feed()
        feed(json.dumps({"timestamp": TS, "type": "session_meta",
                         "payload": {"cwd": "C:\\dev\\my-app", "source": {"subagent": "review"}}}),
             fc("c3", "exec_command", {"cmd": "x"}))
        e = feed.events[0]
        self.assertEqual((e["proj"], e["side"], e["session"]), ("app", True, "bbab"))

    def test_garbage(self):
        lines = ["not json", "[]", "{}", json.dumps({"type": "response_item", "payload": 3}),
                 item({"type": "message", "content": []}), item({"type": "reasoning"}),
                 json.dumps({"type": "event_msg", "payload": {"type": "exec_command_end"}}),
                 fc("c4", "exec_command", "not an object"), fc("", "exec_command", {"cmd": "no id"})]
        self.assertEqual(Feed()(*lines), [])


class TestScan(unittest.TestCase):
    def test_follows_and_skips_compressed(self):
        with tempfile.TemporaryDirectory() as d:
            day = Path(d) / "2026" / "11" / "24"
            day.mkdir(parents=True)
            f = day / "rollout-a.jsonl"
            now = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
            f.write_text(fc("k1", "exec_command", {"cmd": "one"}, now) + "\n", encoding="utf-8")
            (day / "rollout-b.jsonl.zst").write_text(fc("k9", "exec_command", {"cmd": "zst"}, now) + "\n", encoding="utf-8")
            events = []
            t = cx.CodexLogTailer(events.append, root=Path(d))
            t.scan()
            with open(f, "a", encoding="utf-8") as h:
                h.write(fc("k2", "exec_command", {"cmd": "two"}, now) + "\n" + fc("k3", "exec_command", {"cmd": "half"}, now))
            t.scan()
            f.write_text(fc("k1", "exec_command", {"cmd": "one"}, now) + "\n", encoding="utf-8")
            t.scan()
            self.assertEqual([e["text"] for e in events], ["one", "two"])

    def test_old_logs_are_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            day = Path(d) / "2026" / "01" / "01"
            day.mkdir(parents=True)
            f = day / "rollout-old.jsonl"
            f.write_text(fc("o1", "exec_command", {"cmd": "old"}) + "\n", encoding="utf-8")
            old = time.time() - cx.ACTIVE - 60
            os.utime(f, (old, old))
            events = []
            cx.CodexLogTailer(events.append, root=Path(d)).scan()
            self.assertEqual(events, [])


if __name__ == "__main__":
    unittest.main()
