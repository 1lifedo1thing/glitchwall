//! Backgrounds: each one draws into the full-screen scene bitmap that the cards' glass blurs.
//! Ported so far: the page's "pixel", "cascade" and "broadsheet"; every other background falls back to a plain fill until it is.
use crate::fonts::Fonts;
use crate::gfx::{Gfx, rect, rgba};
use crate::looks::{Color, Look};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;

pub trait Background {
    /// how often it changes; 0 for one that only needs drawing when the look changes
    fn fps(&self) -> f32;
    /// draw into the scene (its BeginDraw is open); `level` is the rain setting, 0-1
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()>;
}

/// called outside any drawing (a background may draw its sprites up front)
pub fn make(name: &str, g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Box<dyn Background>> {
    Ok(match name {
        "pixel" => Box::new(Pixel::new(g)?),
        "cascade" => Box::new(Cascade::new(g, fonts, &look.mono.family)?),
        "broadsheet" => Box::new(Broadsheet::new(g, fonts, look)?),
        _ => Box::new(Plain),
    })
}

pub struct Plain;
impl Background for Plain {
    fn fps(&self) -> f32 { 0.0 }
    fn draw(&mut self, g: &Gfx, look: &Look, _: f32, _: f32) -> Result<()> {
        unsafe { g.dc.Clear(Some(&rgba(look.bg))); }
        Ok(())
    }
}

/// xorshift: plenty random for rain and grain
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    pub fn f(&mut self) -> f32 { (self.next() >> 40) as f32 / (1u64 << 24) as f32 }
    pub fn range(&mut self, a: f32, b: f32) -> f32 { a + self.f() * (b - a) }
}

fn px(c: Color) -> u32 {
    0xff00_0000 | ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32
}

/// tiny random 5x7 bit-glyphs falling in columns, at a third of the resolution, upscaled with hard edges.
/// Drawn into memory and handed to the GPU once a frame.
pub struct Pixel { lw: usize, lh: usize, buf: Vec<u32>, small: ID2D1Bitmap1, cols: Vec<(f32, f32)>, rng: Rng, bg: u32 }

impl Pixel {
    fn new(g: &Gfx) -> Result<Pixel> {
        let (lw, lh) = ((g.w as usize).div_ceil(3), (g.h as usize).div_ceil(3));
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let cols = (0..lw / 6).map(|_| (-rng.f() * lh as f32, rng.range(4.0, 11.0))).collect();
        Ok(Pixel { lw, lh, buf: vec![0xff00_0000; lw * lh], small: g.bitmap(lw as u32, lh as u32, None, false)?, cols, rng, bg: 0 })
    }
}

impl Background for Pixel {
    fn fps(&self) -> f32 { 30.0 }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let bg = px(look.bg);
        if bg != self.bg { self.buf.fill(bg); self.bg = bg; }   // a new look: start from its background
        let (br, bgc, bb) = ((bg >> 16) & 255, (bg >> 8) & 255, bg & 255);
        for p in self.buf.iter_mut() {   // 18% toward the background each frame, like the page's translucent fill
            let (r, gg, b) = ((*p >> 16) & 255, (*p >> 8) & 255, *p & 255);
            *p = 0xff00_0000 | ((r * 210 + br * 46) >> 8) << 16 | ((gg * 210 + bgc * 46) >> 8) << 8 | ((b * 210 + bb * 46) >> 8);
        }
        let (lw, lh) = (self.lw as i32, self.lh as i32);
        let (dim, hi) = (px(look.dim), px(look.hi));
        for i in 0..self.cols.len() {
            let c = &mut self.cols[i];
            c.0 += c.1 * dt * 9.0;
            if c.0 > (lh + 40) as f32 { *c = (self.rng.range(-60.0, 0.0), self.rng.range(4.0, 11.0)); }
            if self.rng.f() > level * 1.4 { continue; }
            let color = if self.rng.f() < 0.08 { hi } else { dim };
            let (x, y) = (i as i32 * 6, (self.cols[i].0 / 9.0).floor() as i32 * 9);
            for r in 0..7 {
                let row = self.rng.next() & 31;
                let yy = y + r;
                if yy < 0 || yy >= lh { continue; }
                for b in 0..5 {
                    if row >> b & 1 == 1 && x + b < lw { self.buf[(yy * lw + x + b) as usize] = color; }
                }
            }
        }
        unsafe {
            self.small.CopyFromMemory(None, self.buf.as_ptr() as *const _, (self.lw * 4) as u32)?;
            g.dc.DrawBitmap(&self.small, Some(&rect(0.0, 0.0, (self.lw * 3) as f32, (self.lh * 3) as f32)), 1.0,
                            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
        }
        Ok(())
    }
}

