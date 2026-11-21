"""Where this tool keeps its few files, and its settings.

State dir (key, settings, pid, refusal log):
  Windows  %USERPROFILE%\\.agent-live-wallpaper
  macOS    ~/Library/Application Support/agent-live-wallpaper
  Linux    $XDG_STATE_HOME/agent-live-wallpaper  (default ~/.local/state/...)
Override with AGENT_WALLPAPER_HOME.

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
from pathlib import Path

APP = "agent-live-wallpaper"
DEFAULT_PORT = 47833

DEFAULTS = {
    "port": DEFAULT_PORT,
    "commands_only": False,  # hide tool output, show only the calls
    "paused": False,         # serve nothing at all (stream-safe)
    "mask_user": True,       # replace your user name with "user"
    "mask_email": True,      # replace email local parts with ••••
    "label": "CLAUDE :: live tool feed",
}


def state_dir() -> Path:
    if os.environ.get("AGENT_WALLPAPER_HOME"):
        return Path(os.environ["AGENT_WALLPAPER_HOME"])
    if sys.platform == "win32":
        return Path.home() / f".{APP}"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / APP
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local" / "state") / APP


def legacy_state_dirs() -> list[Path]:
    """Where 0.1.0 kept its state on Windows (see module docstring). Cleaned up by install/uninstall."""
    if sys.platform != "win32" or os.environ.get("AGENT_WALLPAPER_HOME") or not os.environ.get("LOCALAPPDATA"):
        return []
    return [Path(os.environ["LOCALAPPDATA"]) / APP]


def _private_write(path: Path, text: str) -> None:
    """Write a file only this user can read (mode 0600 on POSIX; on Windows the profile folder is per-user)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)


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
