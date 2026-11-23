"""A notification-area (system tray) icon with a right-click menu. Windows only, plain Win32 through ctypes.

The icon lives on its own thread with its own hidden window and message loop, so it works next to any GUI
toolkit. `menu()` is asked for the items each time the menu opens (so labels like Pause/Resume are current);
a left click runs the default (first) item. `watch()`, if given, is asked twice a second: return False to take
the icon away (for example when the window it stands for came back some other way).
"""
from __future__ import annotations

import ctypes
import threading
from ctypes import wintypes
from typing import Callable

# our own handles to the DLLs, so the argtypes set here can't clash with other modules' (windll is shared)
u32 = ctypes.WinDLL("user32", use_last_error=True)
shell32 = ctypes.WinDLL("shell32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

LRESULT = ctypes.c_ssize_t
WNDPROC = ctypes.WINFUNCTYPE(LRESULT, wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)

WM_DESTROY, WM_CLOSE, WM_NULL, WM_TIMER, WM_COMMAND = 0x0002, 0x0010, 0x0000, 0x0113, 0x0111
WM_LBUTTONUP, WM_RBUTTONUP, WM_CONTEXTMENU, WM_APP = 0x0202, 0x0205, 0x007B, 0x8000
WM_TRAY = WM_APP + 1
NIM_ADD, NIM_MODIFY, NIM_DELETE = 0, 1, 2
NIF_MESSAGE, NIF_ICON, NIF_TIP = 0x1, 0x2, 0x4
MF_STRING, MF_SEPARATOR, MF_GRAYED = 0x0, 0x800, 0x1
TPM_RIGHTBUTTON, TPM_RETURNCMD, TPM_NONOTIFY = 0x2, 0x100, 0x80
IMAGE_ICON, LR_LOADFROMFILE, SM_CXSMICON, SM_CYSMICON = 1, 0x10, 49, 50


class GUID(ctypes.Structure):
    _fields_ = [("Data1", wintypes.DWORD), ("Data2", wintypes.WORD), ("Data3", wintypes.WORD), ("Data4", ctypes.c_ubyte * 8)]


class NOTIFYICONDATAW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("hWnd", wintypes.HWND), ("uID", wintypes.UINT), ("uFlags", wintypes.UINT),
                ("uCallbackMessage", wintypes.UINT), ("hIcon", wintypes.HICON), ("szTip", wintypes.WCHAR * 128),
                ("dwState", wintypes.DWORD), ("dwStateMask", wintypes.DWORD), ("szInfo", wintypes.WCHAR * 256),
                ("uVersion", wintypes.UINT), ("szInfoTitle", wintypes.WCHAR * 64), ("dwInfoFlags", wintypes.DWORD),
                ("guidItem", GUID), ("hBalloonIcon", wintypes.HICON)]


class WNDCLASSEXW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.UINT), ("style", wintypes.UINT), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int), ("hInstance", wintypes.HINSTANCE), ("hIcon", wintypes.HICON),
                ("hCursor", wintypes.HANDLE), ("hbrBackground", wintypes.HBRUSH), ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR), ("hIconSm", wintypes.HICON)]


u32.DefWindowProcW.restype = LRESULT
u32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u32.RegisterClassExW.restype = wintypes.ATOM
u32.RegisterClassExW.argtypes = [ctypes.POINTER(WNDCLASSEXW)]
u32.CreateWindowExW.restype = wintypes.HWND
u32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD, ctypes.c_int, ctypes.c_int,
                                ctypes.c_int, ctypes.c_int, wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE, wintypes.LPVOID]