/// katakana falling at three depths: small, dim, slow columns far back; large, bright, fast ones near. Each column
/// has a white-hot head, the trail cools behind it, and its glyphs (some mirrored) change as it falls.
/// Every glyph is drawn once, white, into an atlas (each depth's size, plain and mirrored); a frame is then one
/// sprite batch, each sprite tinted and faded by its own color. (The page draws each glyph as text, every frame.)
pub struct Cascade {
    atlas: ID2D1Bitmap1, batch: ID2D1SpriteBatch, dc3: ID2D1DeviceContext3, cols: Vec<Col>, rng: Rng, n: usize,
    w: f32, h: f32, rows: [u32; 3], cell: [u32; 3], scale: f32,
    dest: Vec<D2D_RECT_F>, src: Vec<D2D_RECT_U>, tint: Vec<D2D1_COLOR_F>,
}

struct Col { d: usize, x: f32, y: f32, v: f32, len: usize, trail: Vec<(u16, bool)>, row: i64, on: bool }

const GL: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789:=+*<>";
const DEPTH: [(f32, f32, f32); 3] = [(11.0, 0.35, 0.6), (15.0, 0.6, 1.0), (21.0, 1.0, 1.45)];   // font size, brightness, speed
const KANA: &str = "DotGothic16";   // the bundled cut holding just the rain's katakana; the rest comes from the theme's mono

impl Cascade {
    fn new(g: &Gfx, fonts: &Fonts, mono: &str) -> Result<Cascade> {
        let glyphs: Vec<char> = GL.chars().collect();
        // atlas rows (device pixels): for each depth, plain then mirrored, each as tall as that depth's cell
        let s = g.scale;
        let cell = DEPTH.map(|(fs, _, _)| (fs * s).ceil() as u32);
        let mut rows = [0u32; 3];
        let mut top = 0u32;
        for d in 0..3 { rows[d] = top; top += 2 * cell[d]; }
        let atlas = g.surface((glyphs.len() as u32 * cell[2]) as f32 / s, top as f32 / s)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&atlas);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);   // the atlas has alpha: no ClearType
            let white = g.brush([1.0, 1.0, 1.0, 1.0])?;
            for (d, &(fs, _, _)) in DEPTH.iter().enumerate() {
                let kana = fonts.format(KANA, fs - 1.0, 400)?;
                let alt = fonts.format(mono, fs - 1.0, 400)?;
                for f in [&kana, &alt] {
                    f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                    f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                }
                for (i, ch) in glyphs.iter().enumerate() {
                    let fmt = if fonts.has_char(KANA, *ch) { &kana } else { &alt };
                    let txt: Vec<u16> = ch.to_string().encode_utf16().collect();
                    let cs = cell[d] as f32 / s;   // the cell in DIPs
                    for flip in [false, true] {
                        let (x, y) = (i as f32 * cs, (rows[d] + if flip { cell[d] } else { 0 }) as f32 / s);
                        // mirrored about the cell's middle, as the page's setTransform(-1, 0, 0, 1, x, y)
                        let (m11, m31) = if flip { (-1.0, 2.0 * x + cs) } else { (1.0, 0.0) };
                        dc.PushAxisAlignedClip(&rect(x, y, cs, cs), D2D1_ANTIALIAS_MODE_ALIASED);
                        dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: m11, M12: 0.0, M21: 0.0, M22: 1.0, M31: m31, M32: 0.0 });
                        dc.DrawText(&txt, fmt, &rect(x, y, cs, cs + 4.0), &white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                        dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                        dc.PopAxisAlignedClip();
                    }
                }
            }
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_DEFAULT);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let dc3: ID2D1DeviceContext3 = g.dc.cast()?;
        let batch = unsafe { dc3.CreateSpriteBatch()? };
        let mut c = Cascade { atlas, batch, dc3, cols: Vec::new(), rng: Rng(0x2545_f491_4f6c_dd1d), n: glyphs.len(),
                              w: g.w as f32, h: g.h as f32, rows, cell, scale: s, dest: Vec::new(), src: Vec::new(), tint: Vec::new() };
        for (d, &(fs, _, _)) in DEPTH.iter().enumerate() {
            for _ in 0..(c.w / fs * [1.0, 0.6, 0.28][d]).ceil() as usize {
                let y = c.rng.range(-c.h * 0.3, c.h * 1.2);
                let mut col = c.col(d, y, 0.5);
                col.trail = (0..col.len).map(|_| c.glyph()).collect();
                c.cols.push(col);
            }
        }
        Ok(c)
    }

    fn glyph(&mut self) -> (u16, bool) {
        let i = ((self.rng.f() * self.n as f32) as usize).min(self.n - 1) as u16;
        (i, self.rng.f() < 0.45)
    }

    fn col(&mut self, d: usize, y: f32, level: f32) -> Col {
        let (fs, _, speed) = DEPTH[d];
        let x = self.rng.range(0.0, self.w / fs).floor() * fs;
        Col { d, x, y, v: self.rng.range(5.0, 13.0) * speed, len: self.rng.range(10.0, 32.0) as usize, trail: Vec::new(),
              row: i64::MIN, on: self.rng.f() < 0.25 + level * 1.2 }
    }
}

