"""glitchwall: command line."""
from __future__ import annotations

import argparse
import os
import shutil
import sys
import time
from pathlib import Path

from . import __version__, themes
from . import platforms as plat
from . import settings as cfg

ON_OFF = {"on": True, "off": False, "true": True, "false": False, "yes": True, "no": False, "1": True, "0": False}


def say(msg: str = "") -> None:
    if sys.stdout is not None:   # None in the windowed glitchwall-feed.exe
        print(msg, flush=True)


# ---------------------------------------------------------------- commands
def cmd_run(a) -> int:
    """Foreground feed. `start` and autostart launch this with pythonw."""
    from .server import run_feed
    s = cfg.load_settings()
    port = a.port or int(s["port"])
    if plat.port_open(port):
        say(f"port {port} is already in use (feed already running?)")
        return 1
    plat.pid_file().parent.mkdir(parents=True, exist_ok=True)
    plat.pid_file().write_text(str(os.getpid()))
    import threading  # in the background: at logon Lively may not be up yet, and the feed must not wait for it
    threading.Thread(target=_heal_wallpaper, args=(s,), daemon=True).start()
    try:
        run_feed(port)
    except KeyboardInterrupt:
        pass
    finally:
        try:
            if plat.pid_file().read_text().strip() == str(os.getpid()):
                plat.pid_file().unlink()
        except OSError:
            pass
    return 0


def cmd_start(a) -> int:
    port = int(cfg.load_settings()["port"])
    if plat.running_pid():
        say(f"feed already running (pid {plat.running_pid()}) on 127.0.0.1:{port}")
        return 0
    if plat.port_open(port):
        say(f"port {port} is in use by another program. Pick another: glitchwall config --port N")
        return 1
    plat.start_background()
    for _ in range(40):
        if plat.port_open(port):
            say(f"feed running on 127.0.0.1:{port} (pid {plat.running_pid()})")
            return 0
        time.sleep(0.25)
    say("feed did not come up; run `glitchwall run` to see the error")
    return 1


def cmd_reset(a) -> int:
    """Settings back to the defaults and the feed (and with it the wallpaper's browser) started fresh."""
    cfg.save_settings(cfg.reset_settings(cfg.load_settings()))
    say("settings back to the defaults (mode, pause and install choices kept)")
    if plat.running_pid():
        cmd_stop(a)
        for _ in range(20):   # let the old one let go of the port
            if not plat.port_open(int(cfg.load_settings()["port"])):
                break
            time.sleep(0.25)
    return cmd_start(a)


def cmd_stop(a) -> int:
    if plat.stop_background():
        say("feed stopped")
    else:
        say("feed was not running")
    return 0


def cmd_status(a) -> int:
    s = cfg.load_settings()
    pid = plat.running_pid()
    say(f"glitchwall {__version__}")
    say(f"  feed        {'running, pid ' + str(pid) if pid else 'stopped'} (127.0.0.1:{s['port']})")
    say(f"  mode        {cfg.mode(s)} ({cfg.MODES[cfg.mode(s)]})")
    say(f"  paused      {'yes (serving nothing)' if s['paused'] else 'no'}")
    say(f"  outputs     {'hidden (commands only)' if s['commands_only'] else 'shown'}")
    say(f"  state dir   {cfg.state_dir()}")
    say(f"  autostart   {plat.autostart_entry() or 'off'}")
    if plat.WIN and cfg.host(s) == "glitchwall":
        from . import winhost
        shown, n = winhost.shown_screens(), len(winhost.monitors())
        say(f"  host        glitchwall's own (WebView2){'' if winhost.available() else ', NOT AVAILABLE: pywebview or WebView2 missing'}")
        say(f"  showing     {f'yes, on {len(shown)} of {n} screen' + ('s' if n > 1 else '') if shown else 'no'}")
    elif plat.WIN:
        lib = plat.lively_library()
        dest = lib / plat.FOLDER if lib else None
        say("  host        Lively Wallpaper")
        say(f"  wallpaper   {dest if dest and dest.exists() else 'not installed in Lively'}")
        layout, nums = plat.lively_layout(), plat.screens()
        if dest and layout:
            mine = [x for x in layout if Path(x["path"]) == dest]
            on = sorted(x["index"] for x in mine if x["index"] is not None)
            arr = plat.lively_arrangement()
            where = (f"all screens ({arr})" if arr != "per" else
                     "screen " + ", ".join(map(str, on)) + (f" of {len(nums)}" if len(nums) > 1 else ""))
            say(f"  showing     {'yes, ' + where if mine else 'no'}")
    refused = cfg.state_dir() / "last-refused.txt"
    if refused.exists():
        say(f"  last refused request: {refused.read_text(encoding='utf-8').strip()}")
    return 0


