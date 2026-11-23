"""Windows: show the wallpaper page behind the desktop icons on every screen, no wallpaper app needed.

How: one frameless window per screen with the page in WebView2 (the Edge engine built into Windows),
via pywebview. Each window is made a child of the desktop layer (Progman, or the WorkerW behind the
icons on older builds), just under the icon layer: the same trick wallpaper apps use. The windows
open off-screen and only appear once they are behind the icons.

Runs on the feed's main thread (window code must). Every 2 seconds it re-checks: screens added,
removed or resized, Explorer restarted (a new desktop layer), and which screens have a fullscreen app
in front (a game, a video), whose page then sleeps. If WebView2 itself goes away (ended in Task
Manager, crashed), the feed restarts itself so the wallpaper comes back on its own.
"""
from __future__ import annotations

import ctypes
import os
import re
import threading
import time
from ctypes import wintypes
from pathlib import Path
from typing import Callable

u32 = ctypes.windll.user32 if os.name == "nt" else None
if u32:
    for _f in ("FindWindowW", "FindWindowExW", "SetParent", "GetParent", "GetForegroundWindow"):
        getattr(u32, _f).restype = wintypes.HWND
    u32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
    u32.SetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int, ctypes.c_ssize_t]
    u32.SetWindowLongPtrW.restype = ctypes.c_ssize_t
    u32.SendMessageTimeoutW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM, wintypes.UINT,
                                        wintypes.UINT, ctypes.POINTER(ctypes.c_size_t)]
    u32.MonitorFromWindow.restype = wintypes.HMONITOR
    u32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
    u32.OpenInputDesktop.restype = wintypes.HANDLE
    u32.GetUserObjectInformationW.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    u32.CloseDesktop.argtypes = [wintypes.HANDLE]
    u32.GetLayeredWindowAttributes.argtypes = [wintypes.HWND, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]

ENUM = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
MONENUM = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HMONITOR, wintypes.HDC, ctypes.POINTER(wintypes.RECT), wintypes.LPARAM)
OFFSCREEN = -32000
DESKTOP_CLASSES = ("Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd")


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD),
                ("szExeFile", ctypes.c_wchar * 260)]


def child_exes(pid: int | None = None) -> list[str]:
    """The program names of a process's direct children (WebView2's browser process is one of ours)."""
    k32 = ctypes.windll.kernel32
    k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    k32.CloseHandle.argtypes = [wintypes.HANDLE]
    pid = pid or os.getpid()
    snap = k32.CreateToolhelp32Snapshot(0x2, 0)   # TH32CS_SNAPPROCESS
    if not snap or snap == ctypes.c_void_p(-1).value:
        return []
    out, e = [], PROCESSENTRY32W()
    e.dwSize = ctypes.sizeof(e)
    try:
        ok = k32.Process32FirstW(wintypes.HANDLE(snap), ctypes.byref(e))
        while ok:
            if e.th32ParentProcessID == pid:
                out.append(e.szExeFile.lower())
            ok = k32.Process32NextW(wintypes.HANDLE(snap), ctypes.byref(e))
    finally:
        k32.CloseHandle(snap)
    return out


class MONITORINFOEXW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", wintypes.RECT), ("rcWork", wintypes.RECT),
                ("dwFlags", wintypes.DWORD), ("szDevice", wintypes.WCHAR * 32)]


def available() -> bool:
    """True if this machine can run the built-in host: Windows, pywebview installed, WebView2 runtime present."""
    if os.name != "nt":
        return False
    try:
        import webview  # noqa: F401
    except Exception:
        return False
    import winreg
    guid = r"{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"   # the WebView2 runtime's id
    for hive, key in ((winreg.HKEY_LOCAL_MACHINE, rf"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{guid}"),
                      (winreg.HKEY_LOCAL_MACHINE, rf"SOFTWARE\Microsoft\EdgeUpdate\Clients\{guid}"),
                      (winreg.HKEY_CURRENT_USER, rf"Software\Microsoft\EdgeUpdate\Clients\{guid}")):
        try:
            with winreg.OpenKey(hive, key) as k:
                if winreg.QueryValueEx(k, "pv")[0] not in ("", "0.0.0.0"):
                    return True
        except OSError:
            continue
    return False


def _cls(h) -> str:
    b = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(h, b, 256)
    return b.value


