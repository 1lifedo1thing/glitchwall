"""Read Gemini CLI session logs and turn tool calls into wallpaper events.

Gemini CLI writes one JSON record per line to ~/.gemini/tmp/<project>/chats/session-*.jsonl (subagents in
chats/<parent session>/<id>.jsonl). A record is a whole message, written again in full each time it changes,
or a "$patch", "$set" or "$rewindTo" line. A tool call first appears with status "scheduled" and its args, and
again once it has finished, with its status and result. Format read from gemini-cli's own source
(packages/core/src/services/chatRecordingService.ts, v0.71.1); see docs/other-agents.md.

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

DONE = ("success", "error", "cancelled")


def _list(v) -> list:
    return v if isinstance(v, list) else []


def logs_root() -> Path:
    base = os.environ.get("GEMINI_CLI_HOME")
    return (Path(base) / ".gemini" if base else Path.home() / ".gemini") / "tmp"


def describe(name: str, args: dict) -> tuple[str | None, str]:
    """A Gemini CLI tool call -> (kind, text) as it should appear on the wall. kind None = skip."""
    def g(k):
        v = args.get(k, "")
        return v if isinstance(v, str) else ("" if v is None else json.dumps(v, ensure_ascii=False))
    if name == "run_shell_command":
        return ("$", g("command"))
    if name == "read_file":
        return ("read", g("file_path"))
    if name == "read_many_files":
        return ("read", g("include"))
    if name == "list_directory":
        return ("read", g("dir_path"))
    if name == "write_file":
        return ("write", g("file_path") + "\n" + clip(g("content"), 600))
    if name == "replace":
        return ("edit", g("file_path") + "\n- " + clip(g("old_string"), 240) + "\n+ " + clip(g("new_string"), 360))
    if name in ("glob", "grep_search"):
        return ("find", g("pattern") + ("  in " + g("dir_path") if g("dir_path") else ""))
    if name == "google_web_search":
        return ("web", g("query"))
    if name == "web_fetch":
        return ("web", clip(g("prompt"), 200))
    if name == "invoke_agent":
        return ("agent", g("description") or g("name") or "subagent")
    if name in ("write_todos", "update_topic", "ask_user", "enter_plan_mode", "exit_plan_mode", "activate_skill",
                "get_internal_docs") or name.startswith("tracker_"):
        return (None, "")
    return ("tool", name + " " + clip(json.dumps(args, ensure_ascii=False), 200))


def project_label(f: Path) -> str:
    # <tmp>/<project>/chats/[<parent>/]<session>.jsonl; the project folder is a short name or a long hash
    d = f.parent.parent if f.parent.name == "chats" else f.parent.parent.parent
    name = d.name
    return "gemini" if re.fullmatch(r"[0-9a-f]{32,}", name) else (name.split("-")[-1] or "gemini")


SHELL_DROP = ("Command: ", "Directory: ", "Signal: ", "Background PIDs: ", "Process Group PGID: ")


def shell_output(text: str) -> tuple[str, bool]:
    """run_shell_command's answer to the model: "<untrusted_context>", then "Output: ...", "Exit Code: N" and
    bookkeeping lines. Keeps the output (and any "Error: ..." line) and says whether the exit code was nonzero."""
    t = text.strip()
    if t.startswith("<untrusted_context>") and t.endswith("</untrusted_context>"):
        t = t[len("<untrusted_context>"):-len("</untrusted_context>")].strip("\n")
    lines = t.split("\n")
    if not any(x.startswith(("Output:", "Exit Code: ")) for x in lines):
        return text, False
    out, code, keep = [], None, True
    for x in lines:
        if x.startswith("Output:"):
            out.append(x[len("Output:"):].lstrip(" "))
            keep = True
        elif x.startswith("Exit Code: "):
            code, keep = x[len("Exit Code: "):].strip(), False
        elif x.startswith(SHELL_DROP):
            keep = False
        elif x.startswith("Error: ") or keep:
            out.append(x)
            keep = True
    failed = code is not None and re.fullmatch(r"-?[0-9]+", code) is not None and int(code) != 0
    return "\n".join(out), failed


def _result_text(tc: dict) -> tuple[str, bool]:
    """What a finished tool call printed (the UI's text if it is plain text, else the function response),
    and whether a shell command in it failed."""
    shown = tc.get("resultDisplay")
    out = shown if isinstance(shown, str) else ""
    if not out:
        parts = tc.get("result")
        parts = parts if isinstance(parts, list) else [parts]   # a list of parts, or one
        texts = []
        for p in parts:
            if isinstance(p, str):
                texts.append(p)
            elif isinstance(p, dict):
                if isinstance(p.get("text"), str):
                    texts.append(p["text"])
                fr = p.get("functionResponse")
                resp = fr.get("response") if isinstance(fr, dict) else None
                if isinstance(resp, dict):
                    for k in ("output", "error"):
                        if isinstance(resp.get(k), str):
                            texts.append(resp[k])
        out = "\n".join(texts)
    failed = False
    if tc.get("name") == "run_shell_command":
        out, failed = shell_output(out)
    out = out.strip() or "(no output)"
    lines = out.splitlines()
    if len(lines) > MAX_OUT_LINES:
        out = "\n".join(lines[:MAX_OUT_LINES]) + f"\n… {len(lines) - MAX_OUT_LINES} more lines"
    return clip(out, 900), failed


class GeminiLogTailer:
    """Follows Gemini CLI's logs and calls emit(event) for each new tool call and each finished one."""

    def __init__(self, emit: Callable[[dict], None], root: Path | None = None):
        self.emit = emit
        self.root = root or logs_root()
        self.offsets: dict[Path, int] = {}
        self.seen: set[str] = set()   # "u"+id once the call is shown, "r"+id once its result is

    def _send(self, key: str, tc: dict, f: Path, kind: str, ref: str, text: str) -> None:
        if key in self.seen or not text.strip():
            return
        self.seen.add(key)
        if len(self.seen) > 5000:
            self.seen.clear()
        ts = tc.get("timestamp")
        self.emit({
            "ts": "" if ts is None else str(ts), "proj": project_label(f), "session": f.stem.split("-")[-1][:4],
            "side": f.parent.name != "chats", "kind": kind, "ref": ref[-12:], "text": text,
        })

    def _tool_call(self, d: dict, tc: dict, f: Path) -> None:
        ref = str(tc.get("id") or "")
        args = tc.get("args")
        kind, text = describe(str(tc.get("name") or ""), args if isinstance(args, dict) else {})
        if not kind or not ref:
            return
        self._send("u" + ref, tc, f, kind, ref, text)
        status = tc.get("status")
        if status in DONE and ("result" in tc or "resultDisplay" in tc):
            self._result(tc, ref, f)

    def _result(self, tc: dict, ref: str, f: Path) -> None:
        text, failed = _result_text(tc)
        kind = "err" if failed or tc.get("status") in ("error", "cancelled") else "out"
        self._send("r" + ref, tc, f, kind, ref, text)

    def parse_line(self, line: bytes, f: Path, newer_than: float | None = None) -> None:
        try:
            d = json.loads(line)
        except (ValueError, RecursionError):
            return
        if not isinstance(d, dict):
            return
        patch = d.get("$patch")
        if isinstance(patch, dict):
            # a late result: {"toolCalls": [{"id", "result"}]} or {"updates": [{"toolCalls": [...]}]}
            groups = [patch] + [u for u in _list(patch.get("updates")) if isinstance(u, dict)]
            for grp in groups:
                for tc in _list(grp.get("toolCalls")):
                    ref = str(tc.get("id") or "") if isinstance(tc, dict) else ""
                    if ref and "u" + ref in self.seen:
                        self._result(tc, ref, f)
            return
        if d.get("type") != "gemini" or (newer_than is not None and _when(d.get("timestamp")) < newer_than):
            return
        for tc in _list(d.get("toolCalls")):
            if isinstance(tc, dict):
                self._tool_call(d, tc, f)

    def _files(self):
        yield from self.root.glob("*/chats/*.jsonl")
        yield from self.root.glob("*/chats/*/*.jsonl")

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
            if st.st_size < pos:   # rewritten in place (Gemini CLI does that on some resumes)
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
