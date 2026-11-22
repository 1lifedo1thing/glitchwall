"""Install / uninstall plumbing: Lively Wallpaper on Windows, autostart, background process.

Everything here only touches:
  - the state dir (settings.state_dir())
  - one wallpaper folder in Lively's library (Windows) or the state dir (macOS/Linux)
  - one autostart entry: HKCU Run value (Windows), ~/.config/autostart/*.desktop (Linux),
    ~/Library/LaunchAgents/*.plist (macOS)
`uninstall` removes exactly those.
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from importlib import resources
from pathlib import Path

from . import settings as cfg

FOLDER = "glitchwall"
RUN_VALUE = "glitchwall"
LAUNCH_LABEL = "io.github.glitchwall"
WIN = sys.platform == "win32"
FROZEN = bool(getattr(sys, "frozen", False))   # running as the standalone glitchwall.exe
CLI_EXE, FEED_EXE = "glitchwall.exe", "glitchwall-feed.exe"


# ---------------------------------------------------------------- wallpaper files
def wallpaper_source() -> Path:
    return Path(str(resources.files("glitchwall") / "wallpaper"))


def copy_wallpaper(dest: Path, port: int, key: str, label: str) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(wallpaper_source(), dest, ignore=shutil.ignore_patterns("config.js", "__pycache__"))
    write_config(dest, port, key, label)


def write_config(dest: Path, port: int, key: str, label: str) -> None:
    (dest / "config.js").write_text(cfg.config_js(port, key, label), encoding="utf-8")


# ---------------------------------------------------------------- Lively (Windows)
def lively_exe() -> Path | None:
    cands = [
        Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Lively Wallpaper" / "Lively.exe",
        Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "Lively Wallpaper" / "Lively.exe",
        Path(os.environ.get("LOCALAPPDATA", "")) / "Microsoft" / "WindowsApps" / "Lively.exe",  # Store alias
    ]
    return next((c for c in cands if c.exists()), None)


def lively_data() -> Path | None:
    local = Path(os.environ.get("LOCALAPPDATA", ""))
    classic = local / "Lively Wallpaper"
    if (classic / "Settings.json").exists():
        return classic
    for p in sorted((local / "Packages").glob("12030rocksdanister.LivelyWallpaper_*")):
        store = p / "LocalCache" / "Local" / "Lively Wallpaper"
        if store.exists():
            return store
    return classic if classic.exists() else None


def lively_library() -> Path | None:
    data = lively_data()
    if not data:
        return None
    lib = data / "Library"
    try:
        s = json.loads((data / "Settings.json").read_text(encoding="utf-8-sig"))
        if s.get("WallpaperDir"):
            lib = Path(s["WallpaperDir"])
    except (OSError, ValueError):
        pass
    return lib / "wallpapers"


def lively_layout() -> list[dict]:
    """What Lively shows now, one entry per screen that has a wallpaper:
    {"index": Lively's screen number (1 = first), "device": "\\\\.\\DISPLAY1", "path": wallpaper folder}."""
    data = lively_data()
    try:
        layout = json.loads((data / "WallpaperLayout.json").read_text(encoding="utf-8-sig"))  # type: ignore[operator]
    except (OSError, ValueError, TypeError):
        return []
    out = []
    for x in layout if isinstance(layout, list) else []:
        if isinstance(x, dict) and x.get("LivelyInfoPath"):
            sc = x.get("LivelyScreen") if isinstance(x.get("LivelyScreen"), dict) else {}
            out.append({"index": sc.get("Index"), "device": sc.get("DeviceName"), "path": x["LivelyInfoPath"]})
    return out


def lively_current() -> list[str]:
    """Folders of the wallpapers Lively shows now, one per screen."""
    return [x["path"] for x in lively_layout()]


def lively_arrangement() -> str:
    """Lively's screen layout: "per" (each screen its own wallpaper), "span" (one across all) or "duplicate"."""
    try:
        n = int(json.loads((lively_data() / "Settings.json").read_text(encoding="utf-8-sig")).get("WallpaperArrangement", 0))  # type: ignore[operator]
    except (OSError, ValueError, TypeError):
        n = 0
    return {1: "span", 2: "duplicate"}.get(n, "per")


def screens() -> list[int]:
    """The connected screens as Lively numbers them (\\\\.\\DISPLAY<n> -> n), primary first. [] if unknown."""
    if not WIN:
        return []
    try:
        import ctypes
        import re
        from ctypes import wintypes

        class MONITORINFOEXW(ctypes.Structure):
            _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", wintypes.RECT), ("rcWork", wintypes.RECT),
                        ("dwFlags", wintypes.DWORD), ("szDevice", wintypes.WCHAR * 32)]

        found = []

        def one(hmon, hdc, rect, data):
            mi = MONITORINFOEXW()
            mi.cbSize = ctypes.sizeof(mi)
            if ctypes.windll.user32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
                m = re.search(r"DISPLAY(\d+)$", mi.szDevice)
                if m:
                    found.append((not mi.dwFlags & 1, int(m.group(1))))   # MONITORINFOF_PRIMARY sorts first
            return True

        proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HMONITOR, wintypes.HDC, ctypes.POINTER(wintypes.RECT), wintypes.LPARAM)
        ctypes.windll.user32.EnumDisplayMonitors(None, None, proc(one), 0)
        return [n for _, n in sorted(found)]
    except Exception:
        return []


