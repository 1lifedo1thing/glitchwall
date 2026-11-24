"""The scan loop the Gemini CLI and Codex CLI readers share: which logs to look at, and what's new in them.

Both agents keep every session's log, so after a year there can be thousands. Looking at all of them every second
costs a good part of a CPU core, so each second only the logs being followed and the recent ones (each reader says
which) are looked at, and every log once a minute (SWEEP). A log that has gone quiet stops being followed; if an old
log grows (a session resumed, or a log whose time didn't move: Codex keeps its file open, and Windows may not update
the time then), only what was added is read, within a minute at most.

Files are opened read-only and only appended data is read.
"""
from __future__ import annotations

import stat
from pathlib import Path
from typing import Callable, Iterable

from .claude_logs import ACTIVE, FIRST_TAIL

SWEEP = 60.0


class Follow:
    """Where each log has been read up to, and the size of the old ones not followed."""

    def __init__(self):
        self.offsets: dict[Path, int] = {}
        self.idle: dict[Path, int] = {}
        self.swept = -SWEEP

    def due(self, now: float) -> bool:
        """True when it's time to look at every log again (and counts that look as done)."""
        if now - self.swept < SWEEP:
            return False
        self.swept = now
        return True

    def scan(self, files: Iterable[Path], everything: bool, now: float,
             parse: Callable[[bytes, Path, float | None], None]) -> None:
        files = sorted(set(files) | set(self.offsets))
        if everything:
            present = set(files)
            self.idle = {f: s for f, s in self.idle.items() if f in present}
        for f in files:
            try:
                st = f.stat()
            except OSError:
                st = None
            if st is None or not stat.S_ISREG(st.st_mode):
                self.offsets.pop(f, None)
                self.idle.pop(f, None)
                continue
            pos = self.offsets.get(f)
            newer_than = None
            if pos is None:
                if now - st.st_mtime > ACTIVE:
                    was = self.idle.get(f)
                    self.idle[f] = st.st_size
                    if was is None or st.st_size <= was:
                        continue
                    pos = was   # an old log that grew: only what was added
                else:
                    # first sight: only its recent calls, never a burst of old ones
                    newer_than = now - ACTIVE
                    pos = max(0, st.st_size - FIRST_TAIL)
                self.idle.pop(f, None)
            elif everything and st.st_size == pos and now - st.st_mtime > ACTIVE:
                del self.offsets[f]   # gone quiet: stop following, remember its size
                self.idle[f] = st.st_size
                continue
            if st.st_size < pos:   # rewritten in place
                pos = 0
            if st.st_size == pos:
                self.offsets[f] = pos
                continue
            try:
                with open(f, "rb") as h:   # read-only
                    h.seek(pos)
                    chunk = h.read()
            except OSError:   # locked, or gone since: try again next time, never stop the others
                continue
            last_nl = chunk.rfind(b"\n")
            if last_nl < 0:   # no whole line yet
                if newer_than is None:
                    self.offsets[f] = pos
                continue
            self.offsets[f] = pos + last_nl + 1
            for line in chunk[:last_nl].split(b"\n"):
                parse(line, f, newer_than)


def newest_dirs(root: Path, depth: int, n: int) -> list[Path]:
    """The n newest folders `depth` levels down, by name (Codex's sessions/YYYY/MM/DD), newest first."""
    out: list[Path] = []

    def walk(d: Path, level: int) -> None:
        try:
            subs = sorted((p for p in d.iterdir() if p.is_dir()), key=lambda p: p.name, reverse=True)
        except OSError:
            return
        for p in subs:
            if len(out) >= n:
                return
            if level == depth:
                out.append(p)
            else:
                walk(p, level + 1)

    walk(root, 1)
    return out
