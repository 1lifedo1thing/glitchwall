"""Record what the Python Codex CLI reader makes of made-up rollout lines, as fixtures the Rust port must match.

Made-up lines only, in the shapes codex writes (docs/other-agents.md): every tool, old and new shell forms, output
headers, apply_patch, web searches, odd and missing fields, garbage. Each stream is fed to a fresh
CodexLogTailer.parse_line. Writes tests/fixtures/codex.json. Run: uv run python tools/codex_fixtures.py
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from glitchwall import codex_logs as cx  # noqa: E402

TS = "2026-11-24T10:00:00.000Z"
FILE = ["sessions", "2026", "11", "24", "rollout-2026-11-24T10-00-00-0199a213-81c0-7800-8aa1-bbab2a035a53.jsonl"]


def rec(typ, payload, ts=TS):
    d = {"timestamp": ts, "type": typ, "payload": payload}
    if ts is None:
        del d["timestamp"]
    return json.dumps(d, ensure_ascii=False)


def item(payload, ts=TS):
    return rec("response_item", payload, ts)


def fc(cid, name, args, **kw):
    return item({"type": "function_call", "name": name, "arguments": args if isinstance(args, str) else json.dumps(args),
                 "call_id": cid, **kw})


def out(cid, output, kind="function_call_output"):
    return item({"type": kind, "call_id": cid, "output": output})


def meta(cwd="C:\\dev\\agent-live-wallpaper", **kw):
    return rec("session_meta", {"id": "0199a213", "timestamp": TS, "cwd": cwd, "originator": "codex_cli_rs",
                                "cli_version": "0.172.0", "source": "cli", **kw})


def ue(code, body, running=False):
    head = "Chunk ID: 7d1f\nWall time: 0.0123 seconds\n"
    head += "Process running with session ID 3\n" if running else f"Process exited with code {code}\n"
    return head + "Original token count: 12\nOutput:\n" + body


words = "the quick brown fox jumps over the lazy dog and keeps on running far away "
long_text = words * 20
many_lines = "\n".join(f"line {i}" for i in range(40))
PATCH = "*** Begin Patch\n*** Update File: src/app.py\n@@ def main():\n-    x = 1\n+    x = 2\n" \
        "*** Add File: notes.md\n+hello\n*** Delete File: old.txt\n*** Update File: a.rs\n*** Move to: b.rs\n" \
        "@@\n-fn a() {}\n+fn b() {}\n*** End of File\n*** End Patch"

streams = []

# every tool the wall knows, each with its result
calls = [
    ("exec_command", {"cmd": "git status -sb", "workdir": "C:\\dev\\x", "yield_time_ms": 1000}),
    ("exec_command", {"cmd": "cargo test", "tty": True}),
    ("shell", {"command": ["bash", "-lc", "ls -la && echo done"], "workdir": "/repo"}),
    ("shell", {"command": ["powershell.exe", "-NoProfile", "-Command", "Get-ChildItem"]}),
    ("shell", {"command": ["rg", "--files", "src"]}),
    ("shell", {"command": "already a string"}),
    ("shell", {"command": ["cmd", "/c", "dir"]}),
    ("shell", {"command": [1, None, True, "x"]}),
    ("shell_command", {"command": "npm test -- --watch=false"}),
    ("apply_patch", {"input": PATCH}),
    ("view_image", {"path": "shots/a.png"}),
    ("mcp__github__search_issues", {"q": "Löwe 🦁 \"quoted\"", "n": 3, "x": 2.5, "ok": True, "none": None}),
    ("some_tool", {"q": "z " * 120 + "end"}),
    ("other_tool", {}),
]
s = [meta()]
for k, (name, args) in enumerate(calls):
    cid = f"call_{k:02d}AbCdEfGhIjKlMnOpQr"
    s.append(fc(cid, name, args))
    s.append(out(cid, f"result {k}\nsecond line"))
s.append(item({"type": "custom_tool_call", "status": "completed", "call_id": "call_patch1", "name": "apply_patch", "input": PATCH}))
s.append(out("call_patch1", json.dumps({"output": "Success. Updated the following files:\nM src/app.py\n",
                                        "metadata": {"exit_code": 0, "duration_seconds": 0.0}}), "custom_tool_call_output"))
s.append(item({"type": "custom_tool_call", "call_id": "call_ct2", "name": "js_repl", "input": "console.log(1)"}))
s.append(out("call_ct2", "1", "custom_tool_call_output"))
s.append(item({"type": "local_shell_call", "call_id": "call_ls1", "status": "completed",
               "action": {"type": "exec", "command": ["bash", "-lc", "pwd"], "timeout_ms": 1000}}))
s.append(out("call_ls1", json.dumps({"output": "/repo\n", "metadata": {"exit_code": 0}})))
s.append(item({"type": "local_shell_call", "id": "lsh_2", "call_id": None, "status": "in_progress",
               "action": {"type": "exec", "command": ["make"]}}))
streams.append({"path": FILE, "newer_than": None, "lines": s})

# hidden tools and their results
s = [meta()]
for k, name in enumerate(cx.HIDDEN):
    s.append(fc(f"h{k}", name, {"plan": [], "chars": "", "session_id": 3}))
    s.append(out(f"h{k}", "ok"))
streams.append({"path": FILE, "newer_than": None, "lines": s})

# outputs: headers, exit codes, old JSON form, content items, many lines, odd line breaks
outs = [
    ue(0, "fine\n"),
    ue(1, "error: build failed\n"),
    ue(0, "", running=True),
    ue(-1, "killed"),
    ue(0, "a\nOutput:\nstill output"),
    "Exit code: 2\nWall time: 1 seconds\nOutput:\nfrom shell_command",
    "Exit code: x\nOutput:\nnot a number",
    "Wall time: 1 seconds\nno output line",
    "Output: on the same line",
    "Process exited with code 000\nOutput:zeros",
    "Process exited with code 99999999999999999999999\nOutput:\nhuge",
    json.dumps({"output": "old form ok", "metadata": {"exit_code": 0, "duration_seconds": 1.5}}),
    json.dumps({"output": "old form failed", "metadata": {"exit_code": 127}}),
    json.dumps({"output": "float code", "metadata": {"exit_code": 1.0}}),
    json.dumps({"output": "bool code", "metadata": {"exit_code": True}}),
    json.dumps({"output": "no meta"}),
    json.dumps({"output": 5, "metadata": {"exit_code": 1}}),
    json.dumps({"not": "output"}),
    "{broken json",
    [{"type": "input_text", "text": "from items"}, {"type": "input_image", "image_url": "data:"}, {"type": "input_text", "text": "two"}],
    [{"type": "input_text", "text": ue(3, "items with header")}],
    [],
    None,
    42,
    "",
    "  padded  \n\n",
    many_lines,
    long_text,
    "cr\rlf\r\nvt\x0bff\x0cfs\x1cnel\x85ls\u2028ps\u2029end",
    "ends in a key ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8",
]
s = [meta()]
for k, o in enumerate(outs):
    s.append(fc(f"o{k}", "exec_command", {"cmd": f"cmd {k}"}))
    s.append(out(f"o{k}", o))
streams.append({"path": FILE, "newer_than": None, "lines": s})

# web searches
acts = [{"type": "search", "query": "rust regex"}, {"type": "search", "queries": ["a", 1, "b"]},
        {"type": "search", "query": "", "queries": ["fallback"]}, {"type": "search"},
        {"type": "open_page", "url": "https://example.com/x"}, {"type": "find_in_page", "url": "https://e.com", "pattern": "needle"},
        {"type": "find_in_page", "pattern": "no url"}, {"type": "other"}, "not a dict", None]
s = [item({"type": "web_search_call", "id": f"ws_{k}", "status": "completed", "action": a}) for k, a in enumerate(acts)]
s.append(item({"type": "web_search_call", "status": "completed", "action": {"type": "search", "query": "no id"}}))
s.append(item({"type": "web_search_call", "status": "completed", "action": {"type": "search", "query": "no id"}}))
s.append(item({"type": "web_search_call", "status": "completed", "action": {"type": "search", "query": "no id"}}, ts="2026-11-24T10:00:01Z"))
streams.append({"path": FILE, "newer_than": None, "lines": s})

# metadata: project from cwd (meta or turn context), subagent sessions, odd values
streams.append({"path": FILE, "newer_than": None, "lines": [
    fc("m0", "exec_command", {"cmd": "before any meta"}),
    meta("/home/me/my-app/"), fc("m1", "exec_command", {"cmd": "in my-app"}),
    rec("turn_context", {"cwd": "C:\\work\\other", "model": "x"}), fc("m2", "exec_command", {"cmd": "in other"}),
    meta("D:\\", parent_thread_id="0199"), fc("m3", "exec_command", {"cmd": "subagent by parent"}),
    meta("/r", source={"subagent": {"thread_spawn": {}}}), fc("m4", "exec_command", {"cmd": "subagent by source"}),
    meta("/r", source="exec", parent_thread_id=""), fc("m5", "exec_command", {"cmd": "not a subagent"}),
    rec("turn_context", {"cwd": 5}), fc("m6", "exec_command", {"cmd": "cwd not a string"}),
    meta(None), fc("m7", "exec_command", {"cmd": "cwd null"}),
    rec("session_meta", {"source": {"subagent": "review"}}), fc("m8", "exec_command", {"cmd": "meta without cwd"}),
    rec("session_meta", "not a dict"), fc("m9", "exec_command", {"cmd": "meta payload not a dict"}),
]})

# garbage, repeats, odd fields
streams.append({"path": FILE, "newer_than": None, "lines": [
    "not json", "[]", "{}", "null", "42", '"a string"', "",
    json.dumps({"type": "response_item"}), json.dumps({"type": "response_item", "payload": [1]}),
    item({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hi"}]}),
    item({"type": "reasoning", "summary": []}),
    rec("event_msg", {"type": "exec_command_end", "call_id": "x", "aggregated_output": "dup"}),
    rec("compacted", {"message": "summary"}),
    item({"type": "tool_search_call", "call_id": "ts1", "execution": "client", "arguments": {}}),
    fc("d1", "exec_command", {"cmd": "ls"}), fc("d1", "exec_command", {"cmd": "ls"}), out("d1", "a"), out("d1", "a"),
    out("never-called", "orphan"), out("", "empty id"), out(None, "null id"),
    fc("", "exec_command", {"cmd": "empty call id"}), fc(None, "exec_command", {"cmd": "null call id"}),
    fc(7, "exec_command", {"cmd": "number call id"}), out(7, "number id out"),
    fc("a1", "exec_command", "not json"), fc("a2", "exec_command", "[1, 2]"), fc("a3", "exec_command", 5),
    fc("a4", "exec_command", {"cmd": None}), fc("a5", "exec_command", {"cmd": 12.5}),
    fc("a6", "exec_command", {"cmd": "   "}), fc("a7", None, {"cmd": "no name"}), fc("a8", 3, {"x": 1}),
    fc("a9", "exec_command", {"cmd": ["argv", "list"]}), fc("a10", "unknown_tool", "raw args, not json"),
    item({"type": "custom_tool_call", "call_id": "c1", "name": "apply_patch", "input": None}),
    item({"type": "custom_tool_call", "call_id": "c2", "name": None, "input": "x"}),
    item({"type": "local_shell_call", "call_id": "l1", "action": "not a dict"}),
    item({"type": "local_shell_call", "call_id": "l2", "action": {"command": "a string"}}),
    fc("call_0123456789abcdefghij", "exec_command", {"cmd": "long id"}),
    fc("n1", "exec_command", {"cmd": "no ts"}).replace('"timestamp": "' + TS + '", ', ""),
    item({"type": "function_call", "name": "exec_command", "arguments": "{\"cmd\": \"num ts\"}", "call_id": "n2"}, ts=12345),
    '{"timestamp": "x", "type": "response_item", "payload": {"type": "function_call", "name": "shell", "name": "exec_command", "arguments": "{\\"cmd\\": \\"dup\\"}", "call_id": "dup"}}',
    fc("cr", "exec_command", {"cmd": "crlf line"}) + "\r",
]})

# code mode (seen in real logs, v0.172.0): one `exec` tool running a short JavaScript; a long one answers
# "Script running with cell ID N" and a later `wait` call brings its output
def ct(cid, name, inp):
    return item({"type": "custom_tool_call", "status": "completed", "call_id": cid, "name": name, "input": inp})


def ctout(cid, output):
    return out(cid, output, "custom_tool_call_output")


scripts = [
    'const r = await tools.exec_command({cmd:"Get-Content -LiteralPath .\\\\notes.txt; python hello.py","workdir":"C:\\\\x"}); text(r.output);',
    'const patch = "*** Begin Patch\\n*** Update File: C:\\\\x\\\\a.txt\\n@@\\n-line two\\n+line 2\\n*** End Patch";\n'
    'const p = await tools.apply_patch(patch); text(p);\n'
    'const r = await tools.exec_command({cmd:"rg -n \\"test\\" .", "workdir":"C:\\\\x"}); text(r.output);',
    "await tools.exec_command({ 'cmd' : 'single quoted' }); await tools.exec_command({\"cmd\":\n  `template ${x}`});",
    'tools.exec_command({cmd: "esc \\u00e9 \\u{1F600} \\x41 \\uD800 \\u{110000} \\u{} \\q \\0 \\\nend"})',
    'tools.exec_command({workdir: "/a", command: "nope", cmd2: 1, cmd: "after others"})',
    'tools.exec_command({cmd: "never closes',
    'tools.exec_command({cmd: 5}); tools.exec_command({cmd: "second"})',
    'tools.exec_command({}); tools.view_image({path: "a.png", cmd: "not exec_command\'s"})',
    'const p = `\n*** Begin Patch\n*** Add File: b.md\n+hi\n*** End Patch`; await tools.apply_patch(p);',
    "x = '*** Begin Patch\\n*** Delete File: gone.txt\\n*** End Patch'",
    '"*** Begin Patch\n*** Update File: unclosed.txt',
    "const r = await tools.view_image({path: 'shot.png'}); text(r);",
    "",
    "Löwe 🦁 tools.exec_command({cmd: \"ünïcode ✓\"})",
]
s = [meta("C:\\dev\\gw-agent-test")]
for k, src in enumerate(scripts):
    s.append(ct(f"x{k}", "exec", src))
    s.append(ctout(f"x{k}", [{"type": "input_text", "text": "Script completed\nWall time 1.5 seconds\nOutput:\n"},
                             {"type": "input_text", "text": f"result {k}"}]))
# running, then a wait brings the output; a wait still running; a wait for an unknown cell; cells per file
s += [
    ct("r1", "exec", 'await tools.exec_command({cmd: "slow build"})'),
    ctout("r1", "Script running with cell ID 7\nWall time 30.0 seconds\nOutput:\n"),
    fc("w1", "wait", {"cell_id": "7", "yield_time_ms": 30000}),
    out("w1", [{"type": "input_text", "text": "Script running with cell ID 7\nWall time 30.0 seconds\nOutput:\n"}]),
    fc("w2", "wait", {"cell_id": 7}),
    out("w2", [{"type": "input_text", "text": "Script completed\nWall time 3.1 seconds\nOutput:\n"},
               {"type": "input_text", "text": "built"}]),
    out("w2", "again"),
    fc("w3", "wait", {"cell_id": "99"}), out("w3", "unknown cell"),
    fc("w4", "wait", {}), out("w4", "no cell"),
    ct("r2", "exec", 'await tools.exec_command({cmd: "blank cell"})'),
    ctout("r2", "Script running with cell ID  \nOutput:\nshown, the cell id is blank"),
    ct("r3", "exec", 'await tools.exec_command({cmd: "header only"})'),
    ctout("r3", "Script completed\nWall time 0.1 seconds"),
    fc("w5", "wait", {"cell_id": "7"}), out("w5", "cell 7 was answered already"),
]
streams.append({"path": FILE, "newer_than": None, "lines": s})

# which lines count as new when a log is first seen
stamps = ["2026-11-24T09:59:59Z", "2026-11-24T10:00:00Z", "2026-11-24T10:00:00.5Z", "2026-11-24T18:00:00+08:00",
          "2026-11-24T04:00:01-06:00", "garbage", "", "2026-11-24", None]
s = [meta()]
for k, st in enumerate(stamps):
    s.append(item({"type": "function_call", "name": "exec_command", "arguments": json.dumps({"cmd": f"stamp {st}"}),
                   "call_id": f"t{k}"}, ts=st))
s.append(out("t1", "output of a new call", ) .replace(TS, "2026-11-24T09:00:00Z"))
streams.append({"path": FILE, "newer_than": 1795514400.0, "lines": s})

labels = ["C:\\dev\\agent-live-wallpaper", "/home/me/app/", "/home/me/ends-with-", "D:\\", "/", "", "plain", "a/b\\c-d"]
shells = [["bash", "-lc", "x"], ["zsh", "-c", "y"], ["a", "b"], ["-c", "z"], [], "s", None, 5, ["pwsh", "-command", "w"]]

res = {"streams": [], "labels": [], "shells": []}
for st in streams:
    events = []
    t = cx.CodexLogTailer(events.append, root=Path("."))
    f = Path(*st["path"])
    for line in st["lines"]:
        t.parse_line(line.encode("utf-8"), f, st["newer_than"])
    res["streams"].append({**st, "events": events})
res["labels"] = [{"cwd": c, "out": cx.project_label(c)} for c in labels]
res["shells"] = [{"argv": a, "out": cx.shell_text(a)} for a in shells]

p = ROOT / "tests" / "fixtures" / "codex.json"
from fixture_text import hide_fakes   # fake keys get an invisible marker, so code hosts don't take them for leaks
p.write_text(hide_fakes(json.dumps(res, indent=1, ensure_ascii=False)), encoding="utf-8")
print(f"{sum(len(s['events']) for s in res['streams'])} events from {len(streams)} streams -> {p.relative_to(ROOT)}")