def show_in_lively(dest: Path, which: str = "all") -> list[str]:
    """Set the wallpaper on every screen ("all") or the main one ("primary"). Returns what happened."""
    arrangement, nums = lively_arrangement(), screens()
    if which == "primary" or arrangement != "per" or len(nums) < 2:
        # span / duplicate: one call covers every screen
        ok = lively("setwp", "--file", str(dest))
        what = {"span": "spanning all screens", "duplicate": "on every screen (duplicate)"}.get(arrangement, "on the main screen")
        return [f"Lively: wallpaper set {what}" if ok else "Lively: could not set it automatically. "
                "Open Lively and pick 'glitchwall' from the library."]
    for n in nums:
        lively("setwp", "--file", str(dest), "--monitor", str(n))
    time.sleep(1.5)
    shown = {x["index"] for x in lively_layout() if Path(x["path"]) == dest}
    missed = [n for n in nums if n not in shown]
    if not missed:
        return [f"Lively: wallpaper set on all {len(nums)} screens"]
    return [f"Lively: set on {len(nums) - len(missed)} of {len(nums)} screens. For screen(s) "
            f"{', '.join(map(str, missed))}: in Lively, pick that screen and choose 'glitchwall'."]


# what each screen showed before glitchwall, so uninstall can put it back
def previous_file() -> Path:
    return cfg.state_dir() / "previous-wallpaper.json"