def _asker(a):
    """Yes/no questions: --yes answers yes; with no one at the keyboard the answer is no."""
    def ask(q: str) -> bool:
        if getattr(a, "yes", False):
            say(f"{q} yes (--yes)")
            return True
        if not (sys.stdin and sys.stdin.isatty()):
            say(f"{q} skipped (not interactive; --yes says yes to these)")
            return False
        try:
            return input(f"{q} [Y/n] ").strip().lower() in ("", "y", "yes")
        except EOFError:
            return False
    return ask


def cmd_doctor(a) -> int:
    """Check everything glitchwall needs, and offer to fetch what's missing."""
    from . import prereqs
    s = cfg.load_settings()
    host = cfg.host(s) if plat.WIN else None
    ok = prereqs.preflight(host, cfg.mode(s), say, _asker(a))
    say("everything glitchwall needs is here" if ok else "")
    return 0 if ok else 1


def cmd_install(a) -> int:
    for msg in plat.migrate_legacy():
        say(msg)
    s = cfg.load_settings()
    if a.port:
        s["port"] = a.port
    if a.commands_only:
        s["commands_only"] = True
    if a.mode:
        s["mode"] = a.mode
    cfg.save_settings(s)
    key = cfg.load_key()
    port, label = int(s["port"]), cfg.display_label(s)
    say(f"mode: {cfg.mode(s)} ({cfg.MODES[cfg.mode(s)]})")
    say(f"state dir: {cfg.state_dir()}")

    # 0. on Windows, what shows it: glitchwall's own windows when it can, else Lively. First check that
    # everything it needs is here, offering to fetch what isn't.
    own = False
    ask = _asker(a)
    if plat.WIN:
        from . import prereqs
        want = a.host or "glitchwall"
        if not prereqs.preflight(want, cfg.mode(s), say, ask):
            if a.host or not plat.lively_exe():
                say("install stopped: fix what's missing above, then run install again (glitchwall doctor re-checks)")
                return 1
            say("host: glitchwall's own windows can't run here yet; using Lively Wallpaper, which is installed")
            want = "lively"
        s["host"], s["screens"] = want, a.screens
        cfg.save_settings(s)
        own = want == "glitchwall"
        say(f"host: {want} ({cfg.HOSTS[want]})")

    # 1. wallpaper files
    lib = plat.lively_library() if plat.WIN else None
    if own:
        if lib is not None:   # switching from Lively: take it off Lively's screens and out of its library
            old = lib / plat.FOLDER
            plat.remember_previous(plat.lively_layout(), old)
            for msg in plat.close_in_lively(old):
                say(f"Lively: {msg}")
            if old.exists():
                shutil.rmtree(old, ignore_errors=True)
        lib, dest = None, None
    elif lib is not None:
        dest = lib / plat.FOLDER
        plat.remember_previous(plat.lively_layout(), dest)   # each screen's wallpaper, for uninstall to put back
    else:
        dest = cfg.state_dir() / "wallpaper"
    if dest is not None:
        dest.parent.mkdir(parents=True, exist_ok=True)
        plat.copy_wallpaper(dest, port, key, label)
        say(f"wallpaper: {dest}")

    # 2. autostart
    if a.no_autostart:
        say("autostart: skipped (--no-autostart)")
    elif plat.ephemeral_env() and not a.force and not plat.FROZEN:
        say("autostart: skipped. This copy runs from a temporary uvx/pipx-run environment that can vanish.\n"
            "  For start-at-logon, install it for good:  uv tool install glitchwall   (or pipx install)\n"
            "  then run:  glitchwall install")
    else:
        say(f"autostart: {plat.add_autostart()}")
    if plat.WIN and not (plat.ephemeral_env() and not plat.FROZEN):
        lnk = plat.add_shortcut()
        say(f"Start menu: {'glitchwall settings' if lnk else 'could not add the settings shortcut'}")
        menu = plat.add_context_menu()
        say(f"desktop right-click menu: {'glitchwall settings' if menu else 'could not add it'}")

    # 3. (re)start the feed so it uses the current port and code
    plat.stop_background()
    time.sleep(0.5)
    for msg in plat.install_binaries():
        say(msg)
    if cmd_start(a) != 0:
        return 1

    # 4. show it
    if own:
        from . import winhost
        for _ in range(40):   # the feed opens one window per screen behind the icons
            shown = winhost.shown_screens()
            if len(shown) >= (1 if a.screens == "primary" else len(winhost.monitors())):
                break
            time.sleep(0.25)
        n = len(winhost.monitors())
        say(f"showing on {len(shown)} of {n} screen{'s' if n > 1 else ''}, behind the desktop icons" if shown else
            "the feed is running but no wallpaper window appeared yet; run `glitchwall status` in a moment")
    elif lib is not None:
        if a.no_set:
            say("Lively: wallpaper added to the library, not set (--no-set)")
        elif not plat.lively_exe():
            say("Lively: could not set it automatically. Open Lively and pick 'glitchwall' from the library.")
        else:
            for msg in plat.show_in_lively(dest, a.screens):
                say(msg)
            time.sleep(1.5)   # let Lively let go of an old-name folder it was showing
            for msg in plat.migrate_legacy():
                say(msg)
    else:
        say("")
        say("Point any web-wallpaper app at this page, or open it fullscreen in a browser:")
        say(f"  {(dest / 'index.html').as_uri()}")
        say("  (see README: macOS / Linux)")
    say("")
    if plat.FROZEN:
        say(f"done. Commands from now on:  \"{plat.bin_dir() / plat.CLI_EXE}\" pause | resume | status | uninstall")
        say("You can delete the downloaded copy.")
    else:
        say("done. Stream-safe switch:  glitchwall pause   /   glitchwall resume")
    say("Settings window: glitchwall settings" + ("  (also in the Start menu)" if plat.WIN else ""))
    return 0