def _rect(h) -> tuple[int, int, int, int]:
    r = wintypes.RECT()
    u32.GetWindowRect(h, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


def monitors() -> dict[int, tuple[int, int, int, int]]:
    """Screen number (\\\\.\\DISPLAY<n>) -> its rectangle in desktop coordinates, primary first."""
    found = []

    def one(hmon, hdc, rect, data):
        mi = MONITORINFOEXW()
        mi.cbSize = ctypes.sizeof(mi)
        if u32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
            m = re.search(r"DISPLAY(\d+)$", mi.szDevice)
            if m:
                r = mi.rcMonitor
                found.append((not mi.dwFlags & 1, int(m.group(1)), (r.left, r.top, r.right, r.bottom)))
        return True

    u32.EnumDisplayMonitors(None, None, MONENUM(one), 0)
    return {n: r for _, n, r in sorted(found)}


def desktop_layer() -> tuple[int, int]:
    """(parent window, window to sit just under) for a wallpaper: behind the icons, in front of the
    Windows wallpaper. Asks Explorer to create the layer first (message 0x052C)."""
    progman = u32.FindWindowW("Progman", None)
    res = ctypes.c_size_t()
    u32.SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, 0, 1000, ctypes.byref(res))
    defview = u32.FindWindowExW(progman, None, "SHELLDLL_DefView", None)
    if defview:   # Windows 11 24H2 and later: icons and wallpaper layers are children of Progman
        return progman, defview
    # earlier builds: the icons live in a top-level WorkerW; the wallpaper layer is the WorkerW after it
    found = []

    def one(h, l):
        if u32.FindWindowExW(h, None, "SHELLDLL_DefView", None):
            found.append(h)
        return True

    u32.EnumWindows(ENUM(one), 0)
    worker = u32.FindWindowExW(None, found[0], "WorkerW", None) if found else None
    return (worker or progman), 0


def attach(h: int, rect: tuple[int, int, int, int]) -> None:
    """Make our window a child of the desktop layer covering `rect` (desktop coordinates)."""
    parent, after = desktop_layer()
    GWL_STYLE, GWL_EXSTYLE = -16, -20
    WS_CHILD, WS_POPUP, WS_CAPTION, WS_THICKFRAME = 0x40000000, 0x80000000, 0x00C00000, 0x00040000
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW = 0x40000, 0x80
    st = u32.GetWindowLongPtrW(h, GWL_STYLE)
    u32.SetWindowLongPtrW(h, GWL_STYLE, (st | WS_CHILD) & ~(WS_POPUP | WS_CAPTION | WS_THICKFRAME))
    ex = u32.GetWindowLongPtrW(h, GWL_EXSTYLE)
    u32.SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW) & ~WS_EX_APPWINDOW)
    if u32.GetParent(h) != parent:
        u32.SetParent(h, parent)
    x0, y0, x1, y1 = rect
    pts = (wintypes.POINT * 2)(wintypes.POINT(x0, y0), wintypes.POINT(x1, y1))
    u32.MapWindowPoints(None, parent, pts, 2)   # desktop -> parent's client coordinates
    u32.SetWindowPos(h, after or 0, pts[0].x, pts[0].y, pts[1].x - pts[0].x, pts[1].y - pts[0].y,
                     0x40 | 0x20 | 0x10)       # show, frame changed, no activate
    u32.EnableWindow(h, False)                 # a wallpaper takes no clicks, drags or keys: they go to the desktop


def fullscreen_screens(mons: dict[int, tuple]) -> set[int]:
    """Screens where the app in front covers the whole screen (a game, a video)."""
    h = u32.GetForegroundWindow()
    if not h or _cls(h) in DESKTOP_CLASSES:
        return set()
    pid = wintypes.DWORD()
    u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    if pid.value == os.getpid():   # our own wallpaper window can get the focus when another window closes
        return set()
    r = _rect(h)
    return {n for n, m in mons.items() if r[0] <= m[0] and r[1] <= m[1] and r[2] >= m[2] and r[3] >= m[3]}


def _frame(h) -> tuple[int, int, int, int]:
    """A window as you see it: without the invisible resize borders (a maximized window's is exactly the work area)."""
    r = wintypes.RECT()
    if ctypes.windll.dwmapi.DwmGetWindowAttribute(wintypes.HWND(h), 9, ctypes.byref(r), ctypes.sizeof(r)) != 0:
        return _rect(h)
    return r.left, r.top, r.right, r.bottom