u32.DestroyWindow.argtypes = [wintypes.HWND]
u32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
u32.GetMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), wintypes.HWND, wintypes.UINT, wintypes.UINT]
u32.TranslateMessage.argtypes = [ctypes.POINTER(wintypes.MSG)]
u32.DispatchMessageW.argtypes = [ctypes.POINTER(wintypes.MSG)]
u32.DispatchMessageW.restype = LRESULT
u32.LoadImageW.restype = wintypes.HANDLE
u32.LoadImageW.argtypes = [wintypes.HINSTANCE, wintypes.LPCWSTR, wintypes.UINT, ctypes.c_int, ctypes.c_int, wintypes.UINT]
u32.LoadIconW.restype = wintypes.HICON
u32.LoadIconW.argtypes = [wintypes.HINSTANCE, wintypes.LPVOID]
u32.DestroyIcon.argtypes = [wintypes.HICON]
u32.CreatePopupMenu.restype = wintypes.HMENU
u32.AppendMenuW.argtypes = [wintypes.HMENU, wintypes.UINT, ctypes.c_size_t, wintypes.LPCWSTR]
u32.SetMenuDefaultItem.argtypes = [wintypes.HMENU, wintypes.UINT, wintypes.UINT]
u32.TrackPopupMenu.argtypes = [wintypes.HMENU, wintypes.UINT, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.HWND, wintypes.LPVOID]
u32.DestroyMenu.argtypes = [wintypes.HMENU]
u32.SetForegroundWindow.argtypes = [wintypes.HWND]
u32.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
u32.SetTimer.restype = ctypes.c_size_t
u32.SetTimer.argtypes = [wintypes.HWND, ctypes.c_size_t, wintypes.UINT, wintypes.LPVOID]
u32.RegisterWindowMessageW.restype = wintypes.UINT
u32.RegisterWindowMessageW.argtypes = [wintypes.LPCWSTR]
shell32.Shell_NotifyIconW.restype = wintypes.BOOL
shell32.Shell_NotifyIconW.argtypes = [wintypes.DWORD, ctypes.POINTER(NOTIFYICONDATAW)]
kernel32.GetModuleHandleW.restype = wintypes.HMODULE
kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]