def cmd_uninstall(a) -> int:
    removed = plat.migrate_legacy()
    if plat.stop_background():
        removed.append("stopped the feed")
    if plat.remove_autostart():
        removed.append("removed autostart entry")
    if plat.remove_shortcut():
        removed.append("removed the Start menu shortcut")
    if plat.remove_context_menu():
        removed.append("removed glitchwall settings from the desktop right-click menu")
    if plat.WIN:
        lib = plat.lively_library()
        dest = lib / plat.FOLDER if lib else None
        if cfg.host(cfg.load_settings()) == "glitchwall":
            removed += plat.put_back_previous()       # the feed is stopped: our windows are gone
        elif dest:
            removed += plat.restore_in_lively(dest)   # every screen showing glitchwall
        if dest and dest.exists():
            shutil.rmtree(dest, ignore_errors=True)
            removed.append(f"deleted {dest}")
    sd = cfg.state_dir()
    if sd.exists():
        shutil.rmtree(sd, ignore_errors=True)
        if sd.exists():   # this exe is running from sd/bin
            plat.delete_later(sd)
        removed.append(f"deleted {sd} (key, settings, logs{', programs' if plat.FROZEN else ''})")
    for r in removed or ["nothing to remove"]:
        say(r)
    say("Your Claude Code logs were never modified."
        + ("" if plat.FROZEN else " To remove the program itself: uv tool uninstall glitchwall"))
    return 0


