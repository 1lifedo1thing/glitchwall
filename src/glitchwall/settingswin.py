"""The settings window (wallpaper/settings.html in a small native window).

Drag it to the left, right or top edge of a screen and it docks there, on top of other windows; after a few
idle seconds it slides away leaving a thin strip, and touching the strip brings it back. Drag it off the edge
to undock. While you hold a control the window fades away, so the wallpaper behind is the preview: the page
asks for that through pywebview's js_api (the `Api` below). Windows only for the fade and docking; elsewhere
the page still works as a plain window.
"""
from __future__ import annotations

import ctypes
import os
import threading
import time
import webbrowser
from ctypes import wintypes

WIN = os.name == "nt"
if WIN:
    u32, dwm = ctypes.windll.user32, ctypes.windll.dwmapi
    u32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
    u32.GetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int]
    u32.SetWindowLongPtrW.argtypes = [wintypes.HWND, ctypes.c_int, ctypes.c_ssize_t]
    u32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.UINT]
    u32.SetLayeredWindowAttributes.argtypes = [wintypes.HWND, wintypes.COLORREF, wintypes.BYTE, wintypes.DWORD]
    u32.MonitorFromWindow.restype = wintypes.HANDLE
    u32.MonitorFromPoint.restype = wintypes.HANDLE
    u32.MonitorFromPoint.argtypes = [wintypes.POINT, wintypes.DWORD]
    u32.GetMonitorInfoW.argtypes = [wintypes.HANDLE, ctypes.c_void_p]

HWND_TOPMOST, HWND_NOTOPMOST = wintypes.HWND(-1), wintypes.HWND(-2)
SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE, SWP_NOMOVE = 0x1, 0x4, 0x10, 0x2
DOCK_NEAR, STRIP, IDLE_HIDE = 28, 6, 3.0   # px from an edge that docks; px left showing; seconds before hiding


class MONITORINFO(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("rcMonitor", wintypes.RECT), ("rcWork", wintypes.RECT), ("dwFlags", wintypes.DWORD)]


def _work_area(hmon) -> tuple[int, int, int, int]:
    mi = MONITORINFO()
    mi.cbSize = ctypes.sizeof(mi)
    u32.GetMonitorInfoW(hmon, ctypes.byref(mi))
    r = mi.rcWork
    return r.left, r.top, r.right, r.bottom


def work_area_at_cursor() -> tuple[int, int, int, int]:
    pt = wintypes.POINT()
    u32.GetCursorPos(ctypes.byref(pt))
    return _work_area(u32.MonitorFromPoint(pt, 2))   # MONITOR_DEFAULTTONEAREST


def _rect(h) -> tuple[int, int, int, int]:
    r = wintypes.RECT()
    u32.GetWindowRect(h, ctypes.byref(r))
    return r.left, r.top, r.right, r.bottom


def _visible_rect(h) -> tuple[int, int, int, int]:
    """The window as you see it: without the invisible resize borders Windows 10/11 put around it."""
    r = wintypes.RECT()
    if dwm.DwmGetWindowAttribute(h, 9, ctypes.byref(r), ctypes.sizeof(r)) == 0:   # DWMWA_EXTENDED_FRAME_BOUNDS
        return r.left, r.top, r.right, r.bottom
    return _rect(h)


def _cursor() -> tuple[int, int]:
    pt = wintypes.POINT()
    u32.GetCursorPos(ctypes.byref(pt))
    return pt.x, pt.y


def _mouse_down() -> bool:
    return bool(u32.GetAsyncKeyState(0x01) & 0x8000)


def _another_screen_beyond(x: int, y: int) -> bool:
    return bool(u32.MonitorFromPoint(wintypes.POINT(x, y), 0))   # MONITOR_DEFAULTTONULL: 0 = no screen there


