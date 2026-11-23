//! The desktop side: a window parented behind the icons (Windows 11 24H2 and later: a child of Progman, just under
//! SHELLDLL_DefView, the same trick as glitchwall/winhost.py), the message pump, and a precise frame timer.
use std::sync::OnceLock;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

#[derive(Clone, Copy, Debug)]
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }

pub fn dpi_aware() { unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); } }

/// screen number (\\.\DISPLAY<n>, as winhost.py numbers them) and its rectangle, primary first
pub fn monitors() -> Vec<(u32, Rect)> {
    use windows::Win32::Graphics::Gdi::*;
    unsafe extern "system" fn one(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let out = &mut *(data.0 as *mut Vec<(bool, u32, Rect)>);
            let mut mi = MONITORINFOEXW::default();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO).as_bool() {
                let name = String::from_utf16_lossy(&mi.szDevice);
                let name = name.trim_end_matches('\0');
                if let Some(n) = name.rsplit("DISPLAY").next().and_then(|d| d.parse().ok()) {
                    let r = mi.monitorInfo.rcMonitor;
                    out.push((mi.monitorInfo.dwFlags & 1 == 0, n, Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }));
                }
            }
            TRUE
        }
    }
    let mut found: Vec<(bool, u32, Rect)> = Vec::new();
    unsafe { let _ = EnumDisplayMonitors(HDC::default(), None, Some(one), LPARAM(&mut found as *mut _ as isize)); }
    found.sort_by_key(|f| (f.0, f.1));
    found.into_iter().map(|(_, n, r)| (n, r)).collect()
}

impl PartialEq for Rect { fn eq(&self, o: &Rect) -> bool { (self.x, self.y, self.w, self.h) == (o.x, o.y, o.w, o.h) } }

/// still behind the icons? Explorer restarting makes a new desktop layer and orphans the window: put it back
pub fn keep_behind_icons(hwnd: HWND, r: Rect) {
    unsafe {
        let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) else { return };
        if GetParent(hwnd).map(|p| p != progman).unwrap_or(true) { let _ = behind_icons(hwnd, r); }
    }
}

extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == WM_DESTROY { PostQuitMessage(0); return LRESULT(0); }
        DefWindowProcW(h, m, w, l)
    }
}

/// a window for one screen: behind the icons, or (peek) a small topmost window to look at while developing
pub fn window(r: Rect, peek: bool) -> Result<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None)?;
        let class = w!("glitchwall-render");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() });
        let (x, y, w, h, ex) = if peek { (r.x + r.w - 980, r.y + r.h - 620, 960, 560, WS_EX_TOPMOST | WS_EX_TOOLWINDOW) }
                               else { (r.x, r.y, r.w, r.h, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) };
        let hwnd = CreateWindowExW(ex, class, w!("glitchwall"), WS_POPUP, x, y, w, h, HWND::default(), HMENU::default(), inst, None)?;
        if peek { let _ = ShowWindow(hwnd, SW_SHOWNA); } else { behind_icons(hwnd, r)?; }
        Ok(hwnd)
    }
}

fn behind_icons(hwnd: HWND, r: Rect) -> Result<()> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null())?;
        let mut res = 0usize;
        // ask Explorer to make its wallpaper layer (0x052C, the message wallpaper apps use)
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, Some(&mut res));
        let icons = FindWindowExW(progman, HWND::default(), w!("SHELLDLL_DefView"), PCWSTR::null())?;
        let st = GetWindowLongPtrW(hwnd, GWL_STYLE);
        // a disabled child: clicks fall through to the desktop
        SetWindowLongPtrW(hwnd, GWL_STYLE, (st | WS_CHILD.0 as isize | WS_DISABLED.0 as isize) & !(WS_POPUP.0 as isize));
        SetParent(hwnd, progman)?;
        let mut pts = [POINT { x: r.x, y: r.y }, POINT { x: r.x + r.w, y: r.y + r.h }];
        MapWindowPoints(HWND::default(), progman, &mut pts);
        SetWindowPos(hwnd, icons, pts[0].x, pts[0].y, r.w, r.h, SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE)
    }
}

/// handle waiting messages; false once the window is gone
pub fn pump() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT { return false; }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        true
    }
}

struct Timer(HANDLE);
unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}
static TIMER: OnceLock<Timer> = OnceLock::new();

/// sleep (waking for window messages) until `at` seconds on `clock`, with a high-resolution timer
pub fn sleep_until(at: f64, clock: &Instant) {
    let rest = at - clock.elapsed().as_secs_f64();
    if rest <= 0.0 { return; }
    unsafe {
        let t = TIMER.get_or_init(|| Timer(CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                                                                  TIMER_ALL_ACCESS.0).unwrap_or_default()));
        let due = -((rest * 1e7) as i64);
        if SetWaitableTimer(t.0, &due, 0, None, None, false).is_ok() {
            MsgWaitForMultipleObjects(Some(&[t.0]), false, 1000, QS_ALLINPUT);
        }
    }
}

/// true while process `pid` is still running
pub fn alive(pid: u32) -> bool {
    use windows::Win32::System::Threading::*;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false };
        let gone = WaitForSingleObject(h, 0) == windows::Win32::Foundation::WAIT_OBJECT_0;
        let _ = windows::Win32::Foundation::CloseHandle(h);
        !gone
    }
}

/// the display scaling of the screen at `r` (1.0 at 100%, 1.5 at 150%), as Windows sets it for that monitor
pub fn scale(r: Rect) -> f32 {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let m = MonitorFromRect(&RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }, MONITOR_DEFAULTTONEAREST);
        let (mut x, mut y) = (96u32, 96u32);
        if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_err() { return 1.0; }
        (x as f32 / 96.0).clamp(1.0, 4.0)
    }
}