class Tray:
    """menu() returns a list of (label, action) or None for a separator."""

    def __init__(self, icon: str | None, tip: str, menu: Callable[[], list], watch: Callable[[], bool] | None = None):
        self.icon_path, self.tip, self.menu, self.watch = icon, tip, menu, watch
        self._hwnd = None
        self._thread: threading.Thread | None = None
        self._ready = threading.Event()
        self._lock = threading.Lock()

    @property
    def shown(self) -> bool:
        return self._thread is not None and self._thread.is_alive()

    def show(self) -> None:
        """Put the icon in the tray (nothing if it's there already)."""
        with self._lock:
            if self.shown:
                return
            self._ready.clear()
            self._thread = threading.Thread(target=self._run, name="glitchwall-tray", daemon=True)
            self._thread.start()
        self._ready.wait(3)

    def hide(self) -> None:
        """Take the icon away. Safe from any thread, and when it isn't there."""
        with self._lock:
            t, h = self._thread, self._hwnd
            if t is None:
                return
            if h:
                u32.PostMessageW(h, WM_CLOSE, 0, 0)
        if t is not threading.current_thread():
            t.join(3)

    def set_tip(self, tip: str) -> None:
        self.tip = tip
        if self._hwnd:
            nid = self._nid(NIF_TIP)
            shell32.Shell_NotifyIconW(NIM_MODIFY, ctypes.byref(nid))

    # ---------- the tray thread
    def _nid(self, flags: int) -> NOTIFYICONDATAW:
        nid = NOTIFYICONDATAW()
        nid.cbSize, nid.hWnd, nid.uID, nid.uFlags = ctypes.sizeof(nid), self._hwnd, 1, flags
        nid.uCallbackMessage, nid.hIcon, nid.szTip = WM_TRAY, self._hicon, self.tip[:127]
        return nid

    def _add(self) -> None:
        nid = self._nid(NIF_MESSAGE | NIF_ICON | NIF_TIP)
        shell32.Shell_NotifyIconW(NIM_ADD, ctypes.byref(nid))

    def _popup(self) -> None:
        items = list(self.menu() or [])
        if not items:
            return
        m = u32.CreatePopupMenu()
        try:
            actions = {}
            for n, item in enumerate(items, start=1):
                if item is None:
                    u32.AppendMenuW(m, MF_SEPARATOR, 0, None)
                else:
                    u32.AppendMenuW(m, MF_STRING, n, item[0])
                    actions[n] = item[1]
            first = next((n for n in actions), 0)
            if first:
                u32.SetMenuDefaultItem(m, first, 0)
            pt = wintypes.POINT()
            u32.GetCursorPos(ctypes.byref(pt))
            u32.SetForegroundWindow(self._hwnd)   # without this the menu won't close when you click elsewhere
            cmd = u32.TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, self._hwnd, None)
            u32.PostMessageW(self._hwnd, WM_NULL, 0, 0)
        finally:
            u32.DestroyMenu(m)
        if cmd in actions:
            self._call(actions[cmd])

    def _default(self) -> None:
        first = next((i for i in (self.menu() or []) if i is not None), None)
        if first:
            self._call(first[1])

    @staticmethod
    def _call(fn) -> None:   # off this thread: an action may hide the icon, which waits for this thread to end
        threading.Thread(target=fn, daemon=True).start()

    def _proc(self, hwnd, msg, wp, lp):
        try:
            if msg == WM_TRAY:
                if lp in (WM_RBUTTONUP, WM_CONTEXTMENU):
                    self._popup()
                elif lp == WM_LBUTTONUP:
                    self._default()
                return 0
            if msg == self._taskbar_created:   # Explorer restarted: the tray is new and empty, add the icon again
                self._add()
                return 0
            if msg == WM_TIMER:
                if self.watch and not self.watch():
                    u32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
                return 0
            if msg == WM_CLOSE:
                shell32.Shell_NotifyIconW(NIM_DELETE, ctypes.byref(self._nid(0)))
                u32.DestroyWindow(hwnd)
                return 0
            if msg == WM_DESTROY:
                u32.PostQuitMessage(0)
                return 0
        except Exception:
            pass
        return u32.DefWindowProcW(hwnd, msg, wp, lp)

    def _run(self) -> None:
        hinst = kernel32.GetModuleHandleW(None)
        cls = f"glitchwall-tray-{id(self)}"
        if getattr(self, "_wndproc", None) is None:   # once per Tray: the class and its callback live as long as it does
            self._wndproc = WNDPROC(self._proc)      # (a freed callback under a still-registered class would crash)
            wc = WNDCLASSEXW(cbSize=ctypes.sizeof(WNDCLASSEXW), lpfnWndProc=self._wndproc, hInstance=hinst, lpszClassName=cls)
            u32.RegisterClassExW(ctypes.byref(wc))
        self._taskbar_created = u32.RegisterWindowMessageW("TaskbarCreated")
        hicon, owned = None, False
        if self.icon_path:
            hicon = u32.LoadImageW(None, self.icon_path, IMAGE_ICON, u32.GetSystemMetrics(SM_CXSMICON),
                                   u32.GetSystemMetrics(SM_CYSMICON), LR_LOADFROMFILE)
            owned = bool(hicon)
        self._hicon = hicon or u32.LoadIconW(None, ctypes.c_void_p(32512))   # IDI_APPLICATION
        # a plain hidden top-level window, not a message-only one: the menu needs a window that can come forward
        h = u32.CreateWindowExW(0, cls, "glitchwall tray", 0, 0, 0, 0, 0, None, None, hinst, None)
        with self._lock:
            self._hwnd = h
        try:
            if not h:
                return
            self._add()
            if self.watch:
                u32.SetTimer(h, 1, 500, None)
            self._ready.set()
            msg = wintypes.MSG()
            while u32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                u32.TranslateMessage(ctypes.byref(msg))
                u32.DispatchMessageW(ctypes.byref(msg))
        finally:
            with self._lock:
                self._hwnd = None
                self._thread = None
            self._ready.set()
            if owned:
                u32.DestroyIcon(hicon)
