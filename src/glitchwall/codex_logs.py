"""Read Codex CLI session logs ("rollouts") and turn tool calls into wallpaper events.

Codex CLI appends one JSON record per line to $CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl
(CODEX_HOME defaults to ~/.codex). Each line is {"timestamp", "type", "payload"}. Tool calls are "response_item"
payloads (function_call, custom_tool_call, local_shell_call, web_search_call); their results are separate
function_call_output / custom_tool_call_output lines with the same call_id. "session_meta" and "turn_context"
lines carry the working folder. Finished sessions may later be compressed to .jsonl.zst; those are never read.
Format read from codex's own source (codex-rs/protocol/src/models.rs, rust-v0.172.0); see docs/other-agents.md.
The format changes between releases, so anything unknown is skipped.

Same rules as claude_logs: files are opened read-only, only appended data is read, nothing is written there.
"""
from __future__ import annotations

import json
import os
import re
import time
from pathlib import Path
from typing import Callable

from .claude_logs import ACTIVE, FIRST_TAIL, MAX_OUT_LINES, _when, clip

HIDDEN = ("update_plan", "write_stdin", "request_permissions", "request_user_input", "tool_search")
SHELL_FLAGS = ("-c", "-lc", "-Command", "-command", "/c", "/C")
HEADER = ("Chunk ID: ", "Wall time: ", "Process exited with code ", "Process running with session ID ",
          "Original token count: ", "Total output lines: ", "Exit code: ")


def logs_root() -> Path:
    base = os.environ.get("CODEX_HOME")
    return (Path(base) if base else Path.home() / ".codex") / "sessions"


def _s(v) -> str:
    return "" if v is None else (v if isinstance(v, str) else str(v))


def shell_text(argv) -> str:
    """A command given as an argv list: the script itself when it is `bash -lc "<script>"` and the like."""
    if isinstance(argv, str):
        return argv
    if not isinstance(argv, list):
        return ""
    a = [_s(x) for x in argv]
    if len(a) >= 3 and a[-2] in SHELL_FLAGS:
        return a[-1]
    return " ".join(a)


def patch_text(p: str) -> str:
    """apply_patch's input, shown as the files it touches and its diff lines."""
    out = []
    for line in p.splitlines():
        if line in ("*** Begin Patch", "*** End Patch", "*** End of File"):
            continue
        for head, show in (("*** Update File: ", ""), ("*** Add File: ", "new: "), ("*** Delete File: ", "deleted: "),
                           ("*** Move to: ", "moved to: ")):
            if line.startswith(head):
                line = show + line[len(head):]
                break
        out.append(line)
    return clip("\n".join(out), 600)


def describe(name: str, args: dict, raw: str) -> tuple[str | None, str]:
    """A Codex function call -> (kind, text) as it should appear on the wall. kind None = skip."""
    def g(k):
        v = args.get(k, "")
        return v if isinstance(v, str) else ("" if v is None else json.dumps(v, ensure_ascii=False))
    if name == "exec_command":
        return ("$", g("cmd"))
    if name == "shell":
        return ("$", shell_text(args.get("command")))
    if name == "shell_command":
        return ("$", g("command"))
    if name == "apply_patch":
        return ("edit", patch_text(g("input")))
    if name == "view_image":
        return ("read", g("path"))
    if name in HIDDEN:
        return (None, "")
    return ("tool", name + " " + clip(json.dumps(args, ensure_ascii=False) if args else raw, 200))


def project_label(cwd) -> str:
    if not isinstance(cwd, str):
        return "codex"
    name = re.split(r"[\\/]", cwd.rstrip("\\/"))[-1]
    return name.split("-")[-1] or "codex"


def output_text(v) -> tuple[str, bool]:
    """A tool's output as the wall shows it, and whether it failed (a nonzero exit code)."""
    if isinstance(v, list):
        text = "\n".join(p["text"] for p in v if isinstance(p, dict) and isinstance(p.get("text"), str))
    else:
        text = v if isinstance(v, str) else ""
    code = None
    d = None
    if text.startswith("{"):
        try:
            d = json.loads(text)
        except (ValueError, RecursionError):
            d = None
    if isinstance(d, dict) and isinstance(d.get("output"), str):   # older shell: {"output", "metadata": {"exit_code"}}
        meta = d.get("metadata")
        c = meta.get("exit_code") if isinstance(meta, dict) else None
        code = c if isinstance(c, (int, float)) and not isinstance(c, bool) else None
        text = d["output"]
    else:   # exec_command: a few header lines, then "Output:" and the output
        lines = text.split("\n")
        i = 0
        while i < len(lines) and lines[i].startswith(HEADER):
            for head in ("Process exited with code ", "Exit code: "):
                if lines[i].startswith(head) and re.fullmatch(r"-?[0-9]+", lines[i][len(head):]):
                    code = int(lines[i][len(head):])
            i += 1
        if i > 0 and i < len(lines) and lines[i].startswith("Output:"):
            text = "\n".join([lines[i][len("Output:"):]] + lines[i + 1:])
        else:
            code = None
    out = text.strip() or "(no output)"
    ls = out.splitlines()
    if len(ls) > MAX_OUT_LINES:
        out = "\n".join(ls[:MAX_OUT_LINES]) + f"\n… {len(ls) - MAX_OUT_LINES} more lines"
    return clip(out, 900), code is not None and code != 0


