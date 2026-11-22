"""Where this tool keeps its few files, and its settings.

State dir (key, settings, pid, refusal log):
  Windows  %USERPROFILE%\\.glitchwall
  macOS    ~/Library/Application Support/glitchwall
  Linux    $XDG_STATE_HOME/glitchwall  (default ~/.local/state/...)
Override with GLITCHWALL_HOME.

Why not %LOCALAPPDATA% on Windows: MSIX-packaged apps (Claude Desktop, Store apps, their
terminals) get new AppData folders redirected into a private per-app copy. `install` run from
such an app and the feed started at logon would then read different key files, and every
request would be refused. The home folder is not redirected.
"""
from __future__ import annotations

import json
import os
import secrets
import sys
import time
from pathlib import Path

APP = "glitchwall"
DEFAULT_PORT = 47833

DEFAULTS = {
    "port": DEFAULT_PORT,
    "commands_only": False,  # hide tool output, show only the calls
    "paused": False,         # serve nothing at all (stream-safe)
    "mask_user": True,       # replace your user name with "user"
    "mask_email": True,      # replace email local parts with ••••
    "label": "CLAUDE :: live tool feed",
    "mode": "live",          # live | ambient | art, see MODES
    "theme": "phosphor",     # see `glitchwall themes`
    "palette": "theme",      # borrow another theme's colors
    "rain": "theme",         # background style; "theme" = the theme's own
    "font": "theme",
    "transition": "theme",   # how cards arrive and leave
}


MODES = {
    "live": "your real Claude Code tool calls, as they happen",
    "ambient": "made-up but realistic coding sessions; reads nothing, no Claude Code needed",
    "art": "just the themed background: no cards, no label",
}
AMBIENT_LABEL = "AMBIENT :: made-up sessions, nothing real"


def mode(s: dict) -> str:
    m = str(s.get("mode", "")).lower()
    return m if m in MODES else "live"


def display_label(s: dict) -> str:
    """The corner label. A label you set always wins; the stock one says which mode is on."""
    label = s.get("label") or DEFAULTS["label"]
    if label != DEFAULTS["label"]:
        return label
    return {"live": label, "ambient": AMBIENT_LABEL, "art": ""}[mode(s)]


def state_dir() -> Path:
    if os.environ.get("GLITCHWALL_HOME"):
        return Path(os.environ["GLITCHWALL_HOME"])
    if sys.platform == "win32":
        return Path.home() / f".{APP}"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / APP
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local" / "state") / APP


LEGACY_NAME = "agent-live-wallpaper"   # this project's name before 0.2.0


def legacy_state_dirs() -> list[Path]:
    """Where earlier versions kept their state. Cleaned up by install/uninstall.
    0.1.0: %LOCALAPPDATA%\\agent-live-wallpaper (see module docstring); 0.1.1: ~/.agent-live-wallpaper."""
    if os.environ.get("GLITCHWALL_HOME"):
        return []
    if sys.platform == "win32":
        out = [Path.home() / f".{LEGACY_NAME}"]
        if os.environ.get("LOCALAPPDATA"):
            out.append(Path(os.environ["LOCALAPPDATA"]) / LEGACY_NAME)
        return out
    return [state_dir().parent / LEGACY_NAME]


def _private_write(path: Path, text: str) -> None:
    """Write a file only this user can read (mode 0600 on POSIX; on Windows the profile folder is per-user).
    Written to a temp file and swapped in, so the running feed never reads a half-written settings file
    (which would read as the defaults: not paused)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    for tries in range(40):   # Windows refuses the swap while the feed has the file open for a moment
        try:
            os.replace(tmp, path)
            return
        except PermissionError:
            if tries == 39:
                raise
            time.sleep(0.05)


def key_file() -> Path:
    return state_dir() / "key"


def load_key() -> str:
    """The per-install random key the wallpaper must present. Made on first use."""
    f = key_file()
    if f.exists():
        k = f.read_text(encoding="utf-8").strip()
        if len(k) >= 32:
            return k
    k = secrets.token_urlsafe(32)
    _private_write(f, k)
    return k


def settings_file() -> Path:
    return state_dir() / "settings.json"


def load_settings() -> dict:
    s = dict(DEFAULTS)
    try:
        s.update(json.loads(settings_file().read_text(encoding="utf-8")))
    except (OSError, ValueError):
        pass
    return s


def save_settings(s: dict) -> None:
    _private_write(settings_file(), json.dumps({k: s[k] for k in DEFAULTS if k in s}, indent=2) + "\n")


def config_js(port: int, key: str, label: str) -> str:
    """The file the wallpaper loads to find the feed. Contains the key: never publish it."""
    return f"window.FEED = {json.dumps({'port': port, 'key': key, 'label': label})};\n"