def load_previous() -> list[dict]:
    """[{"index", "device", "path"}]. Older versions saved a plain list of folders (screen unknown)."""
    try:
        data = json.loads(previous_file().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return []
    out = []
    for x in data if isinstance(data, list) else []:
        if isinstance(x, str):
            out.append({"index": None, "device": None, "path": x})
        elif isinstance(x, dict) and x.get("path"):
            out.append({"index": x.get("index"), "device": x.get("device"), "path": x["path"]})
    return out


def remember_previous(layout: list[dict], dest: Path) -> None:
    """Note each screen's wallpaper unless it's already glitchwall (then the earlier note stands)."""
    keep = {(x["device"], x["index"]): x for x in load_previous()}
    for x in layout:
        if Path(x["path"]) != dest:
            keep.pop((None, None), None)   # a precise note replaces an old version's screen-less one
            keep[(x["device"], x["index"])] = x
    if keep:
        previous_file().parent.mkdir(parents=True, exist_ok=True)
        previous_file().write_text(json.dumps(list(keep.values())), encoding="utf-8")


def restore_in_lively(dest: Path) -> list[str]:
    """Take glitchwall off every screen: put back what each one showed before, or close it there."""
    prev = load_previous()
    done = []
    for x in [x for x in lively_layout() if Path(x["path"]) == dest]:
        mine = next((p for p in prev if p["device"] and p["device"] == x["device"]), None) \
            or next((p for p in prev if p["index"] is not None and p["index"] == x["index"]), None) \
            or next((p for p in prev if p["device"] is None and p["index"] is None), None)
        n = str(x["index"]) if x["index"] is not None else None
        on = ["--monitor", n] if n else []
        if mine and Path(mine["path"]).exists() and lively("setwp", "--file", mine["path"], *on):
            done.append(f"put back {Path(mine['path']).name} on screen {n or 1}")
        elif lively("closewp", "--monitor", n or "-1"):
            done.append(f"closed glitchwall on screen {n or 'all'}")
    if done:
        time.sleep(1.5)   # let Lively let go of the folder
        if any(Path(x["path"]) == dest for x in lively_layout()) and lively("closewp", "--monitor", "-1"):
            done.append("closed the wallpaper on all screens (it was still showing somewhere)")
            time.sleep(1.5)
    return done


def lively(*args: str) -> bool:
    exe = lively_exe()
    if not exe:
        return False
    r = subprocess.run([str(exe), *args], capture_output=True, timeout=60)
    return r.returncode == 0


# ---------------------------------------------------------------- python / environment
def pythonw() -> str:
    exe = Path(sys.executable)
    if WIN:
        w = exe.with_name("pythonw.exe")
        if w.exists():
            return str(w)
    return str(exe)


def ephemeral_env() -> bool:
    """True when running from uvx / pipx run, whose environment can be deleted at any time."""
    p = sys.prefix.replace("\\", "/").lower()
    return any(s in p for s in ("/uv/cache/", "/archive-v", "/builds-v", "/pipx/.cache/", "/pipx/cache/"))


def bin_dir() -> Path:
    """Where the standalone exes are copied, so autostart doesn't depend on the Downloads folder."""
    return cfg.state_dir() / "bin"


def feed_command() -> list[str]:
    if FROZEN:   # windowed feed exe: no console window at logon
        return [str(bin_dir() / FEED_EXE)]
    return [pythonw(), "-m", "glitchwall", "run"]


def install_binaries() -> list[str]:
    """Standalone build: copy glitchwall.exe and glitchwall-feed.exe next to the state."""
    if not FROZEN:
        return []
    src_dir, dest = Path(sys.executable).resolve().parent, bin_dir()
    dest.mkdir(parents=True, exist_ok=True)
    done = []
    for name in (CLI_EXE, FEED_EXE):
        src = src_dir / name
        if not src.exists():
            raise SystemExit(f"{name} not found next to {sys.executable}; keep both exes in the same folder")
        if src.resolve() != (dest / name).resolve():
            shutil.copy2(src, dest / name)
            done.append(f"copied {name} to {dest}")
    return done


def delete_later(path: Path) -> None:
    """Remove a folder after this process exits (a running exe can't delete itself on Windows)."""
    if WIN:
        subprocess.Popen(f'cmd /c ping -n 3 127.0.0.1 >nul & rmdir /s /q "{path}"',
                         creationflags=subprocess.CREATE_NO_WINDOW | subprocess.DETACHED_PROCESS, close_fds=True)


# ---------------------------------------------------------------- autostart
def _desktop_file(name: str = FOLDER) -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "autostart" / f"{name}.desktop"


def _plist(label: str = LAUNCH_LABEL) -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{label}.plist"


def add_autostart() -> str:
    cmd = feed_command()
    if WIN:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run", 0, winreg.KEY_SET_VALUE) as k:
            winreg.SetValueEx(k, RUN_VALUE, 0, winreg.REG_SZ, subprocess.list2cmdline(cmd))
        return "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\" + RUN_VALUE
    if sys.platform == "darwin":
        from xml.sax.saxutils import escape
        args = "".join(f"<string>{escape(a)}</string>" for a in cmd)
        f = _plist()
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
            f'<plist version="1.0"><dict><key>Label</key><string>{LAUNCH_LABEL}</string>'
            f'<key>ProgramArguments</key><array>{args}</array><key>RunAtLoad</key><true/></dict></plist>\n', encoding="utf-8")
        return str(f)
    import shlex
    f = _desktop_file()
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text("[Desktop Entry]\nType=Application\nName=glitchwall feed\n"
                 f"Exec={shlex.join(cmd)}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n", encoding="utf-8")
    return str(f)


def remove_autostart() -> bool:
    return _remove_autostart(RUN_VALUE, LAUNCH_LABEL)


