"""Commands you type in PowerShell or WSL's bash, and what they print, as wallpaper events.

glitchwall's terminal hook (shell-hook.ps1, loaded from your PowerShell profile by `glitchwall terminal on`) writes
one JSON line per command when you press Enter, and one with its output when the command is done, to
<state dir>/shell/<process id>.jsonl. WSL's bash hook (shell-hook.bash, sourced from ~/.bashrc) writes the commands
and how they ended to wsl-<process id>.jsonl there; what they print is not read yet. This follows those files the way claude_logs follows Claude Code's:
read-only, only what is appended, and nothing older than the feed (a file that was there before it started is
read from its end). Everything shown goes through the redaction like Claude's calls do.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Callable

from . import settings as cfg

HOOK_FILE = Path(__file__).resolve().parent / "shell-hook.ps1"
BASH_HOOK = Path(__file__).resolve().parent / "shell-hook.bash"
CLINK_HOOK = Path(__file__).resolve().parent / "shell-hook.lua"
MARK = "# glitchwall terminal"   # ends the line `glitchwall terminal on` adds to a profile
STALE = 24 * 3600                # a shell's file this long untouched is cleared out
LIVE_EVERY, LIVE_FOR = 2.0, 600.0   # a running WSL command's screen: read this often, for this long at most


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


# ---------------------------------------------------------------- WSL: a line in the default distro's ~/.bashrc
# $1: the hook, $2: the folder it writes to, both as Windows paths; wslpath makes them /mnt/... ones
_WSL_ON = r"""h=$(wslpath -a "$1") && d=$(wslpath -a "$2") || exit 1
grep -qF "$3" ~/.bashrc 2>/dev/null && exit 3
[ -s ~/.bashrc ] && [ -n "$(tail -c1 ~/.bashrc)" ] && echo >> ~/.bashrc
printf '[ -f %q ] && . %q %q  %s (`glitchwall terminal off` removes this)\n' "$h" "$h" "$d" "$3" >> ~/.bashrc"""
_WSL_OFF = r"""grep -qF "$1" ~/.bashrc 2>/dev/null || exit 3
sed -i "\|$1|d" ~/.bashrc"""


def _wsl(script: str, *args: str) -> int | None:
    """Runs a bash script in the default WSL distro; its exit code, or None when there is no WSL to run it in."""
    wsl = shutil.which("wsl.exe") if os.name == "nt" else None
    if not wsl:
        return None
    try:
        r = subprocess.run([wsl, "-e", "bash", "-c", script, "bash", *args],  # noqa: S603 (our own script, as arguments)
                           capture_output=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.returncode


def install_wsl_hook() -> bool:
    """Copy the bash hook next to glitchwall's state and source it from WSL's ~/.bashrc. True if a line was added."""
    target = cfg.state_dir() / "shell-hook.bash"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(BASH_HOOK.read_bytes().replace(b"\r\n", b"\n"))   # bash wants LF
    return _wsl(_WSL_ON, str(target), str(shell_dir()), MARK) == 0


def remove_wsl_hook() -> bool:
    """Take the hook's line out of WSL's ~/.bashrc again. True if one was there."""
    (cfg.state_dir() / "shell-hook.bash").unlink(missing_ok=True)
    return _wsl(_WSL_OFF, MARK) == 0


# ---------------------------------------------------------------- cmd: a script folder for Clink, if Clink runs in cmd
def find_clink() -> Path | None:
    """Clink's exe, if Clink is set up to start with cmd (its entry in cmd's AutoRun); without that Clink
    never loads in cmd anyway. glitchwall never sets that up itself: Clink's own installer or `clink autorun install` does."""
    if os.name != "nt":
        return None
    import re
    import winreg
    for root in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
        try:
            with winreg.OpenKey(root, r"Software\Microsoft\Command Processor") as k:
                run = str(winreg.QueryValueEx(k, "AutoRun")[0])
        except OSError:
            continue
        m = re.search(r'"?([^"&|]*?clink[^"&|\\]*\.(?:bat|exe))"?', run, re.IGNORECASE)
        if m:
            d = Path(m.group(1)).parent
            for name in ("clink_x64.exe", "clink_arm64.exe", "clink_x86.exe"):
                if (d / name).is_file():
                    return d / name
    return None   # not a PATH search: on Windows that looks in the current folder first, where anything could sit


