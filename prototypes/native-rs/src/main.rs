//! glitchwall native renderer prototype, Rust edition: the phosphor theme with Direct2D + DirectWrite, behind the icons.
//!
//! Same picture as ../native/PhosphorNative.cs, so the two can be measured side by side, with one change: the rain is
//! drawn into a buffer in memory and handed to the GPU once per frame, instead of thousands of one-pixel draws.
//! Not a product: one theme, one screen, made-up events, Consolas, no card blur, Windows 11 24H2+ desktop layer only.
//!
//! Build: cargo build --release      Run: target\release\phosphor-native.exe [--peek]
#![windows_subsystem = "windows"]

use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;


const SCALE: i32 = 3;
const GLASS: f32 = 0.58;
const CPS: f64 = 30.0; // type speed 5 as the page reckons it
const MAXCARDS: usize = 4;

const fn hex(v: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((v >> 16) & 255) as f32 / 255.0, g: ((v >> 8) & 255) as f32 / 255.0, b: (v & 255) as f32 / 255.0, a }
}
const BG: u32 = 0x04110a;
const SURFACE: u32 = 0x0a1d12;
const INK: u32 = 0x8cffb2;
const DIM: u32 = 0x1f6b40;
const EDGE: u32 = 0x357a51;

const EVENTS: [(&str, &str, &str); 8] = [
    ("orbit", "git status -sb", "## main...origin/main\n M src/api/routes.py\n?? tests/test_totals.py"),
    ("orbit", "pytest -q tests/test_totals.py", "....                                   [100%]\n4 passed in 0.41s"),
    ("orbit", "read  src/api/routes.py", "def list_orders(req):\n    rows = db.orders.filter(user=req.user)\n    return {\"items\": rows, \"total\": len(rows)}"),
    ("lumen", "npm run build", "vite v6.2.0 building for production...\n✓ 214 modules transformed.\ndist/index.html  0.46 kB"),
    ("lumen", "edit  src/components/Chart.tsx", "-  const max = Math.max(...data)\n+  const max = Math.max(1, ...data)"),
    ("orbit", "grep -rn TODO src/", "src/jobs/sync.py:12: # TODO retry on 429\nsrc/api/auth.py:41: # TODO rotate session keys"),
    ("kiln", "cargo test --quiet", "running 18 tests\n..................\ntest result: ok. 18 passed; 0 failed"),
    ("kiln", "write  docs/notes.md", "- [x] totals in list endpoint\n- [ ] retry on 429"),
];

/// xorshift: plenty random for rain
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn f(&mut self) -> f32 { (self.next() >> 40) as f32 / (1u64 << 24) as f32 }
}

struct Card { proj: &'static str, text: Vec<u16>, cmd_len: usize, typed: usize, x: f32, y: f32, born: f64, done: f64, dying: f64 }

struct App {
    w: i32, h: i32, lw: usize, lh: usize,
    rt: ID2D1HwndRenderTarget, brush: ID2D1SolidColorBrush,
    rain_bmp: ID2D1Bitmap, overlay: ID2D1Bitmap, mono: IDWriteTextFormat, bar: IDWriteTextFormat,
    buf: Vec<u32>, col_y: Vec<f32>, col_v: Vec<f32>, rng: Rng,
    cards: Vec<Card>, next_event: f64, ev: usize,
}

extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == WM_DESTROY { PostQuitMessage(0); return LRESULT(0); }
        DefWindowProcW(h, m, w, l)
    }
}

/// the same trick as winhost.py (Windows 11 24H2 and later): a child of Progman, just under the icons
unsafe fn behind_icons(hwnd: HWND, w: i32, h: i32) -> Result<()> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null())?;
        let mut res = 0usize;
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SEND_MESSAGE_TIMEOUT_FLAGS(0), 1000, Some(&mut res));
        let icons = FindWindowExW(progman, HWND::default(), w!("SHELLDLL_DefView"), PCWSTR::null())?;
        let st = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let st = (st | WS_CHILD.0 as isize | WS_DISABLED.0 as isize) & !(WS_POPUP.0 as isize);   // disabled: clicks go to the desktop
        SetWindowLongPtrW(hwnd, GWL_STYLE, st);
        SetParent(hwnd, progman)?;
        let mut pts = [POINT { x: 0, y: 0 }, POINT { x: w, y: h }];
        MapWindowPoints(HWND::default(), progman, &mut pts);
        SetWindowPos(hwnd, icons, pts[0].x, pts[0].y, w, h, SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE)
    }
}

