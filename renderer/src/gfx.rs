//! The drawing surface: a Direct3D 11 device, a flip-model swap chain on the window, and a Direct2D 1.1 device
//! context drawing into it. 1.1 (not the older HwndRenderTarget) because the cards need its effects: blur for the
//! glass, shadow for the glow, saturation and a color matrix for the rest of the CSS filters.
//! Display scaling: the context works in DIPs at the screen's DPI, so every size the page gives in CSS pixels (fonts,
//! padding, blur radii) comes out the size the browser would draw it. `w`/`h` are the screen in DIPs.
use crate::looks::Color;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::*;

pub struct Gfx {
    pub dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    pub back: ID2D1Bitmap1,
    pub w: u32,
    pub h: u32,
    pub scale: f32,   // device pixels per DIP: 1.5 at 150% display scaling
}

pub fn rgba(c: Color) -> D2D1_COLOR_F { D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: c[3] } }
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F { D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h } }

impl Gfx {
    pub fn new(hwnd: HWND, w: u32, h: u32, scale: f32) -> Result<Gfx> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
            let made = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                         Some(&levels), D3D11_SDK_VERSION, Some(&mut device), None, None);
            if made.is_err() {   // no usable GPU (a VM, a remote session): WARP, the software rasterizer
                D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                  Some(&levels), D3D11_SDK_VERSION, Some(&mut device), None, None)?;
            }
            let device = device.unwrap();
            let dxgi: IDXGIDevice = device.cast()?;
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = factory.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let dxgi_factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w, Height: h, Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT, BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, AlphaMode: DXGI_ALPHA_MODE_IGNORE, Scaling: DXGI_SCALING_NONE,
                ..Default::default()
            };
            let swap = dxgi_factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None)?;
            let _ = dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
            // in flip model, buffer 0 is always the one to draw into
            let surface: IDXGISurface = swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0 * scale, dpiY: 96.0 * scale, bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };
            let back = dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            dc.SetDpi(96.0 * scale, 96.0 * scale);
            let (dw, dh) = ((w as f32 / scale).round() as u32, (h as f32 / scale).round() as u32);
            Ok(Gfx { dc, swap, back, w: dw, h: dh, scale })
        }
    }

    /// an offscreen surface to draw into, `w` x `h` DIPs, at the screen's resolution (drawn back at its own size)
    pub fn surface(&self, w: f32, h: f32) -> Result<ID2D1Bitmap1> {
        let (pw, ph) = (((w * self.scale).ceil() as u32).max(1), ((h * self.scale).ceil() as u32).max(1));
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * self.scale, dpiY: 96.0 * self.scale, bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe { self.dc.CreateBitmap(D2D_SIZE_U { width: pw, height: ph }, None, 0, &props) }
    }

    /// a raw bitmap of `w` x `h` pixels (premultiplied BGRA), filled from `data`: draw it with a destination rectangle
    pub fn bitmap(&self, w: u32, h: u32, data: Option<&[u32]>, target: bool) -> Result<ID2D1Bitmap1> {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0, dpiY: 96.0, bitmapOptions: if target { D2D1_BITMAP_OPTIONS_TARGET } else { D2D1_BITMAP_OPTIONS_NONE },
            colorContext: ManuallyDrop::new(None),
        };
        unsafe {
            self.dc.CreateBitmap(D2D_SIZE_U { width: w.max(1), height: h.max(1) }, data.map(|d| d.as_ptr() as *const _),
                                 w.max(1) * 4, &props)
        }
    }

    pub fn effect(&self, id: &GUID) -> Result<ID2D1Effect> { unsafe { self.dc.CreateEffect(id) } }

    pub fn brush(&self, c: Color) -> Result<ID2D1SolidColorBrush> { unsafe { self.dc.CreateSolidColorBrush(&rgba(c), None) } }

    pub fn present(&self) -> Result<()> { unsafe { self.swap.Present(0, DXGI_PRESENT(0)).ok() } }
}

pub fn set_f32(e: &ID2D1Effect, i: u32, v: f32) -> Result<()> {
    unsafe { e.SetValue(i, D2D1_PROPERTY_TYPE_FLOAT, &v.to_le_bytes()) }
}
pub fn set_bool(e: &ID2D1Effect, i: u32, v: bool) -> Result<()> {
    unsafe { e.SetValue(i, D2D1_PROPERTY_TYPE_BOOL, &(v as u32).to_le_bytes()) }
}
pub fn set_floats(e: &ID2D1Effect, i: u32, kind: D2D1_PROPERTY_TYPE, v: &[f32]) -> Result<()> {
    let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
    unsafe { e.SetValue(i, kind, &bytes) }
}
