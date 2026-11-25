"""Install / uninstall plumbing: wallpaper files, autostart, Start menu, background process.

Everything here only touches:
  - the state dir (settings.state_dir())
  - the wallpaper folder in the state dir (macOS/Linux; on Windows the feed serves the page itself)
  - one autostart entry: HKCU Run value (Windows), ~/.config/autostart/*.desktop (Linux),
    ~/Library/LaunchAgents/*.plist (macOS)
  - Windows: the Start menu shortcut and the desktop right-click entry for the settings
`uninstall` removes exactly those.
"""
from __future__ import annotations

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
    return [pythonw(), "-m", "glitchwall", "run"]   # on Windows `run` moves itself to glitchwall-feed.exe


def live_command(*args: str) -> list[str]:
    """For starting something right now (not stored anywhere): glitchwall-feed.exe when this already is it."""
    if WIN and not FROZEN and Path(sys.executable).name.lower() == FEED_EXE:
        return [sys.executable, "-m", "glitchwall", *args]
    return feed_command()[:-1] + list(args) if not FROZEN else [str(bin_dir() / FEED_EXE), *args]


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
def hkcu(sub: str) -> str:
    """A key under HKEY_CURRENT_USER; GLITCHWALL_REG_ROOT (tests) moves every one under that key instead."""
    root = os.environ.get("GLITCHWALL_REG_ROOT")
    return rf"{root}\{sub}" if root else sub


def _desktop_file(name: str = FOLDER) -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "autostart" / f"{name}.desktop"


def _plist(label: str = LAUNCH_LABEL) -> Path:
    return Path.home() / "Library" / "LaunchAgents" / f"{label}.plist"


def add_autostart() -> str:
    cmd = feed_command()
    if WIN:
        import winreg
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, hkcu(r"Software\Microsoft\Windows\CurrentVersion\Run")) as k:
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
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, hkcu(r"Software\Microsoft\Windows\CurrentVersion\Run"), 0, winreg.KEY_SET_VALUE) as k:
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
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, hkcu(r"Software\Microsoft\Windows\CurrentVersion\Run")) as k:
                return winreg.QueryValueEx(k, RUN_VALUE)[0]
        except FileNotFoundError:
            return None
    f = _plist() if sys.platform == "darwin" else _desktop_file()
    return str(f) if f.exists() else None


# ---------------------------------------------------------------- Start menu (Windows)
def shortcut_file() -> Path | None:
    if not WIN or not os.environ.get("APPDATA"):
        return None
    return Path(os.environ["APPDATA"]) / "Microsoft" / "Windows" / "Start Menu" / "Programs" / "glitchwall settings.lnk"


def settings_command() -> list[str]:
    if FROZEN:
        return [str(bin_dir() / FEED_EXE), "settings"]
    return [pythonw(), "-m", "glitchwall", "settings"]


def icon_location() -> str:
    """The icon for the Start menu entry: built into the exe, or the file in the installed package."""
    if FROZEN:
        return f"{bin_dir() / FEED_EXE},0"
    return f"{wallpaper_source() / 'glitchwall.ico'},0"


def add_shortcut() -> str | None:
    """A Start menu entry that opens the settings window. Made with the Windows Script Host (built in)."""
    f = shortcut_file()
    if f is None:
        return None
    cmd = settings_command()

    def q(v) -> str:   # a PowerShell single-quoted string
        return "'" + str(v).replace("'", "''") + "'"
    ps =(f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut({q(f)});$s.TargetPath={q(cmd[0])};"
          f"$s.Arguments={q(subprocess.list2cmdline(cmd[1:]))};$s.Description='glitchwall settings';"
          f"$s.WorkingDirectory={q(Path.home())};$s.IconLocation={q(icon_location())};$s.Save()")
    try:
        f.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", ps], capture_output=True,
                       timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
    except (OSError, subprocess.SubprocessError):
        return None
    return str(f) if f.exists() else None


MENU_KEY = r"Software\Classes\DesktopBackground\Shell\glitchwall"


def add_context_menu() -> bool:
    """'glitchwall settings' in the desktop's right-click menu, beside Display settings and Personalize
    (on Windows 11 under 'Show more options'). Per user, no admin."""
    if not WIN:
        return False
    import winreg
    try:
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, hkcu(MENU_KEY)) as k:
            winreg.SetValueEx(k, "", 0, winreg.REG_SZ, "glitchwall settings")
            winreg.SetValueEx(k, "MUIVerb", 0, winreg.REG_SZ, "glitchwall settings")
            winreg.SetValueEx(k, "Icon", 0, winreg.REG_SZ, icon_location())
            winreg.SetValueEx(k, "Position", 0, winreg.REG_SZ, "Bottom")
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, hkcu(MENU_KEY) + r"\command") as k:
            winreg.SetValueEx(k, "", 0, winreg.REG_SZ, subprocess.list2cmdline(settings_command()))
    except OSError:
        return False
    return True


def remove_context_menu() -> bool:
    if not WIN:
        return False
    import winreg
    try:
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, hkcu(MENU_KEY) + r"\command")
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, hkcu(MENU_KEY))
        return True
    except OSError:
        return False


def remove_shortcut() -> bool:
    f = shortcut_file()
    if f and f.exists():
        f.unlink()
        return True
    return False


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