impl App {
    unsafe fn new(hwnd: HWND, w: i32, h: i32) -> Result<App> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_UNKNOWN, alphaMode: D2D1_ALPHA_MODE_UNKNOWN },
                dpiX: 96.0, dpiY: 96.0, usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd, pixelSize: D2D_SIZE_U { width: w as u32, height: h as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };
            let rt = d2d.CreateHwndRenderTarget(&props, &hp)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = rt.CreateSolidColorBrush(&hex(INK, 1.0), None)?;

            let (lw, lh) = ((w / SCALE) as usize, (h / SCALE) as usize);
            let opaque = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE }, dpiX: 96.0, dpiY: 96.0,
            };
            let rain_bmp = rt.CreateBitmap(D2D_SIZE_U { width: lw as u32, height: lh as u32 }, None, 0, &opaque)?;

            // the tube: scanlines every 3 px, vignette, grain, made once (black with alpha, premultiplied)
            let mut rng = Rng(0x9e3779b97f4a7c15);
            let mut ov = vec![0u32; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let dx = (x as f32 - w as f32 / 2.0) / (w as f32 / 2.0);
                    let dy = (y as f32 - h as f32 / 2.0) / (h as f32 / 2.0);
                    let mut a = ((dx * dx + dy * dy - 0.45) * 0.55).clamp(0.0, 1.0);
                    if y % 3 == 0 { a = a.max(0.22); }
                    a = (a + rng.f() * 0.06).min(1.0);
                    ov[(y * w + x) as usize] = ((a * 255.0) as u32) << 24;
                }
            }
            let premul = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0,
            };
            let overlay = rt.CreateBitmap(D2D_SIZE_U { width: w as u32, height: h as u32 }, Some(ov.as_ptr() as _), (w * 4) as u32, &premul)?;

            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mono = dw.CreateTextFormat(w!("Consolas"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 16.0, w!("en-us"))?;
            mono.SetWordWrapping(DWRITE_WORD_WRAPPING_CHARACTER)?;
            let bar = dw.CreateTextFormat(w!("Consolas"), None, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 12.0, w!("en-us"))?;

            let cols = lw / 6;
            let mut col_y = Vec::with_capacity(cols);
            let mut col_v = Vec::with_capacity(cols);
            for _ in 0..cols { col_y.push(-rng.f() * lh as f32); col_v.push(4.0 + rng.f() * 7.0); }
            let bgpx = 0xff00_0000 | BG;
            Ok(App { w, h, lw, lh, rt, brush, rain_bmp, overlay, mono, bar, buf: vec![bgpx; lw * lh], col_y, col_v, rng,
                     cards: Vec::new(), next_event: 1.0, ev: 0 })
        }
    }

    /// the page's "pixel" background, intensity 5, into the buffer: fade toward the background, then fresh glyphs
    fn rain(&mut self, dt: f32) {
        let (br, bgc, bb) = ((BG >> 16) & 255, (BG >> 8) & 255, BG & 255);
        for p in self.buf.iter_mut() {   // 18% toward the background each frame, like the page's translucent fill
            let (r, g, b) = ((*p >> 16) & 255, (*p >> 8) & 255, *p & 255);
            *p = 0xff00_0000 | ((r * 210 + br * 46) >> 8) << 16 | ((g * 210 + bgc * 46) >> 8) << 8 | ((b * 210 + bb * 46) >> 8);
        }
        let (lw, lh) = (self.lw as i32, self.lh as i32);
        for i in 0..self.col_y.len() {
            self.col_y[i] += self.col_v[i] * dt * 9.0;
            if self.col_y[i] > (lh + 40) as f32 { self.col_y[i] = -self.rng.f() * 60.0; self.col_v[i] = 4.0 + self.rng.f() * 7.0; }
            if self.rng.f() > 0.7 { continue; }
            let color = 0xff00_0000 | if self.rng.f() < 0.08 { INK } else { DIM };
            let (x, y) = (i as i32 * 6, (self.col_y[i] / 9.0).floor() as i32 * 9);
            for r in 0..7 {
                let row = self.rng.next() & 31;
                let yy = y + r;
                if yy < 0 || yy >= lh { continue; }
                for b in 0..5 {
                    if row >> b & 1 == 1 && x + b < lw { self.buf[(yy * lw + x + b) as usize] = color; }
                }
            }
        }
    }

    fn color(&self, c: D2D1_COLOR_F) { unsafe { self.brush.SetColor(&c) } }

    unsafe fn frame(&mut self, now: f64, dt: f32) -> Result<()> {
        self.rain(dt);
        unsafe {
            self.rain_bmp.CopyFromMemory(None, self.buf.as_ptr() as _, (self.lw * 4) as u32)?;
            self.rt.BeginDraw();
            let full = D2D_RECT_F { left: 0.0, top: 0.0, right: self.w as f32, bottom: self.h as f32 };
            self.rt.DrawBitmap(&self.rain_bmp, Some(&full), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None);
            self.cards(now);
            self.rt.DrawBitmap(&self.overlay, Some(&full), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            self.rt.EndDraw(None, None)
        }
    }

    unsafe fn cards(&mut self, now: f64) {
        if now > self.next_event {
            let (proj, cmd, out) = EVENTS[self.ev % EVENTS.len()];
            self.ev += 1;
            if self.cards.len() >= MAXCARDS && self.cards[0].dying < 0.0 { self.cards[0].dying = now; }
            let slot = self.ev % MAXCARDS;
            let text: Vec<u16> = format!("> {cmd}\n{out}").encode_utf16().collect();
            self.cards.push(Card {
                proj, text, cmd_len: cmd.len() + 2, typed: 0, born: now, done: -1.0, dying: -1.0,
                x: 120.0 + (slot % 2) as f32 * 620.0 + self.rng.f() * 80.0, y: 140.0 + (slot / 2) as f32 * 330.0 + self.rng.f() * 60.0,
            });
            self.next_event = now + 3.5 + self.rng.f() as f64 * 3.0;
        }
        self.cards.retain(|c| !(c.dying >= 0.0 && now - c.dying > 0.5));
        let (cw, ch) = (460.0f32, 170.0f32);
        let cursor: Vec<u16> = "█".encode_utf16().collect();
        for i in 0..self.cards.len() {
            let c = &mut self.cards[i];
            if c.typed < c.text.len() {
                let speed = if c.typed < c.cmd_len { 1.0 } else { 5.0 };
                c.typed = (((now - c.born - 0.26) * CPS * speed).max(0.0) as usize + 1).min(c.text.len());
            } else if c.done < 0.0 { c.done = now; }
            if c.done >= 0.0 && c.dying < 0.0 && now - c.done > 7.0 { c.dying = now; }
            let mut k = ((now - c.born) / 0.35).min(1.0) as f32;   // arrive: a CRT line opening up
            if c.dying >= 0.0 { k = (1.0 - (now - c.dying) / 0.5).max(0.0) as f32; }
            let (x, hh) = (c.x, ch * k);
            let top = c.y + (ch - hh) / 2.0;
            let mut shown = c.text[..c.typed].to_vec();
            if c.done < 0.0 && (now * 2.0) as i64 % 2 == 0 { shown.extend_from_slice(&cursor); }
            let proj: Vec<u16> = format!("{} #a1f3", c.proj).encode_utf16().collect();
            let c_box = D2D_RECT_F { left: x, top, right: x + cw, bottom: top + hh };
            unsafe {
                self.color(hex(SURFACE, GLASS)); self.rt.FillRectangle(&c_box, &self.brush);
                self.color(hex(EDGE, 0.9)); self.rt.DrawRectangle(&c_box, &self.brush, 1.0, None);
                if k < 0.9 { continue; }
                let bar = D2D_RECT_F { left: x, top, right: x + cw, bottom: top + 22.0 };
                self.color(hex(DIM, 0.55)); self.rt.FillRectangle(&bar, &self.brush);
                let bt = D2D_RECT_F { left: x + 10.0, top: top + 4.0, right: x + cw - 10.0, bottom: top + 20.0 };
                self.color(hex(INK, 1.0));
                self.rt.DrawText(&proj, &self.bar, &bt, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                let area = D2D_RECT_F { left: x + 12.0, top: top + 30.0, right: x + cw - 12.0, bottom: top + hh - 10.0 };
                self.color(hex(INK, 0.16));   // glow: soft offset copies under the crisp text
                for g in 0..4 {
                    let (ox, oy) = (if g % 2 == 0 { -1.5 } else { 1.5 }, if g < 2 { -1.5 } else { 1.5 });
                    let ga = D2D_RECT_F { left: area.left + ox, top: area.top + oy, right: area.right + ox, bottom: area.bottom + oy };
                    self.rt.DrawText(&shown, &self.mono, &ga, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
                self.color(hex(INK, 1.0));
                self.rt.DrawText(&shown, &self.mono, &area, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
    }
}

fn main() -> Result<()> {
    let peek = std::env::args().any(|a| a == "--peek");
    let fps: f64 = std::env::args().skip_while(|a| a != "--fps").nth(1).and_then(|v| v.parse().ok()).unwrap_or(30.0);   // a normal window for a few seconds, to look at it
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let inst = GetModuleHandleW(None)?;
        let class = w!("glitchwall-native");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: class, ..Default::default() });
        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let (x, y, ww, wh, ex) = if peek { (w - 980, h - 620, 960, 560, WS_EX_TOPMOST | WS_EX_TOOLWINDOW) } else { (-32000, -32000, w, h, WS_EX_TOOLWINDOW) };
        let hwnd = CreateWindowExW(ex, class, w!("glitchwall native prototype"), WS_POPUP, x, y, ww, wh, HWND::default(), HMENU::default(), inst, None)?;
        if peek { let _ = ShowWindow(hwnd, SW_SHOWNA); } else { behind_icons(hwnd, w, h)?; }
        let mut app = App::new(hwnd, w, h)?;

        let timer = CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)?;
        let clock = Instant::now();
        let mut last = 0.0f64;
        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT { return Ok(()); }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let t0 = clock.elapsed().as_secs_f64();
            let dt = (t0 - last).min(0.1) as f32;
            last = t0;
            let _ = app.frame(t0, dt);
            let rest = 1.0 / fps - (clock.elapsed().as_secs_f64() - t0);
            if rest > 0.0 {
                let due = -((rest * 1e7) as i64);
                SetWaitableTimer(timer, &due, 0, None, None, false)?;
                MsgWaitForMultipleObjects(Some(&[timer]), false, 1000, QS_ALLINPUT);
            }
        }
    }
}
