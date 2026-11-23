"""Commands you type in PowerShell, as wallpaper events.

PowerShell's line editor (PSReadLine) already appends every command to a history file the moment Enter is pressed:
%APPDATA%\\Microsoft\\Windows\\PowerShell\\PSReadLine\\<host>_history.txt (ConsoleHost_history.txt for terminals; the
VS Code host has its own). This follows those files the way claude_logs follows Claude Code's: read-only, only what
is appended, nothing old on first sight. No profile hook and no second copy of the commands anywhere. PSReadLine
itself leaves out lines that look like they hold a secret, and everything shown goes through the redaction as well.

Only the commands: PowerShell keeps no record of what they printed.
"""
from __future__ import annotations

import itertools
import os
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable

_n = itertools.count(1)


def history_dir() -> Path | None:
    base = os.environ.get("APPDATA")
    return Path(base) / "Microsoft" / "Windows" / "PowerShell" / "PSReadLine" if base else None


def commands(chunk: str) -> list[str]:
    """Whole commands in appended history text. A line ending in a backtick goes on to the next one, as
    PSReadLine writes a command typed over several lines."""
    out, cur = [], []
    for line in chunk.split("\n"):
        line = line.rstrip("\r")
        if line.endswith("`"):
            cur.append(line[:-1])
            continue
        cur.append(line)
        cmd = "\n".join(cur).strip()
        cur = []
        if cmd:
            out.append(cmd)
    return out


class ShellHistoryTailer:
    """Follows PowerShell's history files and calls emit(event) for each new command, while enabled() says so."""

    def __init__(self, emit: Callable[[dict], None], enabled: Callable[[], bool] = lambda: True, root: Path | None = None):
        self.emit, self.enabled = emit, enabled
        self.root = root or history_dir()
        self.offsets: dict[Path, int] = {}

    def scan(self) -> None:
        if not self.root or not self.root.is_dir():
            return
        for f in self.root.glob("*_history.txt"):
            try:
                size = f.stat().st_size
            except OSError:
                continue
            pos = self.offsets.get(f)
            if pos is None or size < pos:   # first sight, or the file was cut back: start from its end
                self.offsets[f] = size
                continue
            if size == pos:
                continue
            with open(f, "rb") as h:   # read-only
                h.seek(pos)
                chunk = h.read()
            last_nl = chunk.rfind(b"\n")
            if last_nl < 0:
                continue
            self.offsets[f] = pos + last_nl + 1
            if not self.enabled():   # off: keep up with the file, show nothing (and nothing piles up for later)
                continue
            ts = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
            host = f.name[:-len("_history.txt")]
            for cmd in commands(chunk[:last_nl].decode("utf-8", "replace").lstrip("﻿")):
                self.emit({"ts": ts, "proj": "terminal", "session": "you" if host == "ConsoleHost" else host[:4].lower(),
                           "side": False, "kind": "PS>", "ref": f"sh{next(_n)}", "text": cmd})

    def run(self, stop=None) -> None:
        while not (stop and stop.is_set()):
            try:
                self.scan()
            except Exception:
                pass
            time.sleep(1)
