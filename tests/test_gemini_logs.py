"""Gemini CLI log reader: made-up lines in the shapes gemini-cli writes (see docs/other-agents.md)."""
import json
import os
import tempfile
import time
import unittest
from pathlib import Path

from glitchwall import gemini_logs as gl

TS = "2026-11-24T10:00:00.000Z"
F = Path("tmp") / "my-app" / "chats" / "session-2026-11-24T10-00-abcd1234.jsonl"


def msg(tool_calls, mid="m1", ts=TS):
    return json.dumps({"id": mid, "timestamp": ts, "type": "gemini", "content": "", "toolCalls": tool_calls})


def call(i, name, args, status="scheduled", **kw):
    return {"id": i, "name": name, "args": args, "status": status, "timestamp": TS, **kw}


class Feed:
    def __init__(self, f=F):
        self.events = []
        self.t = gl.GeminiLogTailer(self.events.append, root=Path("."))
        self.f = f

    def __call__(self, *lines):
        for line in lines:
            self.t.parse_line(line.encode("utf-8") if isinstance(line, str) else line, self.f)
        return [(e["kind"], e["text"]) for e in self.events]


class TestDescribe(unittest.TestCase):
    def test_tools(self):
        self.assertEqual(gl.describe("run_shell_command", {"command": "npm test"}), ("$", "npm test"))
        self.assertEqual(gl.describe("read_file", {"file_path": "src/a.ts"}), ("read", "src/a.ts"))
        self.assertEqual(gl.describe("list_directory", {"dir_path": "src"}), ("read", "src"))
        self.assertEqual(gl.describe("grep_search", {"pattern": "TODO", "dir_path": "src"}), ("find", "TODO  in src"))
        self.assertEqual(gl.describe("glob", {"pattern": "**/*.ts"}), ("find", "**/*.ts"))
        self.assertEqual(gl.describe("google_web_search", {"query": "rust regex"}), ("web", "rust regex"))
        self.assertEqual(gl.describe("replace", {"file_path": "a.py", "old_string": "x = 1", "new_string": "x = 2"}),
                         ("edit", "a.py\n- x = 1\n+ x = 2"))
        self.assertEqual(gl.describe("write_file", {"file_path": "b.txt", "content": "hi"}), ("write", "b.txt\nhi"))
        self.assertEqual(gl.describe("invoke_agent", {"description": "find the bug"}), ("agent", "find the bug"))

    def test_hidden_and_unknown(self):
        for name in ("write_todos", "update_topic", "ask_user", "tracker_create_task", "exit_plan_mode"):
            self.assertEqual(gl.describe(name, {}), (None, ""), name)
        kind, text = gl.describe("mcp_github_search", {"q": "x"})
        self.assertEqual(kind, "tool")
        self.assertTrue(text.startswith("mcp_github_search "))

    def test_odd_args(self):
        self.assertEqual(gl.describe("run_shell_command", {"command": None}), ("$", ""))
        self.assertEqual(gl.describe("read_file", {"file_path": 12}), ("read", "12"))


class TestParse(unittest.TestCase):
    def test_call_then_result(self):
        feed = Feed()
        out = feed(msg([call("c1", "run_shell_command", {"command": "ls"})]),
                   msg([call("c1", "run_shell_command", {"command": "ls"}, status="success",
                             resultDisplay="a.txt\nb.txt", result=[])]))
        self.assertEqual(out, [("$", "ls"), ("out", "a.txt\nb.txt")])

    def test_error_and_function_response(self):
        feed = Feed()
        res = [{"functionResponse": {"id": "c2", "name": "read_file", "response": {"error": "no such file"}}}]
        out = feed(msg([call("c2", "read_file", {"file_path": "x"}, status="error", result=res)]))
        self.assertEqual(out, [("read", "x"), ("err", "no such file")])

    def test_repeats_are_shown_once(self):
        line = msg([call("c3", "read_file", {"file_path": "x"}, status="success", resultDisplay="ok", result=[])])
        self.assertEqual(Feed()(line, line, line), [("read", "x"), ("out", "ok")])

    def test_patch_result(self):
        feed = Feed()
        out = feed(msg([call("c4", "glob", {"pattern": "*.md"})]),
                   json.dumps({"$patch": {"id": "m1", "toolCalls": [{"id": "c4", "result": [{"text": "README.md"}]}]}}))
        self.assertEqual(out, [("find", "*.md"), ("out", "README.md")])

    def test_patch_error_and_odd_shapes(self):
        feed = Feed()
        out = feed(msg([call("c7", "glob", {"pattern": "*.rs"})]),
                   json.dumps({"$patch": {"toolCalls": 5, "updates": 7}}),
                   json.dumps({"$patch": {"updates": [{"toolCalls": [{"id": "c7", "status": "error", "result": "denied"}]}]}}),
                   json.dumps({"type": "gemini", "toolCalls": 12}))
        self.assertEqual(out, [("find", "*.rs"), ("err", "denied")])

    def test_patch_for_unknown_call_is_ignored(self):
        self.assertEqual(Feed()(json.dumps({"$patch": {"toolCalls": [{"id": "zz", "result": "x"}]}})), [])

    def test_other_records_ignored(self):
        lines = [json.dumps({"$set": {"sessionId": "s", "kind": "main"}}), json.dumps({"$rewindTo": "m1"}),
                 json.dumps({"id": "u1", "timestamp": TS, "type": "user", "content": "hi"}),
                 "not json", "[]", "null", "{}", json.dumps({"type": "gemini", "toolCalls": "nope"}),
                 json.dumps({"type": "gemini", "toolCalls": [1, None, {"name": "read_file"}]})]
        self.assertEqual(Feed()(*lines), [])

    def test_long_output_is_cut(self):
        text = "\n".join(f"line {i}" for i in range(40))
        out = Feed()(msg([call("c5", "run_shell_command", {"command": "seq"}, status="success", resultDisplay=text)]))
        self.assertIn("more lines", out[1][1])

    def test_subagent_is_a_side_session(self):
        f = Path("tmp") / "my-app" / "chats" / "parent-id" / "sub-1234.jsonl"
        feed = Feed(f)
        feed(msg([call("c6", "read_file", {"file_path": "y"})]))
        self.assertTrue(feed.events[0]["side"])
        self.assertEqual(feed.events[0]["proj"], "app")

    def test_hashed_project_folder(self):
        f = Path("tmp") / ("a" * 64) / "chats" / "session-x.jsonl"
        self.assertEqual(gl.project_label(f), "gemini")


