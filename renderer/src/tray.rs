//! A notification-area (system tray) icon with a right-click menu (tray.py, piece 3d). It lives on its own thread
//! with its own hidden window and message loop. `menu` is asked for the items each time the menu opens, so the
//! ticks are current; a left click runs the first item. Explorer restarting (a new, empty tray) adds it again.
use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

pub type Action = Box<dyn Fn() + Send>;

pub enum Item {
    Sep,
    /// label, ticked, what it does
    Act(String, bool, Action),
    Sub(String, Vec<Item>),
}

const WM_TRAY: u32 = WM_APP + 1;
static SHOWN: AtomicIsize = AtomicIsize::new(0);

struct State { menu: Box<dyn Fn() -> Vec<Item>>, icon: HICON, tip: String, taskbar_created: u32 }

thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

/// Take the icon away (from any thread), or it lingers in the tray until the mouse passes over it.
pub fn hide() {
    let h = SHOWN.load(Ordering::SeqCst);
    if h != 0 { unsafe { let _ = PostMessageW(HWND(h as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0)); } }
}

fn nid(h: HWND, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
    let (icon, tip) = STATE.with(|s| s.borrow().as_ref().map(|s| (s.icon, s.tip.clone())).unwrap_or_default());
    let mut n = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: h, uID: 1, uFlags: flags,
                                  uCallbackMessage: WM_TRAY, hIcon: icon, ..Default::default() };
    for (i, c) in tip.encode_utf16().take(127).enumerate() { n.szTip[i] = c; }
    n
}

fn add(h: HWND) { unsafe { let _ = Shell_NotifyIconW(NIM_ADD, &nid(h, NIF_MESSAGE | NIF_ICON | NIF_TIP)); } }

fn items() -> Vec<Item> { STATE.with(|s| s.borrow().as_ref().map(|s| (s.menu)()).unwrap_or_default()) }

/// off this thread: an action may take the icon away, or take a while (writing the settings, starting a program)
fn call(f: Action) { std::thread::spawn(move || f()); }

fn build(items: Vec<Item>, actions: &mut Vec<Action>) -> HMENU {
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return HMENU::default() };
        for it in items {
            match it {
                Item::Sep => { let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null()); }
                Item::Sub(label, sub) => {
                    let s = build(sub, actions);
                    let _ = AppendMenuW(m, MF_POPUP, s.0 as usize, &HSTRING::from(label));
                }
                Item::Act(label, ticked, f) => {
                    actions.push(f);
                    let flags = if ticked { MF_STRING | MF_CHECKED } else { MF_STRING };
                    let _ = AppendMenuW(m, flags, actions.len(), &HSTRING::from(label));
                }
            }
        }
        m
    }
}

fn popup(h: HWND) {
    let items = items();
    if items.is_empty() { return; }
    let first_is_action = matches!(items.first(), Some(Item::Act(..)));
    let mut actions = Vec::new();
    let m = build(items, &mut actions);   // destroying it destroys its submenus too
    let cmd = unsafe {
        if first_is_action { let _ = SetMenuDefaultItem(m, 1, 0); }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(h);   // without this the menu won't close when you click elsewhere
        let cmd = TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, h, None).0;
        let _ = PostMessageW(h, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        cmd
    };
    if cmd > 0 && (cmd as usize) <= actions.len() { call(actions.swap_remove(cmd as usize - 1)); }
}

fn default_item() {
    if let Some(Item::Act(_, _, f)) = items().into_iter().find(|i| matches!(i, Item::Act(..))) { call(f); }
}

extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == WM_TRAY {
            let ev = l.0 as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU { popup(h) } else if ev == WM_LBUTTONUP { default_item() }
            return LRESULT(0);
        }
        if m != 0 && m == STATE.with(|s| s.borrow().as_ref().map_or(0, |s| s.taskbar_created)) { add(h); return LRESULT(0); }
        if m == WM_CLOSE {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid(h, NOTIFY_ICON_DATA_FLAGS(0)));
            let _ = DestroyWindow(h);
            return LRESULT(0);
        }
        if m == WM_DESTROY { SHOWN.store(0, Ordering::SeqCst); PostQuitMessage(0); return LRESULT(0); }
        DefWindowProcW(h, m, w, l)
    }
}

/// Shows the icon and runs its message loop until it is taken away (hide). Call on a thread of its own.
pub fn run(icon: &Path, tip: &str, menu: impl Fn() -> Vec<Item> + 'static) {
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let class = w!("glitchwall-tray");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() });
        let loaded = LoadImageW(None, &HSTRING::from(icon.as_os_str()), IMAGE_ICON, GetSystemMetrics(SM_CXSMICON),
                                GetSystemMetrics(SM_CYSMICON), LR_LOADFROMFILE).ok().map(|i| HICON(i.0));
        let hicon = loaded.or_else(|| LoadIconW(None, IDI_APPLICATION).ok()).unwrap_or_default();
        STATE.with(|s| *s.borrow_mut() = Some(State { menu: Box::new(menu), icon: hicon, tip: tip.into(),
                                                       taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")) }));
        // a plain hidden top-level window, not a message-only one: the menu needs a window that can come forward
        let Ok(h) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("glitchwall tray"), WINDOW_STYLE(0), 0, 0, 0, 0,
                                    None, None, inst, None) else { return };
        SHOWN.store(h.0 as isize, Ordering::SeqCst);
        add(h);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some(i) = loaded { let _ = DestroyIcon(i); }
    }
}
