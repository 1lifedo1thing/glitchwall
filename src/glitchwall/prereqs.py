"""What glitchwall needs on this machine, checked before `install` (and by `glitchwall doctor`).

Each check says whether it's there, why it matters, and how to get it. glitchwall never downloads or installs
anything itself: for a missing piece it can only offer to open Microsoft's download page in your browser, and you
take it from there.
"""
from __future__ import annotations

import os
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

WIN = sys.platform == "win32"
FROZEN = bool(getattr(sys, "frozen", False))

WEBVIEW2_PAGE = "https://developer.microsoft.com/microsoft-edge/webview2/"
DOTNET_PAGE = "https://dotnet.microsoft.com/download/dotnet-framework/net481"
DOTNET_472 = 461808   # the .NET Framework 4.7.2 release number; pywebview's Windows backend needs it


@dataclass
class Check:
    name: str
    ok: bool
    detail: str                    # what was found, or why it matters
    required: bool = True          # False: glitchwall still works without it, with less
    fix_prompt: str = ""           # the question to ask before fixing it
    fix: Callable[[], bool] | None = field(default=None, repr=False)   # returns True if it's there now
    hint: str = ""                 # what to do by hand


# ---------------------------------------------------------------- probes
def _reg(hive_name: str, path: str, value: str):
    import winreg
    hive = getattr(winreg, hive_name)
    try:
        with winreg.OpenKey(hive, path) as k:
            return winreg.QueryValueEx(k, value)[0]
    except OSError:
        return None


def windows_build() -> int:
    return sys.getwindowsversion().build if WIN else 0


def dotnet_release() -> int:
    return int(_reg("HKEY_LOCAL_MACHINE", r"SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full", "Release") or 0)


def webview2_version() -> str | None:
    guid = r"{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
    for hive, path in (("HKEY_LOCAL_MACHINE", rf"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{guid}"),
                       ("HKEY_LOCAL_MACHINE", rf"SOFTWARE\Microsoft\EdgeUpdate\Clients\{guid}"),
                       ("HKEY_CURRENT_USER", rf"Software\Microsoft\EdgeUpdate\Clients\{guid}")):
        v = _reg(hive, path, "pv")
        if v and v != "0.0.0.0":
            return v
    return None


def pywebview_ok() -> bool:
    try:
        import webview  # noqa: F401
        return True
    except Exception:   # noqa: BLE001  any import failure means the windows can't open
        return False


def claude_logs() -> Path | None:
    p = Path(os.environ.get("CLAUDE_CONFIG_DIR") or Path.home() / ".claude") / "projects"
    return p if p.is_dir() else None


# ---------------------------------------------------------------- fixes
def open_page(url: str) -> bool:
    import webbrowser
    webbrowser.open(url)
    return False   # still missing until the person installs it


# ---------------------------------------------------------------- the list
def checks(mode: str = "live") -> list[Check]:
    """Everything `install` needs for this mode."""
    out: list[Check] = []
    v = sys.version_info
    out.append(Check("Python", FROZEN or v >= (3, 9),
                     "built into the exe" if FROZEN else f"{v.major}.{v.minor}.{v.micro} (needs 3.9 or newer)",
                     hint="install a newer Python, or use the standalone exe"))
    if WIN:
        b = windows_build()
        out.append(Check("Windows", b >= 17134, f"build {b} (needs Windows 10 1803 or newer)",
                         hint="update Windows"))
        rel = dotnet_release()
        out.append(Check(".NET Framework 4.7.2+", rel >= DOTNET_472,
                         "present" if rel >= DOTNET_472 else
                         "missing: glitchwall's own desktop windows need it (built into Windows 10 1803 and later)",
                         fix_prompt="Open Microsoft's .NET Framework 4.8.1 download page?",
                         fix=lambda: open_page(DOTNET_PAGE), hint=DOTNET_PAGE))
        wv = webview2_version()
        out.append(Check("WebView2 runtime", wv is not None,
                         f"{wv}" if wv else "missing: the engine that draws the wallpaper behind your icons",
                         fix_prompt="Open Microsoft's WebView2 download page? (Install the Evergreen runtime, then "
                                    "run install again.)",
                         fix=lambda: open_page(WEBVIEW2_PAGE), hint=WEBVIEW2_PAGE))
        out.append(Check("pywebview", pywebview_ok(),
                         "bundled" if FROZEN else ("installed" if pywebview_ok() else
                                                   "missing: the library that opens those windows"),
                         hint="uv tool install --reinstall glitchwall   (it's a dependency on Windows)"))
    if mode == "live":
        logs = claude_logs()
        out.append(Check("Claude Code logs", logs is not None, str(logs) if logs else
                         "not found: live mode shows nothing until you use Claude Code here. "
                         "Ambient mode works without it (--mode ambient)",
                         required=False))
    return out


def preflight(mode: str, say: Callable[[str], None], ask: Callable[[str], bool]) -> bool:
    """Check, offer to fix what's missing, check again. True if everything required is there."""
    found = checks(mode)
    say("checking what glitchwall needs:")
    for c in found:
        say(f"  {'ok ' if c.ok else ('-- ' if not c.required else 'XX ')} {c.name}: {c.detail}")
    for c in [c for c in found if not c.ok and c.fix]:
        if ask(c.fix_prompt):
            c.ok = bool(c.fix())
            say(f"  {c.name}: {'ready' if c.ok else 'still missing'}")
    missing = [c for c in found if not c.ok and c.required]
    for c in missing:
        say(f"missing: {c.name}. {('To fix: ' + c.hint) if c.hint else ''}")
    return not missing