impl Background for Cascade {
    fn fps(&self) -> f32 { 30.0 }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let sp = 0.3 + level * 1.4;
        let pm = |c: Color, a: f32| D2D1_COLOR_F { r: c[0] * a, g: c[1] * a, b: c[2] * a, a };   // premultiplied, as the atlas is
        self.dest.clear(); self.src.clear(); self.tint.clear();
        for d in 0..3 {   // far columns first, so the near ones draw over them
            let (fs, br, _) = DEPTH[d];
            for i in 0..self.cols.len() {
                if self.cols[i].d != d { continue; }
                let c = &mut self.cols[i];
                c.y += c.v * dt * fs * sp;
                let row = (c.y / fs).floor() as i64;
                let new_row = row != c.row;
                c.row = row;
                if (row - c.len as i64) as f32 * fs > self.h {   // fallen off the bottom: a new column from above
                    let y = self.rng.range(-self.h / 2.0, 0.0);
                    self.cols[i] = self.col(d, y, level);
                    continue;
                }
                if new_row {
                    let gl = self.glyph();
                    let c = &mut self.cols[i];
                    c.trail.insert(0, gl);
                    c.trail.truncate(c.len);
                }
                if !self.cols[i].on { continue; }
                let len = self.cols[i].trail.len();
                if len > 1 && self.rng.f() < 0.06 {   // a glyph in the trail changes
                    let k = (1 + (self.rng.f() * (len - 1) as f32) as usize).min(len - 1);
                    self.cols[i].trail[k] = self.glyph();
                }
                let c = &self.cols[i];
                for (k, &(gi, flip)) in c.trail.iter().enumerate() {
                    let y = (row - k as i64) as f32 * fs;
                    if y < -fs || y > self.h { continue; }
                    let a = br * if k > 0 { (1.0 - k as f32 / c.len as f32).max(0.0).powf(1.4) } else { 1.0 };
                    let color = if k == 0 { look.acc } else if k < 3 { look.hi } else { look.ink };
                    let cp = self.cell[d];
                    let (sx, sy) = (gi as u32 * cp, self.rows[d] + if flip { cp } else { 0 });
                    let cs = cp as f32 / self.scale;
                    self.dest.push(rect(c.x, y, cs, cs));
                    self.src.push(D2D_RECT_U { left: sx, top: sy, right: sx + cp, bottom: sy + cp });
                    self.tint.push(pm(color, a));
                }
            }
        }
        unsafe {
            g.dc.Clear(Some(&rgba(look.bg)));
            if !self.dest.is_empty() {
                let n = self.dest.len() as u32;
                self.batch.Clear();
                self.batch.AddSprites(n, self.dest.as_ptr(), Some(self.src.as_ptr()), Some(self.tint.as_ptr()), None,
                                      size_of::<D2D_RECT_F>() as u32, size_of::<D2D_RECT_U>() as u32, size_of::<D2D1_COLOR_F>() as u32, 0)?;
                g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // sprite batches need it
                self.dc3.DrawSpriteBatch(&self.batch, 0, n, &self.atlas, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_SPRITE_OPTIONS_NONE);
                g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
        }
        Ok(())
    }
}