class TestScan(unittest.TestCase):
    def test_follows_appends_and_rewrites(self):
        with tempfile.TemporaryDirectory() as d:
            chats = Path(d) / "proj" / "chats"
            chats.mkdir(parents=True)
            f = chats / "session-a.jsonl"
            now = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
            f.write_text(msg([call("k1", "read_file", {"file_path": "one"})], ts=now) + "\n", encoding="utf-8")
            events = []
            t = gl.GeminiLogTailer(events.append, root=Path(d))
            t.scan()
            with open(f, "a", encoding="utf-8") as h:
                h.write(msg([call("k2", "read_file", {"file_path": "two"})], mid="m2", ts=now) + "\n")
                h.write(msg([call("k3", "read_file", {"file_path": "half"})], mid="m3", ts=now))   # no newline yet
            t.scan()
            self.assertEqual([e["text"] for e in events], ["one", "two"])
            f.write_text(msg([call("k1", "read_file", {"file_path": "one"})], ts=now) + "\n", encoding="utf-8")
            t.scan()   # shrank: read from the start again, nothing shown twice
            self.assertEqual([e["text"] for e in events], ["one", "two"])

    def test_old_logs_are_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            chats = Path(d) / "proj" / "chats"
            chats.mkdir(parents=True)
            f = chats / "session-old.jsonl"
            f.write_text(msg([call("o1", "read_file", {"file_path": "old"})]) + "\n", encoding="utf-8")
            old = time.time() - gl.ACTIVE - 60
            os.utime(f, (old, old))
            events = []
            gl.GeminiLogTailer(events.append, root=Path(d)).scan()
            self.assertEqual(events, [])

    def test_quiet_folders_are_looked_at_once_a_minute(self):
        with tempfile.TemporaryDirectory() as d:
            now = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
            quiet, busy = Path(d) / "old-proj" / "chats", Path(d) / "new-proj" / "chats"
            quiet.mkdir(parents=True)
            busy.mkdir(parents=True)
            events = []
            t = gl.GeminiLogTailer(events.append, root=Path(d))
            t.scan()   # the first look is at everything
            (quiet / "session-resumed.jsonl").write_text(msg([call("r1", "read_file", {"file_path": "resumed"})], ts=now) + "\n", encoding="utf-8")
            old = time.time() - gl.ACTIVE - 60
            os.utime(quiet, (old, old))   # a folder with no new file for a while
            sub = busy / "session-a"
            sub.mkdir()
            (sub / "agent-1.jsonl").write_text(msg([call("s1", "read_file", {"file_path": "subagent"})], ts=now) + "\n", encoding="utf-8")
            t.scan()
            self.assertEqual([e["text"] for e in events], ["subagent"])
            t.follow.swept -= 60
            t.scan()
            self.assertEqual([e["text"] for e in events], ["subagent", "resumed"])


class TestShellOutput(unittest.TestCase):
    def test_wrapped(self):
        self.assertEqual(gl.shell_output("<untrusted_context>\nOutput: hello\nProcess Group PGID: 4\n</untrusted_context>"),
                         ("hello", False))
        self.assertEqual(gl.shell_output("Output: not found\nExit Code: 1"), ("not found", True))
        self.assertEqual(gl.shell_output("Command: ls\nDirectory: (root)\nOutput: a\nb\nError: (none)\nExit Code: 0"),
                         ("a\nb\nError: (none)", False))

    def test_codes(self):
        for code, failed in (("-1", True), ("000", False), ("--1", False), ("(none)", False), ("127", True)):
            self.assertEqual(gl.shell_output(f"Output: x\nExit Code: {code}")[1], failed, code)

    def test_no_markers_left_alone(self):
        self.assertEqual(gl.shell_output("plain text"), ("plain text", False))
        t = "<untrusted_context>no markers</untrusted_context>"
        self.assertEqual(gl.shell_output(t), (t, False))

    def test_failed_shell_is_err(self):
        grid = [[{"text": "row"}]]
        o = "<untrusted_context>\nOutput: python: not found\nExit Code: 1\n</untrusted_context>"
        got = Feed()(msg([call("s1", "run_shell_command", {"command": "python a.py"})]),
                     msg([call("s1", "run_shell_command", {"command": "python a.py"}, status="success",
                               resultDisplay=grid, result=[{"functionResponse": {"response": {"output": o}}}])]))
        self.assertEqual(got, [("$", "python a.py"), ("err", "python: not found")])


if __name__ == "__main__":
    unittest.main()