def _clink(*args: str) -> int | None:
    exe = find_clink()
    if not exe:
        return None
    try:
        return subprocess.run([str(exe), *args], capture_output=True, timeout=60).returncode  # noqa: S603 (Clink's own exe)
    except (OSError, subprocess.TimeoutExpired):
        return None


def clink_dir() -> Path:
    return cfg.state_dir() / "clink"   # a folder of its own: Clink loads every script in a folder it is given


def install_clink_hook() -> bool:
    """Put the cmd hook in its own folder and add that folder to Clink's script paths. True if Clink took it."""
    if not find_clink():
        return False
    clink_dir().mkdir(parents=True, exist_ok=True)
    (clink_dir() / "glitchwall.lua").write_bytes(CLINK_HOOK.read_bytes())
    return _clink("installscripts", str(clink_dir())) == 0


def remove_clink_hook() -> bool:
    """Take the folder out of Clink's script paths again, and the hook away. True if Clink had it."""
    had = clink_dir().is_dir()
    if had:
        _clink("uninstallscripts", str(clink_dir()))
        shutil.rmtree(clink_dir(), ignore_errors=True)
    return had


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
        self.cmds: dict[str, str] = {}   # WSL commands waiting for their output, by ref
        self.screen_output = None        # tests: (cmd, prompt) -> output, in place of reading the screens
        self.running: dict[str, dict] = {}   # WSL commands still running: their card, polled for live output

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
        except (ValueError, RecursionError):   # RecursionError: a line nested absurdly deep
            return
        if not isinstance(d, dict) or not str(d.get("text", "")).strip() and d.get("t") == "cmd":
            return
        ref = f"sh{shell}-{d.get('id')}"
        bash, cmd = d.get("sh") == "bash", d.get("sh") == "cmd"   # WSL bash (shell-hook.bash), cmd via Clink (shell-hook.lua)
        base = {"ts": str(d.get("ts", "")), "proj": "wsl" if bash else "cmd" if cmd else "terminal", "session": "you", "side": False, "ref": ref}
        if d.get("t") == "cmd":
            if bash:
                self.cmds[ref] = str(d.get("text", ""))
                now = time.time()
                self.running[ref] = {"base": base, "cmd": self.cmds[ref], "until": now + LIVE_FOR, "next": now + LIVE_EVERY, "last": None}
                while len(self.cmds) > 50:
                    self.cmds.pop(next(iter(self.cmds)))
            self.emit({**base, "kind": "$" if bash else ">" if cmd else "PS>", "text": str(d.get("text", ""))})
        elif d.get("t") == "out":
            self.running.pop(ref, None)
            from .claude_logs import _output_text   # trimmed like Claude's: 14 lines, 900 characters
            if bash and ref in self.cmds and self.enabled():   # what it printed, read off its WSL window's screen
                from . import wslscreen
                read = self.screen_output or wslscreen.output_of
                got = read(self.cmds.pop(ref), str(d.get("prompt", "")))
                if got is not None:
                    d = {**d, "text": got}
            # live: the newest output of a command still running, to replace what its card showed before
            self.emit({**base, "kind": "out" if d.get("ok", True) is not False else "err", "live": bool(d.get("live")),
                       "text": _last_lines(str(d.get("text", ""))) if d.get("live") else _output_text(str(d.get("text", "")))})

    def poll_live(self) -> None:
        """WSL commands still running: every LIVE_EVERY seconds, what they have printed so far (ping never finishes),
        read off their window's screen, as live output that replaces what the card showed. For LIVE_FOR at most."""
        now = time.time()
        for ref, r in list(self.running.items()):
            if now > r["until"] or not self.enabled():
                self.running.pop(ref, None)
                continue
            if now < r["next"]:
                continue
            r["next"] = now + LIVE_EVERY
            from . import wslscreen
            got = (self.screen_output or wslscreen.output_of)(r["cmd"], "")
            if got and got != r["last"]:
                r["last"] = got
                self.emit({**r["base"], "kind": "out", "live": True, "text": _last_lines(got)})

    def run(self, stop=None) -> None:
        while not (stop and stop.is_set()):
            try:
                self.scan()
                self.poll_live()
            except Exception:
                pass
            time.sleep(0.5)