class Settings:
    """The one settings window."""

    def __init__(self, url: str, icon: str | None = None):
        self.url, self.icon = url, icon
        self.window = None
        self._fade_gen, self._alpha = 0, 1.0
        self._keep = False                      # the page says: don't hide now (typing in the label)

    def open(self):
        import webview
        # as tall as it likes on a big screen, never taller than the screen it opens on (sizes in logical px)
        x0, y0, x1, y1 = work_area_at_cursor() if WIN else (0, 0, 1600, 900)
        scale = (ctypes.windll.user32.GetDpiForSystem() / 96) if WIN else 1.0
        h = max(560, min(940, int((y1 - y0) / scale) - 48))
        x = int(x0 / scale + ((x1 - x0) / scale - 560) / 2)   # centred on that screen, all of it on screen
        y = int(y0 / scale + max(0, ((y1 - y0) / scale - h) / 2))
        self.window = webview.create_window("glitchwall settings", self.url, js_api=Api(self), width=560, height=h,
                                            x=x, y=y, min_size=(480, 520), background_color="#010401")
        threading.Thread(target=self._dock_loop, args=(self.window,), daemon=True).start()
        return self.window

    def hwnd(self, w=None):
        w = w or self.window
        native = getattr(w, "native", None)
        return wintypes.HWND(int(str(native.Handle))) if native is not None else None

    # ---------- asked for by the page
    def fade(self, to: float, ms: int = 260) -> bool:
        h = self.hwnd() if WIN else None
        if not h:
            return False
        ex = u32.GetWindowLongPtrW(h, -20)                       # GWL_EXSTYLE
        if not ex & 0x80000:
            u32.SetWindowLongPtrW(h, -20, ex | 0x80000)          # WS_EX_LAYERED: the window can be see-through
            u32.SetLayeredWindowAttributes(h, 0, int(self._alpha * 255), 2)
        to = min(1.0, max(0.0, float(to)))
        self._fade_gen += 1
        gen, start, steps = self._fade_gen, self._alpha, max(1, int(ms) // 16)

        def run():
            for i in range(1, steps + 1):
                if gen != self._fade_gen:
                    return                                       # a newer fade took over
                self._alpha = start + (to - start) * i / steps
                u32.SetLayeredWindowAttributes(h, 0, int(self._alpha * 255), 2)   # LWA_ALPHA
                if i < steps:
                    time.sleep(0.016)
        threading.Thread(target=run, daemon=True).start()
        return True

    def keep(self, on: bool) -> None:
        self._keep = bool(on)

    # ---------- dock to an edge, slide away when idle
    def _slide(self, h, x: int, y: int, ms: int = 140) -> None:
        sx, sy = _rect(h)[:2]
        steps = max(1, ms // 16)
        for i in range(1, steps + 1):
            u32.SetWindowPos(h, None, round(sx + (x - sx) * i / steps), round(sy + (y - sy) * i / steps), 0, 0,
                             SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)
            time.sleep(0.016)

    def _dock_loop(self, w) -> None:
        if not WIN:
            return
        while self.hwnd(w) is None:
            time.sleep(0.1)
        edge, hidden, last_in, was_down, moved = None, False, time.time(), False, False
        prev, was_fg = None, False
        while True:
            time.sleep(0.08)
            try:
                h = self.hwnd(w)
                if h is None or not u32.IsWindow(h):
                    return
                r, v = _rect(h), _visible_rect(h)                # r: as Windows places it; v: as you see it
                pad = (v[0] - r[0], v[1] - r[1], r[2] - v[2], r[3] - v[3])
                wa = _work_area(u32.MonitorFromWindow(h, 2))
                down, cx, cy = _mouse_down(), *_cursor()
                inside = r[0] <= cx < r[2] and r[1] <= cy < r[3]
                fg = u32.GetForegroundWindow() == h.value
                came_forward, was_fg = fg and not was_fg, fg    # alt-tab or the taskbar brought it forward just now
                if down and prev is not None and r != prev:
                    moved = True                                 # the window is being dragged
                if edge is None:
                    if was_down and not down and moved:          # a drag just ended: near an edge?
                        e = None
                        if abs(v[0] - wa[0]) < DOCK_NEAR and not _another_screen_beyond(wa[0] - 2, (v[1] + v[3]) // 2):
                            e = "left"
                        elif abs(v[2] - wa[2]) < DOCK_NEAR and not _another_screen_beyond(wa[2] + 1, (v[1] + v[3]) // 2):
                            e = "right"
                        elif abs(v[1] - wa[1]) < DOCK_NEAR and not _another_screen_beyond((v[0] + v[2]) // 2, wa[1] - 2):
                            e = "top"
                        if e:
                            edge, last_in = e, time.time()
                            x = wa[0] - pad[0] if e == "left" else wa[2] - (v[2] - v[0]) - pad[0] if e == "right" else r[0]
                            y = wa[1] - pad[1] if e == "top" else min(max(r[1], wa[1] - pad[1]), wa[3] - (v[3] - v[1]) - pad[1])
                            u32.SetWindowPos(h, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE)
                else:
                    if inside or self._keep or came_forward and hidden and not down:
                        last_in = time.time()
                        if hidden:                               # touched the strip: slide back in
                            hidden = False
                            x = wa[0] - pad[0] if edge == "left" else wa[2] - (v[2] - v[0]) - pad[0] if edge == "right" else r[0]
                            y = wa[1] - pad[1] if edge == "top" else r[1]
                            self._slide(h, x, y)
                    if not hidden and down and moved:            # dragged away from the edge: undock
                        flush = (edge == "left" and abs(v[0] - wa[0]) < 3) or (edge == "right" and abs(v[2] - wa[2]) < 3) \
                            or (edge == "top" and abs(v[1] - wa[1]) < 3)
                        if not flush:
                            edge = None
                            u32.SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE)
                    elif not hidden and not down and time.time() - last_in > IDLE_HIDE:
                        hidden = True                            # idle: slide away, a strip stays at the edge
                        x = wa[0] - (v[2] - v[0]) - pad[0] + STRIP if edge == "left" else wa[2] - STRIP - pad[0] if edge == "right" else r[0]
                        y = wa[1] - (v[3] - v[1]) - pad[1] + STRIP if edge == "top" else r[1]
                        self._slide(h, x, y, 180)
                if not down:
                    moved = False
                was_down, prev = down, _rect(h)
            except Exception:
                pass

    def run(self) -> None:
        import webview
        if WIN:   # its own taskbar button with its own icon, not grouped under Python
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("glitchwall.settings")
            from .winhost import quiet_webview
            quiet_webview()
        self.open()
        from . import settings as cfg
        webview.start(private_mode=True, storage_path=str(cfg.state_dir() / "webview-settings"), icon=self.icon)


# the one page the settings window may open outside itself: the theme request form
THEME_REQUEST_URL = "https://github.com/marklesterb28/glitchwall/issues/new?template=theme-request.yml"


class Api:
    """What the settings page may call (window.pywebview.api.*). Kept to window chores, nothing else."""

    def __init__(self, s: Settings):
        self._s = s

    def fade(self, to: float, ms: int = 260) -> bool:
        return self._s.fade(to, ms)

    def keep(self, on: bool) -> None:
        self._s.keep(on)

    def request_theme(self) -> bool:
        """Open the theme request form in the default browser. Fixed address: the page cannot pick another."""
        return webbrowser.open(THEME_REQUEST_URL)