/// a broadsheet page ghosted under the cards: a masthead between rules, columns of greeked type with headlines, a
/// halftone photo or two. It sets itself line by line, holds, then fades for the next edition.
/// The page is drawn once, white, into its own surface (with a second drawing context, since the scene's is busy
/// when items are set); each frame fills it with the theme's ink through it as a mask, so a palette change shows
/// at once. While it holds, nothing moves: 1 frame a second instead of 15.
pub struct Broadsheet {
    page: ID2D1Bitmap1, pdc: ID2D1DeviceContext, white: ID2D1SolidColorBrush,
    mono: String, display: (String, u32), dw: IDWriteFactory5, fonts_coll: Option<IDWriteFontCollection>, display_coll: Option<IDWriteFontCollection>,
    items: Vec<Sheet>, n: usize, phase: u8, t: f32, fade: f32, w: f32, h: f32, rng: Rng, still: bool,
    clock: std::time::Instant, last: f32,
}

enum Sheet { Photo(f32, f32, f32, f32), Bar(f32, f32, f32, f32, f32) }   // x, y, w, h (and alpha)

const SET: u8 = 0;
const HOLD: u8 = 1;
const OUT: u8 = 2;

impl Broadsheet {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Broadsheet> {
        let page = g.surface(g.w as f32, g.h as f32)?;
        let pdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        unsafe { pdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); pdc.SetTarget(&page); }
        let white = unsafe { pdc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)? };
        let coll = |fam: &str| -> Option<IDWriteFontCollection> { if fonts.has(fam) { fonts.set.cast().ok() } else { None } };
        let mut b = Broadsheet {
            fonts_coll: coll(&look.mono.family), display_coll: coll(&look.display.family),
            page, pdc, white, mono: look.mono.family.clone(), display: (look.display.family.clone(), look.display.weight),
            dw: fonts.dw.clone(), items: Vec::new(), n: 0, phase: SET, t: 0.0, fade: 1.0, w: g.w as f32, h: g.h as f32,
            rng: Rng(0x5eed_0f_b0a2_d5ee), still: false, clock: std::time::Instant::now(), last: 0.0,
        };
        b.layout()?;
        b.put_upto(usize::MAX)?;   // up at once; later editions set line by line
        b.phase = HOLD;
        Ok(b)
    }

    fn format(&self, fam: &str, coll: &Option<IDWriteFontCollection>, size: f32, weight: u32) -> Result<IDWriteTextFormat> {
        let w: Vec<u16> = fam.encode_utf16().chain([0]).collect();
        unsafe {
            self.dw.CreateTextFormat(PCWSTR(w.as_ptr()), coll.as_ref(), DWRITE_FONT_WEIGHT(weight as i32), DWRITE_FONT_STYLE_NORMAL,
                                     DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
        }
    }

    /// a new edition: where everything goes, and the masthead and column rules drawn straight away
    fn layout(&mut self) -> Result<()> {
        let (w, h) = (self.w, self.h);
        let m = (w.min(h) * 0.05).round();
        let cols = if w > h { 6 } else { 3 };
        let gut = (w * 0.012).round();
        let top = (h * 0.17).round();
        let cw = (w - m * 2.0 - gut * (cols as f32 - 1.0)) / cols as f32;
        let lh = (h / 95.0).round().max(7.0);
        let r = &mut self.rng;
        let mut items = Vec::new();
        // the photos: one or two blocks spanning columns
        let mut photos: Vec<(usize, usize, f32, f32, f32, f32)> = Vec::new();
        for _ in 0..(if r.f() < 0.5 { 1 } else { 2 }) {
            let c0 = (r.f() * (cols - 1) as f32) as usize;
            let span = (cols - c0).min(2 + (r.f() * 2.0) as usize);
            let y0 = top + r.range(0.05, 0.45) * (h - top);
            if photos.iter().any(|p| c0 < p.1 && c0 + span > p.0) { continue; }
            let ph = (h - m - y0).min(r.range(0.22, 0.34) * h);
            photos.push((c0, c0 + span, m + c0 as f32 * (cw + gut), y0, span as f32 * cw + (span as f32 - 1.0) * gut, ph));
        }
        for p in &photos { items.push(Sheet::Photo(p.2, p.3, p.4, p.5)); }
        for c in 0..cols {   // each column: a headline now and then, then lines of type
            let x = m + c as f32 * (cw + gut);
            let mut y = top + lh;
            let mut first = true;
            while y < h - m - lh {
                if let Some(p) = photos.iter().find(|p| c >= p.0 && c < p.1 && y + lh > p.3 - lh && y < p.3 + p.5 + lh) { y = p.3 + p.5 + lh * 2.2; first = false; continue; }
                if r.f() < 0.05 || first {   // a headline: one or two heavy lines
                    let n = 1 + (r.f() * 2.0) as usize;
                    for i in 0..n { let ww = cw * if i == n - 1 { r.range(0.5, 0.9) } else { 1.0 }; items.push(Sheet::Bar(x, y, ww, lh * 1.5, 1.0)); y += lh * 2.1; }
                    y += lh * 0.4;
                }
                first = false;
                let para = 4 + (r.f() * 9.0) as usize;
                let mut i = 0;
                while i < para && y < h - m - lh {
                    let ww = if i == para - 1 { cw * r.range(0.2, 0.8) } else { cw };
                    let ind = if i == 0 { cw * 0.06 } else { 0.0 };
                    items.push(Sheet::Bar(x + ind, y, ww - ind, lh * 0.55, 0.55));
                    y += lh;
                    i += 1;
                }
                y += lh * 0.5;
            }
        }
        let key = |s: &Sheet| match *s { Sheet::Photo(x, y, ..) | Sheet::Bar(x, y, ..) => (x, y) };
        items.sort_by(|a, b| key(a).0.total_cmp(&key(b).0).then(key(a).1.total_cmp(&key(b).1)));
        self.items = items;
        self.n = 0;
        // the masthead between double rules, the column rules
        let fs = (lh * 1.3).round().max(10.0);
        let mono = self.format(&self.mono.clone(), &self.fonts_coll, fs, 400)?;
        let title = self.format(&self.display.0.clone(), &self.display_coll, fs * 2.6, self.display.1)?;
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
        let vol = format!("VOL. {}  ·  NO. {}", t.wYear as i32 - 2000, t.wMonth as u32 * 31 + t.wDay as u32);
        let date = format!("{} {} {:02} {}", DAYS[t.wDayOfWeek as usize % 7], MONTHS[(t.wMonth as usize + 11) % 12], t.wDay, t.wYear);
        let r1 = (top * 0.3).round();
        let mid = (r1 + top) / 2.0;
        let dc = &self.pdc;
        unsafe {
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let b = &self.white;
            b.SetOpacity(1.0);
            for (yy, hh) in [(r1, 2.0), (r1 + 5.0, 1.0), (top - 8.0, 1.0), (top - 5.0, 2.0)] { dc.FillRectangle(&rect(m, yy, w - m * 2.0, hh), b); }
            b.SetOpacity(0.9);
            for (text, fmt, align) in [(vol, &mono, DWRITE_TEXT_ALIGNMENT_LEADING), (date, &mono, DWRITE_TEXT_ALIGNMENT_TRAILING),
                                       ("The Live Feed".to_string(), &title, DWRITE_TEXT_ALIGNMENT_CENTER)] {
                fmt.SetTextAlignment(align)?;
                fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let s: Vec<u16> = text.encode_utf16().collect();
                dc.DrawText(&s, fmt, &rect(m, mid - 100.0, w - m * 2.0, 200.0), b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            b.SetOpacity(0.5);
            for c in 1..cols {
                let x = (m + c as f32 * (cw + gut) - gut / 2.0).round();
                dc.FillRectangle(&rect(x, top + lh, 1.0, h - m - top - lh), b);
            }
            dc.EndDraw(None, None)?;
        }
        Ok(())
    }

    /// set the next items, up to `upto` of them
    fn put_upto(&mut self, upto: usize) -> Result<()> {
        if self.n >= self.items.len() || upto == 0 { return Ok(()); }
        let end = self.items.len().min(self.n.saturating_add(upto));
        let (dc, b) = (&self.pdc, &self.white);
        unsafe {
            dc.BeginDraw();
            for it in &self.items[self.n..end] {
                match *it {
                    Sheet::Photo(px, py, pw, ph) => {   // a halftone: dots sized by a soft made-up image
                        let s = (self.w.min(self.h) / 150.0).round().max(4.0);
                        let (f1, f2, phs) = (self.rng.range(1.0, 3.0), self.rng.range(1.0, 3.0), self.rng.range(0.0, std::f32::consts::TAU));
                        b.SetOpacity(0.9);
                        let mut y = py + s / 2.0;
                        while y < py + ph {
                            let mut x = px + s / 2.0;
                            while x < px + pw {
                                let (u, v) = ((x - px) / pw, (y - py) / ph);
                                let k = 0.5 + 0.3 * (u * f1 * std::f32::consts::TAU + phs).sin() * (v * f2 * std::f32::consts::PI).cos()
                                        + 0.25 * (v - 0.5) - 0.2 * (-(((u - 0.5) / 0.18).powi(2) + ((v - 0.42) / 0.25).powi(2))).exp();
                                let rad = s * 0.5 * k.clamp(0.0, 1.0);
                                if rad > 0.3 { dc.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: rad, radiusY: rad }, b); }
                                x += s;
                            }
                            y += s;
                        }
                        b.SetOpacity(0.6);
                        dc.FillRectangle(&rect(px, py + ph + 4.0, pw * 0.7, 2.0), b);
                    }
                    Sheet::Bar(x, y, w, h, a) => { b.SetOpacity(a); dc.FillRectangle(&rect(x, y, w, h), b); }
                }
            }
            dc.EndDraw(None, None)?;
        }
        self.n = end;
        Ok(())
    }
}