def cmd_demo(a) -> int:
    from .demo import DEMO_PORT, run_demo
    port = a.port or DEMO_PORT
    if plat.port_open(port):
        say(f"port {port} is in use; try --port N")
        return 1
    if a.open:
        import threading
        import webbrowser
        threading.Timer(0.8, webbrowser.open, [f"http://127.0.0.1:{port}/"]).start()
    run_demo(port, a.pace, {k: getattr(a, k) for k in themes.KINDS if getattr(a, k)}, a.mode)
    return 0


def _settings_api_up(port: int, key: str) -> bool:
    """True if the feed on this port answers the settings window (an older feed doesn't)."""
    import urllib.error
    import urllib.request
    from urllib.parse import quote
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/settings?key={quote(key)}", timeout=3) as r:
            return r.status == 200
    except (urllib.error.URLError, OSError):
        return False


class _WindowFade:
    """What the settings page may ask of its window (pywebview's js_api): fade it, so that while you change
    something you see the real wallpaper behind it. Windows only; elsewhere a no-op."""

    def __init__(self):
        self._window, self._gen, self._alpha = None, 0, 1.0

    def fade(self, to: float, ms: int = 260) -> bool:
        if not plat.WIN or self._window is None or getattr(self._window, "native", None) is None:
            return False
        import ctypes
        import threading
        u32 = ctypes.windll.user32
        u32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
        u32.SetWindowLongPtrW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_ssize_t]
        h = ctypes.c_void_p(int(str(self._window.native.Handle)))
        ex = u32.GetWindowLongPtrW(h, -20)                       # GWL_EXSTYLE
        if not ex & 0x80000:
            u32.SetWindowLongPtrW(h, -20, ex | 0x80000)          # WS_EX_LAYERED: the window can be see-through
        to = min(1.0, max(0.05, float(to)))
        self._gen += 1
        gen, start, steps = self._gen, self._alpha, max(1, int(ms) // 16)

        def run():
            for i in range(1, steps + 1):
                if gen != self._gen:
                    return                                       # a newer fade took over
                self._alpha = start + (to - start) * i / steps
                u32.SetLayeredWindowAttributes(h, 0, int(self._alpha * 255), 2)   # LWA_ALPHA
                time.sleep(0.016)
        threading.Thread(target=run, daemon=True).start()
        return True


def cmd_settings(a) -> int:
    """The settings window: every setting, applied to the real wallpaper as you change it (the window fades
    while you do, so you see it). Starts the feed if it isn't running."""
    s, key = cfg.load_settings(), cfg.load_key()
    port = int(s["port"])
    if not _settings_api_up(port, key):
        if plat.running_pid():   # an older feed without the settings window: restart it on this version
            plat.stop_background()
            time.sleep(0.5)
        if cmd_start(a) != 0:
            return 1
        for _ in range(20):
            if _settings_api_up(port, key):
                break
            time.sleep(0.25)
        else:
            say(f"the feed on port {port} does not answer the settings window; run `glitchwall status`")
            return 1
    url = f"http://127.0.0.1:{port}/settings.html#key={key}"
    if not a.browser:
        try:
            import webview
            fader = _WindowFade()
            fader._window = webview.create_window("glitchwall settings", url, width=560, height=940, min_size=(480, 600),
                                                  background_color="#010401", js_api=fader)
            if plat.WIN:   # its own taskbar button with its own icon, not grouped under Python
                import ctypes
                ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("glitchwall.settings")
            webview.start(private_mode=True, storage_path=str(cfg.state_dir() / "webview-settings"),
                          icon=str(plat.wallpaper_source() / "glitchwall.ico"))
            return 0
        except Exception as e:   # no pywebview / WebView2 here: a browser tab does the same job
            say(f"no settings window available ({type(e).__name__}); opening it in your browser")
    import webbrowser
    webbrowser.open(url)
    return 0


def cmd_pause(a, paused: bool) -> int:
    s = cfg.load_settings()
    s["paused"] = paused
    cfg.save_settings(s)
    say("feed paused: the wallpaper shows only rain" if paused else "feed resumed")
    return 0


def cmd_config(a) -> int:
    s = cfg.load_settings()
    changed = False
    for name in ("commands_only", "mask_user", "mask_email"):
        v = getattr(a, name)
        if v is not None:
            s[name] = ON_OFF[v.lower()]
            changed = True
    if a.label is not None:
        s["label"] = a.label
        changed = True
    if a.port is not None:
        s["port"] = a.port
        changed = True
    if a.mode is not None:
        s["mode"] = a.mode   # the running feed switches source by itself within a second
        changed = True
    for k in [*themes.KINDS, *cfg.TUNE]:
        if getattr(a, k) is not None:
            s[k] = getattr(a, k)
            changed = True
    if changed:
        cfg.save_settings(s)
        if a.port is not None or a.label is not None or a.mode is not None:
            _refresh_config(s)
        if a.port is not None or a.mask_user is not None or a.mask_email is not None:
            if plat.running_pid():
                plat.stop_background()
                time.sleep(0.3)
                cmd_start(a)
    for k in cfg.DEFAULTS:
        say(f"{k:14} {s[k]}")
    return 0


def cmd_themes(a) -> int:
    s = themes.look(cfg.load_settings())
    for k, names in themes.KINDS.items():
        say(f"{k}  (glitchwall config --{k} NAME)")
        for n, desc in names.items():
            say(f"  {'*' if s[k] == n else ' '} {n:10} {desc}")
        say()
    say("Changes reach the running wallpaper within a second. Lively's own dropdowns win over these.")
    say("Preview any mix: glitchwall demo --open --theme synthwave --rain stars")
    return 0


def _installed_wallpaper() -> Path:
    lib = plat.lively_library() if plat.WIN else None
    return (lib / plat.FOLDER) if lib else cfg.state_dir() / "wallpaper"


def sync_config(dest: Path, s: dict) -> bool:
    """Make the installed wallpaper's config.js match the feed's key, port and label. True if it was stale.
    The key is made anew if its file goes missing; without this the wallpaper would be refused for good."""
    if not dest.is_dir():
        return False
    want = cfg.config_js(int(s["port"]), cfg.load_key(), cfg.display_label(s))
    try:
        if (dest / "config.js").read_text(encoding="utf-8") == want:
            return False
    except OSError:
        pass
    (dest / "config.js").write_text(want, encoding="utf-8")
    return True


def _heal_wallpaper(s: dict) -> None:
    try:
        dest = _installed_wallpaper()
        if sync_config(dest, s) and plat.WIN and any(Path(c) == dest for c in plat.lively_current()):
            plat.lively("setwp", "--file", str(dest))   # reload it so the page reads the new config.js
    except Exception:
        pass


def _refresh_config(s: dict) -> None:
    """Rewrite config.js in the installed wallpaper after port/label changes."""
    lib = plat.lively_library() if plat.WIN else None
    dest = (lib / plat.FOLDER) if lib else cfg.state_dir() / "wallpaper"
    if dest.exists():
        plat.write_config(dest, int(s["port"]), cfg.load_key(), cfg.display_label(s))
        say(f"updated {dest / 'config.js'} (reload the wallpaper to pick it up)")


def cmd_export(a) -> int:
    s = cfg.load_settings()
    dest = Path(a.dir).expanduser().resolve()
    plat.copy_wallpaper(dest, int(s["port"]), cfg.load_key(), cfg.display_label(s))
    say(f"wallpaper written to {dest}  (config.js holds your feed key: keep it private)")
    say(f"open: {(dest / 'index.html').as_uri()}")
    return 0


# ---------------------------------------------------------------- parser
def _mode_arg(p) -> None:
    p.add_argument("--mode", choices=list(cfg.MODES), help="live: your real Claude Code activity; "
                   "ambient: made-up coding sessions, reads nothing; art: background only")


def _look_args(p) -> None:
    _mode_arg(p)
    for k, names in themes.KINDS.items():
        p.add_argument(f"--{k}", choices=list(names), metavar="NAME", help=f"{k} (see: glitchwall themes)")


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="glitchwall",
                                description="Your coding agent's tool calls as a live desktop wallpaper. Local only.")
    p.add_argument("--version", action="version", version=__version__)
    sub = p.add_subparsers(dest="cmd", metavar="command")

    i = sub.add_parser("install", help="put the wallpaper into Lively, start the feed, start it at logon")
    i.add_argument("--port", type=int)
    i.add_argument("--commands-only", action="store_true", help="never show tool output, only the calls")
    _mode_arg(i)
    i.add_argument("--no-autostart", action="store_true", help="don't start the feed at logon")
    i.add_argument("--no-set", action="store_true", help="add to Lively's library but don't switch to it")
    i.add_argument("--screens", choices=["all", "primary"], default="all",
                   help="with several monitors: show it on all of them (default) or only the main one")
    i.add_argument("--host", choices=list(cfg.HOSTS),
                   help="Windows: glitchwall's own windows (default when WebView2 is there) or Lively Wallpaper")
    i.add_argument("--force", action="store_true", help="add autostart even from a temporary uvx environment")
    i.add_argument("--yes", "-y", action="store_true", help="say yes to fetching anything missing (WebView2, Lively)")
    i.set_defaults(fn=cmd_install)

    dr = sub.add_parser("doctor", help="check everything glitchwall needs; offer to fetch what's missing")
    dr.add_argument("--yes", "-y", action="store_true", help="say yes to every fix offered")
    dr.set_defaults(fn=cmd_doctor)

    sub.add_parser("uninstall", help="remove everything install added").set_defaults(fn=cmd_uninstall)
    sub.add_parser("start", help="start the feed in the background").set_defaults(fn=cmd_start)
    sub.add_parser("stop", help="stop the background feed").set_defaults(fn=cmd_stop)
    sub.add_parser("status", help="show what is installed and running").set_defaults(fn=cmd_status)
    sub.add_parser("reset", help="frozen or broken? every setting back to the default, wallpaper restarted fresh"
                   ).set_defaults(fn=cmd_reset)
    sub.add_parser("pause", help="stream-safe: serve nothing until resume").set_defaults(fn=lambda a: cmd_pause(a, True))
    sub.add_parser("resume", help="undo pause").set_defaults(fn=lambda a: cmd_pause(a, False))

    r = sub.add_parser("run", help="run the feed in the foreground")
    r.add_argument("--port", type=int)
    r.set_defaults(fn=cmd_run)

    d = sub.add_parser("demo", help="fake tool calls (no logs read) + the wallpaper page in your browser")
    d.add_argument("--port", type=int)
    d.add_argument("--pace", type=float, default=1.0, help="slower >1, faster <1")
    d.add_argument("--open", action="store_true", help="open it in your browser")
    _look_args(d)
    d.set_defaults(fn=cmd_demo)

    c = sub.add_parser("config", help="show or change settings")
    c.add_argument("--commands-only", dest="commands_only", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--mask-user", dest="mask_user", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--mask-email", dest="mask_email", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--label", help="text in the corner of the wallpaper")
    c.add_argument("--port", type=int)
    _look_args(c)
    helps = {"type_speed": "typing speed", "linger": "seconds a block stays after it finishes",
             "cards": "blocks on screen at once", "intensity": "background intensity (0 = still)",
             "glass": "card opacity (lower = more see-through)"}
    for k, (_, lo, hi) in cfg.TUNE.items():
        c.add_argument(f"--{k.replace('_', '-')}", dest=k, type=int, choices=range(lo, hi + 1), metavar=f"{lo}-{hi}",
                       help=helps[k])
    c.set_defaults(fn=cmd_config)

    st = sub.add_parser("settings", help="open the settings window (every setting, applied to the wallpaper as you change it)")
    st.add_argument("--browser", action="store_true", help="open it in your browser instead of its own window")
    st.set_defaults(fn=cmd_settings)

    sub.add_parser("themes", help="list themes, rain styles and fonts").set_defaults(fn=cmd_themes)

    e = sub.add_parser("export", help="write the wallpaper + config.js to a folder (other wallpaper apps)")
    e.add_argument("dir")
    e.set_defaults(fn=cmd_export)
    return p


def main(argv: list[str] | None = None) -> int:
    p = parser()
    a = p.parse_args(argv)
    if not getattr(a, "fn", None):
        p.print_help()
        return 0
    return a.fn(a) or 0


if __name__ == "__main__":
    sys.exit(main())
