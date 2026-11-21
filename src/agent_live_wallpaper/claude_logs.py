"""Read Claude Code session logs and turn tool calls into wallpaper events.

Claude Code writes one JSON object per line to ~/.claude/projects/<project>/<session>.jsonl.
Files are opened read-only, only appended data is read, and nothing is ever written there.
"""
from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Callable

ACTIVE = 30 * 60       # only start following logs touched in the last 30 minutes
FIRST_TAIL = 200_000   # on first sight of a log, read only its recent tail
MAX_OUT_LINES = 14


def logs_root() -> Path:
    base = os.environ.get("CLAUDE_CONFIG_DIR")
    return (Path(base) if base else Path.home() / ".claude") / "projects"


def describe(name: str, inp: dict) -> tuple[str | None, str]:
    """A tool call -> (kind, text) as it should appear on the wall. kind None = skip."""
    def g(k):
        return str(inp.get(k, "") or "")
    if name in ("Bash", "PowerShell"):
        return ("$" if name == "Bash" else "PS>", g("command"))
    if name == "Read":
        return ("read", g("file_path"))
    if name == "Write":
        return ("write", g("file_path") + "\n" + g("content")[:600])
    if name == "Edit":
        return ("edit", g("file_path") + "\n- " + g("old_string")[:240] + "\n+ " + g("new_string")[:360])
    if name in ("Grep", "Glob"):
        return ("find", g("pattern") + ("  in " + g("path") if g("path") else ""))
    if name in ("WebFetch", "WebSearch"):
        return ("web", g("url") or g("query"))
    if name in ("Agent", "Task"):
        return ("agent", g("description"))
    if "update_plan" in name or "wrap_up" in name or name in ("ToolSearch", "TodoWrite"):
        return (None, "")
    if "computer" in name:
        return ("mouse", g("action") + " " + g("action_summary"))
    short = name.split("__")[-1]
    return ("tool", short + " " + json.dumps(inp, ensure_ascii=False)[:200])


def project_label(f: Path) -> str:
    # folder names are the project path with separators turned into "-"; show the last piece
    return f.parent.name.rstrip("-").split("-")[-1] or "project"


def _output_text(content) -> str:
    if isinstance(content, list):
        content = "\n".join(x.get("text", "") for x in content if isinstance(x, dict) and x.get("type") == "text")
    out = (content or "").strip() if isinstance(content, str) else ""
    out = out or "(no output)"
    lines = out.splitlines()
    if len(lines) > MAX_OUT_LINES:
        out = "\n".join(lines[:MAX_OUT_LINES]) + f"\n… {len(lines) - MAX_OUT_LINES} more lines"
    return out[:900]


class ClaudeLogTailer:
    """Follows the logs and calls emit(event) for each new tool call and each matching result."""

    def __init__(self, emit: Callable[[dict], None], root: Path | None = None):
        self.emit = emit
        self.root = root or logs_root()
        self.offsets: dict[Path, int] = {}
        self.shown: set[str] = set()   # tool_use ids we displayed, so their results are shown too
        self.seen: set[str] = set()    # already emitted (logs can repeat a message)

    def parse_line(self, line: bytes, f: Path) -> None:
        try:
            d = json.loads(line)
        except ValueError:
            return
        if not isinstance(d, dict):
            return
        m = d.get("message") or {}
        content = m.get("content") if isinstance(m, dict) else None
        if not isinstance(content, list):
            return
        for c in content:
            if not isinstance(c, dict):
                continue
            if c.get("type") == "tool_result" and c.get("tool_use_id") in self.shown:
                ref = c["tool_use_id"]
                kind, text = ("err" if c.get("is_error") else "out"), _output_text(c.get("content"))
            elif c.get("type") == "tool_use":
                kind, text = describe(str(c.get("name", "")), c.get("input") or {})
                ref = str(c.get("id") or "")
                if kind and ref:
                    self.shown.add(ref)
            else:
                continue
            if not kind or not text.strip():
                continue
            key = ("r" if kind in ("out", "err") else "u") + ref
            if key in self.seen:
                continue
            self.seen.add(key)
            if len(self.seen) > 5000:
                self.seen.clear()
            if len(self.shown) > 5000:
                self.shown.clear()
            self.emit({
                "ts": str(d.get("timestamp", "")), "proj": project_label(f), "session": f.stem[:4],
                "side": bool(d.get("isSidechain")), "kind": kind, "ref": ref[-12:], "text": text,
            })

    def scan(self) -> None:
        now = time.time()
        for f in self.root.glob("*/*.jsonl"):
            try:
                st = f.stat()
            except OSError:
                continue
            if now - st.st_mtime > ACTIVE and f not in self.offsets:
                continue
            pos = self.offsets.get(f)
            if pos is None:
                pos = max(0, st.st_size - FIRST_TAIL)
            if st.st_size < pos:
                pos = 0
            if st.st_size == pos:
                self.offsets[f] = pos
                continue
            with open(f, "rb") as h:   # read-only
                h.seek(pos)
                chunk = h.read()
            last_nl = chunk.rfind(b"\n")
            if last_nl < 0:
                continue
            self.offsets[f] = pos + last_nl + 1
            for line in chunk[:last_nl].split(b"\n"):
                self.parse_line(line, f)

    def run(self, stop=None) -> None:
        while not (stop and stop.is_set()):
            try:
                self.scan()
            except Exception:
                pass
            time.sleep(1)
