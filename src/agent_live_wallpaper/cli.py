"""agent-wallpaper: command line."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

from . import __version__
from . import platforms as plat
from . import settings as cfg

ON_OFF = {"on": True, "off": False, "true": True, "false": False, "yes": True, "no": False, "1": True, "0": False}


def say(msg: str = "") -> None:
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
        say(f"port {port} is in use by another program. Pick another: agent-wallpaper config --port N")
        return 1
    plat.start_background()
    for _ in range(40):
        if plat.port_open(port):
            say(f"feed running on 127.0.0.1:{port} (pid {plat.running_pid()})")
            return 0
        time.sleep(0.25)
    say("feed did not come up; run `agent-wallpaper run` to see the error")
    return 1


def cmd_stop(a) -> int:
    if plat.stop_background():
        say("feed stopped")
    else:
        say("feed was not running")
    return 0


def cmd_status(a) -> int:
    s = cfg.load_settings()
    pid = plat.running_pid()
    say(f"agent-live-wallpaper {__version__}")
    say(f"  feed        {'running, pid ' + str(pid) if pid else 'stopped'} (127.0.0.1:{s['port']})")
    say(f"  paused      {'yes (serving nothing)' if s['paused'] else 'no'}")
    say(f"  outputs     {'hidden (commands only)' if s['commands_only'] else 'shown'}")
    say(f"  state dir   {cfg.state_dir()}")
    say(f"  autostart   {plat.autostart_entry() or 'off'}")
    if plat.WIN:
        lib = plat.lively_library()
        dest = lib / plat.FOLDER if lib else None
        say(f"  wallpaper   {dest if dest and dest.exists() else 'not installed in Lively'}")
        cur = plat.lively_current()
        if dest and cur:
            say(f"  showing     {'yes' if any(Path(c) == dest for c in cur) else 'no'}")
    refused = cfg.state_dir() / "last-refused.txt"
    if refused.exists():
        say(f"  last refused request: {refused.read_text(encoding='utf-8').strip()}")
    return 0


def cmd_install(a) -> int:
    s = cfg.load_settings()
    if a.port:
        s["port"] = a.port
    if a.commands_only:
        s["commands_only"] = True
    cfg.save_settings(s)
    key = cfg.load_key()
    port, label = int(s["port"]), s["label"]
    say(f"state dir: {cfg.state_dir()}")

    # 1. wallpaper files
    lib = plat.lively_library() if plat.WIN else None
    if lib is not None:
        dest = lib / plat.FOLDER
        prev = [c for c in plat.lively_current() if Path(c) != dest]
        prev_file = cfg.state_dir() / "previous-wallpaper.json"
        if prev and not prev_file.exists():
            prev_file.write_text(json.dumps(prev), encoding="utf-8")
    else:
        dest = cfg.state_dir() / "wallpaper"
    lib_root = dest.parent
    lib_root.mkdir(parents=True, exist_ok=True)
    plat.copy_wallpaper(dest, port, key, label)
    say(f"wallpaper: {dest}")

    # 2. autostart
    if a.no_autostart:
        say("autostart: skipped (--no-autostart)")
    elif plat.ephemeral_env() and not a.force:
        say("autostart: skipped. This copy runs from a temporary uvx/pipx-run environment that can vanish.\n"
            "  For start-at-logon, install it for good:  uv tool install agent-live-wallpaper   (or pipx install)\n"
            "  then run:  agent-wallpaper install")
    else:
        say(f"autostart: {plat.add_autostart()}")

    # 3. (re)start the feed so it uses the current port and code
    plat.stop_background()
    time.sleep(0.3)
    if cmd_start(a) != 0:
        return 1

    # 4. show it
    if lib is not None:
        if a.no_set:
            say("Lively: wallpaper added to the library, not set (--no-set)")
        elif plat.lively(*["setwp", "--file", str(dest)]):
            say("Lively: wallpaper set")
        else:
            say("Lively: could not set it automatically. Open Lively and pick 'Agent Live Wallpaper' from the library.")
    else:
        say("")
        say("Point any web-wallpaper app at this page, or open it fullscreen in a browser:")
        say(f"  {(dest / 'index.html').as_uri()}")
        say("  (see README: macOS / Linux)")
    say("")
    say("done. Stream-safe switch:  agent-wallpaper pause   /   agent-wallpaper resume")
    return 0


def cmd_uninstall(a) -> int:
    removed = []
    if plat.stop_background():
        removed.append("stopped the feed")
    if plat.remove_autostart():
        removed.append("removed autostart entry")
    if plat.WIN:
        lib = plat.lively_library()
        dest = lib / plat.FOLDER if lib else None
        if dest and any(Path(c) == dest for c in plat.lively_current()):
            prev = []
            try:
                prev = json.loads((cfg.state_dir() / "previous-wallpaper.json").read_text(encoding="utf-8"))
            except (OSError, ValueError):
                pass
            prev = [p for p in prev if Path(p).exists()]
            if prev and plat.lively("setwp", "--file", prev[0]):
                removed.append(f"put back previous wallpaper {prev[0]}")
            elif plat.lively("closewp", "--monitor", "-1"):
                removed.append("closed the wallpaper in Lively")
            time.sleep(1.5)   # let Lively release the folder
        if dest and dest.exists():
            shutil.rmtree(dest, ignore_errors=True)
            removed.append(f"deleted {dest}")
    sd = cfg.state_dir()
    if sd.exists():
        shutil.rmtree(sd, ignore_errors=True)
        removed.append(f"deleted {sd} (key, settings, logs)")
    for r in removed or ["nothing to remove"]:
        say(r)
    say("Your Claude Code logs were never modified. To remove the program itself: uv tool uninstall agent-live-wallpaper")
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
    run_demo(port, a.pace)
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
    if changed:
        cfg.save_settings(s)
        if a.port is not None or a.label is not None:
            _refresh_config(s)
        if a.port is not None or a.mask_user is not None or a.mask_email is not None:
            if plat.running_pid():
                plat.stop_background()
                time.sleep(0.3)
                cmd_start(a)
    for k in cfg.DEFAULTS:
        say(f"{k:14} {s[k]}")
    return 0


def _refresh_config(s: dict) -> None:
    """Rewrite config.js in the installed wallpaper after port/label changes."""
    lib = plat.lively_library() if plat.WIN else None
    dest = (lib / plat.FOLDER) if lib else cfg.state_dir() / "wallpaper"
    if dest.exists():
        plat.write_config(dest, int(s["port"]), cfg.load_key(), s["label"])
        say(f"updated {dest / 'config.js'} (reload the wallpaper to pick it up)")


def cmd_export(a) -> int:
    s = cfg.load_settings()
    dest = Path(a.dir).expanduser().resolve()
    plat.copy_wallpaper(dest, int(s["port"]), cfg.load_key(), s["label"])
    say(f"wallpaper written to {dest}  (config.js holds your feed key: keep it private)")
    say(f"open: {(dest / 'index.html').as_uri()}")
    return 0


# ---------------------------------------------------------------- parser
def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="agent-wallpaper",
                                description="Your coding agent's tool calls as a live desktop wallpaper. Local only.")
    p.add_argument("--version", action="version", version=__version__)
    sub = p.add_subparsers(dest="cmd", metavar="command")

    i = sub.add_parser("install", help="put the wallpaper into Lively, start the feed, start it at logon")
    i.add_argument("--port", type=int)
    i.add_argument("--commands-only", action="store_true", help="never show tool output, only the calls")
    i.add_argument("--no-autostart", action="store_true", help="don't start the feed at logon")
    i.add_argument("--no-set", action="store_true", help="add to Lively's library but don't switch to it")
    i.add_argument("--force", action="store_true", help="add autostart even from a temporary uvx environment")
    i.set_defaults(fn=cmd_install)

    sub.add_parser("uninstall", help="remove everything install added").set_defaults(fn=cmd_uninstall)
    sub.add_parser("start", help="start the feed in the background").set_defaults(fn=cmd_start)
    sub.add_parser("stop", help="stop the background feed").set_defaults(fn=cmd_stop)
    sub.add_parser("status", help="show what is installed and running").set_defaults(fn=cmd_status)
    sub.add_parser("pause", help="stream-safe: serve nothing until resume").set_defaults(fn=lambda a: cmd_pause(a, True))
    sub.add_parser("resume", help="undo pause").set_defaults(fn=lambda a: cmd_pause(a, False))

    r = sub.add_parser("run", help="run the feed in the foreground")
    r.add_argument("--port", type=int)
    r.set_defaults(fn=cmd_run)

    d = sub.add_parser("demo", help="fake tool calls (no logs read) + the wallpaper page in your browser")
    d.add_argument("--port", type=int)
    d.add_argument("--pace", type=float, default=1.0, help="slower >1, faster <1")
    d.add_argument("--open", action="store_true", help="open it in your browser")
    d.set_defaults(fn=cmd_demo)

    c = sub.add_parser("config", help="show or change settings")
    c.add_argument("--commands-only", dest="commands_only", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--mask-user", dest="mask_user", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--mask-email", dest="mask_email", choices=sorted(ON_OFF), metavar="on|off")
    c.add_argument("--label", help="text in the corner of the wallpaper")
    c.add_argument("--port", type=int)
    c.set_defaults(fn=cmd_config)

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