def _see_through(h) -> bool:
    """Cloaked (on another virtual desktop, a suspended app), click-through, or faded: doesn't hide what's behind."""
    c = wintypes.DWORD()
    if ctypes.windll.dwmapi.DwmGetWindowAttribute(wintypes.HWND(h), 14, ctypes.byref(c), ctypes.sizeof(c)) == 0 and c.value:
        return True                                    # DWMWA_CLOAKED
    ex = u32.GetWindowLongPtrW(h, -20)
    if ex & 0x20:                                      # WS_EX_TRANSPARENT: an overlay
        return True
    if ex & 0x80000:                                   # WS_EX_LAYERED: opaque unless it set a see-through alpha
        alpha, flags = ctypes.c_ubyte(), wintypes.DWORD()
        if u32.GetLayeredWindowAttributes(h, None, ctypes.byref(alpha), ctypes.byref(flags)) and flags.value & 2 \
                and alpha.value < 255:
            return True
    return False


def covers(frame: tuple[int, int, int, int], area: tuple[int, int, int, int]) -> bool:
    return frame[0] <= area[0] and frame[1] <= area[1] and frame[2] >= area[2] and frame[3] >= area[3]


def covered_screens(work: dict[int, tuple]) -> set[int]:
    """Screens whose desktop can't be seen: an opaque window (maximized, fullscreen) fills the whole work area.
    `work` is screen -> work area. The page there can stop animating; nobody would see it."""
    out, me = set(), os.getpid()

    def one(h, _):
        if len(out) == len(work):
            return False                               # every screen covered: done
        if not u32.IsWindowVisible(h) or u32.IsIconic(h) or _cls(h) in DESKTOP_CLASSES:
            return True
        pid = wintypes.DWORD()
        u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
        if pid.value == me:
            return True
        f = None
        for n, area in work.items():
            if n not in out:
                f = f or _frame(h)
                if covers(f, area) and not _see_through(h):
                    out.add(n)
        return True

    u32.EnumWindows(ENUM(one), 0)
    return out


def session_hidden() -> bool:
    """The screen is locked (or on a UAC prompt) or the screen saver runs: no wallpaper is visible anywhere."""
    running = wintypes.BOOL()
    if u32.SystemParametersInfoW(0x72, 0, ctypes.byref(running), 0) and running.value:   # SPI_GETSCREENSAVERRUNNING
        return True
    d = u32.OpenInputDesktop(0, False, 0x0001)          # DESKTOP_READOBJECTS; fails while locked
    if not d:
        return True
    try:
        name = ctypes.create_unicode_buffer(64)
        ok = u32.GetUserObjectInformationW(d, 2, name, ctypes.sizeof(name), None)   # UOI_NAME
        return bool(ok) and name.value.lower() != "default"
    finally:
        u32.CloseDesktop(d)


def work_areas() -> dict[int, tuple[int, int, int, int]]:
    """Screen number -> its work area (the screen minus the taskbar)."""
    found = {}

    def one(hmon, hdc, rect, data):
        mi = MONITORINFOEXW()
        mi.cbSize = ctypes.sizeof(mi)
        if u32.GetMonitorInfoW(hmon, ctypes.byref(mi)):
            m = re.search(r"DISPLAY(\d+)$", mi.szDevice)
            if m:
                w = mi.rcWork
                found[int(m.group(1))] = (w.left, w.top, w.right, w.bottom)
        return True

    u32.EnumDisplayMonitors(None, None, MONENUM(one), 0)
    return found


CORNER_DWELL, CORNER_REACH = 0.5, 3   # seconds the mouse must rest there; px from the exact corner that count


def corner_hit(corner: str, x: int, y: int, mons: dict[int, tuple]) -> int | None:
    """The screen whose `corner` (top-left, top-right, bottom-left, bottom-right) the point (x, y) is in, if any."""
    if corner == "off":
        return None
    for n, (l, t, r, b) in mons.items():
        cx = l if corner.endswith("left") else r - 1
        cy = t if corner.startswith("top") else b - 1
        if abs(x - cx) <= CORNER_REACH and abs(y - cy) <= CORNER_REACH:
            return n
    return None


