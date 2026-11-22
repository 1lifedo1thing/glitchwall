"""Windows: run as "glitchwall" in Task Manager instead of "Python".

Task Manager names a process by its exe's description, and uv's Python says "Python". So glitchwall makes a
copy of that Python's windowed interpreter (pythonw.exe), named glitchwall-feed.exe, gives the copy glitchwall's
name and icon (Windows' own resource-update calls, no tools), and runs the feed and the settings window with it.

The copy lives next to the tool's pyvenv.cfg, so it is a normal interpreter of that environment: same packages,
same standard library. It's rebuilt whenever its source Python changes, and if anything about it fails, glitchwall
just runs on plain pythonw.exe as before. uv's Python isn't code-signed, so the copy loses no signature.
"""
from __future__ import annotations

import ctypes
import os
import shutil
import struct
import sys
from ctypes import wintypes
from pathlib import Path

NAME = "glitchwall-feed.exe"
STAMP = "glitchwall-feed.stamp"
BUILD = 1   # bump when what's written into the copy changes
RT_ICON, RT_GROUP_ICON, RT_VERSION = 3, 14, 16


def target() -> Path | None:
    """Where the copy goes: the Scripts folder of the environment glitchwall is installed in. None outside a venv."""
    if os.name != "nt" or getattr(sys, "frozen", False) or sys.prefix == sys.base_prefix:
        return None
    return Path(sys.prefix) / "Scripts" / NAME


def running_as_it() -> bool:
    return Path(sys.executable).name.lower() == NAME


def _source() -> Path:
    return Path(sys.base_prefix) / "pythonw.exe"


def _dlls() -> list[Path]:
    """What the interpreter needs beside it: python3XX.dll, python3.dll, the C runtime."""
    base = Path(sys.base_prefix)
    names = [f"python{sys.version_info[0]}{sys.version_info[1]}.dll", "python3.dll", "vcruntime140.dll", "vcruntime140_1.dll"]
    return [base / n for n in names if (base / n).exists()]


def _stamp(icon: Path) -> str:
    parts = [str(BUILD)]
    for f in [_source(), icon, *_dlls()]:
        st = f.stat()
        parts.append(f"{f}|{st.st_size}|{int(st.st_mtime)}")
    return "\n".join(parts)


def ensure(icon: Path, version: str) -> Path | None:
    """The renamed interpreter, made or remade if needed. None if it can't be (then use pythonw.exe)."""
    exe = target()
    if exe is None:
        return None
    try:
        want = _stamp(icon)
        stamp = exe.with_name(STAMP)
        if exe.exists() and stamp.exists() and stamp.read_text(encoding="utf-8") == want \
                and all((exe.parent / d.name).exists() for d in _dlls()):
            return exe
        for d in _dlls():   # a hard link costs no space; a copy if links aren't possible
            dst = exe.parent / d.name
            if dst.exists():
                if dst.stat().st_size == d.stat().st_size and dst.stat().st_mtime >= d.stat().st_mtime:
                    continue
                dst.unlink()
            try:
                os.link(d, dst)
            except OSError:
                shutil.copy2(d, dst)
        tmp = exe.with_name(NAME + ".tmp")
        shutil.copy2(_source(), tmp)
        brand(tmp, icon, version)
        os.replace(tmp, exe)   # fails while a glitchwall-feed.exe runs: then the old one is used this time
        stamp.write_text(want, encoding="utf-8")
        return exe
    except OSError:
        return exe if exe.exists() else None


# ---------------------------------------------------------------- resources
def _node(key: str, value: bytes = b"", children: bytes = b"", wtype: int = 1, value_len: int | None = None) -> bytes:
    """One VS_VERSIONINFO block: header, key, value, children, each 4-byte aligned."""
    def pad(b: bytes) -> bytes:
        return b + b"\0" * (-len(b) % 4)
    body = pad(struct.pack("<HHH", 0, 0, 0) + (key + "\0").encode("utf-16-le"))
    body = pad(body + value) + children if value else body + children
    vl = len(value) if value_len is None else value_len
    return struct.pack("<HHH", len(body), vl, wtype) + body[6:]


