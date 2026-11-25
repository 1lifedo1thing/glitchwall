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
pub fn monitors() -> Vec<(u32, Rect)> { screens(false) }

/// the same, each screen's work area: the screen minus the taskbar (winhost.work_areas)
pub fn work_areas() -> Vec<(u32, Rect)> { screens(true) }

fn screens(work: bool) -> Vec<(u32, Rect)> {
    use windows::Win32::Graphics::Gdi::*;
    unsafe extern "system" fn one(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let (out, work) = &mut *(data.0 as *mut (Vec<(bool, u32, Rect)>, bool));
            let mut mi = MONITORINFOEXW::default();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO).as_bool() {
                let name = String::from_utf16_lossy(&mi.szDevice);
                let name = name.trim_end_matches('\0');
                if let Some(n) = name.rsplit("DISPLAY").next().and_then(|d| d.parse().ok()) {
                    let r = if *work { mi.monitorInfo.rcWork } else { mi.monitorInfo.rcMonitor };
                    out.push((mi.monitorInfo.dwFlags & 1 == 0, n, Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }));
                }
            }
            TRUE
        }
    }
    let mut found: (Vec<(bool, u32, Rect)>, bool) = (Vec::new(), work);
    unsafe { let _ = EnumDisplayMonitors(HDC::default(), None, Some(one), LPARAM(&mut found as *mut _ as isize)); }
    found.0.sort_by_key(|f| (f.0, f.1));
    found.0.into_iter().map(|(_, n, r)| (n, r)).collect()
}

// ---------------------------------------------------------------- who can see the wallpaper (winhost.py, piece 3a)
/// windows that are the desktop itself or the taskbar: they never hide the wallpaper
const DESKTOP_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// a window's frame (left, top, right, bottom) covers the whole area
pub fn covers(f: (i32, i32, i32, i32), a: Rect) -> bool { f.0 <= a.x && f.1 <= a.y && f.2 >= a.x + a.w && f.3 >= a.y + a.h }

/// the desktop itself or the taskbar
pub fn is_desktop(h: HWND) -> bool { DESKTOP_CLASSES.contains(&class_of(h).as_str()) }

fn class_of(h: HWND) -> String {
    let mut b = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

/// a window as you see it: without the invisible resize borders (a maximized window's is exactly the work area)
fn frame(h: HWND) -> (i32, i32, i32, i32) {
    use windows::Win32::Graphics::Dwm::*;
    let mut r = RECT::default();
    unsafe {
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_err() {
            let _ = GetWindowRect(h, &mut r);
        }
    }
    (r.left, r.top, r.right, r.bottom)
}

/// cloaked (another virtual desktop, a suspended app), click-through, or faded: doesn't hide what's behind
fn see_through(h: HWND) -> bool {
    use windows::Win32::Graphics::Dwm::*;
    unsafe {
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok() && cloaked != 0 { return true; }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 { return true; }   // an overlay
        if ex & WS_EX_LAYERED.0 != 0 {   // opaque unless it set a see-through alpha
            let (mut alpha, mut flags) = (0u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            if GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags)).is_ok() && flags.0 & LWA_ALPHA.0 != 0 && alpha < 255 {
                return true;
            }
        }
        false
    }
}

/// Screens whose desktop can't be seen: an opaque window (maximized, fullscreen) fills the whole work area
/// (winhost.covered_screens). Nobody would see the wallpaper there, so it can stop animating.
pub fn covered(work: &[(u32, Rect)]) -> Vec<u32> {
    struct S<'a> { work: &'a [(u32, Rect)], out: Vec<u32>, me: u32 }
    unsafe extern "system" fn one(h: HWND, data: LPARAM) -> BOOL {
        unsafe {
            let s = &mut *(data.0 as *mut S);
            if s.out.len() == s.work.len() { return FALSE; }   // every screen covered: done
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || DESKTOP_CLASSES.contains(&class_of(h).as_str()) { return TRUE; }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == s.me { return TRUE; }
            let mut f = None;
            for (n, area) in s.work {
                if !s.out.contains(n) {
                    let fr = *f.get_or_insert_with(|| frame(h));
                    if covers(fr, *area) && !see_through(h) { s.out.push(*n); }
                }
            }
            TRUE
        }
    }
    let mut s = S { work, out: Vec::new(), me: std::process::id() };
    unsafe { let _ = EnumWindows(Some(one), LPARAM(&mut s as *mut _ as isize)); }
    s.out
}