def watch_corner(which: Callable[[], str], open_settings: Callable[[], None], stop: threading.Event) -> None:
    """Rest the mouse in the hot corner for half a second: the settings open (or come forward if open).
    Never while a fullscreen app is in front, and once per visit to the corner."""
    pt, since, fired = wintypes.POINT(), None, False
    mons, mons_at, c = {}, 0.0, "off"
    while not stop.wait(0.1):
        try:
            if time.monotonic() - mons_at > 2:   # the screens and the setting rarely change: look every 2 s, not 10x a second
                mons, mons_at, c = monitors(), time.monotonic(), which()
            u32.GetCursorPos(ctypes.byref(pt))
            n = corner_hit(c, pt.x, pt.y, mons)
            if n is None:
                since, fired = None, False
                continue
            since = since or time.time()
            if not fired and time.time() - since >= CORNER_DWELL and n not in fullscreen_screens(mons):
                fired = True
                open_settings()
        except Exception:
            pass


def bring_settings_forward() -> bool:
    """If the settings window is open, put it in front (and un-hide a docked mini). True if it was there."""
    h = u32.FindWindowW(None, "glitchwall settings")
    if not h:
        return False
    u32.ShowWindow(h, 9)                    # SW_RESTORE
    u32.SetForegroundWindow(h)
    return True


def shown_screens() -> list[tuple[int, int, int, int]]:
    """Where our wallpaper windows are right now (any process): their rectangles in the desktop layer."""
    if os.name != "nt":
        return []
    parent, out = desktop_layer()[0], []
    c = u32.FindWindowExW(parent, None, None, None)
    while c:
        b = ctypes.create_unicode_buffer(64)
        u32.GetWindowTextW(c, b, 64)
        if b.value.startswith("glitchwall screen") and u32.IsWindowVisible(c):
            out.append(_rect(c))
        c = u32.FindWindowExW(parent, c, None, None)
    return out


def dpi_aware() -> None:
    """Work in real pixels, so windows line up with screens at any display scaling."""
    try:
        u32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))   # per-monitor v2
    except Exception:
        pass


def quiet_webview() -> None:
    """No background calls home from WebView2 (component updates, field trials, pings): our pages need none.
    Call before webview.start. The variable replaces pywebview's own arguments, so its one comes along.
    (--in-process-gpu would save ~55 MB but tripled the CPU use when measured, so it's left out.)"""
    os.environ.setdefault("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-features=ElasticOverscroll "
                          "--disable-background-networking --disable-component-update --disable-sync --no-pings")


def render_exe() -> Path | None:
    """glitchwall-render.exe, the native renderer: GLITCHWALL_RENDER_EXE, else the one shipped in the package."""
    for p in (os.environ.get("GLITCHWALL_RENDER_EXE"), Path(__file__).parent / "bin" / "glitchwall-render.exe"):
        if p and Path(p).is_file():
            return Path(p)
    return None