def version_info(description: str, version: str) -> bytes:
    nums = ([int(x) for x in version.split(".") if x.isdigit()] + [0, 0, 0, 0])[:4]
    ms, ls = nums[0] << 16 | nums[1], nums[2] << 16 | nums[3]
    py = ".".join(map(str, sys.version_info[:3]))
    fixed = struct.pack("<13I", 0xFEEF04BD, 0x00010000, ms, ls, ms, ls, 0x3F, 0, 0x40004, 1, 0, 0, 0)
    strings = {"FileDescription": description, "ProductName": "glitchwall", "FileVersion": version,
               "ProductVersion": version, "InternalName": "glitchwall", "OriginalFilename": NAME,
               "Comments": f"Python {py} (pythonw.exe), renamed so glitchwall shows under its own name"}
    table = b"".join(_node(k, (v + "\0").encode("utf-16-le"), value_len=len(v) + 1) for k, v in strings.items())
    sfi = _node("StringFileInfo", children=_node("040904B0", children=table), value_len=0)
    vfi = _node("VarFileInfo", children=_node("Translation", struct.pack("<HH", 0x0409, 0x04B0), wtype=0), value_len=0)
    return _node("VS_VERSION_INFO", fixed, sfi + vfi, wtype=0)


def _existing(path: Path, rtype: int) -> list[tuple[object, int]]:
    """(name, language) of every resource of a type in an exe."""
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.LoadLibraryExW.restype = wintypes.HMODULE
    k32.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
    k32.FreeLibrary.argtypes = [wintypes.HMODULE]
    NAMEPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)
    LANGPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p, wintypes.WORD, ctypes.c_void_p)
    k32.EnumResourceNamesW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, NAMEPROC, ctypes.c_void_p]
    k32.EnumResourceLanguagesW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p, LANGPROC, ctypes.c_void_p]
    h = k32.LoadLibraryExW(str(path), None, 0x2 | 0x20)   # as a data file, as an image resource
    if not h:
        return []
    names, out = [], []
    try:
        def name_cb(m, t, n, _):
            names.append(n if n < 0x10000 else ctypes.wstring_at(n))
            return True
        k32.EnumResourceNamesW(h, rtype, NAMEPROC(name_cb), None)
        for n in names:
            def lang_cb(m, t, nn, lang, _, n=n):
                out.append((n, lang))
                return True
            nn = ctypes.c_void_p(n) if isinstance(n, int) else ctypes.c_wchar_p(n)
            k32.EnumResourceLanguagesW(h, rtype, nn, LANGPROC(lang_cb), None)
    finally:
        k32.FreeLibrary(h)
    return out


def brand(exe: Path, icon: Path, version: str) -> None:
    """Replace the exe's version text and icons with glitchwall's."""
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.BeginUpdateResourceW.restype = wintypes.HANDLE
    k32.BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]
    k32.UpdateResourceW.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_void_p, wintypes.WORD, ctypes.c_void_p, wintypes.DWORD]
    k32.EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]
    old = {t: _existing(exe, t) for t in (RT_VERSION, RT_ICON, RT_GROUP_ICON)}
    ico = icon.read_bytes()
    _, kind, count = struct.unpack_from("<HHH", ico, 0)
    if kind != 1 or not count:
        raise OSError("not an .ico file")
    h = k32.BeginUpdateResourceW(str(exe), False)
    if not h:
        raise ctypes.WinError(ctypes.get_last_error())
    ok = False
    try:
        def put(rtype, name, lang, data):
            nm = ctypes.c_void_p(name) if isinstance(name, int) else ctypes.c_wchar_p(name)
            buf = ctypes.create_string_buffer(data, len(data)) if data else None
            if not k32.UpdateResourceW(h, rtype, nm, lang, buf, len(data)):
                raise ctypes.WinError(ctypes.get_last_error())
        for t, entries in old.items():   # out with Python's
            for name, lang in entries:
                put(t, name, lang, b"")
        group = struct.pack("<HHH", 0, 1, count)
        for i in range(count):           # the .ico's images, then the group that lists them
            w, hgt, colors, _, planes, bits, size, offset = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
            put(RT_ICON, i + 1, 0x0409, ico[offset:offset + size])
            group += struct.pack("<BBBBHHIH", w, hgt, colors, 0, planes, bits, size, i + 1)
        put(RT_GROUP_ICON, 1, 0x0409, group)
        put(RT_VERSION, 1, 0x0409, version_info("glitchwall", version))
        ok = True
    finally:
        if not k32.EndUpdateResourceW(h, not ok) and ok:
            raise ctypes.WinError(ctypes.get_last_error())
