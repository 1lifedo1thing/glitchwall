"""What a WSL command printed, read off its window's screen (Windows only).

bash can't see its own screen, and a Windows program started from inside WSL gets a console of its own. So when the
bash hook says a command is done, the feed runs this module as a short-lived helper process: it attaches to each
wsl.exe's console in turn, reads the lines on screen (read-only) and prints them as JSON. The feed then picks out the
lines between the command and the next prompt. It runs as its own process so the feed never attaches to a console.
Like everything else shown, the result goes through the redaction before it is kept or shown.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys

MAX_LINES = 400   # most screen lines read per window


def screens() -> list[list[str]]:
    """Each open WSL window's screen, as lines (via the helper process). [] when there is none or it fails."""
    if os.name != "nt":
        return []
    try:   # -I: isolated, so nothing from the current folder or PYTHON* variables gets loaded
        r = subprocess.run([sys.executable, "-I", "-m", "glitchwall.wslscreen"], capture_output=True, timeout=10,  # noqa: S603
                           creationflags=0x08000000)   # CREATE_NO_WINDOW
        data = json.loads(r.stdout.decode("utf-8", "replace") or "[]")
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return []
    return [[str(x) for x in s] for s in data if isinstance(s, list)] if isinstance(data, list) else []


def find_output(screen: list[str], cmd: str, prompt: str) -> str | None:
    """The lines a command printed: below its own line(s), above the next prompt. None if it isn't on this screen."""
    first = cmd.split("\n", 1)[0].rstrip()
    if not first:
        return None
    at = next((i for i in range(len(screen) - 1, -1, -1) if first in screen[i]), None)   # its latest showing
    if at is None:
        return None
    out = []
    for line in screen[at + cmd.count("\n") + 1:]:
        if prompt.strip() and line.rstrip().startswith(prompt.rstrip()):   # the next prompt: the end of its output
            break
        out.append(line.rstrip())
    return "\n".join(out).strip("\n")


def output_of(cmd: str, prompt: str) -> str | None:
    """A finished WSL command's output, from whichever window shows it; None if none does."""
    for s in screens():
        o = find_output(s, cmd, prompt)
        if o is not None:
            return o
    return None


# ---------------------------------------------------------------- the helper process
def _read_all() -> list[list[str]]:
    import ctypes
    from ctypes import wintypes as w
    k = ctypes.WinDLL("kernel32", use_last_error=True)

    class PE(ctypes.Structure):   # PROCESSENTRY32W
        _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD), ("heap", ctypes.c_void_p),
                    ("module", w.DWORD), ("threads", w.DWORD), ("parent", w.DWORD), ("prio", ctypes.c_long),
                    ("flags", w.DWORD), ("exe", ctypes.c_wchar * 260)]

    class COORD(ctypes.Structure):
        _fields_ = [("X", ctypes.c_short), ("Y", ctypes.c_short)]

    class INFO(ctypes.Structure):   # CONSOLE_SCREEN_BUFFER_INFO
        _fields_ = [("size", COORD), ("cursor", COORD), ("attr", w.WORD), ("win", ctypes.c_short * 4), ("max", COORD)]

    k.CreateToolhelp32Snapshot.restype = w.HANDLE
    k.CreateFileW.restype = w.HANDLE
    snap = k.CreateToolhelp32Snapshot(0x2, 0)   # TH32CS_SNAPPROCESS
    pids, pe = [], PE()
    pe.size = ctypes.sizeof(PE)
    ok = k.Process32FirstW(snap, ctypes.byref(pe))
    while ok:
        if pe.exe.lower() == "wsl.exe":
            pids.append(pe.pid)
        ok = k.Process32NextW(snap, ctypes.byref(pe))
    k.CloseHandle(snap)

    seen, out = set(), []
    for pid in pids:
        k.FreeConsole()
        if not k.AttachConsole(pid):
            continue
        try:
            h = k.CreateFileW("CONOUT$", 0x80000000, 3, None, 3, 0, None)   # GENERIC_READ, shared, OPEN_EXISTING
            if h in (None, w.HANDLE(-1).value):
                continue
            info = INFO()
            k.GetConsoleScreenBufferInfo(h, ctypes.byref(info))
            width, end = info.size.X, info.cursor.Y
            lines = []
            for y in range(max(0, end - MAX_LINES), end + 1):
                buf, n = ctypes.create_unicode_buffer(width), w.DWORD()
                k.ReadConsoleOutputCharacterW(h, buf, width, COORD(0, y), ctypes.byref(n))
                lines.append(buf.value[: n.value].rstrip())
            k.CloseHandle(h)
        finally:
            k.FreeConsole()
        key = "\n".join(lines)
        if key not in seen:   # a window's wsl.exe processes share one console
            seen.add(key)
            out.append(lines)
    return out


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    try:
        print(json.dumps(_read_all() if os.name == "nt" else []))
    except Exception:   # noqa: BLE001 (a helper: any failure is simply "no screens")
        print("[]")