impl Background for Broadsheet {
    fn fps(&self) -> f32 { if self.still { 0.0 } else if self.phase == HOLD { 1.0 } else { 15.0 } }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, _: f32) -> Result<()> {
        // its own clock: while holding it is called once a second, and the scene caps the dt it passes
        let now = self.clock.elapsed().as_secs_f32();
        let dt = (now - self.last).min(1.5);
        self.last = now;
        self.still = level <= 0.0;   // 0 stills it: the page as it is stays up
        if !self.still {
            let sp = 0.4 + level * 1.6;   // the page's .4 + RAIN * 1.6 (RAIN 0-1, as `level`)
            match self.phase {
                SET => {
                    self.put_upto((dt * sp * 40.0).ceil() as usize)?;
                    if self.n >= self.items.len() { self.phase = HOLD; self.t = 0.0; }
                }
                HOLD => { self.t += dt * sp; if self.t > 40.0 { self.phase = OUT; } }
                _ => {
                    self.fade -= dt * 0.4;
                    if self.fade <= 0.0 { self.fade = 1.0; self.phase = SET; self.layout()?; }
                }
            }
        }
        unsafe {
            g.dc.Clear(Some(&rgba(look.bg)));
            let a = if look.light { 0.13 } else { 0.11 } * self.fade;
            let ink = g.brush([look.ink[0], look.ink[1], look.ink[2], a])?;
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            g.dc.FillOpacityMask(&self.page, &ink, Some(&rect(0.0, 0.0, self.w, self.h)), None);
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        Ok(())
    }
}
