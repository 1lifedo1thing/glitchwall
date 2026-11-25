//! `glitchwall-render settings` (piece 5, the settings window in Rust). 5a, the spike: a plain window showing
//! settings.html from the running feed in WebView2, through the webview2-com crate. Not yet: settingswin.py's
//! size rules, the page's fade/keep/feedback calls, docking, starting the feed (5b-5e).
//! WebView2Loader.dll (Microsoft's, beside the exe) is loaded only here, when the window opens: under the GNU
//! toolchain the loader can only be linked as that DLL, and a load-time link would stop the whole exe (the
//! wallpaper too) from starting where it is missing.
use crate::server::state_dir;
use std::cell::RefCell;
use std::sync::mpsc;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::CreateCoreWebView2EnvironmentCompletedHandler;
use webview2_com::CreateCoreWebView2ControllerCompletedHandler;
use windows::core::{HSTRING, Interface, PCWSTR, w};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::*;

type CreateEnv = unsafe extern "system" fn(PCWSTR, PCWSTR, *mut core::ffi::c_void, *mut core::ffi::c_void) -> windows::core::HRESULT;

thread_local! { static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) }; }

fn fit(hwnd: HWND) {
    let mut r = RECT::default();
    unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r); }
    CONTROLLER.with(|c| if let Some(c) = c.borrow().as_ref() { unsafe { let _ = c.SetBounds(r); } });
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => { fit(h); LRESULT(0) }
        WM_DESTROY => { unsafe { PostQuitMessage(0) }; LRESULT(0) }
        _ => unsafe { DefWindowProcW(h, msg, w, l) },
    }
}

fn loader() -> Result<CreateEnv, String> {
    let dir = std::env::current_exe().map_err(|e| e.to_string())?.parent().map(|d| d.join("WebView2Loader.dll")).unwrap_or_default();
    unsafe {
        let m = LoadLibraryW(&HSTRING::from(dir.as_os_str())).map_err(|_| format!("{} is missing", dir.display()))?;
        let f = GetProcAddress(m, windows::core::s!("CreateCoreWebView2EnvironmentWithOptions")).ok_or("WebView2Loader.dll has no CreateCoreWebView2EnvironmentWithOptions")?;
        Ok(std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateEnv>(f))
    }
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let create = loader()?;
    let schema = crate::settings::Schema::from_file(&crate::server::wallpaper_dir(_args).join("looks.js"))?;
    let port = crate::cli::load(&schema).get("port").num().unwrap_or(crate::settings::DEFAULT_PORT as f64) as u16;
    let key = std::fs::read_to_string(state_dir().join("key")).map_err(|_| "no feed key yet: run glitchwall once to make one")?;
    let url = format!("http://127.0.0.1:{port}/settings.html#key={}", key.trim());
    // no calls home from WebView2, as winhost.quiet_webview
    unsafe { std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-features=ElasticOverscroll --disable-background-networking --disable-component-update --disable-sync --no-pings"); }
    crate::desk::dpi_aware();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("glitchwall-settings");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class,
                             hbrBackground: windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(0x00010401)), ..Default::default() };
        RegisterClassW(&wc);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE::default(), class, w!("glitchwall settings"), style, CW_USEDEFAULT, CW_USEDEFAULT,
                                   560, 1000, HWND::default(), HMENU::default(), inst, None).map_err(|e| e.to_string())?;
        let _ = ShowWindow(hwnd, SW_SHOW);

        let data = HSTRING::from(state_dir().join("webview-settings-rs").as_os_str());
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |h| create(PCWSTR::null(), PCWSTR(data.as_ptr()), std::ptr::null_mut(), h.as_raw()).ok().map_err(webview2_com::Error::WindowsError)),
            Box::new(move |e, env| { e?; tx.send(env.ok_or_else(|| windows::core::Error::from(E_POINTER))).ok(); Ok(()) }),
        ).map_err(|e| format!("WebView2: {e:?}"))?;
        let env = rx.recv().map_err(|e| e.to_string())?.map_err(|e| format!("WebView2 environment: {e}"))?;
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |h| env.CreateCoreWebView2Controller(hwnd, &h).map_err(webview2_com::Error::WindowsError)),
            Box::new(move |e, c| { e?; tx.send(c.ok_or_else(|| windows::core::Error::from(E_POINTER))).ok(); Ok(()) }),
        ).map_err(|e| format!("WebView2: {e:?}"))?;
        let controller = rx.recv().map_err(|e| e.to_string())?.map_err(|e| format!("WebView2 controller: {e}"))?;
        let view = controller.CoreWebView2().map_err(|e| e.to_string())?;
        CONTROLLER.with(|c| *c.borrow_mut() = Some(controller.clone()));
        fit(hwnd);
        let _ = controller.SetIsVisible(true);
        view.Navigate(&HSTRING::from(url)).map_err(|e| e.to_string())?;

        let mut m = MSG::default();
        while GetMessageW(&mut m, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        CONTROLLER.with(|c| { if let Some(c) = c.borrow_mut().take() { let _ = c.Close(); } });
    }
    Ok(())
}