/// the screen is locked (or on a UAC prompt) or the screen saver runs: no wallpaper is visible anywhere
pub fn session_hidden() -> bool {
    use windows::Win32::System::StationsAndDesktops::*;
    unsafe {
        let mut running = BOOL(0);
        if SystemParametersInfoW(SPI_GETSCREENSAVERRUNNING, 0, Some(&mut running as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok()
            && running.as_bool() { return true; }
        let Ok(d) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x0001)) else { return true };   // fails while locked
        let mut name = [0u16; 64];
        let ok = GetUserObjectInformationW(HANDLE(d.0), UOI_NAME, Some(name.as_mut_ptr() as *mut _), (name.len() * 2) as u32, None).is_ok();
        let _ = CloseDesktop(d);
        let n = String::from_utf16_lossy(&name);
        ok && !n.trim_end_matches('\0').eq_ignore_ascii_case("default")
    }
}

/// which of these screens nobody can see right now
pub fn asleep(ours: &[u32]) -> Vec<u32> {
    if session_hidden() { return ours.to_vec(); }
    let work: Vec<(u32, Rect)> = work_areas().into_iter().filter(|(n, _)| ours.contains(n)).collect();
    covered(&work)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn covers_like_winhost() {   // winhost.covers
        let a = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        assert!(covers((0, 0, 1920, 1040), a));
        assert!(covers((-8, -8, 1928, 1048), a));   // a maximized window's frame can overhang
        assert!(!covers((0, 0, 1919, 1040), a));
        assert!(!covers((1, 0, 1920, 1040), a));
        assert!(!covers((1920, 0, 3840, 1040), a));   // the next screen
        assert!(covers((0, 0, 3840, 1080), Rect { x: 1920, y: 0, w: 1920, h: 1040 }));   // spanning both
    }
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
        if m == WM_DESTROY {
            if !REBUILDING.load(std::sync::atomic::Ordering::Relaxed) { PostQuitMessage(0); }
            return LRESULT(0);
        }
        DefWindowProcW(h, m, w, l)
    }
}

static REBUILDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Close a screen's window because the screens changed (host): unlike a window closed any other way, it does not end
/// the renderer.
pub fn close(hwnd: HWND) {
    REBUILDING.store(true, std::sync::atomic::Ordering::Relaxed);
    unsafe { let _ = DestroyWindow(hwnd); }
    REBUILDING.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// a window for one screen: behind the icons, or (peek) a small topmost window to look at while developing
pub fn window(r: Rect, peek: Option<(u32, u32)>) -> Result<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None)?;
        let class = w!("glitchwall-render");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() });
        let (x, y, w, h, ex) = if let Some((pw, ph)) = peek {
            let (pw, ph) = (pw as i32, ph as i32);
            ((r.x + r.w - pw - 20).max(r.x), (r.y + r.h - ph - 60).max(r.y), pw, ph, WS_EX_TOPMOST | WS_EX_TOOLWINDOW)
        } else { (r.x, r.y, r.w, r.h, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) };
        let hwnd = CreateWindowExW(ex, class, w!("glitchwall"), WS_POPUP, x, y, w, h, HWND::default(), HMENU::default(), inst, None)?;
        if peek.is_some() { let _ = ShowWindow(hwnd, SW_SHOWNA); } else { behind_icons(hwnd, r)?; }
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