def _remove_autostart(name: str, label: str) -> bool:
    if WIN:
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run", 0, winreg.KEY_SET_VALUE) as k:
                winreg.DeleteValue(k, name)
            return True
        except FileNotFoundError:
            return False
    f = _plist(label) if sys.platform == "darwin" else _desktop_file(name)
    if f.exists():
        f.unlink()
        return True
    return False


def autostart_entry() -> str | None:
    if WIN:
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run") as k:
                return winreg.QueryValueEx(k, RUN_VALUE)[0]
        except FileNotFoundError:
            return None
    f = _plist() if sys.platform == "darwin" else _desktop_file()
    return str(f) if f.exists() else None


# ---------------------------------------------------------------- background process
def pid_file() -> Path:
    return cfg.state_dir() / "feed.pid"


def port_open(port: int) -> bool:
    with socket.socket() as s:
        s.settimeout(0.5)
        return s.connect_ex(("127.0.0.1", port)) == 0


def _is_ours(pid: int) -> bool:
    """The pid is alive and is a python process (guards against a reused pid)."""
    try:
        if WIN:
            out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
                                 capture_output=True, text=True, timeout=10).stdout.lower()
            return "python" in out or "glitchwall" in out
        os.kill(pid, 0)
        cmd = subprocess.run(["ps", "-p", str(pid), "-o", "command="], capture_output=True, text=True).stdout
        return "glitchwall" in cmd
    except (OSError, subprocess.SubprocessError):
        return False


def running_pid(pf: Path | None = None) -> int | None:
    try:
        pid = int((pf or pid_file()).read_text().strip())
    except (OSError, ValueError):
        return None
    return pid if _is_ours(pid) else None


def start_background() -> int:
    if WIN:
        flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.CREATE_NO_WINDOW
        p = subprocess.Popen(feed_command(), creationflags=flags, close_fds=True,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        p = subprocess.Popen(feed_command(), start_new_session=True, close_fds=True,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return p.pid


def stop_background(pf: Path | None = None) -> bool:
    pf = pf or pid_file()
    pid = running_pid(pf)
    if pid is None:
        return False
    try:
        os.kill(pid, signal.SIGTERM)   # on Windows this is TerminateProcess
    except OSError:
        return False
    try:
        pf.unlink()
    except OSError:
        pass
    return True


def _legacy_lively_folder() -> Path | None:
    lib = lively_library() if WIN else None
    return lib / cfg.LEGACY_NAME if lib else None


def migrate_legacy() -> list[str]:
    """Clean up installs of earlier versions (0.1.x was called agent-live-wallpaper and 0.1.0 kept
    its state in %LOCALAPPDATA%): stop their feed, keep their settings (not the key), remove the
    old state folder, autostart entry and Lively folder."""
    done = []
    new = cfg.state_dir()
    old_lively = _legacy_lively_folder()
    for old in cfg.legacy_state_dirs():
        if not old.exists():
            continue
        if stop_background(old / "feed.pid"):
            done.append(f"stopped the old feed started from {old}")
        new.mkdir(parents=True, exist_ok=True)
        if (old / "settings.json").exists() and not (new / "settings.json").exists():
            shutil.copy2(old / "settings.json", new / "settings.json")
        prev_old, prev_new = old / "previous-wallpaper.json", new / "previous-wallpaper.json"
        if prev_old.exists() and not prev_new.exists():
            try:
                prev = [p for p in json.loads(prev_old.read_text(encoding="utf-8"))
                        if not old_lively or Path(p) != old_lively]
                if prev:
                    prev_new.write_text(json.dumps(prev), encoding="utf-8")
            except (OSError, ValueError):
                pass
        shutil.rmtree(old, ignore_errors=True)
        done.append(f"moved settings from {old} to {new} and deleted the old folder")
    if _remove_autostart(cfg.LEGACY_NAME, f"io.github.{cfg.LEGACY_NAME}"):
        done.append(f"removed the old autostart entry ({cfg.LEGACY_NAME})")
    if old_lively and old_lively.exists():
        showing = any(Path(c) == old_lively for c in lively_current())
        if not showing:   # if it is on screen, install switches away first and calls this again
            shutil.rmtree(old_lively, ignore_errors=True)
            done.append(f"deleted the old wallpaper folder {old_lively}")
    return done
