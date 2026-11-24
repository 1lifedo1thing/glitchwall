"""Record what the Python Gemini CLI reader makes of made-up Gemini log lines, as fixtures the Rust port must match.

Made-up lines only, in the shapes gemini-cli writes (docs/other-agents.md): every tool, odd and missing fields,
results inline and as "$patch" records, garbage. Each stream is fed to a fresh GeminiLogTailer.parse_line.
Writes tests/fixtures/gemini.json. Run: uv run python tools/gemini_fixtures.py
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from glitchwall import gemini_logs as gl  # noqa: E402

TS = "2026-11-24T10:00:00.000Z"
MAIN = ["tmp", "agent-live-wallpaper", "chats", "session-2026-11-24T10-00-abcd1234.jsonl"]
SUB = ["tmp", "agent-live-wallpaper", "chats", "abcd1234-full-id", "e5f6a7b8.jsonl"]
HASHED = ["tmp", "0123456789abcdef" * 4, "chats", "session-2026-11-24T10-00-9f9f.jsonl"]


def msg(calls, mid="m1", ts=TS, kind="gemini"):
    d = {"id": mid, "timestamp": ts, "type": kind, "content": "", "toolCalls": calls}
    if ts is None:
        del d["timestamp"]
    return json.dumps(d, ensure_ascii=False)


def call(i, name, args, status="scheduled", ts=TS, **kw):
    return {"id": i, "name": name, "args": args, "status": status, "timestamp": ts, **kw}


def patch(calls, **kw):
    return json.dumps({"$patch": {"id": "m1", "toolCalls": calls, **kw}}, ensure_ascii=False)


words = "the quick brown fox jumps over the lazy dog and keeps on running far away "
long_text = words * 20
many_lines = "\n".join(f"line {i}" for i in range(40))

streams = []

# every tool the wall knows, scheduled and then finished
tools = [
    ("run_shell_command", {"command": "git status -sb", "description": "status", "dir_path": "src"}),
    ("read_file", {"file_path": "C:\\dev\\x\\main.rs", "offset": 10, "limit": 40}),
    ("read_many_files", {"include": ["src/**/*.ts", "README.md"]}),
    ("list_directory", {"dir_path": "/home/me/app"}),
    ("write_file", {"file_path": "/tmp/a.txt", "content": long_text}),
    ("write_file", {"file_path": "/tmp/b.txt", "content": "short"}),
    ("replace", {"file_path": "a.py", "old_string": long_text, "new_string": long_text + "!"}),
    ("replace", {"file_path": "a.py", "old_string": "x = 1", "new_string": "x = 2", "allow_multiple": True}),
    ("grep_search", {"pattern": "fn main", "dir_path": "src"}),
    ("grep_search", {"pattern": "TODO"}),
    ("glob", {"pattern": "**/*.rs", "dir_path": ""}),
    ("google_web_search", {"query": "rust fancy-regex"}),
    ("web_fetch", {"prompt": "summarize https://example.com/a " + long_text}),
    ("invoke_agent", {"description": "Find the bug", "name": "codebase_investigator"}),
    ("invoke_agent", {"name": "only a name"}),
    ("invoke_agent", {}),
    ("mcp_github_search_issues", {"title": "Löwe 🦁 \"quoted\"", "n": 3, "x": 2.5, "ok": True, "none": None,
                                  "list": [1, "two", {"three": 3}], "tab": "a\tb\nc", "neg": -7, "big": 1e20,
                                  "small": 1e-07, "ctl": "\x01\x7f\u2028"}),
    ("some_tool", {"q": "z " * 120 + "end"}),
    ("other_tool", {}),
]
s = []
for k, (name, args) in enumerate(tools):
    i = f"{name}-{k:02d}-1732440000000-abcdef"
    s.append(msg([call(i, name, args)]))
    s.append(msg([call(i, name, args, status="success", resultDisplay=f"result {k}\nsecond line", result=[])]))
streams.append({"path": MAIN, "newer_than": None, "lines": s})

# hidden tools, and finished calls for them
hidden = ["write_todos", "update_topic", "ask_user", "enter_plan_mode", "exit_plan_mode", "activate_skill",
          "get_internal_docs", "tracker_create_task", "tracker_update"]
streams.append({"path": MAIN, "newer_than": None, "lines": [
    msg([call(f"h{k}", n, {"todos": []}) for k, n in enumerate(hidden)]),
    msg([call(f"h{k}", n, {}, status="success", resultDisplay="x") for k, n in enumerate(hidden)]),
    patch([{"id": "h0", "result": "late"}]),
]})

# results: display text, function responses, parts, errors, cancels, many lines, odd line breaks
fr = lambda **r: [{"functionResponse": {"id": "x", "name": "t", "response": r}}]  # noqa: E731
results = [
    {"resultDisplay": "plain display"},
    {"resultDisplay": "", "result": fr(output="from the response")},
    {"resultDisplay": None, "result": fr(error="it broke")},
    {"resultDisplay": {"fileDiff": "@@ -1 +1 @@"}, "result": fr(output="diff shown as output")},
    {"result": [{"text": "part one"}, {"inlineData": {}}, {"text": "part two"}, "a bare string"]},
    {"result": {"text": "one part, not a list"}},
    {"result": "just a string"},
    {"result": None},
    {"result": []},
    {"resultDisplay": many_lines},
    {"resultDisplay": long_text},
    {"resultDisplay": "cr\rlf\r\nvt\x0bff\x0cfs\x1cnel\x85ls\u2028ps\u2029end"},
    {"resultDisplay": "\n".join(["x" * 100] * 13)},
    {"resultDisplay": "  padded  \n\n", "result": fr(output="ignored")},
    {"result": fr(output=12, error=["no"])},
    {"resultDisplay": "ends in a key ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"},
]
statuses = ["success", "error", "cancelled"]
s = []
for k, r in enumerate(results):
    i = f"r{k}"
    s.append(msg([call(i, "run_shell_command", {"command": f"cmd {k}"})]))
    s.append(msg([call(i, "run_shell_command", {"command": f"cmd {k}"}, status=statuses[k % 3], **r)]))
streams.append({"path": MAIN, "newer_than": None, "lines": s})

# statuses that are not finished, finished without a result, repeats, several calls in one message
a = call("a1", "read_file", {"file_path": "/a"})
streams.append({"path": MAIN, "newer_than": None, "lines": [
    msg([call("p1", "read_file", {"file_path": "/p"}, status="executing", resultDisplay="not yet")]),
    msg([call("p1", "read_file", {"file_path": "/p"}, status="awaiting_approval", result=[])]),
    msg([call("p1", "read_file", {"file_path": "/p"}, status="success")]),
    msg([call("p1", "read_file", {"file_path": "/p"}, status="success", resultDisplay="now")]),
    msg([a]), msg([a]), msg([a, call("a2", "glob", {"pattern": "*"})]),
    msg([call("a1", "read_file", {"file_path": "/a"}, status="success", resultDisplay="x")] * 2),
    msg([call("a2", "glob", {"pattern": "*"}, status="SUCCESS", resultDisplay="case matters")]),
    msg([call("toolu_0123456789abcdefghij", "run_shell_command", {"command": "long id"})]),
]})

# "$patch" records: late results by id, nested updates, unknown ids, odd shapes
streams.append({"path": MAIN, "newer_than": None, "lines": [
    msg([call("q1", "glob", {"pattern": "*.md"}), call("q2", "glob", {"pattern": "*.rs"}),
         call("q3", "glob", {"pattern": "*.py"}), call("q4", "glob", {"pattern": "*.go"})]),
    patch([{"id": "q1", "result": [{"text": "README.md"}]}]),
    patch([{"id": "q1", "result": [{"text": "again"}]}]),
    patch([], updates=[{"toolCalls": [{"id": "q2", "status": "error", "result": fr(error="denied")}]}, "x", 3]),
    patch([{"id": "q3", "status": "cancelled", "resultDisplay": "stopped"}]),
    patch([{"id": "zz", "result": "unknown id"}, "not a dict", None, {"result": "no id"}]),
    json.dumps({"$patch": {"toolCalls": 5, "updates": 7}}),
    json.dumps({"$patch": "not a dict"}),
    json.dumps({"$patch": {"updates": {"toolCalls": [{"id": "q4", "result": "dict, not list"}]}}}),
    patch([{"id": "q4", "result": "the go files", "timestamp": "2026-11-24T10:05:00Z"}]),
]})

# garbage and odd fields
streams.append({"path": MAIN, "newer_than": None, "lines": [
    "not json", "[]", "{}", "null", "42", '"a string"', "",
    json.dumps({"$set": {"sessionId": "s", "kind": "main"}}), json.dumps({"$rewindTo": "m1"}),
    msg([call("u1", "read_file", {"file_path": "/user"})], kind="user"),
    msg([call("u2", "read_file", {"file_path": "/info"})], kind="info"),
    json.dumps({"type": "gemini", "toolCalls": "nope"}), json.dumps({"type": "gemini", "toolCalls": 12}),
    json.dumps({"type": "gemini", "toolCalls": {"id": "x"}}), json.dumps({"type": "gemini"}),
    msg([1, None, "x", [], {"name": "read_file"}]),
    msg([call("", "run_shell_command", {"command": "empty id"}), call(None, "run_shell_command", {"command": "null id"}),
         call(0, "run_shell_command", {"command": "zero id"}), call(7, "run_shell_command", {"command": "number id"}),
         call(True, "run_shell_command", {"command": "true id"})]),
    msg([call("w1", "run_shell_command", {"command": "   "}), call("w2", "run_shell_command", {"command": None}),
         call("w3", "run_shell_command", {"command": 0}), call("w4", "run_shell_command", {"command": 12.5}),
         call("w5", "run_shell_command", {}), call("w6", "run_shell_command", None),
         call("w7", "run_shell_command", ["ls"]), call("w8", "read_file", {"file_path": True}),
         call("w9", "read_file", {"file_path": "é" * 3}), call("w10", None, {"command": "no name"}),
         call("w11", 5, {"x": 1}), call("w12", "read_file", {"file_path": {"nested": [1, 2]}})]),
    msg([call("n1", "read_file", {"file_path": "/no ts"}, ts=None)]),
    msg([call("n2", "read_file", {"file_path": "/num ts"}, ts=12345)]),
    msg([call("n3", "read_file", {"file_path": "/bool ts"}, ts=False)]),
    '{"type": "gemini", "toolCalls": [{"id": "dup", "name": "glob", "name": "read_file", "args": {"file_path": "/dup"}}]}',
    msg([call("c1", "run_shell_command", {"command": "crlf line"})]) + "\r",
]})

# which messages count as new when a log is first seen (the message's timestamp decides)
stamps = ["2026-11-24T09:59:59Z", "2026-11-24T10:00:00Z", "2026-11-24T10:00:00.5Z", "2026-11-24T18:00:00+08:00",
          "2026-11-24T04:00:01-06:00", "garbage", "", "2026-11-24", None]
streams.append({"path": MAIN, "newer_than": 1795514400.0, "lines": [
    msg([call(f"t{k}", "run_shell_command", {"command": f"stamp {st}"})], mid=f"t{k}", ts=st) for k, st in enumerate(stamps)]
    + [patch([{"id": "t1", "result": "patches have no timestamp"}])]})

# a subagent's log, and a project folder named by a hash
for p in (SUB, HASHED):
    streams.append({"path": p, "newer_than": None, "lines": [
        msg([call("s1", "read_file", {"file_path": "/side"})]),
        msg([call("s1", "read_file", {"file_path": "/side"}, status="success", resultDisplay="side out")]),
    ]})

labels = [MAIN, SUB, HASHED, ["tmp", "plain", "chats", "s.jsonl"], ["tmp", "ends-with-", "chats", "s.jsonl"],
          ["tmp", "0123ABCD" * 4, "chats", "s.jsonl"], ["tmp", "abc123", "chats", "x", "s.jsonl"]]

out = {"streams": [], "labels": []}
for st in streams:
    events = []
    t = gl.GeminiLogTailer(events.append, root=Path("."))
    f = Path(*st["path"])
    for line in st["lines"]:
        t.parse_line(line.encode("utf-8"), f, st["newer_than"])
    out["streams"].append({**st, "events": events})
out["labels"] = [{"path": p, "out": gl.project_label(Path(*p))} for p in labels]

p = ROOT / "tests" / "fixtures" / "gemini.json"
from fixture_text import hide_fakes   # fake keys get an invisible marker, so code hosts don't take them for leaks
p.write_text(hide_fakes(json.dumps(out, indent=1, ensure_ascii=False)), encoding="utf-8")
print(f"{sum(len(s['events']) for s in out['streams'])} events from {len(streams)} streams, {len(labels)} labels -> {p.relative_to(ROOT)}")
