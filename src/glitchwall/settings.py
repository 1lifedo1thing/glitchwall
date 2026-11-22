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
import threading
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
    # the page's sliders
    "type_speed": 5,         # 1-20
    "linger": 7,             # seconds a block stays after it finishes, 2-30
    "cards": 4,              # blocks on screen at once, 1-6
    "intensity": 5,          # background intensity, 0 (still) - 10
    "glass": 58,             # card opacity, 20-95
    "corner": "top-left",    # rest the mouse in this screen corner to open the settings; see CORNERS
    "screens": "all",        # all | primary
}

TUNE = {   # setting -> (page slider name, lowest, highest)
    "type_speed": ("typeSpeed", 1, 20), "linger": ("linger", 2, 30), "cards": ("cards", 1, 6),
    "intensity": ("rain", 0, 10), "glass": ("glass", 20, 95),
}


def tune(s: dict) -> dict:
    """The slider settings as the page names them, each kept in its range."""
    out = {}
    for k, (name, lo, hi) in TUNE.items():
        try:
            out[name] = min(hi, max(lo, int(s.get(k, DEFAULTS[k]))))
        except (TypeError, ValueError):
            out[name] = DEFAULTS[k]
    return out


def apply_changes(s: dict, changes: dict) -> tuple[dict, list[str]]:
    """Settings changed from the settings window: only known keys, only valid values. Port and
    screens stay command-line only (they need a restart or a reinstall). Returns (new settings, refused keys)."""
    from . import themes
    out, refused = dict(s), []
    for k, v in (changes or {}).items():
        ok = False
        if k in themes.KINDS:
            ok = isinstance(v, str) and v in themes.KINDS[k]
        elif k == "mode":
            ok = v in MODES
        elif k in ("paused", "commands_only", "mask_user", "mask_email"):
            ok = isinstance(v, bool)
        elif k in TUNE:
            _, lo, hi = TUNE[k]
            ok = isinstance(v, int) and not isinstance(v, bool) and lo <= v <= hi
        elif k == "corner":
            ok = v in CORNERS
        elif k == "label":
            ok = isinstance(v, str) and len(v) <= 80 and "\n" not in v
        if ok:
            out[k] = v
        else:
            refused.append(k)
    return out, refused


CORNERS = {
    "off": "no hot corner; open the settings from the right-click menu or the Start menu",
    "top-left": "rest the mouse in the top-left corner of a screen to open the settings",
    "top-right": "rest the mouse in the top-right corner of a screen to open the settings",
    "bottom-left": "rest the mouse in the bottom-left corner of a screen to open the settings",
    "bottom-right": "rest the mouse in the bottom-right corner of a screen to open the settings",
}


def corner(s: dict) -> str:
    c = str(s.get("corner", "")).lower()
    return c if c in CORNERS else DEFAULTS["corner"]


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


def _private_write(path: Path, text: str) -> None:
    """Write a file only this user can read (mode 0600 on POSIX; on Windows the profile folder is per-user).
    Written to a temp file and swapped in, so the running feed never reads a half-written settings file
    (which would read as the defaults: not paused)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    # per process and thread: the feed answers requests on several threads, and two saves at once must not share it
    tmp = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
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


KEEP_ON_RESET = ("port", "screens", "mode", "paused")   # how it's installed, what it shows, stream-safety


def reset_settings(s: dict) -> dict:
    """Everything back to the defaults (phosphor, no overrides, standard motion, masks on), except how it is
    installed, the mode (resetting to live would start showing real tool calls) and pause."""
    return {**DEFAULTS, **{k: s[k] for k in KEEP_ON_RESET if k in s}}


def save_settings(s: dict) -> None:
    _private_write(settings_file(), json.dumps({k: s[k] for k in DEFAULTS if k in s}, indent=2) + "\n")


def config_js(port: int, key: str, label: str) -> str:
    """The file the wallpaper loads to find the feed. Contains the key: never publish it."""
    return f"window.FEED = {json.dumps({'port': port, 'key': key, 'label': label})};\n"