class CodexLogTailer:
    """Follows Codex CLI's rollouts and calls emit(event) for each new tool call and each result."""

    def __init__(self, emit: Callable[[dict], None], root: Path | None = None):
        self.emit = emit
        self.root = root or logs_root()
        self.offsets: dict[Path, int] = {}
        self.seen: set[str] = set()   # "u"+id once the call is shown, "r"+id once its result is
        self.proj: dict[Path, str] = {}
        self.side: dict[Path, bool] = {}

    def _send(self, key: str, d: dict, f: Path, kind: str, ref: str, text: str) -> None:
        if key in self.seen or not text.strip():
            return
        self.seen.add(key)
        if len(self.seen) > 5000:
            self.seen.clear()
        ts = d.get("timestamp")
        self.emit({
            "ts": "" if ts is None else str(ts), "proj": self.proj.get(f, "codex"),
            "session": f.stem.split("-")[-1][:4], "side": self.side.get(f, False),
            "kind": kind, "ref": ref[-12:], "text": text,
        })

    def parse_line(self, line: bytes, f: Path, newer_than: float | None = None) -> None:
        try:
            d = json.loads(line)
        except (ValueError, RecursionError):
            return
        if not isinstance(d, dict) or not isinstance(d.get("payload"), dict):
            return
        typ, p = d.get("type"), d["payload"]
        if typ in ("session_meta", "turn_context"):
            if isinstance(p.get("cwd"), str):
                self.proj[f] = project_label(p["cwd"])
            if typ == "session_meta":
                src = p.get("source")
                self.side[f] = bool(p.get("parent_thread_id")) or (isinstance(src, dict) and "subagent" in src)
            return
        if typ != "response_item" or (newer_than is not None and _when(d.get("timestamp")) < newer_than):
            return
        item = p.get("type")
        ref = _s(p.get("call_id") or "")
        if item == "function_call":
            raw = p.get("arguments")
            raw = raw if isinstance(raw, str) else ""
            try:
                args = json.loads(raw)
            except (ValueError, RecursionError):
                args = None
            kind, text = describe(_s(p.get("name") or ""), args if isinstance(args, dict) else {}, raw)
        elif item == "custom_tool_call":
            name, inp = _s(p.get("name") or ""), p.get("input")
            inp = inp if isinstance(inp, str) else ""
            kind, text = ("edit", patch_text(inp)) if name == "apply_patch" else ("tool", name + " " + clip(inp, 200))
        elif item == "local_shell_call":
            ref = ref or _s(p.get("id") or "")
            a = p.get("action")
            kind, text = "$", shell_text(a.get("command") if isinstance(a, dict) else None)
        elif item == "web_search_call":
            a = p.get("action")
            a = a if isinstance(a, dict) else {}
            if a.get("type") == "search":
                qs = a.get("queries")
                q = a.get("query")
                text = q if isinstance(q, str) and q else " | ".join(x for x in qs if isinstance(x, str)) if isinstance(qs, list) else ""
            elif a.get("type") == "open_page":
                text = _s(a.get("url"))
            elif a.get("type") == "find_in_page":
                text = _s(a.get("pattern")) + ("  in " + _s(a.get("url")) if a.get("url") else "")
            else:
                text = ""
            ref = _s(p.get("id") or "") or _s(d.get("timestamp")) + text
            kind = "web"
        elif item in ("function_call_output", "custom_tool_call_output"):
            if ref and "u" + ref in self.seen:
                text, failed = output_text(p.get("output"))
                self._send("r" + ref, d, f, "err" if failed else "out", ref, text)
            return
        else:
            return
        if kind and ref:
            self._send("u" + ref, d, f, kind, ref, text)

    def _files(self):
        yield from self.root.glob("*/*/*/*.jsonl")

    def scan(self) -> None:
        now = time.time()
        for f in self._files():
            try:
                st = f.stat()
            except OSError:
                continue
            if now - st.st_mtime > ACTIVE and f not in self.offsets:
                continue
            pos = self.offsets.get(f)
            newer_than = now - ACTIVE if pos is None else None
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
                self.parse_line(line, f, newer_than)

    def run(self, stop=None) -> None:
        while not (stop and stop.is_set()):
            try:
                self.scan()
            except Exception:
                pass
            time.sleep(1)
