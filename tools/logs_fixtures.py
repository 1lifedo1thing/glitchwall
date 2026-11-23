"""Record what the Python log follower makes of Claude Code log lines, as fixtures the Rust port must match.

Made-up lines only (never real logs, which are private): every tool shape, odd and missing fields, long texts that
get clipped, garbage. Each stream is fed to a fresh ClaudeLogTailer.parse_line and the events it emits are kept.
Writes tests/fixtures/logs.json. Run: uv run python tools/logs_fixtures.py

With --real OUT, it instead runs every real log under ~/.claude/projects through the Python follower and writes
the events to OUT, for a local check that the Rust port agrees on real data (OUT stays on this machine; never
commit it): GW_REAL_LOGS=OUT cargo test --release -- --ignored real_logs
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from glitchwall import claude_logs as cl  # noqa: E402

TS = "2026-11-23T10:00:00.000Z"


def ln(content, ts=TS, **kw):
    return json.dumps({"timestamp": ts, "message": {"role": "assistant", "content": content}, **kw}, ensure_ascii=False)


def use(i, name, inp):
    return {"type": "tool_use", "id": i, "name": name, "input": inp}


def res(i, content, err=None):
    d = {"type": "tool_result", "tool_use_id": i, "content": content}
    if err is not None:
        d["is_error"] = err
    return d


words = "the quick brown fox jumps over the lazy dog and keeps on running far away "
long_text = words * 20
many_lines = "\n".join(f"line {i}" for i in range(40))

streams = []

# every tool the wall knows, each with its result
tools = [
    ("Bash", {"command": "git status -sb", "description": "status"}),
    ("PowerShell", {"command": "Get-ChildItem C:\\dev"}),
    ("Read", {"file_path": "C:\\dev\\x\\main.rs", "offset": 10, "limit": 40}),
    ("Write", {"file_path": "/tmp/a.txt", "content": long_text}),
    ("Write", {"file_path": "/tmp/b.txt", "content": "short"}),
    ("Edit", {"file_path": "a.py", "old_string": long_text, "new_string": long_text + "!"}),
    ("Edit", {"file_path": "a.py", "old_string": "x = 1", "new_string": "x = 2", "replace_all": True}),
    ("Grep", {"pattern": "fn main", "path": "src"}),
    ("Grep", {"pattern": "TODO"}),
    ("Glob", {"pattern": "**/*.rs", "path": ""}),
    ("WebFetch", {"url": "https://example.com/a", "prompt": "summarize"}),
    ("WebSearch", {"query": "rust fancy-regex"}),
    ("WebSearch", {"url": "", "query": "fallback to query"}),
    ("Agent", {"description": "Find the bug", "prompt": "look"}),
    ("Task", {"description": "Old style task"}),
    ("mcp__claude-in-chrome__computer", {"action": "left_click", "action_summary": "Opens the menu", "coordinate": [10, 20]}),
    ("mcp__computer-use__screenshot", {}),
    ("mcp__ccd_session__start_session", {"title": "Löwe 🦁 \"quoted\"", "n": 3, "x": 2.5, "ok": True, "none": None,
                                          "list": [1, "two", {"three": 3}], "tab": "a\tb\nc", "neg": -7, "big": 1e20,
                                          "small": 1e-07, "ctl": "\x01\x7f\u2028"}),
    ("mcp__x__y", {"q": "z " * 120 + "end"}),
    ("SomeTool", {}),
    ("NotebookEdit", {"notebook_path": "n.ipynb", "new_source": "print(1)"}),
]
s = []
for k, (name, inp) in enumerate(tools):
    i = f"toolu_{k:02d}_ABCDEFGHIJKLMNOP"
    s.append(ln([use(i, name, inp)]))
    s.append(ln([res(i, f"result {k}\nsecond line")]))
streams.append({"newer_than": None, "lines": s})

# hidden tools, and results nobody asked for
streams.append({"newer_than": None, "lines": [
    ln([use("h1", "TodoWrite", {"todos": []}), use("h2", "ToolSearch", {"query": "x"}),
        use("h3", "mcp__ccd_turn__update_plan", {"items": []}), use("h4", "mcp__ccd_turn__wrap_up", {"recap": "r"})]),
    ln([res("h1", "x"), res("h2", "y"), res("unknown", "z"), res("h3", "w")]),
]})

# outputs: list content, strings, empties, errors, many lines, long lines, odd line breaks
outs = [
    [{"type": "text", "text": "from a list"}, {"type": "image", "source": {}}, {"type": "text", "text": "and more"}],
    "  padded with spaces  \n\n",
    "",
    None,
    [],
    many_lines,
    long_text,
    "cr\rlf\r\nvt\x0bff\x0cfs\x1cnel\x85ls\u2028ps\u2029end",
    "\n".join(["x" * 100] * 13),
    "tabs\tand  double  spaces",
    "ends in a key ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8",
]
s = []
for k, o in enumerate(outs):
    i = f"o{k}"
    s.append(ln([use(i, "Bash", {"command": f"cmd {k}"})]))
    s.append(ln([res(i, o, err=(True if k % 3 == 0 else (False if k % 3 == 1 else None)))]))
streams.append({"newer_than": None, "lines": s})

# repeats, the same id twice in one line, sidechains, garbage, partial and odd lines
u = use("d1", "Bash", {"command": "ls"})
streams.append({"newer_than": None, "lines": [
    "not json", "[]", "{}", "null", "42", '"a string"', ln("text content"), ln([]), ln([1, "x", None]),
    json.dumps({"timestamp": TS, "message": "not a dict"}), json.dumps({"timestamp": TS}),
    ln([u]), ln([u]), ln([u, res("d1", "a\nb")]), ln([res("d1", "a\nb")]),
    ln([use("s1", "Read", {"file_path": "/side"})], isSidechain=True),
    ln([use("s2", "Read", {"file_path": "/side2"})], isSidechain="yes"),
    ln([use("s3", "Read", {"file_path": "/side3"})], isSidechain=0),
    ln([use("", "Bash", {"command": "no id"})]),
    ln([use(None, "Bash", {"command": "null id"})]),
    ln([{"type": "tool_use", "name": "Bash", "input": {"command": "missing id"}, "id": "m1"}]) + "\r",
    ln([use("w1", "Bash", {"command": "   "})]),
    ln([use("w2", "Bash", {"command": None})]),
    ln([use("w3", "Bash", {"command": 0})]),
    ln([use("w4", "Bash", {"command": 12})]),
    ln([use("w5", "Bash", {})]),
    ln([use("w6", "Bash", None)]),
    ln([{"type": "tool_use", "id": "w7", "input": {"command": "no name"}}]),
    ln([use("w8", "Read", {"file_path": True})]),
    ln([use("w9", "Read", {"file_path": "é" * 3})]),
    ln([use("toolu_0123456789abcdefghij", "Bash", {"command": "long id"})]),
    json.dumps({"message": {"content": [use("nt", "Bash", {"command": "no timestamp"})]}}),
    json.dumps({"timestamp": 12345, "message": {"content": [use("nt2", "Bash", {"command": "number timestamp"})]}}),
    '{"timestamp": "x", "message": {"content": [{"type": "tool_use", "id": "dup", "name": "Bash", "name": "Read", "input": {"file_path": "/dup"}}]}}',
]})

# which lines count as new when a log is first seen
stamps = ["2026-11-23T09:59:59Z", "2026-11-23T10:00:00Z", "2026-11-23T10:00:00.5Z", "2026-11-23T18:00:00+08:00",
          "2026-11-23T04:00:01-06:00", "2026-11-23T10:00:00.123456+00:00", "garbage", "", "2026-11-23", None]
streams.append({"newer_than": 1795428000.0, "lines": [
    ln([use(f"t{k}", "Bash", {"command": f"stamp {st}"})], ts=st) if st is not None
    else json.dumps({"message": {"content": [use(f"t{k}", "Bash", {"command": "no stamp"})]}})
    for k, st in enumerate(stamps)]})

clips = [("short", 10), ("one two three", 9), ("one two three", 8), ("one two three", 3), ("onetwothree", 5),
         ("one two\nthree four", 10), ("abc def\nghi", 8), ("abc  def", 4), ("abc\u3000def ghi", 5), ("", 0),
         ("é" * 10, 5), ("x y", 1), ("x y", 2)]

out = {"project": "C--code-orbit", "file": "abcd1234.jsonl", "streams": [], "clips": []}
for st in streams:
    t = cl.ClaudeLogTailer(lambda e: events.append(e), root=Path("."))
    events = []
    f = Path(out["project"]) / out["file"]
    for line in st["lines"]:
        t.parse_line(line.encode("utf-8"), f, st["newer_than"])
    out["streams"].append({**st, "events": events})
out["clips"] = [{"s": a, "n": n, "out": cl.clip(a, n)} for a, n in clips]

if len(sys.argv) > 2 and sys.argv[1] == "--real":
    real = []
    for f in sorted(cl.logs_root().glob("*/*.jsonl")):
        events = []
        t = cl.ClaudeLogTailer(lambda e: events.append(e), root=cl.logs_root())
        data = f.read_bytes()
        cut = data.rfind(b"\n")
        for line in data[:cut].split(b"\n") if cut >= 0 else []:
            t.parse_line(line, f)
        real.append({"path": str(f), "events": events})
    Path(sys.argv[2]).write_text(json.dumps(real, ensure_ascii=False), encoding="utf-8")
    print(f"{len(real)} logs, {sum(len(r['events']) for r in real)} events -> {sys.argv[2]}")
else:
    p = ROOT / "tests" / "fixtures" / "logs.json"
    p.write_text(json.dumps(out, indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"{sum(len(s['events']) for s in out['streams'])} events from {len(streams)} streams, {len(clips)} clips -> {p.relative_to(ROOT)}")