class NativeHost:
    """renderer = native: glitchwall-render.exe draws every screen (Direct2D, far lighter than WebView2) and polls
    the feed for events and looks, like the page. This keeps it running: started again when it exits (it does
    by itself when the screens change, to start over with the new layout), at most 5 times a minute, and ended
    with the feed (it also checks, every 2 s, that the feed process is still there). Sleep works as for the
    pages: this tells the feed which screens are covered, and the renderer asks."""

    def __init__(self, exe: Path, port: int, key: str, which: str = "all", wallpaper: Path | None = None):
        self.exe, self.port, self.key, self.which, self.wallpaper = exe, port, key, which, wallpaper
        self.windows: dict[int, object] = {}   # the screens drawn on (what check_asleep looks at)
        self.asleep: set[int] = set()
        self.proc = None
        self.starts: list[float] = []

    def screens(self) -> dict[int, tuple]:
        return Host.screens(self)

    def check_asleep(self) -> None:
        Host.check_asleep(self)

    def launch(self) -> None:
        import subprocess
        now = time.monotonic()
        self.starts = [t for t in self.starts if now - t < 60]
        if len(self.starts) >= 5:
            return   # keeps failing (see render.log): try again once the minute is up
        self.starts.append(now)
        args = [str(self.exe), "--port", str(self.port), "--which", self.which, "--parent", str(os.getpid())]
        if self.wallpaper:
            args += ["--wallpaper", str(self.wallpaper)]
        self.proc = subprocess.Popen(args, env={**os.environ, "GLITCHWALL_KEY": self.key}, close_fds=True,
                                     creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))

    def run(self, stop: threading.Event) -> None:
        """Blocks until `stop` is set (or the process ends), keeping the renderer up."""
        import atexit
        dpi_aware()
        atexit.register(self.end)
        n = 0
        try:
            while not stop.is_set():
                try:
                    if n % 4 == 0:
                        self.windows = dict.fromkeys(self.screens())
                        if self.proc is None or self.proc.poll() is not None:
                            self.launch()
                    self.check_asleep()
                except Exception:
                    pass
                n += 1
                stop.wait(0.5)
        finally:
            self.end()

    def end(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()


def on_ui(native, fn: Callable[[], None]) -> None:
    """Run `fn` on the thread that owns the WinForms window `native` (and waits for it). WebView2 throws if it
    is moved or resized from any other thread, and .NET shows that as a crash dialog."""
    if native.InvokeRequired:
        from System import Action
        native.Invoke(Action(fn))
    else:
        fn()


class Host:
    """One page window per screen. `url(n)` is the page address for screen n."""

    def __init__(self, url: Callable[[int], str], which: str = "all", storage: Path | None = None,
                 on_dead: Callable[[], None] | None = None):
        self.url, self.which, self.storage, self.on_dead = url, which, storage, on_dead
        self.windows: dict[int, object] = {}   # screen -> pywebview window
        self.attached: dict[int, tuple] = {}   # screen -> (parent, rect) it was last attached with
        self.asleep: set[int] = set()
        self.engine_seen, self.engine_missing = False, 0

    def engine_alive(self) -> bool:
        """False once WebView2 (the page's browser, a child of this process) has gone: ended in Task Manager,
        crashed, or updated underneath. The windows stay but show nothing, and nothing in pywebview says so."""
        up = "msedgewebview2.exe" in child_exes()
        self.engine_seen |= up
        self.engine_missing = 0 if up else self.engine_missing + 1
        return not (self.engine_seen and self.engine_missing >= 2)   # gone for two checks in a row

    def screens(self) -> dict[int, tuple]:
        mons = monitors()
        if self.which == "primary" and mons:
            first = next(iter(mons))
            return {first: mons[first]}
        return mons

    def new_window(self, n: int):
        import webview
        w = webview.create_window(f"glitchwall screen {n}", self.url(n), frameless=True, focus=False, easy_drag=False,
                                  x=OFFSCREEN, y=OFFSCREEN, width=800, height=600, background_color="#000000")
        self.windows[n] = w
        return w

    def tick(self) -> None:
        mons = self.screens()
        for n in [n for n in self.windows if n not in mons]:   # a screen went away
            self.windows.pop(n).destroy()
            self.attached.pop(n, None)
        for n, rect in mons.items():
            w = self.windows.get(n) or self.new_window(n)
            native = getattr(w, "native", None)
            # not created yet: next tick. Reading .Handle before the window has one would create it on *this*
            # thread; WebView2 then refuses every later resize ("only from the UI thread") and the feed dies
            if native is None or not native.IsHandleCreated:
                continue
            h = int(str(native.Handle))
            parent = desktop_layer()[0]
            # (re)attach when new, after Explorer restarted (new desktop layer), when the screen moved or
            # resized, or when the window itself drifted from its screen for any reason
            if self.attached.get(n) != (parent, rect) or u32.GetParent(h) != parent or _rect(h) != rect:
                on_ui(native, lambda h=h, rect=rect: attach(h, rect))   # resizing it resizes WebView2: its thread
                self.attached[n] = (parent, rect)

    def check_asleep(self) -> None:
        """Which screens' pages may stop animating: covered by a window, or the whole session hidden (locked,
        screen saver). Cheap, so it runs often: uncovering a screen wakes its page within about a second."""
        work = {n: a for n, a in work_areas().items() if n in self.windows}
        self.asleep = set(work) if session_hidden() else covered_screens(work)

    def manage(self, stop: threading.Event) -> None:
        n = 0
        while not stop.is_set():
            try:
                if n % 4 == 0:   # every 2 s: screens, desktop layer, WebView2 still there
                    self.tick()
                    if self.on_dead and not self.engine_alive():
                        self.on_dead()   # start over in a fresh process: the reliable way to get a new WebView2
                self.check_asleep()
            except Exception:
                pass
            n += 1
            stop.wait(0.5)
        for w in list(self.windows.values()):
            w.destroy()

    def run(self, stop: threading.Event) -> None:
        """Blocks: runs the window loop on this (the main) thread until `stop` is set."""
        import webview
        dpi_aware()
        quiet_webview()
        first = next(iter(self.screens()), 1)
        self.new_window(first)
        webview.start(self.manage, (stop,), gui="edgechromium", private_mode=True,
                      storage_path=str(self.storage) if self.storage else None)
