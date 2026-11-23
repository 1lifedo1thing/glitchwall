"""Commands you type in PowerShell, and what they print, as wallpaper events.

glitchwall's terminal hook (shell-hook.ps1, loaded from your PowerShell profile by `glitchwall terminal on`) writes
one JSON line per command when you press Enter, and one with its output when the command is done, to
<state dir>/shell/<process id>.jsonl. This follows those files the way claude_logs follows Claude Code's:
read-only, only what is appended, and nothing older than the feed (a file that was there before it started is
read from its end). Everything shown goes through the redaction like Claude's calls do.
"""
from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Callable

from . import settings as cfg

HOOK_FILE = Path(__file__).resolve().parent / "shell-hook.ps1"
MARK = "# glitchwall terminal"   # ends the line `glitchwall terminal on` adds to a profile
STALE = 24 * 3600                # a shell's file this long untouched is cleared out


def shell_dir() -> Path:
    return cfg.state_dir() / "shell"


# ---------------------------------------------------------------- turning the hook on and off
def profiles() -> list[Path]:
    """Each PowerShell's profile for all its hosts: PowerShell 7 and Windows PowerShell 5.1."""
    docs = None
    if os.name == "nt":
        import ctypes
        buf = ctypes.create_unicode_buffer(520)
        if ctypes.windll.shell32.SHGetFolderPathW(None, 5, None, 0, buf) == 0:   # CSIDL_PERSONAL: Documents, wherever it lives
            docs = Path(buf.value)
    docs = docs or Path.home() / "Documents"
    return [docs / "PowerShell" / "profile.ps1", docs / "WindowsPowerShell" / "profile.ps1"]


def hook_line() -> str:
    target = cfg.state_dir() / "shell-hook.ps1"
    return f'if (Test-Path "{target}") {{ . "{target}" }}  {MARK} (`glitchwall terminal off` removes this)'


def hook_installed() -> bool:
    return any(p.is_file() and MARK in p.read_text(encoding="utf-8-sig", errors="replace") for p in profiles())


def install_hook() -> list[Path]:
    """Copy the hook next to glitchwall's state and load it from each profile. Returns the profiles changed."""
    target = cfg.state_dir() / "shell-hook.ps1"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(HOOK_FILE.read_text(encoding="utf-8"), encoding="utf-8-sig")   # a BOM: Windows PowerShell reads it as UTF-8
    changed = []
    for p in profiles():
        text = p.read_text(encoding="utf-8-sig", errors="replace") if p.is_file() else ""
        if MARK in text:
            continue
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text + ("" if not text or text.endswith("\n") else "\n") + hook_line() + "\n", encoding="utf-8-sig")
        changed.append(p)
    return changed


def remove_hook() -> list[Path]:
    """Take the hook's line out of each profile again, and the hook and its files away. Returns the profiles changed."""
    changed = []
    for p in profiles():
        if not p.is_file():
            continue
        text = p.read_text(encoding="utf-8-sig", errors="replace")
        if MARK not in text:
            continue
        p.write_text("".join(l for l in text.splitlines(keepends=True) if MARK not in l), encoding="utf-8-sig")
        changed.append(p)
    (cfg.state_dir() / "shell-hook.ps1").unlink(missing_ok=True)
    for f in shell_dir().glob("*.jsonl") if shell_dir().is_dir() else []:
        f.unlink(missing_ok=True)
    return changed


def _last_lines(text: str, n: int = 14) -> str:
    """A running command's output as the wall shows it: its newest lines (where the first ones suit a finished one)."""
    from .claude_logs import clip
    lines = text.strip().splitlines()
    out = "\n".join(lines[-n:])
    return clip(("… " + str(len(lines) - n) + " lines before\n" if len(lines) > n else "") + out, 900)


# ---------------------------------------------------------------- following what it writes
class ShellTailer:
    """Follows the hook's files and calls emit(event) for each command and its output, while enabled() says so."""

    def __init__(self, emit: Callable[[dict], None], enabled: Callable[[], bool] = lambda: True, root: Path | None = None):
        self.emit, self.enabled = emit, enabled
        self.root = root or shell_dir()
        self.offsets: dict[Path, int] = {}
        self.started = time.time()

    def scan(self) -> None:
        if not self.root.is_dir():
            return
        now = time.time()
        for f in self.root.glob("*.jsonl"):
            try:
                st = f.stat()
            except OSError:
                continue
            if now - st.st_mtime > STALE:   # that shell is long gone
                f.unlink(missing_ok=True)
                self.offsets.pop(f, None)
                continue
            pos = self.offsets.get(f)
            if pos is None:   # first sight: a shell started since the feed is read whole, an older one from its end
                pos = 0 if st.st_ctime >= self.started - 1 else st.st_size
            if st.st_size < pos:   # the hook cut it back
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
            if not self.enabled():   # off: keep up with the file, show nothing (and nothing piles up for later)
                continue
            for line in chunk[:last_nl].split(b"\n"):
                self.parse(line, f.stem)

    def parse(self, line: bytes, shell: str) -> None:
        try:
            d = json.loads(line.decode("utf-8", "replace").lstrip("﻿"))
        except ValueError:
            return
        if not isinstance(d, dict) or not str(d.get("text", "")).strip() and d.get("t") == "cmd":
            return
        ref = f"sh{shell}-{d.get('id')}"
        base = {"ts": str(d.get("ts", "")), "proj": "terminal", "session": "you", "side": False, "ref": ref}
        if d.get("t") == "cmd":
            self.emit({**base, "kind": "PS>", "text": str(d.get("text", ""))})
        elif d.get("t") == "out":
            from .claude_logs import _output_text   # trimmed like Claude's: 14 lines, 900 characters
            # live: the newest output of a command still running, to replace what its card showed before
            self.emit({**base, "kind": "out" if d.get("ok", True) is not False else "err", "live": bool(d.get("live")),
                       "text": _last_lines(str(d.get("text", ""))) if d.get("live") else _output_text(str(d.get("text", "")))})

    def run(self, stop=None) -> None:
        while not (stop and stop.is_set()):
            try:
                self.scan()
            except Exception:
                pass
            time.sleep(0.5)
