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
from importlib import resources
from pathlib import Path

from . import settings as cfg

FOLDER = "glitchwall"
RUN_VALUE = "glitchwall"
LAUNCH_LABEL = "io.github.glitchwall"
WIN = sys.platform == "win32"


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


def lively_current() -> list[str]:
    """Folders of the wallpapers Lively shows now, one per screen."""
    data = lively_data()
    try:
        layout = json.loads((data / "WallpaperLayout.json").read_text(encoding="utf-8-sig"))  # type: ignore[operator]
        return [x.get("LivelyInfoPath") for x in layout if x.get("LivelyInfoPath")]
    except (OSError, ValueError, TypeError):
        return []


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


def feed_command() -> list[str]:
    return [pythonw(), "-m", "glitchwall", "run"]


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
            return "python" in out
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
