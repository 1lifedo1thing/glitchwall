//! Backgrounds: each one draws into the full-screen scene bitmap that the cards' glass blurs.
//! Ported so far: the page's "pixel", "cascade", "broadsheet", "halftone", "stop", "grid", "record", "notebook", "textmode", "sunprint", "draft", "topo", "radar" and "orb"; every other background falls back to a plain fill until it is.
use crate::fonts::Fonts;
use crate::gfx::{Gfx, rect, rgba};
use crate::json::Json;
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
    /// the background's own options from the settings, {"spin": "both", ...}; most have none. True: they change
    /// something drawn once, so make it again (with them)
    fn opts(&mut self, _: &Json) -> bool { false }
}

/// called outside any drawing (a background may draw its sprites up front)
/// `o`: the background's own options ({"rings": "off", ...}), for those that draw them in once
pub fn make(name: &str, g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Box<dyn Background>> {
    Ok(match name {
        "pixel" => Box::new(Pixel::new(g)?),
        "cascade" => Box::new(Cascade::new(g, fonts, &look.mono.family, o)?),
        "broadsheet" => Box::new(Broadsheet::new(g, fonts, look)?),
        "halftone" => Box::new(Halftone::new(g, o)?),
        "stop" => Box::new(Stop::new(g, fonts, look)?),
        "grid" => Box::new(Grid { ph: 0.0, still: false, sun: true, stripes: true, scroll: true, hz: 0.6 }),
        "record" => Box::new(Record::new(g, fonts, look)?),
        "notebook" => Box::new(Notebook::new(g)?),
        "textmode" => Box::new(Textmode::new(g, fonts, look)?),
        "sunprint" => Box::new(Sunprint::new(g)?),
        "draft" => Box::new(Draft::new(g, fonts, look)?),
        "topo" => Box::new(Topo::new(g, fonts, look, o)?),
        "radar" => Box::new(Radar::new(g, fonts, look, o)?),
        "orb" => Box::new(Orb::new(g, look)?),
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
pub struct Pixel { lw: usize, lh: usize, buf: Vec<u32>, small: ID2D1Bitmap1, cols: Vec<(f32, f32)>, rng: Rng, bg: u32,
                   k: f32, glyphs: u8, up: bool, keep: u32 }   // options: density, bits/blocks/dots, rising, how much stays each frame (of 256)

impl Pixel {
    fn new(g: &Gfx) -> Result<Pixel> {
        let (lw, lh) = ((g.w as usize).div_ceil(3), (g.h as usize).div_ceil(3));
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let cols = (0..lw / 6).map(|_| (-rng.f() * lh as f32, rng.range(4.0, 11.0))).collect();
        Ok(Pixel { lw, lh, buf: vec![0xff00_0000; lw * lh], small: g.bitmap(lw as u32, lh as u32, None, false)?, cols, rng, bg: 0,
                   k: 1.4, glyphs: 0, up: false, keep: 210 })
    }
}

impl Background for Pixel {
    fn fps(&self) -> f32 { 30.0 }
    fn opts(&mut self, o: &Json) -> bool {
        self.k = match o.get("density").str() { Some("sparse") => 0.6, Some("dense") => 2.6, _ => 1.4 };
        self.glyphs = match o.get("glyphs").str() { Some("blocks") => 1, Some("dots") => 2, _ => 0 };
        self.up = o.get("flow").str() == Some("up");
        self.keep = match o.get("trails").str() { Some("short") => 166, Some("long") => 236, _ => 210 };
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let bg = px(look.bg);
        if bg != self.bg { self.buf.fill(bg); self.bg = bg; }   // a new look: start from its background
        let (br, bgc, bb) = ((bg >> 16) & 255, (bg >> 8) & 255, bg & 255);
        for p in self.buf.iter_mut() {   // 18% toward the background each frame, like the page's translucent fill
            let (r, gg, b) = ((*p >> 16) & 255, (*p >> 8) & 255, *p & 255);
            let (k, q) = (self.keep, 256 - self.keep);
            *p = 0xff00_0000 | ((r * k + br * q) >> 8) << 16 | ((gg * k + bgc * q) >> 8) << 8 | ((b * k + bb * q) >> 8);
        }
        let (lw, lh) = (self.lw as i32, self.lh as i32);
        let (dim, hi) = (px(look.dim), px(look.hi));
        for i in 0..self.cols.len() {
            let c = &mut self.cols[i];
            c.0 += c.1 * dt * 9.0;
            if c.0 > (lh + 40) as f32 { *c = (self.rng.range(-60.0, 0.0), self.rng.range(4.0, 11.0)); }
            if self.rng.f() > level * self.k { continue; }
            let color = if self.rng.f() < 0.08 { hi } else { dim };
            let (x, y0) = (i as i32 * 6, (self.cols[i].0 / 9.0).floor() as i32 * 9);
            let y = if self.up { lh - y0 - 7 } else { y0 };
            for r in 0..7 {
                let row = match self.glyphs { 1 => 31, 2 => if r % 2 == 0 { 4 } else { 0 }, _ => self.rng.next() & 31 };
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
    dens: f32, one: bool, set: (usize, usize), green: bool,   // options: density, one layer, the glyphs used, heads
}

struct Col { d: usize, x: f32, y: f32, v: f32, len: usize, trail: Vec<(u16, bool)>, row: i64, on: bool }

const GL: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ0123456789:=+*<>";
const DEPTH: [(f32, f32, f32); 3] = [(11.0, 0.35, 0.6), (15.0, 0.6, 1.0), (21.0, 1.0, 1.45)];   // font size, brightness, speed
const KANA: &str = "DotGothic16";   // the bundled cut holding just the rain's katakana; the rest comes from the theme's mono

impl Cascade {
    fn new(g: &Gfx, fonts: &Fonts, mono: &str, o: &Json) -> Result<Cascade> {
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
                              w: g.w as f32, h: g.h as f32, rows, cell, scale: s, dest: Vec::new(), src: Vec::new(), tint: Vec::new(),
                              dens: 1.0, one: false, set: (0, glyphs.len()), green: false };
        c.opts(o);
        for (d, &(fs, _, _)) in DEPTH.iter().enumerate() {
            let share = if c.one { [0.0, 1.6, 0.0] } else { [1.0, 0.6, 0.28] };   // one: just the middle depth
            for _ in 0..(c.w / fs * share[d] * c.dens).ceil() as usize {
                let y = c.rng.range(-c.h * 0.3, c.h * 1.2);
                let mut col = c.col(d, y, 0.5);
                col.trail = (0..col.len).map(|_| c.glyph()).collect();
                c.cols.push(col);
            }
        }
        Ok(c)
    }

    fn glyph(&mut self) -> (u16, bool) {
        let (a, b) = self.set;
        let i = (a + ((self.rng.f() * (b - a) as f32) as usize).min(b - a - 1)) as u16;
        (i, self.rng.f() < 0.45)
    }

    fn col(&mut self, d: usize, y: f32, level: f32) -> Col {
        let (fs, _, speed) = DEPTH[d];
        let x = self.rng.range(0.0, self.w / fs).floor() * fs;
        Col { d, x, y, v: self.rng.range(5.0, 13.0) * speed, len: self.rng.range(10.0, 32.0) as usize, trail: Vec::new(),
              row: i64::MIN, on: self.rng.f() < (0.25 + level * 1.2) * self.dens }
    }
}

impl Background for Cascade {
    fn fps(&self) -> f32 { 30.0 }
    fn opts(&mut self, o: &Json) -> bool {
        let was = (self.dens, self.one);
        self.dens = match o.get("density").str() { Some("sparse") => 0.5, Some("dense") => 1.6, _ => 1.0 };
        self.one = o.get("layers").str() == Some("one");
        self.set = match o.get("glyphs").str() { Some("katakana") => (0, 45), Some("digits") => (45, 55), _ => (0, self.n) };
        self.green = o.get("heads").str() == Some("green");
        (self.dens, self.one) != was && !self.cols.is_empty()   // the columns are laid out when it's made
    }
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
                    let color = if k == 0 { if self.green { look.hi } else { look.acc } } else if k < 3 { look.hi } else { look.ink };
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

/// two-drum riso print: a slowly breathing halftone field, the dim drum plus the bright drum off true.
/// One circle drawn up front; each frame is one sprite batch of it, scaled per dot. The page blends the dots with
/// multiply (light themes) or screen (dark); over its flat background that equals plain drawing in the blended
/// color, worked out here once per drum.
pub struct Halftone { dot: ID2D1Bitmap1, batch: ID2D1SpriteBatch, dc3: ID2D1DeviceContext3, t: f32, w: f32, h: f32,
                      still: bool, dest: Vec<D2D_RECT_F>, tint: Vec<D2D1_COLOR_F>,
                      sp: f32, inks: usize, drift: bool, off: f32 }   // options: dot spacing (the dot is made for it), one or two inks, breathing, misprint

impl Halftone {
    fn new(g: &Gfx, o: &Json) -> Result<Halftone> {
        let sp = Self::spacing(o);
        let max = sp * 0.48;
        let d = max * 2.0 + 2.0;
        let dot = g.surface(d, d)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&dot);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x: d / 2.0, y: d / 2.0 }, radiusX: max, radiusY: max },
                           &g.brush([1.0, 1.0, 1.0, 1.0])?);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let dc3: ID2D1DeviceContext3 = g.dc.cast()?;
        let batch = unsafe { dc3.CreateSpriteBatch()? };
        let mut rng = Rng(0x7a11_7012_0b5e_ed00);
        Ok(Halftone { dot, batch, dc3, t: rng.range(0.0, 100.0), w: g.w as f32, h: g.h as f32, still: false, dest: Vec::new(), tint: Vec::new(),
                      sp, inks: 2, drift: true, off: 1.0 })
    }
    fn spacing(o: &Json) -> f32 { if o.get("dots").str() == Some("coarse") { 30.0 } else { 18.0 } }
}

impl Background for Halftone {
    fn fps(&self) -> f32 { if self.still || !self.drift { 0.0 } else { 12.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.inks = if o.get("inks").str() == Some("one") { 1 } else { 2 };
        self.drift = o.get("drift").str() != Some("still");
        self.off = if o.get("misprint").str() == Some("off") { 0.0 } else { 1.0 };
        Self::spacing(o) != self.sp
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        if self.drift { self.t += dt * (0.15 + level * 0.35); }
        self.still = level <= 0.0;
        unsafe { g.dc.Clear(Some(&rgba(look.bg))); }
        if self.still { return Ok(()); }
        let (t, bg) = (self.t, look.bg);
        let a = 0.16 + level * 0.12;
        let (sp, max) = (self.sp, self.sp * 0.48);
        let half = (max + 1.0) / max;   // the sprite's half-size per unit of radius (it has a pixel of margin)
        self.dest.clear(); self.tint.clear();
        for &(col, ph, ox, oy) in &[(look.dim, 0.0f32, 0.0f32, 0.0f32), (look.hi, 2.1, 3.0 * self.off, 2.0 * self.off)][..self.inks] {
            // multiply / screen over the flat background, then premultiplied at the drum's alpha
            let mix = |i: usize| if look.light { bg[i] * col[i] } else { bg[i] + col[i] - bg[i] * col[i] };
            let c = D2D1_COLOR_F { r: mix(0) * a, g: mix(1) * a, b: mix(2) * a, a };
            let mut y = sp / 2.0;
            while y < self.h {
                let mut x = sp / 2.0;
                while x < self.w {
                    let v = ((x * 0.004 + t + ph).sin() + (y * 0.005 - t * 0.8 + ph).sin() + ((x + y) * 0.0025 + t * 0.5).sin()) / 3.0;
                    let r = v.max(0.0).powf(1.4) * max;
                    if r >= 0.6 {
                        let s = r * half;
                        self.dest.push(rect(x + ox - s, y + oy - s, s * 2.0, s * 2.0));
                        self.tint.push(c);
                    }
                    x += sp;
                }
                y += sp;
            }
        }
        if self.dest.is_empty() { return Ok(()); }
        unsafe {
            let n = self.dest.len() as u32;
            self.batch.Clear();
            self.batch.AddSprites(n, self.dest.as_ptr(), None, Some(self.tint.as_ptr()), None,
                                  size_of::<D2D_RECT_F>() as u32, 0, size_of::<D2D1_COLOR_F>() as u32, 0)?;
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // sprite batches need it
            self.dc3.DrawSpriteBatch(&self.batch, 0, n, &self.dot, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_SPRITE_OPTIONS_NONE);
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        Ok(())
    }
}

/// the old stop screen, faint, behind the cards; only the cursor moves. The text is laid out once; each blink
/// (every .55 s, its own clock) redraws it.
pub struct Stop { title: IDWriteTextLayout, tw: f32, lines: Vec<IDWriteTextLayout>, cursor: [IDWriteTextLayout; 2],
                  fs: f32, w: f32, h: f32, clock: std::time::Instant }

const STOP_LINES: [&str; 12] = ["*** STOP: 0x00000ACE (0xC0FFEE00, 0x00000047, 0x00000833, 0x00000000)", "LIVE_TOOL_FEED_STILL_RUNNING", "",
    "Nothing has crashed. The agent is working and its tool calls appear on this screen",
    "as they happen. Secrets are masked before they get here.", "",
    "Technical information:", "", "*** GLITCHWALL.SYS - Address 7F000001 base at 7F000000, port 47833", "",
    "Beginning dump of physical memory . . . not really.", ""];

impl Stop {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Stop> {
        let (w, h) = (g.w as f32, g.h as f32);
        let fs = (h / 46.0).round().max(14.0);
        let fmt = fonts.format(&look.mono.family, fs, 400)?;
        let lay = |s: &str| -> Result<IDWriteTextLayout> {
            let t: Vec<u16> = s.encode_utf16().collect();
            unsafe { fonts.dw.CreateTextLayout(&t, &fmt, w * 2.0, fs * 2.0) }
        };
        let title = lay(" glitchwall ")?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { title.GetMetrics(&mut m)?; }
        let lines = STOP_LINES.iter().map(|s| lay(s)).collect::<Result<Vec<_>>>()?;
        let cursor = [lay("Press any key to continue _")?, lay("Press any key to continue  ")?];
        Ok(Stop { title, tw: m.widthIncludingTrailingWhitespace, lines, cursor, fs, w, h, clock: std::time::Instant::now() })
    }
}

impl Background for Stop {
    fn fps(&self) -> f32 { 1.0 / 0.55 }
    fn draw(&mut self, g: &Gfx, look: &Look, _: f32, _: f32) -> Result<()> {
        let on = (self.clock.elapsed().as_secs_f32() / 0.55) as u64 % 2 == 0;
        let (fs, x) = (self.fs, self.w * 0.06);
        let mut y = self.h * 0.12;
        let a = |c: Color, a: f32| [c[0], c[1], c[2], a];
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            // the grey title chip, centred
            let tx = self.w / 2.0 - self.tw / 2.0;
            dc.FillRectangle(&rect(tx - 4.0, y - 4.0, self.tw + 8.0, fs + 8.0), &g.brush(a(look.muted, 0.55))?);
            let o = D2D1_DRAW_TEXT_OPTIONS_NONE;
            dc.DrawTextLayout(D2D_POINT_2F { x: tx, y }, &self.title, &g.brush(a(look.bg, 0.55))?, o);
            y += fs * 2.4;
            let ink = g.brush(a(look.ink, 0.17))?;   // faint, so cards on top stay readable
            for l in &self.lines { dc.DrawTextLayout(D2D_POINT_2F { x, y }, l, &ink, o); y += fs * 1.35; }
            dc.DrawTextLayout(D2D_POINT_2F { x, y }, &self.cursor[if on { 0 } else { 1 }], &ink, o);
        }
        Ok(())
    }
}

/// synthwave: striped sun over a perspective grid rolling toward you. Plain Direct2D shapes, 30 fps; still at 0.
pub struct Grid { ph: f32, still: bool, sun: bool, stripes: bool, scroll: bool, hz: f32 }

impl Background for Grid {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.sun = o.get("sun").str() != Some("off");
        self.stripes = o.get("stripes").str() != Some("off");
        self.scroll = o.get("scroll").str() != Some("off");
        self.hz = match o.get("horizon").str() { Some("low") => 0.7, Some("high") => 0.5, _ => 0.6 };
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        if self.scroll { self.ph = (self.ph + dt * (0.25 + level * 0.9)) % 1.0; }
        self.still = level <= 0.0;
        unsafe { g.dc.Clear(Some(&rgba(look.bg))); }
        if self.still { return Ok(()); }
        let (w, h) = (g.w as f32, g.h as f32);
        let (y0, cxm, a) = (h * self.hz, w / 2.0, 0.25 + level * 0.5);
        let sr = w.min(h) * 0.2;
        let sy = y0 - sr * 0.35;
        unsafe {
            let dc = &g.dc;
            // the sun: accent at the top to ink at the bottom
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: rgba(look.acc) }, D2D1_GRADIENT_STOP { position: 1.0, color: rgba(look.ink) }];
            let rt: &ID2D1RenderTarget = dc;   // the plain version (the device context's own takes six arguments)
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let sun = dc.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { x: 0.0, y: sy - sr }, endPoint: D2D_POINT_2F { x: 0.0, y: sy + sr } },
                Some(&D2D1_BRUSH_PROPERTIES { opacity: a * 0.9, transform: Default::default() }), &coll)?;
            if self.sun { dc.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x: cxm, y: sy }, radiusX: sr, radiusY: sr }, &sun); }
            // its stripes, and the ground hiding its lower part
            let bg = g.brush(look.bg)?;
            for k in 0..if self.sun && self.stripes { 7 } else { 0 } {
                let (yy, hh) = (sy + sr * (0.05 + k as f32 * 0.14), 1.0 + k as f32 * 1.6);
                dc.FillRectangle(&rect(cxm - sr, yy, sr * 2.0, hh), &bg);
            }
            dc.FillRectangle(&rect(0.0, y0, w, h - y0), &bg);
            // the grid: rows closing in on the horizon, fading near it; rays fanning out from it
            let line = g.brush(look.dim)?;
            let c = (h - y0) * 0.5;
            for k in 0..40 {
                let z = k as f32 + 1.0 - self.ph;
                if z < 0.5 { continue; }
                let y = y0 + c / z;
                line.SetOpacity(a * ((y - y0) / (h - y0) * 3.0).min(1.0));
                dc.DrawLine(D2D_POINT_2F { x: 0.0, y }, D2D_POINT_2F { x: w, y }, &line, 1.2, None);
            }
            line.SetOpacity(a);
            let sp = w / 14.0;
            for j in -20..=20 {
                let j = j as f32;
                dc.DrawLine(D2D_POINT_2F { x: cxm + j * sp * 0.08, y: y0 }, D2D_POINT_2F { x: cxm + j * sp * 2.2, y: h }, &line, 1.2, None);
            }
        }
        Ok(())
    }
}

/// a record turning slowly in the corner of the sleeve; the light on the grooves stays put. The disc (grooves, two
/// dark bands, two lamp highlights) is drawn once; each frame draws it, then the label turned. 30 fps; still at 0.
pub struct Record { disc: ID2D1Bitmap1, d: f32, r: f32, a: f32, still: bool, at: (f32, f32),
                    texts: [(IDWriteTextLayout, f32); 4], lh: f32,
                    spin: f32, fast: bool, centre: bool, mid: f32 }   // options: turns per unit (0 resting, - anticlockwise), 45 rpm, bottom middle

impl Record {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Record> {
        let (w, h) = (g.w as f32, g.h as f32);
        let r = w.min(h) * 0.62;
        let d = (r * 2.0 + 4.0).ceil();
        let c = r + 2.0;
        let disc = g.surface(d, d)?;
        let ctr = D2D_POINT_2F { x: c, y: c };
        let circle = |rr: f32| D2D1_ELLIPSE { point: ctr, radiusX: rr, radiusY: rr };
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&disc);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.FillEllipse(&circle(r), &g.brush([0x16 as f32 / 255.0, 0x10 as f32 / 255.0, 0x0b as f32 / 255.0, 1.0])?);
            let groove = g.brush([1.0, 240.0 / 255.0, 220.0 / 255.0, 1.0])?;
            let mut rng = Rng(0x5eed_0f_d15c_0001);
            let mut rr = r * 0.36;
            while rr < r * 0.985 {
                groove.SetOpacity(0.025 + rng.f() * 0.03);
                dc.DrawEllipse(&circle(rr), &groove, 1.0, None);
                rr += 2.2;
            }
            let band = g.brush([0.0, 0.0, 0.0, 0.5])?;
            for (rr, bw) in [(r * 0.62, 5.0), (r * 0.8, 4.0)] { dc.DrawEllipse(&circle(rr), &band, bw, None); }
            // two soft highlights, like a lamp across the grooves: the page's conic gradient, as thin ring slices
            let factory = dc.GetFactory()?;
            let lamp = g.brush([1.0, 245.0 / 255.0, 230.0 / 255.0, 1.0])?;
            let (ri, ro, n) = (r * 0.34, r * 0.985, 36);
            let pt = |rad: f32, ang: f32| D2D_POINT_2F { x: c + rad * ang.cos(), y: c + rad * ang.sin() };
            for st in [-0.35f32, 2.8] {
                for k in 0..n {
                    let (f0, f1) = (k as f32 / n as f32 * 0.09, (k + 1) as f32 / n as f32 * 0.09);
                    let fm = (f0 + f1) / 2.0;
                    let alpha = if fm < 0.04 { fm / 0.04 } else { (0.09 - fm) / 0.05 } * 0.13;
                    let (a0, a1) = (st + f0 * std::f32::consts::TAU, st + f1 * std::f32::consts::TAU);
                    let path = factory.CreatePathGeometry()?;
                    let sink = path.Open()?;
                    sink.BeginFigure(pt(ro, a0), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddArc(&D2D1_ARC_SEGMENT { point: pt(ro, a1), size: D2D_SIZE_F { width: ro, height: ro }, rotationAngle: 0.0,
                                                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                    sink.AddLine(pt(ri, a1));
                    sink.AddArc(&D2D1_ARC_SEGMENT { point: pt(ri, a0), size: D2D_SIZE_F { width: ri, height: ri }, rotationAngle: 0.0,
                                                    sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    sink.Close()?;
                    lamp.SetOpacity(alpha);
                    dc.FillGeometry(&path, &lamp, None);
                }
            }
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        // the label's lines, centred in a box as wide as the label
        let lr = r * 0.32;
        let lh = lr;
        let lay = |s: &str, fam: &str, size: f32| -> Result<IDWriteTextLayout> {
            let f = fonts.format(fam, size.round().max(1.0), 400)?;
            unsafe {
                f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let t: Vec<u16> = s.encode_utf16().collect();
                fonts.dw.CreateTextLayout(&t, &f, lr * 2.0, lh)
            }
        };
        let texts = [(lay("glitchwall", "Instrument Serif", lr * 0.2)?, -lr * 0.45),
                     (lay("SIDE A  ·  33⅓ RPM", &look.mono.family, lr * 0.085)?, lr * 0.42),
                     (lay("LIVE TOOL FEED", &look.mono.family, lr * 0.085)?, lr * 0.58),
                     (lay("SIDE A  ·  45 RPM", &look.mono.family, lr * 0.085)?, lr * 0.42)];
        Ok(Record { disc, d, r, a: 0.0, still: false, at: (w * 0.12, h * 0.84), texts, lh, spin: 1.0, fast: false, centre: false, mid: w / 2.0 })
    }
}

impl Background for Record {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.fast = o.get("speed").str() == Some("45");
        let dir = if o.get("direction").str() == Some("anticlockwise") { -1.0 } else { 1.0 };
        self.spin = if o.get("spin").str() == Some("off") { 0.0 } else if self.fast { 1.35 * dir } else { dir };
        self.centre = o.get("place").str() == Some("centre");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.a += dt * level * 0.9 * self.spin;
        self.still = level <= 0.0 || self.spin == 0.0;   // a still record: drawn once where it stopped
        let (cx, cy) = (if self.centre { self.mid } else { self.at.0 }, self.at.1);
        let lr = self.r * 0.32;
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            dc.DrawBitmap(&self.disc, Some(&rect(cx - self.d / 2.0, cy - self.d / 2.0, self.d, self.d)), 0.92,
                          D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            let (s, c) = self.a.sin_cos();
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: cx, M32: cy });
            let at0 = D2D_POINT_2F::default();
            let e = |rr: f32| D2D1_ELLIPSE { point: at0, radiusX: rr, radiusY: rr };
            dc.FillEllipse(&e(lr), &g.brush(look.acc)?);
            dc.DrawEllipse(&e(lr * 0.82), &g.brush([look.ink[0], look.ink[1], look.ink[2], 0.35])?, 1.0, None);
            let ink = g.brush(look.ink)?;
            for (t, y) in [&self.texts[0], &self.texts[if self.fast { 3 } else { 1 }], &self.texts[2]] {
                dc.DrawTextLayout(D2D_POINT_2F { x: -lr, y: y - self.lh / 2.0 }, t, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            dc.FillEllipse(&e(lr * 0.07), &g.brush(look.bg)?);
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        }
        Ok(())
    }
}

/// ruled notebook paper, red margin, punched holes; pencil doodles sketch themselves, hold, then fade. The paper is a
/// few dozen lines, cheap enough to draw each frame (so a palette change shows at once); 30 fps, still at 0 once
/// the last doodle has faded.
pub struct Notebook { doodles: Vec<Doodle>, wait: f32, rng: Rng, round: ID2D1StrokeStyle, factory: ID2D1Factory }

struct Doodle { strokes: Vec<Vec<(f32, f32)>>, n: usize, x: f32, y: f32, rot: f32, t: f32, dur: f32, hold: f32 }

const NB_LINE: f32 = 30.0;

impl Notebook {
    fn new(g: &Gfx) -> Result<Notebook> {
        let factory = unsafe { g.dc.GetFactory()? };
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND,
                                                   dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
        let round = unsafe { factory.CreateStrokeStyle(&props, None)? };
        Ok(Notebook { doodles: Vec::new(), wait: 1.0, rng: Rng(0x0b00c_5ce7c4_0001), round, factory })
    }

    /// the page's six doodles: spiral, star, arrow, wave, ticked box, cube
    fn shape(&mut self, w: f32, h: f32) -> Doodle {
        let r = &mut self.rng;
        let s = r.range(14.0, 30.0);
        let k = r.f();
        let mut strokes: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut poly = |r: &mut Rng, pts: Vec<(f32, f32)>| strokes.push(pts.into_iter().map(|(x, y)| (x + r.range(-0.9, 0.9), y + r.range(-0.9, 0.9))).collect());
        let pi = std::f32::consts::PI;
        let sq = |dx: f32, dy: f32| vec![(-s / 2.0 + dx, -s / 2.0 + dy), (s / 2.0 + dx, -s / 2.0 + dy), (s / 2.0 + dx, s / 2.0 + dy),
                                         (-s / 2.0 + dx, s / 2.0 + dy), (-s / 2.0 + dx, -s / 2.0 + dy)];
        if k < 0.2 {
            let mut p = Vec::new(); let mut a = 0.0f32;
            while a < pi * 6.0 { let rr = 2.0 + a * s / 9.0; p.push((a.cos() * rr, a.sin() * rr)); a += 0.2; }
            poly(r, p);
        } else if k < 0.4 {
            poly(r, (0..=10).map(|i| { let a = -pi / 2.0 + i as f32 * pi / 5.0; let rr = if i % 2 == 1 { s * 0.42 } else { s }; (a.cos() * rr, a.sin() * rr) }).collect());
        } else if k < 0.55 {
            poly(r, vec![(-s * 2.0, 0.0), (s * 2.0, 0.0)]);
            poly(r, vec![(s * 1.4, -s * 0.5), (s * 2.0, 0.0), (s * 1.4, s * 0.5)]);
        } else if k < 0.7 {
            let mut p = Vec::new(); let mut x = -s * 3.0;
            while x <= s * 3.0 { p.push((x, (x * 0.18).sin() * s * 0.25)); x += 4.0; }
            poly(r, p);
        } else if k < 0.85 {
            poly(r, sq(0.0, 0.0));
            poly(r, vec![(-s * 0.3, 0.0), (-s * 0.05, s * 0.3), (s * 0.55, -s * 0.6)]);
        } else {
            let o = s * 0.4;
            poly(r, sq(0.0, 0.0));
            poly(r, sq(o, -o));
            for (x, y) in [(-s / 2.0, -s / 2.0), (s / 2.0, -s / 2.0), (s / 2.0, s / 2.0), (-s / 2.0, s / 2.0)] { poly(r, vec![(x, y), (x + o, y - o)]); }
        }
        let n = strokes.iter().map(|p| p.len()).sum();
        Doodle { strokes, n, x: r.range(w * 0.12, w * 0.95), y: r.range(h * 0.12, h * 0.92), rot: r.range(-0.3, 0.3),
                 t: 0.0, dur: r.range(1.5, 3.0), hold: r.range(5.0, 9.0) }
    }
}

impl Background for Notebook {
    fn fps(&self) -> f32 { if self.doodles.is_empty() && self.wait <= -1.0e6 { 0.0 } else { 30.0 } }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let (w, h) = (g.w as f32, g.h as f32);
        let with = |c: Color, a: f32| [c[0], c[1], c[2], a];
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let rule = g.brush(with(look.dim, 0.75))?;
            let mut y = NB_LINE * 3.0 + 0.5;
            while y < h { dc.DrawLine(D2D_POINT_2F { x: 0.0, y }, D2D_POINT_2F { x: w, y }, &rule, 1.0, None); y += NB_LINE; }
            let margin = g.brush(with(look.acc, 0.55))?;
            let mx = (w * 0.08).round() + 0.5;
            for x in [mx, mx + 3.0] { dc.DrawLine(D2D_POINT_2F { x, y: 0.0 }, D2D_POINT_2F { x, y: h }, &margin, 1.0, None); }
            let hole = g.brush([0.0, 0.0, 0.0, 0.07])?;
            let hr = w.min(h) * 0.016;
            for f in [0.18, 0.5, 0.82] {
                dc.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x: w * 0.03, y: h * f }, radiusX: hr, radiusY: hr }, &hole);
            }
        }
        // a new doodle every 1.5-4 s, up to `level` x 8 at once; none while the rain is off
        self.wait -= dt;
        if level <= 0.0 { if self.doodles.is_empty() { self.wait = -1.0e7; } }   // nothing left to animate: still
        else if self.wait < -1.0e6 { self.wait = 1.0; }
        if self.wait <= 0.0 && self.wait > -1.0e6 && self.doodles.len() < (level * 8.0).round() as usize {
            let d = self.shape(w, h);
            self.doodles.push(d);
            self.wait = self.rng.range(1.5, 4.0);
        }
        let pencil = g.brush(look.hi)?;
        let mut keep = Vec::new();
        for mut d in std::mem::take(&mut self.doodles) {
            d.t += dt;
            let end = d.dur + d.hold;
            let a = if d.t < end { 1.0 } else { 1.0 - (d.t - end) / 2.0 };
            if a <= 0.0 { continue; }
            let mut left = (d.n as f32 * (d.t / d.dur).min(1.0)).floor() as usize;
            unsafe {
                let (s, c) = d.rot.sin_cos();
                g.dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: d.x, M32: d.y });
                pencil.SetOpacity(0.55 * a);
                for p in &d.strokes {
                    if left <= 1 { break; }
                    let upto = p.len().min(left);
                    let path = self.factory.CreatePathGeometry()?;
                    let sink = path.Open()?;
                    sink.BeginFigure(D2D_POINT_2F { x: p[0].0, y: p[0].1 }, D2D1_FIGURE_BEGIN_HOLLOW);
                    for &(x, y) in &p[1..upto] { sink.AddLine(D2D_POINT_2F { x, y }); }
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    sink.Close()?;
                    g.dc.DrawGeometry(&path, &pencil, 1.5, &self.round);
                    left -= p.len().min(left);
                }
                g.dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
            }
            keep.push(d);
        }
        self.doodles = keep;
        Ok(())
    }
}

/// a text-mode desktop: a shaded backdrop in character cells, a grey menu bar with a clock, a status bar of function
/// keys, and the block of a text-mode mouse cursor drifting from place to place. The dither is drawn once as a
/// full-screen mask; each frame tints it. 12 fps while the mouse moves, otherwise 1 (the clock).
pub struct Textmode { mask: ID2D1Bitmap1, fmt: IDWriteTextFormat, fs: f32, cw: f32, cols: i32, rows: i32,
                      mx: i32, my: i32, goal: Option<(i32, i32)>, wait: f32, rng: Rng, w: f32, h: f32 }

const TM_MENU: [&str; 7] = ["File", "Edit", "Search", "Run", "Options", "Window", "Help"];
const TM_KEYS: [(&str, &str); 6] = [("F1", "Help"), ("F2", "Save"), ("F3", "Open"), ("F5", "Zoom"), ("F6", "Next"), ("F10", "Menu")];

impl Textmode {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Textmode> {
        let (w, h) = (g.w as f32, g.h as f32);
        let fs = (h / 34.0).round().max(16.0);
        let (cw, ch) = ((fs * 0.5).round(), fs);
        // the shade: a light dither in every cell, like the lightest shade block
        let tile = g.surface(cw, ch)?;
        let mask = g.surface(w, h)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            let white = g.brush([1.0, 1.0, 1.0, 1.0])?;
            dc.SetTarget(&tile);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let px = (cw / 8.0).round().max(1.0);
            let mut y = 0.0;
            while y < ch {
                let mut x = (((y / px / 2.0) as i32) % 2) as f32 * px * 2.0;
                while x < cw { dc.FillRectangle(&rect(x, y, px, px), &white); x += px * 4.0; }
                y += px * 2.0;
            }
            dc.EndDraw(None, None)?;
            let props = D2D1_BITMAP_BRUSH_PROPERTIES1 { extendModeX: D2D1_EXTEND_MODE_WRAP, extendModeY: D2D1_EXTEND_MODE_WRAP,
                                                        interpolationMode: D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR };
            let pat = dc.CreateBitmapBrush(&tile, Some(&props), None)?;
            dc.SetTarget(&mask);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.FillRectangle(&rect(0.0, ch, w, h - ch * 2.0), &pat);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let fmt = fonts.format(&look.mono.family, fs, 400)?;
        let (cols, rows) = ((w / cw) as i32, (h / ch) as i32);
        Ok(Textmode { mask, fmt, fs, cw, cols, rows, mx: (cols as f32 * 0.6) as i32, my: rows / 2, goal: None, wait: 2.0,
                      rng: Rng(0x7e47_30de_0000_0001), w, h })
    }
}

impl Background for Textmode {
    fn fps(&self) -> f32 { if self.goal.is_some() { 12.0 } else { 1.0 } }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        if level > 0.0 {
            self.wait -= dt;
            if self.goal.is_none() && self.wait <= 0.0 {
                self.goal = Some((2 + (self.rng.f() * (self.cols - 4) as f32) as i32, 2 + (self.rng.f() * (self.rows - 4) as f32) as i32));
            }
            // the mouse moves a cell at a time toward where it is going, then rests
            let mut n = ((dt * 30.0 * (0.4 + level * 1.6)).round() as i32).max(1);
            while let (Some((gx, gy)), true) = (self.goal, n > 0) {
                self.mx += (gx - self.mx).signum();
                if self.rng.f() < 0.6 { self.my += (gy - self.my).signum(); }
                if self.mx == gx && self.my == gy { self.goal = None; self.wait = self.rng.range(1.5, 6.0); }
                n -= 1;
            }
        }
        let (fs, cw, w, h) = (self.fs, self.cw, self.w, self.h);
        let bar = if look.bar[3] >= 1.0 { look.bar } else { look.ink };
        let on = if look.barb[3] >= 1.0 { look.barb } else { look.bg };
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let shade = g.brush([look.dim[0], look.dim[1], look.dim[2], 0.5])?;
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.mask, &shade, Some(&rect(0.0, 0.0, w, h)), None);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let (bar_b, on_b, key_b) = (g.brush(bar)?, g.brush(on)?, g.brush(look.err)?);
            let text = |s: &str, col: f32, y: f32, b: &ID2D1SolidColorBrush| {
                let t: Vec<u16> = s.encode_utf16().collect();
                dc.DrawText(&t, &self.fmt, &rect(col * cw, y, cw * (t.len() as f32 + 1.0), fs * 1.5), b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            };
            // the menu bar: first letters picked out, a clock at the right
            dc.FillRectangle(&rect(0.0, 0.0, w, fs), &bar_b);
            let mut x = 2.0;
            for m in TM_MENU { text(&m[..1], x, 0.0, &key_b); text(&m[1..], x + 1.0, 0.0, &on_b); x += m.len() as f32 + 3.0; }
            let t = windows::Win32::System::SystemInformation::GetLocalTime();
            let clock = format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond);
            text(&clock, (self.cols - clock.len() as i32 - 2) as f32, 0.0, &on_b);
            // the status bar
            let y = (self.rows - 1) as f32 * fs;
            dc.FillRectangle(&rect(0.0, y, w, h - y), &bar_b);
            let mut x = 1.0;
            for (k, l) in TM_KEYS { text(k, x, y, &key_b); text(l, x + k.len() as f32 + 1.0, y, &on_b); x += (k.len() + l.len() + 3) as f32; }
            // the mouse: an inverted cell
            dc.FillRectangle(&rect(self.mx as f32 * cw, self.my as f32 * fs, cw, fs), &g.brush([look.hi[0], look.hi[1], look.hi[2], 0.85])?);
        }
        Ok(())
    }
}

/// a botanical sun print: fern fronds and leaves leave pale silhouettes in the blue, soft at the edges; the print
/// develops, holds, and clears for the next arrangement. The silhouettes are drawn by a second device context into a
/// mask (a blurred copy under the sharp one, the page's shadowBlur); each frame tints it. 15 fps; still at 0.
pub struct Sunprint { mask: ID2D1Bitmap1, pdc: ID2D1DeviceContext, blur: ID2D1Effect,
                      white: ID2D1SolidColorBrush, clear: ID2D1SolidColorBrush, round: ID2D1StrokeStyle, factory: ID2D1Factory,
                      streaks: Vec<(D2D_POINT_2F, D2D_POINT_2F, f32, f32)>, rng: Rng, u: f32, w: f32, h: f32,
                      k: f32, phase: u8, t: f32, still: bool }

fn turn(a: f32, x: f32, y: f32) -> windows::Foundation::Numerics::Matrix3x2 {
    let (s, c) = a.sin_cos();
    windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: x, M32: y }
}
fn p2(x: f32, y: f32) -> D2D_POINT_2F { D2D_POINT_2F { x, y } }

impl Sunprint {
    fn new(g: &Gfx) -> Result<Sunprint> {
        let (w, h) = (g.w as f32, g.h as f32);
        let u = w.min(h);
        let mask = g.surface(w, h)?;
        let pdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        unsafe { pdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); }
        let blur = unsafe { pdc.CreateEffect(&CLSID_D2D1GaussianBlur)? };
        crate::gfx::set_f32(&blur, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, u * 0.006 / 2.0)?;
        let white = unsafe { pdc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)? };
        let clear = unsafe { pdc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)? };
        let factory = unsafe { pdc.GetFactory()? };
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND,
                                                   lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
        let round = unsafe { factory.CreateStrokeStyle(&props, None)? };
        // the coating brushed on: uneven, streaky edges (drawn each frame in the theme's ink)
        let mut rng = Rng(0x5a9f_e7ce_0000_0001);
        let mut streaks = Vec::new();
        for i in 0..36 {
            let side = i % 4;
            let (sw, a) = (rng.range(u * 0.01, u * 0.04), rng.range(0.008, 0.03));
            let (p, q) = if side < 2 {
                let y = if side == 1 { h - rng.range(0.0, u * 0.04) } else { rng.range(0.0, u * 0.04) };
                let x0 = rng.range(-50.0, w);
                (p2(x0, y), p2(x0 + rng.range(0.1, 0.5) * w, y + rng.range(-6.0, 6.0)))
            } else {
                let x = if side == 3 { w - rng.range(0.0, u * 0.04) } else { rng.range(0.0, u * 0.04) };
                let y0 = rng.range(-50.0, h);
                (p2(x, y0), p2(x + rng.range(-6.0, 6.0), y0 + rng.range(0.1, 0.5) * h))
            };
            streaks.push((p, q, sw, a));
        }
        let mut s = Sunprint { mask, pdc, blur, white, clear, round, factory, streaks, rng, u, w, h,
                               k: 1.0, phase: HOLD, t: 0.0, still: false };   // the first print is already developed
        s.arrange()?;
        Ok(s)
    }

    /// a new arrangement: two or three fronds and a scatter of leaves, as white silhouettes, then the soft mask
    fn arrange(&mut self) -> Result<()> {
        let (w, h, u) = (self.w, self.h, self.u);
        let pi = std::f32::consts::PI;
        unsafe {
            // the sharp silhouettes only live while the mask is made (a whole screen of memory saved)
            let (mut dx, mut dy) = (0.0f32, 0.0f32);
            self.mask.GetDpi(&mut dx, &mut dy);
            let props = D2D1_BITMAP_PROPERTIES1 { pixelFormat: self.mask.GetPixelFormat(), dpiX: dx, dpiY: dy,
                                                  bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET, colorContext: std::mem::ManuallyDrop::new(None) };
            let sharp = self.pdc.CreateBitmap(self.mask.GetPixelSize(), None, 0, &props)?;
            self.pdc.SetTarget(&sharp);
            self.pdc.BeginDraw();
            self.pdc.Clear(Some(&D2D1_COLOR_F::default()));
            for _ in 0..2 + (self.rng.f() * 2.0) as usize {
                let (x, y) = (self.rng.range(0.1, 0.9) * w, self.rng.range(0.15, 0.95) * h);
                let (a, l) = (self.rng.range(-pi, 0.0) - 0.3, self.rng.range(0.35, 0.6) * u);
                self.frond(x, y, a, l)?;
            }
            for _ in 0..4 + (self.rng.f() * 4.0) as usize {
                let (x, y) = (self.rng.range(0.0, w), self.rng.range(0.0, h));
                let (a, s) = (self.rng.range(0.0, pi * 2.0), self.rng.range(0.05, 0.11) * u);
                self.leaf(x, y, a, s)?;
            }
            self.pdc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
            self.pdc.EndDraw(None, None)?;
            self.pdc.SetTarget(&self.mask);
            self.pdc.BeginDraw();
            self.pdc.Clear(Some(&D2D1_COLOR_F::default()));
            self.blur.SetInput(0, &sharp, true);
            self.pdc.DrawImage(&self.blur.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            self.pdc.DrawBitmap(&sharp, Some(&rect(0.0, 0.0, w, h)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            self.pdc.EndDraw(None, None)?;
            self.blur.SetInput(0, None, true);   // let the sharp layer go
        }
        Ok(())
    }

    fn frond(&mut self, x: f32, y: f32, ang: f32, len: f32) -> Result<()> { unsafe {
        let n = 18 + (self.rng.f() * 8.0) as usize;
        let bend = self.rng.range(-0.5, 0.5);
        let (mut px, mut py, mut a) = (x, y, ang);
        let dc = &self.pdc;
        dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        let path = self.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(p2(px, py), D2D1_FIGURE_BEGIN_HOLLOW);
        let mut stem = Vec::new();
        for i in 0..=n {
            a += bend / n as f32;
            px += a.cos() * len / n as f32;
            py += a.sin() * len / n as f32;
            sink.AddLine(p2(px, py));
            stem.push((px, py, a, i as f32 / n as f32));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        dc.DrawGeometry(&path, &self.white, (len * 0.008).max(1.5), &self.round);
        // pinnae both sides, longest in the middle, each a row of small lobes
        for (sx, sy, sa, f) in stem {
            if f < 0.06 { continue; }
            let l = len * 0.28 * (std::f32::consts::PI * (f * 1.1).min(1.0)).sin() * (1.0 - f * 0.35);
            for side in [-1.0f32, 1.0] {
                let pa = sa + side * (1.1 - f * 0.3);
                let m = ((l / (len * 0.018)).round() as usize).max(3);
                for j in 0..m {
                    let t = (j as f32 + 0.5) / m as f32;
                    let r = len * 0.017 * (1.0 - t * 0.55);
                    dc.SetTransform(&turn(pa + side * 0.6, sx + pa.cos() * l * t, sy + pa.sin() * l * t));
                    dc.FillEllipse(&D2D1_ELLIPSE { point: p2(0.0, 0.0), radiusX: r * 1.5, radiusY: r }, &self.white);
                }
            }
        }
        Ok(())
    } }

    fn leaf(&mut self, x: f32, y: f32, ang: f32, s: f32) -> Result<()> { unsafe {
        let dc = &self.pdc;
        dc.SetTransform(&turn(ang, x, y));
        let path = self.factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(p2(-s, 0.0), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p2(-s * 0.4, -s * 0.55), point2: p2(s * 0.5, -s * 0.45), point3: p2(s, 0.0) });
        sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p2(s * 0.5, s * 0.45), point2: p2(-s * 0.4, s * 0.55), point3: p2(-s, 0.0) });
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        dc.FillGeometry(&path, &self.white, None);
        dc.DrawLine(p2(-s * 1.35, s * 0.05), p2(-s, 0.0), &self.white, (s * 0.03).max(1.0), &self.round);
        // the veins, cut out of the leaf (the page uses destination-out)
        dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
        let vw = (s * 0.025).max(1.0);
        dc.DrawLine(p2(-s * 0.9, 0.0), p2(s * 0.85, 0.0), &self.clear, vw, &self.round);
        for i in 1..6 {
            let vx = -s + i as f32 * s * 0.3;
            dc.DrawLine(p2(vx, 0.0), p2(vx + s * 0.22, -s * 0.28), &self.clear, vw, &self.round);
            dc.DrawLine(p2(vx, 0.0), p2(vx + s * 0.22, s * 0.28), &self.clear, vw, &self.round);
        }
        dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        Ok(())
    } }
}

impl Background for Sunprint {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 15.0 } }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;   // 0 stills it: the print as it is stays up
        if !self.still {
            let sp = 0.4 + level * 1.6;
            match self.phase {
                SET => { self.k += dt * sp * 0.12; if self.k >= 1.0 { self.k = 1.0; self.phase = HOLD; self.t = 0.0; } }
                HOLD => { self.t += dt * sp; if self.t > 30.0 { self.phase = OUT; } }
                _ => { self.k -= dt * sp * 0.2; if self.k <= 0.0 { self.k = 0.0; self.arrange()?; self.phase = SET; } }
            }
        }
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let ink = g.brush(look.ink)?;
            for (p, q, sw, a) in &self.streaks {
                ink.SetOpacity(*a);
                dc.DrawLine(*p, *q, &ink, *sw, &self.round);
            }
            ink.SetOpacity(0.2 * self.k);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.mask, &ink, Some(&rect(0.0, 0.0, self.w, self.h)), None);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        Ok(())
    }
}

/// a drawing grid with a title block; lines, circles and boxes draft themselves, get dimensioned, then fade. The grid
/// is a few hundred hairlines, drawn each frame (a palette change shows at once). 30 fps (it always has a shape or two).
pub struct Draft { shapes: Vec<Part>, wait: f32, rng: Rng, factory: ID2D1Factory, dashdot: ID2D1StrokeStyle,
                   mono: IDWriteTextFormat, mid: IDWriteTextFormat, big: IDWriteTextFormat,
                   only: Option<f32>, dims: bool, grid: u8, title: bool }   // options: one kind of shape, dimensions, grid fine/coarse/off, title block

enum Kind { Line(f32, bool), Circle(f32), Box(f32, f32, bool) }
struct Part { kind: Kind, x: f32, y: f32, t: f32, dur: f32, hold: f32 }

const DG: f32 = 24.0;

impl Draft {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Draft> {
        let factory = unsafe { g.dc.GetFactory()? };
        let props = D2D1_STROKE_STYLE_PROPERTIES { dashStyle: D2D1_DASH_STYLE_CUSTOM, ..Default::default() };
        let dashdot = unsafe { factory.CreateStrokeStyle(&props, Some(&[10.0, 3.0, 2.0, 3.0]))? };
        let fmt = |fam: &str, size: f32, centred: bool| -> Result<IDWriteTextFormat> {
            let f = fonts.format(fam, size, 400)?;
            unsafe {
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                if centred { f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?; }
            }
            Ok(f)
        };
        Ok(Draft { shapes: Vec::new(), wait: 0.0, rng: Rng(0xd2af_7000_0000_0001), factory, dashdot,
                   mono: fmt(&look.mono.family, 11.0, false)?, mid: fmt(&look.mono.family, 11.0, true)?,
                   big: fmt(&look.display.family, 17.6, false)?, only: None, dims: true, grid: 0, title: true })
    }

    fn spawn(&mut self, w: f32, h: f32) {
        let r = &mut self.rng;
        let snap = |v: f32| (v / DG).round() * DG;
        let (x, y) = (snap(r.range(w * 0.05, w * 0.9)), snap(r.range(h * 0.1, h * 0.9)));
        let (dur, hold) = (r.range(1.6, 3.2), r.range(4.0, 8.0));
        let k = self.only.unwrap_or_else(|| r.f());
        let kind = if k < 0.4 { Kind::Line(snap(r.range(DG * 4.0, DG * 16.0)), r.f() < 0.4) }
                   else if k < 0.7 { Kind::Circle(snap(r.range(DG * 1.5, DG * 6.0))) }
                   else { Kind::Box(snap(r.range(DG * 3.0, DG * 10.0)), snap(r.range(DG * 2.0, DG * 7.0)), r.f() < 0.4) };
        self.shapes.push(Part { kind, x, y, t: 0.0, dur, hold });
    }

    fn text(g: &Gfx, f: &IDWriteTextFormat, s: &str, x: f32, y: f32, centred: bool, b: &ID2D1SolidColorBrush) {
        let t: Vec<u16> = s.encode_utf16().collect();
        let r = if centred { rect(x - 150.0, y - 12.0, 300.0, 24.0) } else { rect(x, y - 12.0, 400.0, 24.0) };
        unsafe { g.dc.DrawText(&t, f, &r, b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL); }
    }
}

impl Background for Draft {
    fn fps(&self) -> f32 { 30.0 }
    fn opts(&mut self, o: &Json) -> bool {
        self.only = match o.get("shapes").str() { Some("lines") => Some(0.1), Some("circles") => Some(0.5), Some("boxes") => Some(0.9), _ => None };
        self.dims = o.get("dimensions").str() != Some("off");
        self.grid = match o.get("grid").str() { Some("coarse") => 1, Some("off") => 2, _ => 0 };
        self.title = o.get("title").str() != Some("off");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let (w, h) = (g.w as f32, g.h as f32);
        let pt = |x: f32, y: f32| D2D_POINT_2F { x, y };
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            // the grid: fine every 24, heavier every 120
            let dim = g.brush(look.dim)?;
            for &(step, a) in &[(DG, 0.1), (DG * 5.0, 0.22)][[0, 1, 2][self.grid as usize]..] {
                dim.SetOpacity(a);
                let mut x = 0.5; while x < w { dc.DrawLine(pt(x, 0.0), pt(x, h), &dim, 1.0, None); x += step; }
                let mut y = 0.5; while y < h { dc.DrawLine(pt(0.0, y), pt(w, y), &dim, 1.0, None); y += step; }
            }
        }
        if self.title { unsafe {
            let dc = &g.dc;
            // the title block, bottom right
            let (bw, bh) = ((w * 0.3).min(330.0), 88.0);
            let (bx, by) = (w - bw - DG, h - bh - DG);
            dc.FillRectangle(&rect(bx, by, bw, bh), &g.brush(look.bg)?);
            let hi = g.brush(look.hi)?;
            hi.SetOpacity(0.4);
            dc.DrawRectangle(&rect(bx + 0.5, by + 0.5, bw, bh), &hi, 1.0, None);
            for f in [0.36, 0.58, 0.8] { dc.DrawLine(pt(bx, by + bh * f + 0.5), pt(bx + bw, by + bh * f + 0.5), &hi, 1.0, None); }
            dc.DrawLine(pt(bx + bw * 0.55 + 0.5, by + bh * 0.58), pt(bx + bw * 0.55 + 0.5, by + bh), &hi, 1.0, None);
            hi.SetOpacity(0.6);
            Self::text(g, &self.big, "GLITCHWALL", bx + 10.0, by + bh * 0.18, false, &hi);
            for (s, x, y) in [("TITLE  LIVE TOOL FEED", 0.0, 0.47), ("DWG NO  GW-0047", 0.0, 0.69), ("REV  C", 1.0, 0.69),
                              ("SCALE  1:1", 0.0, 0.9), ("SHEET  1 OF 1", 1.0, 0.9)] {
                Self::text(g, &self.mono, s, bx + 10.0 + x * bw * 0.55, by + bh * y, false, &hi);
            }
        } }
        // a new shape every .4-1.6 s, up to 2 + level x 12 at once
        self.wait -= dt;
        if self.wait <= 0.0 && self.shapes.len() < (2.0 + level * 12.0).round() as usize {
            self.spawn(w, h);
            self.wait = self.rng.range(0.4, 1.6);
        }
        let ink = g.brush(look.hi)?;
        let mut keep = Vec::new();
        for mut s in std::mem::take(&mut self.shapes) {
            s.t += dt;
            let p = (s.t / s.dur).min(1.0);
            let end = s.dur + s.hold;
            let a = if s.t < end { 1.0 } else { 1.0 - (s.t - end) / 1.6 };
            if a <= 0.0 { continue; }
            let done = p >= 1.0;
            unsafe {
                let dc = &g.dc;
                ink.SetOpacity(0.5 * a);
                match s.kind {
                    Kind::Line(len, vert) => {
                        let (dx, dy) = if vert { (0.0, len * p) } else { (len * p, 0.0) };
                        dc.DrawLine(pt(s.x, s.y), pt(s.x + dx, s.y + dy), &ink, 1.0, None);
                        if done && self.dims {   // a dimension line beside it, with end ticks and the length
                            let (ox, oy) = if vert { (14.0, 0.0) } else { (0.0, -14.0) };
                            ink.SetOpacity(0.35 * a);
                            let (ex, ey) = if vert { (0.0, len) } else { (len, 0.0) };
                            dc.DrawLine(pt(s.x + ox, s.y + oy), pt(s.x + ox + ex, s.y + oy + ey), &ink, 1.0, None);
                            for e in [0.0, len] {
                                let (qx, qy) = if vert { (s.x, s.y + e) } else { (s.x + e, s.y) };
                                let (sx, sy) = if vert { (4.0, 0.0) } else { (0.0, -4.0) };
                                dc.DrawLine(pt(qx + sx, qy + sy), pt(qx + ox * 1.4, qy + oy * 1.4), &ink, 1.0, None);
                            }
                            ink.SetOpacity(0.6 * a);
                            let (lx, ly) = (s.x + ox * 2.0 + if vert { 8.0 } else { len / 2.0 }, s.y + oy * 1.9 + if vert { len / 2.0 } else { 0.0 });
                            Self::text(g, &self.mid, &format!("{}", len as i32), lx, ly, true, &ink);
                        }
                    }
                    Kind::Circle(r) => {
                        // the rim, drawn clockwise from the top
                        if done { dc.DrawEllipse(&D2D1_ELLIPSE { point: pt(s.x, s.y), radiusX: r, radiusY: r }, &ink, 1.0, None); }
                        else if p > 0.0 {
                            let at = |f: f32| { let an = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f; pt(s.x + r * an.cos(), s.y + r * an.sin()) };
                            let path = self.factory.CreatePathGeometry()?;
                            let sink = path.Open()?;
                            sink.BeginFigure(at(0.0), D2D1_FIGURE_BEGIN_HOLLOW);
                            let arc = |to: D2D_POINT_2F| D2D1_ARC_SEGMENT { point: to, size: D2D_SIZE_F { width: r, height: r }, rotationAngle: 0.0,
                                                                           sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL };
                            if p > 0.5 { sink.AddArc(&arc(at(0.5))); }
                            sink.AddArc(&arc(at(p)));
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            sink.Close()?;
                            dc.DrawGeometry(&path, &ink, 1.0, None);
                        }
                        // centre lines, dash-dot, past the rim
                        ink.SetOpacity(0.35 * a);
                        let c = r * 1.2 * p;
                        dc.DrawLine(pt(s.x - c, s.y), pt(s.x + c, s.y), &ink, 1.0, &self.dashdot);
                        dc.DrawLine(pt(s.x, s.y - c), pt(s.x, s.y + c), &ink, 1.0, &self.dashdot);
                        if done {
                            dc.DrawLine(pt(s.x, s.y), pt(s.x + r * 0.707, s.y - r * 0.707), &ink, 1.0, None);
                            ink.SetOpacity(0.6 * a);
                            if self.dims { Self::text(g, &self.mid, &format!("R{}", r as i32), s.x + r * 0.707 + 18.0, s.y - r * 0.707 - 8.0, true, &ink); }
                        }
                    }
                    Kind::Box(bw, bh, hatch) => {
                        // the outline, drawn round from the top left
                        let mut left = 2.0 * (bw + bh) * p;
                        let (mut x, mut y) = (s.x, s.y);
                        for (vx, vy) in [(bw, 0.0), (0.0, bh), (-bw, 0.0), (0.0, -bh)] {
                            if left <= 0.0 { break; }
                            let u = (vx + vy as f32).abs().min(left);
                            let (nx, ny) = (x + vx.signum() * u * (vx != 0.0) as i32 as f32, y + vy.signum() * u * (vy != 0.0) as i32 as f32);
                            dc.DrawLine(pt(x, y), pt(nx, ny), &ink, 1.0, None);
                            x = nx; y = ny; left -= u;
                        }
                        if done {
                            if hatch {   // section hatching at 45 degrees, inside the outline
                                dc.PushAxisAlignedClip(&rect(s.x, s.y, bw, bh), D2D1_ANTIALIAS_MODE_ALIASED);
                                ink.SetOpacity(0.22 * a);
                                let mut d = 0.0;
                                while d < bw + bh { dc.DrawLine(pt(s.x + d, s.y), pt(s.x + d - bh, s.y + bh), &ink, 1.0, None); d += 8.0; }
                                dc.PopAxisAlignedClip();
                            }
                            ink.SetOpacity(0.6 * a);
                            if self.dims { Self::text(g, &self.mid, &format!("{} × {}", bw as i32, bh as i32), s.x + bw / 2.0, s.y + bh + 12.0, true, &ink); }
                        }
                    }
                }
            }
            keep.push(s);
        }
        self.shapes = keep;
        Ok(())
    }
}

/// a field map: contour lines of made-up ground (every fifth heavier), a lettered grid, a north arrow and a scale
/// bar; a route draws itself from waypoint to waypoint to an objective, which pulses; then a new route. The map is
/// all one ink at different strengths, so it is drawn once as a mask and tinted each frame. 24 fps; still at 0.
pub struct Topo { mask: ID2D1Bitmap1, factory: ID2D1Factory, dash: ID2D1StrokeStyle, fmt: IDWriteTextFormat,
                  rng: Rng, u: f32, w: f32, h: f32, gs: f32, fs: f32,
                  pts: Vec<(f32, f32)>, refs: Vec<String>, len: f32, p: f32, hold: f32, fade: f32, still: bool,
                  route: bool, built: (usize, bool, bool) }   // options: the route; drawn into the map: contours, grid refs, markers

impl Topo {
    fn built(o: &Json) -> (usize, bool, bool) {
        (match o.get("contours").str() { Some("sparse") => 14, Some("dense") => 40, _ => 26 },
         o.get("refs").str() != Some("off"), o.get("markers").str() != Some("off"))
    }
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Topo> {
        let built = Self::built(o);
        let nc = built.0;
        let (w, h) = (g.w as f32, g.h as f32);
        let u = w.min(h);
        let light = look.light;
        let mut rng = Rng(0x70b0_f1e1_d000_0001);
        let mask = g.surface(w, h)?;
        let factory = unsafe { g.dc.GetFactory()? };
        let gs = (u / 5.0).round();
        let fs = (u / 70.0).round().max(10.0);
        let fmt = fonts.format(&look.mono.family, fs, 400)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&mask);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let white = g.brush([1.0, 1.0, 1.0, 1.0])?;
            // the ground: a sum of soft hills and hollows, contoured by marching squares
            let hills: Vec<(f32, f32, f32, f32)> = (0..9).map(|_| (rng.range(-0.1, 1.1) * w, rng.range(-0.1, 1.1) * h,
                                                                  rng.range(0.12, 0.32) * u * 1.6, rng.range(-0.7, 1.0))).collect();
            let ht = |x: f32, y: f32| hills.iter().map(|&(hx, hy, r, a)| a * (-((x - hx).powi(2) + (y - hy).powi(2)) / (r * r)).exp()).sum::<f32>();
            let c = (u / 90.0).round().max(8.0);
            let (nx, ny) = ((w / c).ceil() as usize + 1, (h / c).ceil() as usize + 1);
            let mut f = vec![0.0f32; nx * ny];
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for j in 0..ny { for i in 0..nx { let v = ht(i as f32 * c, j as f32 * c); f[j * nx + i] = v; lo = lo.min(v); hi = hi.max(v); } }
            let step = (hi - lo) / nc as f32;
            for k in 1..nc {
                let lv = lo + k as f32 * step;
                let heavy = k % 5 == 0;
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                for j in 0..ny - 1 {
                    for i in 0..nx - 1 {
                        let (a, b, d, e) = (f[j * nx + i], f[j * nx + i + 1], f[(j + 1) * nx + i], f[(j + 1) * nx + i + 1]);
                        let (x, y) = (i as f32 * c, j as f32 * c);
                        let mut pts: Vec<D2D_POINT_2F> = Vec::with_capacity(4);
                        if (a < lv) != (b < lv) { pts.push(p2(x + c * (lv - a) / (b - a), y)); }
                        if (b < lv) != (e < lv) { pts.push(p2(x + c, y + c * (lv - b) / (e - b))); }
                        if (d < lv) != (e < lv) { pts.push(p2(x + c * (lv - d) / (e - d), y + c)); }
                        if (a < lv) != (d < lv) { pts.push(p2(x, y + c * (lv - a) / (d - a))); }
                        let mut q = 0;
                        while q + 1 < pts.len() {
                            sink.BeginFigure(pts[q], D2D1_FIGURE_BEGIN_HOLLOW);
                            sink.AddLine(pts[q + 1]);
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            q += 2;
                        }
                    }
                }
                sink.Close()?;
                white.SetOpacity(if heavy { if light { 0.22 } else { 0.2 } } else if light { 0.1 } else { 0.09 });
                dc.DrawGeometry(&path, &white, if heavy { 1.4 } else { 1.0 }, None);
            }
            // the grid with its references
            white.SetOpacity(if light { 0.16 } else { 0.14 });
            let (gw, gh) = if built.1 { (w, h) } else { (0.0, 0.0) };   // without the grid: none of it
            let mut x = gs; while x < gw { let xx = x.round() + 0.5; dc.DrawLine(p2(xx, 0.0), p2(xx, h), &white, 1.0, None); x += gs; }
            let mut y = gs; while y < gh { let yy = y.round() + 0.5; dc.DrawLine(p2(0.0, yy), p2(w, yy), &white, 1.0, None); y += gs; }
            white.SetOpacity(0.45);
            let text = |s: &str, x: f32, y: f32, al: f32| {   // al: 0 left, .5 centre, 1 right of x; y is the top
                let t: Vec<u16> = s.encode_utf16().collect();
                let wd = 200.0;
                if let Ok(l) = fonts.dw.CreateTextLayout(&t, &fmt, wd, fs * 2.0) {
                    let _ = l.SetTextAlignment(if al == 0.0 { DWRITE_TEXT_ALIGNMENT_LEADING } else if al == 1.0 { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_CENTER });
                    dc.DrawTextLayout(p2(x - wd * al, y), &l, &white, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
            };
            let (mut x, mut n) = (0.0, 0u8); while x < gw { text(&((b'A' + n % 26) as char).to_string(), x + fs * 0.4, fs * 0.4, 0.0); x += gs; n += 1; }
            let (mut y, mut n) = (gs, 1); while y < gh { text(&format!("{n:02}"), fs * 0.4, y + fs * 0.4, 0.0); y += gs; n += 1; }
            // north arrow and scale bar, bottom right
            if built.2 {
            let (ax, ay) = (w - u * 0.07, h - u * 0.16);
            white.SetOpacity(0.5);
            let arrow = factory.CreatePathGeometry()?;
            let sink = arrow.Open()?;
            sink.BeginFigure(p2(ax, ay - u * 0.05), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(p2(ax + u * 0.015, ay)); sink.AddLine(p2(ax, ay - u * 0.012)); sink.AddLine(p2(ax - u * 0.015, ay));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            dc.FillGeometry(&arrow, &white, None);
            text("N", ax, ay + fs * 0.4, 0.5);
            let (sx, sy) = (w - u * 0.3, h - u * 0.05);
            for i in 0..4 { white.SetOpacity(if i % 2 == 1 { 0.2 } else { 0.5 }); dc.FillRectangle(&rect(sx + i as f32 * u * 0.05, sy, u * 0.05, fs * 0.4), &white); }
            white.SetOpacity(0.45);
            text("0", sx, sy + fs * 0.8, 0.0);
            text("2 KM", sx + u * 0.2, sy + fs * 0.8, 1.0);
            }
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let lw = (u / 400.0).max(1.5);
        let props = D2D1_STROKE_STYLE_PROPERTIES { dashStyle: D2D1_DASH_STYLE_CUSTOM, ..Default::default() };
        let dash = unsafe { factory.CreateStrokeStyle(&props, Some(&[u / 90.0 / lw, u / 140.0 / lw]))? };
        let mut t = Topo { mask, factory, dash, fmt, rng, u, w, h, gs, fs, pts: Vec::new(), refs: Vec::new(), len: 0.0,
                           p: 0.0, hold: 0.0, fade: 1.0, still: false, route: true, built };
        t.route();
        t.p = t.len;   // the first route is already drawn; later ones draw themselves
        Ok(t)
    }

    /// waypoints in a loose line across the map, ending at the objective
    fn route(&mut self) {
        let (w, h) = (self.w, self.h);
        let m = self.u * 0.12;
        let n = 4 + (self.rng.f() * 3.0) as usize;
        let (mut x, mut y) = (self.rng.range(m, w * 0.3), self.rng.range(m, h - m));
        self.pts.clear();
        for _ in 0..n {
            self.pts.push((x, y));
            x += self.rng.range(0.1, 0.2) * w;
            y = (y + self.rng.range(-0.25, 0.25) * h).clamp(m, h - m);
        }
        self.len = self.pts.windows(2).map(|s| ((s[1].0 - s[0].0).powi(2) + (s[1].1 - s[0].1).powi(2)).sqrt()).sum();
        let gs = self.gs;
        self.refs = self.pts.iter().map(|&(px, py)| format!("{}{:02}", (b'A' + ((px / gs) as u32 % 26) as u8) as char, (py / gs) as u32 + 1)).collect();
        self.p = 0.0; self.hold = 0.0; self.fade = 1.0;
    }
}

impl Background for Topo {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 24.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.route = o.get("route").str() != Some("off");
        Self::built(o) != self.built
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0 || !self.route;   // just the map: drawn once
        if !self.still {
            let sp = 0.4 + level * 1.6;
            if self.p < self.len { self.p += dt * sp * self.u * 0.06; }
            else { self.hold += dt * sp; if self.hold > 14.0 { self.fade -= dt * 0.5; if self.fade <= 0.0 { self.route(); } } }
        }
        let (fs, s) = (self.fs, self.u / 110.0);
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let ink = g.brush(look.ink)?;
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.mask, &ink, Some(&rect(0.0, 0.0, self.w, self.h)), None);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            if !self.route { return Ok(()); }
            // the route so far, dashed
            let acc = g.brush(look.acc)?;
            acc.SetOpacity(0.85 * self.fade);
            let lw = (self.u / 400.0).max(1.5);
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let pts = &self.pts;
            sink.BeginFigure(p2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
            let (mut left, mut reached) = (self.p, 0);
            for i in 1..pts.len() {
                if left <= 0.0 { break; }
                let ((ax, ay), (bx, by)) = (pts[i - 1], pts[i]);
                let d = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
                let f = (left / d).min(1.0);
                sink.AddLine(p2(ax + (bx - ax) * f, ay + (by - ay) * f));
                left -= d;
                if f >= 1.0 { reached = i; }
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            dc.DrawGeometry(&path, &acc, lw, &self.dash);
            let label = |t: &str, x: f32, y: f32, b: &ID2D1SolidColorBrush| {
                let t: Vec<u16> = t.encode_utf16().collect();
                let f = &self.fmt;
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                dc.DrawText(&t, f, &rect(x, y - fs, 400.0, fs * 2.0), b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            };
            let inkt = g.brush(look.ink)?;
            inkt.SetOpacity(0.7 * self.fade);
            for i in 0..=reached {
                let (x, y) = pts[i];
                acc.SetOpacity(0.9 * self.fade);
                if i == pts.len() - 1 {   // the objective: a ringed cross, pulsing
                    let k = 0.5 + 0.5 * (self.hold * 4.0).sin();
                    dc.DrawEllipse(&D2D1_ELLIPSE { point: p2(x, y), radiusX: s * (2.0 + k), radiusY: s * (2.0 + k) }, &acc, lw, None);
                    dc.DrawLine(p2(x - s * 3.5, y), p2(x + s * 3.5, y), &acc, lw, None);
                    dc.DrawLine(p2(x, y - s * 3.5), p2(x, y + s * 3.5), &acc, lw, None);
                    label(&format!("OBJ  {}", self.refs[i]), x + s * 4.5, y - s * 3.0, &acc);
                } else {
                    let tri = self.factory.CreatePathGeometry()?;
                    let sk = tri.Open()?;
                    sk.BeginFigure(p2(x, y - s * 1.6), D2D1_FIGURE_BEGIN_HOLLOW);
                    sk.AddLine(p2(x + s * 1.4, y + s)); sk.AddLine(p2(x - s * 1.4, y + s));
                    sk.EndFigure(D2D1_FIGURE_END_CLOSED);
                    sk.Close()?;
                    dc.DrawGeometry(&tri, &acc, lw, None);
                    label(&format!("WP{}  {}", i + 1, self.refs[i]), x + s * 2.4, y - s * 1.8, &inkt);
                }
            }
        }
        Ok(())
    }
}

/// an amber radar screen: range rings and bearing ticks on dark glass, a sweep turning, and what it finds (ground
/// clutter, a few slow contacts with their tracks) lit as the beam passes and fading until it comes round again.
/// The glass is one mask (tinted each frame); what the beam lights goes into a phosphor layer kept by a second
/// device context, faded a little each frame; the trailing glow is one wedge mask drawn turned. 30 fps; still at 0.
pub struct Radar { glass: ID2D1Bitmap1, wedge: ID2D1Bitmap1, pc: ID2D1Bitmap1, pdc: ID2D1DeviceContext, flood: ID2D1Effect,
                   fmt: IDWriteTextFormat, r: f32, ox: f32, oy: f32, fs: f32, w: f32, h: f32, a: f32, still: bool,
                   clut: Vec<(f32, f32, f32)>, tgts: Vec<Contact>, rng: Rng, dir: f32, built: (bool, bool, bool) }   // built: rings, clutter, many contacts

struct Contact { x: f32, y: f32, hd: f32, v: f32, id: String, seen: bool, trail: Vec<(f32, f32)> }

const TAU: f32 = std::f32::consts::TAU;

impl Radar {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Radar> {
        let built = (o.get("rings").str() != Some("off"), o.get("clutter").str() != Some("off"), o.get("contacts").str() == Some("many"));
        let dir = if o.get("sweep").str() == Some("anticlockwise") { -1.0 } else { 1.0 };
        let (w, h) = (g.w as f32, g.h as f32);
        let r = w.min(h) * 0.44;
        let (ox, oy) = (w / 2.0, h / 2.0);
        let light = look.light;
        let fs = (r * 0.035).round().max(10.0);
        let fmt = fonts.format(&look.mono.family, fs, 400)?;
        unsafe {
            fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
        let mut rng = Rng(0x7ada_2000_0000_0001);
        let (glass, pc) = (g.surface(w, h)?, g.surface(w, h)?);
        let wd = (r + 2.0).ceil();
        let wedge = g.surface(wd, wd * 2.0)?;   // the glow: origin at its left middle, pointing right, trailing upwards
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            let white = g.brush([1.0, 1.0, 1.0, 1.0])?;
            let pos = |rr: f32, a: f32| p2(ox + a.sin() * rr, oy - a.cos() * rr);
            dc.SetTarget(&glass);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            // the faint glow of the glass, then rings, spokes, ticks and bearings
            let rt: &ID2D1RenderTarget = dc;
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if light { 0.06 } else { 0.05 } } },
                         D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F::default() }];
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let glow = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(ox, oy), gradientOriginOffset: p2(0.0, 0.0),
                                                                                         radiusX: r * 1.1, radiusY: r * 1.1 }, None, &coll)?;
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &glow);
            for i in if built.0 { 1 } else { 5 }..=5 {   // without the rings: just the rim
                white.SetOpacity(if i == 5 { 0.32 } else { 0.14 });
                let rr = r * i as f32 / 5.0;
                dc.DrawEllipse(&D2D1_ELLIPSE { point: p2(ox, oy), radiusX: rr, radiusY: rr }, &white, 1.0, None);
            }
            white.SetOpacity(0.08);
            for k in 0..if built.0 { 12 } else { 0 } { dc.DrawLine(p2(ox, oy), pos(r, (k * 30) as f32 * TAU / 360.0), &white, 1.0, None); }
            white.SetOpacity(0.3);
            for k in 0..72 {
                let deg = k * 5;
                let l = if deg % 30 != 0 { r * 0.02 } else { r * 0.045 };
                let t = deg as f32 * TAU / 360.0;
                dc.DrawLine(pos(r, t), pos(r + l, t), &white, 1.0, None);
            }
            white.SetOpacity(0.45);
            for k in 0..12 {
                let deg = k * 30;
                let at = pos(r + fs * 2.0, deg as f32 * TAU / 360.0);
                let s: Vec<u16> = format!("{deg:03}").encode_utf16().collect();
                let l = fonts.dw.CreateTextLayout(&s, &fmt, 100.0, fs * 2.0)?;
                l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                dc.DrawTextLayout(p2(at.x - 50.0, at.y - fs), &l, &white, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            dc.EndDraw(None, None)?;
            // the wedge of glow trailing the beam: .9 rad, brightest at the beam, as thin slices
            dc.SetTarget(&wedge);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let factory = dc.GetFactory()?;
            let n = 40;
            for k in 0..n {
                let (a0, a1) = (-0.9 + 0.9 * k as f32 / n as f32, -0.9 + 0.9 * (k + 1) as f32 / n as f32 + 0.004);
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                sink.BeginFigure(p2(0.0, wd), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(p2(r * a0.cos(), wd + r * a0.sin()));
                sink.AddArc(&D2D1_ARC_SEGMENT { point: p2(r * a1.cos(), wd + r * a1.sin()), size: D2D_SIZE_F { width: r, height: r },
                                                rotationAngle: 0.0, sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                white.SetOpacity((k as f32 + 0.5) / n as f32);
                dc.FillGeometry(&path, &white, None);
            }
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        // ground clutter: a few patches of land, speckled; and six slow contacts
        let mut clut = Vec::new();
        for _ in 0..5 {
            let (ca, cr) = (rng.range(0.0, TAU), rng.range(0.35, 0.9));
            let n = 90 + (rng.f() * 160.0) as usize;
            let (sa, sr) = (rng.range(0.08, 0.3), rng.range(0.04, 0.1));
            for _ in 0..n {
                let rr = (cr + (rng.f() + rng.f() - 1.0) * sr).clamp(0.05, 0.98);
                let a = ca + (rng.f() + rng.f() - 1.0) * sa / rr * 0.4;
                clut.push((a.rem_euclid(TAU), rr, rng.range(0.25, 0.9)));
            }
        }
        let tgts = (0..if built.2 { 14 } else { 6 }).map(|_| Contact { x: rng.range(-0.8, 0.8), y: rng.range(-0.8, 0.8), hd: rng.range(0.0, TAU), v: rng.range(0.004, 0.012),
                                            id: format!("TK {}", 100 + (rng.f() * 800.0) as u32), seen: false, trail: Vec::new() }).collect();
        let pdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        let flood = unsafe { pdc.CreateEffect(&CLSID_D2D1Flood)? };
        unsafe { pdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); pdc.SetTarget(&pc); }
        let a = rng.range(0.0, TAU);
        if !built.1 { clut.clear(); }
        let mut rd = Radar { glass, wedge, pc, pdc, flood, fmt, r, ox, oy, fs, w, h, a, still: false, clut, tgts, rng, dir, built };
        // the stilled frame: one turn's worth already swept, older bearings dimmer
        unsafe {
            rd.pdc.BeginDraw();
            rd.pdc.Clear(Some(&D2D1_COLOR_F::default()));
            for i in 0..64 {
                let f = (i + 1) as f32 / 64.0;
                if dir > 0.0 { rd.sweep(look, a - TAU + i as f32 * TAU / 64.0, a - TAU + (i + 1) as f32 * TAU / 64.0, f)?; }
                else { rd.sweep(look, a + TAU - (i + 1) as f32 * TAU / 64.0, a + TAU - i as f32 * TAU / 64.0, f)?; }
            }
            rd.pdc.EndDraw(None, None)?;
        }
        Ok(rd)
    }

    fn pos(&self, rr: f32, a: f32) -> (f32, f32) { (self.ox + a.sin() * rr * self.r, self.oy - a.cos() * rr * self.r) }

    /// light what lies between bearings a0 and a1 into the phosphor layer (its BeginDraw is open); f scales brightness
    fn sweep(&mut self, look: &Look, a0: f32, a1: f32, f: f32) -> Result<()> { unsafe {
        let in_arc = |a: f32| (a - a0).rem_euclid(TAU) < a1 - a0;
        let dc = &self.pdc;
        dc.SetPrimitiveBlend(if look.light { D2D1_PRIMITIVE_BLEND_SOURCE_OVER } else { D2D1_PRIMITIVE_BLEND_ADD });
        let ink = dc.CreateSolidColorBrush(&rgba(look.ink), None)?;
        let cs = (self.r * 0.007).max(2.0);
        for &(ca, cr, v) in &self.clut {
            if !in_arc(ca) { continue; }
            let (x, y) = self.pos(cr, ca);
            ink.SetOpacity((v * f).min(1.0));
            dc.FillRectangle(&rect(x - cs / 2.0, y - cs / 2.0, cs, cs), &ink);
        }
        let s = (self.r * 0.011).max(2.5);
        let rt: &ID2D1RenderTarget = dc;
        for i in 0..self.tgts.len() {
            let (tx, ty) = (self.tgts[i].x, self.tgts[i].y);
            let rr = (tx * tx + ty * ty).sqrt();
            let a = tx.atan2(-ty).rem_euclid(TAU);
            if rr > 0.97 || !in_arc(a) { continue; }
            let (x, y) = self.pos(rr, a);
            let c = |col: Color, al: f32| D2D1_COLOR_F { r: col[0], g: col[1], b: col[2], a: al };
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: c(look.hi, f) },
                         D2D1_GRADIENT_STOP { position: 0.35, color: c(look.ink, 0.7 * f) },
                         D2D1_GRADIENT_STOP { position: 1.0, color: c(look.ink, 0.0) }];
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let blip = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(x, y), gradientOriginOffset: p2(0.0, 0.0),
                                                                                         radiusX: s * 2.4, radiusY: s * 2.4 }, None, &coll)?;
            dc.FillRectangle(&rect(x - s * 2.4, y - s * 2.4, s * 4.8, s * 4.8), &blip);
            let t = &mut self.tgts[i];
            t.trail.insert(0, (x, y));
            t.trail.truncate(6);
            t.seen = true;
        }
        dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
        Ok(())
    } }
}

impl Background for Radar {
    fn opts(&mut self, o: &Json) -> bool {
        self.dir = if o.get("sweep").str() == Some("anticlockwise") { -1.0 } else { 1.0 };
        (o.get("rings").str() != Some("off"), o.get("clutter").str() != Some("off"), o.get("contacts").str() == Some("many")) != self.built
    }
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if !self.still {
            let sp = 0.4 + level * 1.6;
            unsafe {
                self.pdc.BeginDraw();
                // the phosphor fades: everything lit loses a little each frame (the page's destination-out)
                let k = 1.0 - (-dt * sp * 0.12).exp();
                crate::gfx::set_floats(&self.flood, D2D1_FLOOD_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, &[0.0, 0.0, 0.0, k])?;
                self.pdc.DrawImage(&self.flood.GetOutput()?, None, Some(&rect(0.0, 0.0, self.w, self.h)),
                                   D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_DESTINATION_OUT);
                // contacts drift on their headings, and turn now and then
                for i in 0..self.tgts.len() {
                    let turn = self.rng.f() < dt * 0.05;
                    let dh = self.rng.range(-0.6, 0.6);
                    let t = &mut self.tgts[i];
                    t.x += t.hd.sin() * t.v * dt * sp;
                    t.y -= t.hd.cos() * t.v * dt * sp;
                    if turn { t.hd += dh; }
                    if (t.x * t.x + t.y * t.y).sqrt() > 1.05 { t.x *= -0.9; t.y *= -0.9; t.trail.clear(); t.seen = false; }
                }
                let a0 = self.a;
                self.a += dt * sp * 0.6 * self.dir;
                if self.dir > 0.0 { self.sweep(look, a0, self.a, 1.0)?; } else { self.sweep(look, self.a, a0, 1.0)?; }
                self.pdc.EndDraw(None, None)?;
            }
        }
        let (r, fs, ox, oy) = (self.r, self.fs, self.ox, self.oy);
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let ink = g.brush(look.ink)?;
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.glass, &ink, Some(&rect(0.0, 0.0, self.w, self.h)), None);
            dc.DrawBitmap(&self.pc, Some(&rect(0.0, 0.0, self.w, self.h)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            // the glow trailing the beam, turned to where the beam points (screen angle a - 90 degrees)
            let wd = (r + 2.0).ceil();
            let (s, c) = (self.a - TAU / 4.0).sin_cos();
            // anticlockwise: the wedge mirrored, trailing the other way
            let (m21, m22) = if self.dir > 0.0 { (-s, c) } else { (s, -c) };
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: m21, M22: m22, M31: ox, M32: oy });
            ink.SetOpacity(if look.light { 0.12 } else { 0.16 });
            dc.FillOpacityMask(&self.wedge, &ink, Some(&rect(0.0, -wd, wd, wd * 2.0)), None);
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let (ex, ey) = self.pos(1.0, self.a);
            dc.DrawLine(p2(ox, oy), p2(ex, ey), &g.brush([look.hi[0], look.hi[1], look.hi[2], 0.85])?, 1.5, None);
            // the contacts' tracks and tags, for those the beam has found
            let text = |s: &str, x: f32, y: f32, right: bool, b: &ID2D1SolidColorBrush| {
                let t: Vec<u16> = s.encode_utf16().collect();
                let rc = if right { rect(x - 400.0, y - fs, 400.0, fs * 2.0) } else { rect(x, y - fs, 400.0, fs * 2.0) };
                let _ = self.fmt.SetTextAlignment(if right { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING });
                dc.DrawText(&t, &self.fmt, &rc, b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            };
            for t in &self.tgts {
                if !t.seen || t.trail.is_empty() { continue; }
                for (i, &(x, y)) in t.trail.iter().enumerate().skip(1) {
                    ink.SetOpacity(0.4 * (1.0 - i as f32 / 6.0));
                    dc.FillRectangle(&rect(x - 1.0, y - 1.0, 2.0, 2.0), &ink);
                }
                let (x, y) = t.trail[0];
                ink.SetOpacity(0.55);
                dc.DrawRectangle(&rect(x - fs * 0.5, y - fs * 0.5, fs, fs), &ink, 1.0, None);
                ink.SetOpacity(0.7);
                text(&t.id, x + fs * 0.9, y - fs * 0.5, false, &ink);
            }
            ink.SetOpacity(0.5);
            text("RANGE  40 NM", ox - r - fs * 6.0, oy + r + fs * 1.5, false, &ink);
            let brg = (self.a.rem_euclid(TAU) * 360.0 / TAU).round() as u32 % 360;
            let seen = self.tgts.iter().filter(|t| t.seen).count();
            text(&format!("BRG {brg:03}°   TRK {seen:02}"), ox + r + fs * 6.0, oy + r + fs * 1.5, true, &ink);
            let _ = self.fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
        }
        Ok(())
    }
}

/// a ribbed black sphere sunk in the floor of a vast, dark data hall: rows of racks to the horizon with status lights
/// blinking, the sphere's slats lit by the room, and on its shell rows of red lights that flicker, with packets
/// streaming along them. The hall and the floor in front of the opening are drawn once (in the theme's colors); each
/// frame draws the rack lights and the glows as sprite batches, and a geodesic cage over the shell. 20 fps; still at 0. (The page's readouts: not yet.)
pub struct Orb { back: ID2D1Bitmap1, front: ID2D1Bitmap1, glow: ID2D1Bitmap1, dot: ID2D1Bitmap1, batch: ID2D1SpriteBatch,
                 dc3: ID2D1DeviceContext3, factory: ID2D1Factory, sphere: [ID2D1Brush; 3], rim: ID2D1Brush,
                 leds: Vec<Led>, ribs: Vec<Rib>, packets: Vec<(usize, f32, f32, usize)>, rng: Rng,
                 s: f32, ox: f32, oy: f32, w: f32, h: f32, t: f32, spin: f32, still: bool,
                 dest: Vec<D2D_RECT_F>, tint: Vec<D2D1_COLOR_F>, net: Vec<[f32; 3]>, tris: Vec<[u16; 3]>, mesh: ID2D1Bitmap1, mdc: ID2D1DeviceContext, mdc3: ID2D1DeviceContext3,
                 mbatch: ID2D1SpriteBatch, hole: ID2D1Bitmap1, holes: ID2D1Bitmap1, edge: ID2D1Bitmap1, cage: f32,
                 turns: (f32, f32), caged: bool, calm: bool }   // how fast the sphere and the cage turn, from the options: 1 right, -1 left, 0 still

struct Led { x: f32, y: f32, s: f32, a: f32, red: bool, kind: u8, f: f32, p: f32 }
struct Rib { phi: f32, n: usize, o: f32, base: Vec<f32>, b: Vec<f32> }

const ORB_L: usize = 30;
const ORB_N: usize = 110;
const ORB_NET: usize = 14;
const CAGE_R: f32 = 1.04;
const CAGE_RIM: f32 = 1.0;   // how strongly the holes' rims catch the light: 1 full, 0 none   // the cage stands a little off the sphere, so it slides over what's under it as it turns
const HOLE_IN: f32 = 0.06;   // the hole texture's triangle: (a, a), (c, a), (a, c), a = HOLE_IN, c = 1 - 2.5a

/// a geodesic net on the unit sphere (an icosahedron, each face cut into n x n triangles): its joints and cells
fn geodesic(n: usize) -> (Vec<[f32; 3]>, Vec<[u16; 3]>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let v: [[f32; 3]; 12] = [[-1.0, t, 0.0], [1.0, t, 0.0], [-1.0, -t, 0.0], [1.0, -t, 0.0], [0.0, -1.0, t], [0.0, 1.0, t],
                             [0.0, -1.0, -t], [0.0, 1.0, -t], [t, 0.0, -1.0], [t, 0.0, 1.0], [-t, 0.0, -1.0], [-t, 0.0, 1.0]];
    let faces = [[0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
                 [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1]];
    let mut pts: Vec<[f32; 3]> = Vec::new();
    let mut seen = std::collections::HashMap::new();
    let mut tris = Vec::new();
    let nf = n as f32;
    for [a, b, c] in faces {
        // i/n of the way from corner a to b, j/n from a to c, pushed out onto the sphere
        let mut at = |i: usize, j: usize| -> u16 {
            let (i, j) = (i as f32, j as f32);
            let p: [f32; 3] = std::array::from_fn(|d| (v[a][d] * (nf - i - j) + v[b][d] * i + v[c][d] * j) / nf);
            let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            let key = p.map(|x| (x / l * 1e4).round() as i32);
            *seen.entry(key).or_insert_with(|| { pts.push(p.map(|x| x / l)); (pts.len() - 1) as u16 })
        };
        for i in 0..n {
            for j in 0..n - i {
                tris.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
                if i + j < n - 1 { tris.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]); }
            }
        }
    }
    (pts, tris)
}

fn mixc(p: Color, q: Color, t: f32) -> Color { [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t, 1.0] }
fn ca(c: Color, a: f32) -> D2D1_COLOR_F { D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a } }

impl Orb {
    fn new(g: &Gfx, look: &Look) -> Result<Orb> {
        let (w, h) = (g.w as f32, g.h as f32);
        // the hall is laid out from s0; the sphere (radius s) is smaller than that, sitting in a smaller opening
        let s0 = w.min(h) * 0.29;
        let s = s0 * 0.75;
        let (ox, oy0) = (w / 2.0, h * 0.44);
        let light = look.light;
        let mut rng = Rng(0x0eb0_5f1e_0000_0001);
        let (ink, bg, acc) = (look.ink, look.bg, look.acc);
        let body: Color = if light { ink } else { [0.0, 0.0, 0.0, 1.0] };
        let (cy, cr, ce) = (oy0 + s0 * 0.45, s * 0.96, 0.24f32);
        let oy = cy - s * 0.45;
        let vy = oy0 - s0 * 0.38;
        let (hc, rh, ceil) = (5.0f32, 2.2f32, 8.0f32);
        let f = (cy - vy) * 10.0 / hc;
        let sx = |x: f32, z: f32| ox + f * x / z;
        let sy = |y: f32, z: f32| vy + f * (hc - y) / z;
        let back = g.surface(w, h)?;
        let front = g.surface(w, h)?;
        let factory = unsafe { g.dc.GetFactory()? };
        let mut leds = Vec::new();
        let rt: &ID2D1RenderTarget = &g.dc;
        let grad = |stops: &[(f32, D2D1_COLOR_F)]| -> Result<ID2D1GradientStopCollection> {
            let st: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|&(position, color)| D2D1_GRADIENT_STOP { position, color }).collect();
            unsafe { rt.CreateGradientStopCollection(&st, D2D1_GAMMA_1_0, D2D1_EXTEND_MODE_CLAMP) }
        };
        let radial = |cx: f32, cy: f32, offx: f32, offy: f32, rx: f32, ry: f32, stops: &[(f32, D2D1_COLOR_F)]| -> Result<ID2D1RadialGradientBrush> {
            unsafe { g.dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(cx, cy), gradientOriginOffset: p2(offx, offy),
                                                                                         radiusX: rx, radiusY: ry }, None, &grad(stops)?) }
        };
        let quad = |pts: [D2D_POINT_2F; 4]| -> Result<ID2D1PathGeometry> {
            unsafe {
                let p = factory.CreatePathGeometry()?;
                let k = p.Open()?;
                k.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_FILLED);
                for q in &pts[1..] { k.AddLine(*q); }
                k.EndFigure(D2D1_FIGURE_END_CLOSED);
                k.Close()?;
                Ok(p)
            }
        };
        let ell = |r: f32| D2D1_ELLIPSE { point: p2(ox, cy), radiusX: r, radiusY: r * ce };
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&back);
            dc.BeginDraw();
            dc.Clear(Some(&rgba(bg)));
            // a faint haze lying along the horizon
            let lin = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p2(0.0, 0.0), endPoint: p2(0.0, h) }, None,
                &grad(&[(0.0, ca(ink, 0.0)), (vy / h * 0.8, ca(ink, if light { 0.03 } else { 0.015 })), (vy / h, ca(ink, if light { 0.09 } else { 0.06 })),
                        ((cy / h).min(1.0), ca(ink, if light { 0.03 } else { 0.02 })), (1.0, ca(ink, 0.0))])?)?;
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &lin);
            let pen = g.brush(ink)?;
            // the ceiling: long strips of light running away from us, most dark, the rest barely on
            let mut x = -48.75f32;
            while x <= 48.75 {
                pen.SetOpacity(if light { 0.05 } else { 0.035 });
                dc.DrawLine(p2(sx(x, 4.0), sy(ceil, 4.0)), p2(sx(x, 400.0), sy(ceil, 400.0)), &pen, 1.0, None);
                let mut z = 6.0f32;
                while z < 260.0 {
                    if rng.f() >= 0.2 {
                        let z2 = z + 1.6;
                        pen.SetOpacity((if light { 0.14 } else { 0.2 }) * (30.0 / z).min(1.0) * rng.range(0.6, 1.0));
                        dc.DrawLine(p2(sx(x, z), sy(ceil, z)), p2(sx(x, z2), sy(ceil, z2)), &pen, (f * 0.12 / z).clamp(1.0, 2.5), None);
                    }
                    z += 4.5;
                }
                x += 7.5;
            }
            // the floor of the clearing: tiles running toward the racks
            pen.SetOpacity(if light { 0.06 } else { 0.045 });
            let mut x = -40.0f32; while x <= 40.0 { dc.DrawLine(p2(sx(x, 2.5), sy(0.0, 2.5)), p2(sx(x, 20.0), sy(0.0, 20.0)), &pen, 1.0, None); x += 1.2; }
            let mut z = 3.0f32; while z < 20.0 { dc.DrawLine(p2(0.0, sy(0.0, z)), p2(w, sy(0.0, z)), &pen, 1.0, None); z *= 1.12; }
            // on the floor round the opening: rings, and red light spilling; drawn before the racks, which stand on the
            // floor and hide what lies behind them
            for (r, a) in [(1.14, 0.2), (1.36, 0.1), (1.75, 0.06), (2.4, 0.04)] { pen.SetOpacity(a); dc.DrawEllipse(&ell(cr * r), &pen, 1.0, None); }
            let spill = g.brush([acc[0], acc[1], acc[2], 0.15 / 24.0])?;
            for i in 0..24 { dc.FillEllipse(&ell(cr * (2.6 - 1.6 * i as f32 / 24.0)), &spill); }
            // rows of racks to the horizon, broken by cross aisles; far ones sink into the haze
            let fog = if light { bg } else { mixc(bg, ink, 0.06) };
            let near = if light { mixc(bg, ink, 0.12) } else { mixc(bg, ink, 0.025) };
            let (seg, gapx) = (6.6f32, 2.2f32);
            let fill = g.brush(ink)?;
            for k in (0..=60).rev() {
                let kf = k as f32;
                let z = 20.0 + kf * 2.6 * (1.0 + kf * 0.012);
                let d = z + 1.1;
                let haze = ((z - 20.0) / 160.0).min(1.0).powf(0.8);
                let (yt, yb, ytb) = (sy(rh, z), sy(0.0, z), sy(rh, d));
                if yb - yt < 1.2 { continue; }
                let face = mixc(near, fog, haze);
                let top = mixc(mixc(near, ink, if light { 0.06 } else { 0.09 }), fog, haze);
                let half = (ox + 40.0) * z / f;
                let mut x0 = -(half / (seg + gapx)).ceil() * (seg + gapx) - gapx / 2.0;
                while x0 < half {
                    let x1 = x0 + seg;
                    let (l, r) = (sx(x0, z), sx(x1, z));
                    if !(r < 0.0 || l > w) {
                        fill.SetColor(&rgba(top));
                        dc.FillGeometry(&quad([p2(sx(x0, d), ytb), p2(sx(x1, d), ytb), p2(r, yt), p2(l, yt)])?, &fill, None);
                        fill.SetColor(&rgba(face));
                        dc.FillRectangle(&rect(l, yt, r - l, yb - yt), &fill);
                        fill.SetColor(&rgba(mixc(face, bg, 0.5)));   // the run's end faces, seen down the cross aisles
                        if x0 > 0.0 { dc.FillGeometry(&quad([p2(l, yt), p2(sx(x0, d), ytb), p2(sx(x0, d), sy(0.0, d)), p2(l, yb)])?, &fill, None); }
                        if x1 < 0.0 { dc.FillGeometry(&quad([p2(r, yt), p2(sx(x1, d), ytb), p2(sx(x1, d), sy(0.0, d)), p2(r, yb)])?, &fill, None); }
                        pen.SetOpacity((if light { 0.22 } else { 0.2 }) * (1.0 - haze * 0.7));
                        dc.DrawLine(p2(l, yt + 0.5), p2(r, yt + 0.5), &pen, 1.0, None);
                        let rw = f * 0.6 / z;
                        if rw > 3.5 {   // seams between the racks
                            pen.SetOpacity((if light { 0.1 } else { 0.07 }) * (1.0 - haze));
                            let mut xx = x0 + 0.6;
                            while xx < x1 - 0.1 { let q = sx(xx, z).round() + 0.5; dc.DrawLine(p2(q, yt), p2(q, yb), &pen, 1.0, None); xx += 0.6; }
                        }
                        // status lights on the rack fronts, only where the next row nearer us leaves them showing
                        if z < 70.0 && rw > 1.5 {
                            let nk = kf - 1.0;
                            let yhide = if k > 0 { yb.min(sy(rh, 20.0 + nk * 2.6 * (1.0 + nk * 0.012) + 1.1)) } else { yb };
                            let u = (yb - yt) / 22.0;
                            let mut rr = 0.0f32;
                            while rr < seg / 0.6 - 0.5 {
                                let (lx, lx2) = (sx(x0 + rr * 0.6 + 0.1, z).round(), sx(x0 + rr * 0.6 + 0.32, z).round());
                                for uu in 1..21 {
                                    let y = (yt + uu as f32 * u).round();
                                    if y > yhide - 1.0 || rng.f() > 0.3 { continue; }
                                    let red = rng.f() < 0.18;
                                    let kind = rng.f();
                                    let xx = if rng.f() < 0.7 { lx } else { lx2 };
                                    leds.push(Led { x: xx, y, s: (rw / 10.0).clamp(1.0, 2.0), a: (if red { 0.95 } else { 0.6 }) * (1.0 - haze * 0.85) * rng.range(0.35, 1.0),
                                                    red, kind: if kind < 0.45 { 0 } else if kind < 0.8 { 1 } else { 2 }, f: rng.range(0.3, 2.2), p: rng.range(0.0, TAU) });
                                }
                                rr += 1.0;
                            }
                        }
                    }
                    x0 += seg + gapx;
                }
            }
            // its raised lip and the dark well inside
            fill.SetColor(&rgba(body));
            dc.FillEllipse(&ell(cr * 1.07), &fill);
            fill.SetColor(&ca(ink, if light { 0.12 } else { 0.07 }));
            let lip = factory.CreatePathGeometry()?;
            {
                let k = lip.Open()?;
                k.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
                for r in [cr * 1.07, cr] {
                    k.BeginFigure(p2(ox + r, cy), D2D1_FIGURE_BEGIN_FILLED);
                    for end in [p2(ox - r, cy), p2(ox + r, cy)] {
                        k.AddArc(&D2D1_ARC_SEGMENT { point: end, size: D2D_SIZE_F { width: r, height: r * ce }, rotationAngle: 0.0,
                                                     sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                    }
                    k.EndFigure(D2D1_FIGURE_END_CLOSED);
                }
                k.Close()?;
            }
            dc.FillGeometry(&lip, &fill, None);
            pen.SetOpacity(0.28);
            dc.DrawEllipse(&ell(cr * 1.07), &pen, 1.0, None);
            // a little light falling on the sphere from above; on dark themes the hall darkens toward the edges
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &radial(ox, oy, 0.0, -s * 0.8, s * 2.4, s * 2.4,
                &[(0.08, ca(ink, if light { 0.06 } else { 0.05 })), (1.0, ca(ink, 0.0))])?);
            if !light {
                let far = (w * w + h * h).sqrt() * 0.6;
                dc.FillRectangle(&rect(0.0, 0.0, w, h), &radial(ox, vy, 0.0, 0.0, far, far,
                    &[(s * 1.2 / far, D2D1_COLOR_F::default()), (1.0, D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.55 })])?);
            }
            dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            dc.EndDraw(None, None)?;
            // the floor in front of the opening, drawn again over the sphere's lower part, and the opening's lit near edge
            dc.SetTarget(&front);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let fg = factory.CreatePathGeometry()?;
            {
                let k = fg.Open()?;
                k.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
                k.BeginFigure(p2(0.0, cy), D2D1_FIGURE_BEGIN_FILLED);
                for q in [p2(w, cy), p2(w, h), p2(0.0, h)] { k.AddLine(q); }
                k.EndFigure(D2D1_FIGURE_END_CLOSED);
                k.BeginFigure(p2(ox + cr, cy), D2D1_FIGURE_BEGIN_FILLED);
                k.AddArc(&D2D1_ARC_SEGMENT { point: p2(ox - cr, cy), size: D2D_SIZE_F { width: cr, height: cr * ce }, rotationAngle: 0.0,
                                             sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                k.EndFigure(D2D1_FIGURE_END_CLOSED);
                k.Close()?;
            }
            let bprops = D2D1_BITMAP_BRUSH_PROPERTIES1 { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP,
                                                         interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR };
            dc.FillGeometry(&fg, &dc.CreateBitmapBrush(&back, Some(&bprops), None)?, None);
            let edge = |r: f32, dy: f32, a0: f32, a1: f32, al: f32| -> Result<()> {
                let p = factory.CreatePathGeometry()?;
                let k = p.Open()?;
                let at = |a: f32| p2(ox + r * a.cos(), cy + dy + r * ce * a.sin());
                k.BeginFigure(at(a0), D2D1_FIGURE_BEGIN_HOLLOW);
                let m = (a0 + a1) / 2.0;
                for e in [m, a1] { k.AddArc(&D2D1_ARC_SEGMENT { point: at(e), size: D2D_SIZE_F { width: r, height: r * ce }, rotationAngle: 0.0,
                                                                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL }); }
                k.EndFigure(D2D1_FIGURE_END_OPEN);
                k.Close()?;
                pen.SetOpacity(al);
                dc.DrawGeometry(&p, &pen, 1.0, None);
                Ok(())
            };
            edge(cr, 0.0, 0.05, std::f32::consts::PI - 0.05, 0.45)?;
            edge(cr * 1.07, s * 0.012, 0.0, std::f32::consts::PI, 0.14)?;
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        // one light's glow, drawn scaled for every light; and a white dot for the rack lights
        let glow = g.surface(32.0, 32.0)?;
        let dot = g.surface(2.0, 2.0)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&glow);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.FillRectangle(&rect(0.0, 0.0, 32.0, 32.0), &radial(16.0, 16.0, 0.0, 0.0, 16.0, 16.0,
                &[(0.0, ca(acc, 0.7)), (0.3, ca(acc, 0.28)), (1.0, ca(acc, 0.0))])?);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&dot);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }));
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        // the sphere's body: near black, a faint sheen up and to the left, a fresnel glow, a thin rim of light
        let gloss = if light { bg } else { ink };
        let sphere: [ID2D1Brush; 3] = [
            g.brush(body)?.cast()?,
            radial(ox - s * 0.2, oy - s * 0.2, -s * 0.18, -s * 0.22, s * 1.35, s * 1.35, &[(0.0, ca(gloss, 0.1)), (0.45, ca(gloss, 0.025)), (1.0, ca(gloss, 0.0))])?.cast()?,
            radial(ox, oy, 0.0, 0.0, s, s, &[(0.7, ca(gloss, 0.0)), (0.94, ca(gloss, 0.015)), (1.0, ca(gloss, 0.16))])?.cast()?,
        ];
        let rim: ID2D1Brush = unsafe { g.dc.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p2(ox + s * 0.7, oy - s * 0.7), endPoint: p2(ox - s * 0.7, oy + s * 0.7) }, None,
            &grad(&[(0.0, ca(gloss, 0.7)), (0.5, ca(gloss, 0.12)), (1.0, ca(gloss, 0.03))])?)?.cast()? };
        // the shell: stacked slats, each carrying a row of red lights; some burn steadily, others flicker
        let ribs: Vec<Rib> = (0..ORB_L).map(|k| {
            let phi = -1.4 + 2.8 * (k as f32 + 0.5) / ORB_L as f32;
            let n = ((phi.cos() * 84.0).round() as usize).max(6);
            let base: Vec<f32> = (0..n).map(|_| if rng.f() < 0.17 { rng.range(0.3, 0.65) } else { 0.0 }).collect();
            let b = base.iter().map(|&v| v.max(if rng.f() < 0.12 { rng.range(0.3, 1.0) } else { 0.0 })).collect();
            Rib { phi, n, o: rng.range(0.0, TAU), base, b }
        }).collect();
        let dc3: ID2D1DeviceContext3 = g.dc.cast()?;
        let batch = unsafe { dc3.CreateSpriteBatch()? };
        // the cage: a fine net of holes in a second skin over the sphere, drawn on its own context each frame
        let (net, tris) = geodesic(ORB_NET);
        let c = s * (CAGE_R + 0.04) + 4.0;
        let mesh = g.surface(c * 2.0, c * 2.0)?;
        let mdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        unsafe { mdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); }
        let mdc3: ID2D1DeviceContext3 = mdc.cast()?;
        let mbatch = unsafe { mdc3.CreateSpriteBatch()? };
        // a hole: a white triangle, mapped onto each cell; the holes are drawn as these on their own layer, which is
        // then cut out of the skin
        let hole = g.surface(64.0, 64.0)?;
        let edge = g.surface(64.0, 64.0)?;
        let holes = g.surface(c * 2.0, c * 2.0)?;
        unsafe {
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&hole);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let (a, c) = (HOLE_IN * 64.0, (1.0 - 2.5 * HOLE_IN) * 64.0);
            dc.FillGeometry(&quad([p2(a, a), p2(c, a), p2(a, c), p2(a, a)])?, &g.brush([1.0, 1.0, 1.0, 1.0])?, None);
            dc.EndDraw(None, None)?;
            // a hole's rim: the same triangle's outline, which the light catches
            dc.SetTarget(&edge);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.DrawGeometry(&quad([p2(a, a), p2(c, a), p2(a, c), p2(a, a)])?, &g.brush([1.0, 1.0, 1.0, 1.0])?, 2.2, None);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let mut o = Orb { back, front, glow, dot, batch, dc3, factory, sphere, rim, leds, ribs, packets: Vec::new(), rng,
                          s, ox, oy, w, h, t: 0.0, spin: 0.0, still: false, dest: Vec::new(), tint: Vec::new(), net, tris, mesh, mdc, mdc3, mbatch, hole, holes, edge, cage: 0.0, turns: (1.0, 1.0), caged: true, calm: false };
        o.t = o.rng.range(0.0, 300.0);
        o.spin = o.rng.range(-0.5, 0.1);
        o.cage = o.spin;
        o.packets = (0..22).map(|_| o.packet()).collect();
        Ok(o)
    }

    fn packet(&mut self) -> (usize, f32, f32, usize) {
        let k = (self.rng.f() * ORB_L as f32) as usize % ORB_L;
        let pos = self.rng.range(0.0, self.ribs[k].n as f32);
        let v = self.rng.range(3.0, 11.0) * if self.rng.f() < 0.5 { -1.0 } else { 1.0 };
        (k, pos, v, 2 + (self.rng.f() * 6.0) as usize)
    }

    /// a point on the shell: latitude phi, body longitude th; returns x, y, depth, light
    fn pt(&self, phi: f32, th: f32) -> (f32, f32, f32, f32) {
        let a = th + self.spin;
        let cp = phi.cos();
        let (x, y, z) = (cp * a.sin(), phi.sin(), cp * a.cos());
        let t1 = 0.34f32;
        let (y2, z2) = (y * t1.cos() - z * t1.sin(), y * t1.sin() + z * t1.cos());
        let lam = (-0.48 * x + 0.58 * y2 + 0.66 * z2).max(0.0);
        (self.ox + x * self.s, self.oy - y2 * self.s, z2, lam)
    }

    /// a point given in a frame turned by angle a about the upright, at radius r; returns x, y, depth, light
    fn proj(&self, v: [f32; 3], a: f32, r: f32) -> (f32, f32, f32, f32) {
        let (c, sn) = (a.cos(), a.sin());
        let (x, z) = (v[0] * c + v[2] * sn, v[2] * c - v[0] * sn);
        let t1 = 0.34f32;
        let (y2, z2) = (v[1] * t1.cos() - z * t1.sin(), v[1] * t1.sin() + z * t1.cos());
        (self.ox + x * r, self.oy - y2 * r, z2, (-0.48 * x + 0.58 * y2 + 0.66 * z2).max(0.0))
    }

    /// the cage: drawn over the sphere when the option has it on
    fn cage_layer(&mut self, g: &Gfx, look: &Look, s: f32, ox: f32, oy: f32, u: f32) -> Result<()> {
        let light = look.light;
        // the cage: the sphere's skin again over all of it, with a fine hex net of holes punched through (a min blend), so
        // the slats and lights only peek through; the holes close up toward the edge, where we see them side on
        let c = s * (CAGE_R + 0.04) + 4.0;
        // each cell's hole: the texture's triangle mapped (an affine sprite) onto the cell shrunk toward its middle
        let rc = s * CAGE_R;
        let pr: Vec<(f32, f32, f32, f32)> = self.net.iter().map(|&v| { let p = self.proj(v, self.cage, rc); (p.0 - ox + c, p.1 - oy + c, p.2, p.3) }).collect();
        let gloss = if light { look.bg } else { look.ink };
        let mut lit = Vec::new();   // each rim's tint: brighter where the cell faces us and the light
        let (ha, hd) = (HOLE_IN, 1.0 - 3.5 * HOLE_IN);
        let mut mats = Vec::new();
        for t in &self.tris {
            let (a, b, d) = (pr[t[0] as usize], pr[t[1] as usize], pr[t[2] as usize]);
            if a.2 + b.2 + d.2 < 0.15 { continue; }
            let (mx, my) = ((a.0 + b.0 + d.0) / 3.0, (a.1 + b.1 + d.1) / 3.0);
            let k = 0.74;
            let q = |p: (f32, f32, f32, f32)| (mx + (p.0 - mx) * k, my + (p.1 - my) * k);
            let al = CAGE_RIM * ((a.2 + b.2 + d.2) / 3.0).max(0.0).sqrt() * (0.03 + 0.25 * (a.3 + b.3 + d.3) / 3.0);
            lit.push(D2D1_COLOR_F { r: gloss[0] * al, g: gloss[1] * al, b: gloss[2] * al, a: al });
            let (q0, q1, q2) = (q(a), q(b), q(d));
            let (m11, m12, m21, m22) = ((q1.0 - q0.0) / hd, (q1.1 - q0.1) / hd, (q2.0 - q0.0) / hd, (q2.1 - q0.1) / hd);
            mats.push(windows::Foundation::Numerics::Matrix3x2 { M11: m11, M12: m12, M21: m21, M22: m22,
                                                                 M31: q0.0 - ha * (m11 + m21), M32: q0.1 - ha * (m12 + m22) });
        }
        let holes = vec![rect(0.0, 0.0, 1.0, 1.0); mats.len()];
        unsafe {
            let m = &self.mdc;
            m.SetTarget(&self.holes);
            m.BeginDraw();
            m.Clear(Some(&D2D1_COLOR_F::default()));
            if !holes.is_empty() {
                self.mbatch.Clear();
                self.mbatch.AddSprites(holes.len() as u32, holes.as_ptr(), None, None, Some(mats.as_ptr()), size_of::<D2D_RECT_F>() as u32, 0, 0,
                                       size_of::<windows::Foundation::Numerics::Matrix3x2>() as u32)?;
                m.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
                self.mdc3.DrawSpriteBatch(&self.mbatch, 0, holes.len() as u32, &self.hole, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_SPRITE_OPTIONS_NONE);
                m.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
            m.EndDraw(None, None)?;
            m.SetTarget(&self.mesh);
            m.BeginDraw();
            m.Clear(Some(&D2D1_COLOR_F::default()));
            // the cage's skin: the sphere's own look, a little larger
            m.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: CAGE_R, M12: 0.0, M21: 0.0, M22: CAGE_R,
                                                                       M31: c - CAGE_R * ox, M32: c - CAGE_R * oy });
            let e = D2D1_ELLIPSE { point: p2(ox, oy), radiusX: s, radiusY: s };
            for b in &self.sphere { m.FillEllipse(&e, b); }
            m.DrawEllipse(&D2D1_ELLIPSE { point: p2(ox, oy), radiusX: s - 0.7, radiusY: s - 0.7 }, &self.rim, 1.4, None);
            m.SetTransform(&turn(0.0, 0.0, 0.0));
            m.DrawImage(&self.holes, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_DESTINATION_OUT);
            if !holes.is_empty() && CAGE_RIM > 0.0 {   // the rims, catching the light
                self.mbatch.Clear();
                self.mbatch.AddSprites(holes.len() as u32, holes.as_ptr(), None, Some(lit.as_ptr()), Some(mats.as_ptr()), size_of::<D2D_RECT_F>() as u32, 0,
                                       size_of::<D2D1_COLOR_F>() as u32, size_of::<windows::Foundation::Numerics::Matrix3x2>() as u32)?;
                m.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
                self.mdc3.DrawSpriteBatch(&self.mbatch, 0, holes.len() as u32, &self.edge, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_SPRITE_OPTIONS_NONE);
                m.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
            m.EndDraw(None, None)?;
            m.SetTarget(None);
            g.dc.DrawBitmap(&self.mesh, Some(&rect(ox - c, oy - c, c * 2.0, c * 2.0)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
        }
        Ok(())
    }

    fn sprites(&mut self, g: &Gfx, bmp: &ID2D1Bitmap1, add: bool) -> Result<()> {
        if self.dest.is_empty() { return Ok(()); }
        unsafe {
            let n = self.dest.len() as u32;
            self.batch.Clear();
            self.batch.AddSprites(n, self.dest.as_ptr(), None, Some(self.tint.as_ptr()), None,
                                  size_of::<D2D_RECT_F>() as u32, 0, size_of::<D2D1_COLOR_F>() as u32, 0)?;
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // sprite batches need it
            if add { g.dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_ADD); }
            self.dc3.DrawSpriteBatch(&self.batch, 0, n, bmp, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_SPRITE_OPTIONS_NONE);
            g.dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            g.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        self.dest.clear(); self.tint.clear();
        Ok(())
    }
}

impl Background for Orb {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 20.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        let d = if o.get("direction").str() == Some("left") { -1.0 } else { 1.0 };
        self.caged = o.get("cage").str() != Some("off");
        self.calm = o.get("lights").str() == Some("calm");
        self.turns = match o.get("spin").str() { Some("sphere") => (d, 0.0), Some("cage") => (0.0, d), Some("still") => (0.0, 0.0), _ => (d, d) };
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;   // 0 stills it: the frame as it is stays up
        if !self.still {
            let sp = 0.4 + level * 1.6;
            self.t += dt * sp;
            self.spin += dt * 0.05 * sp * self.turns.0;
            self.cage += dt * 0.05 * sp * self.turns.1;
            let fade = (-dt * sp * 1.1).exp();   // lights die down to their resting glow
            for rb in &mut self.ribs { for j in 0..rb.n { rb.b[j] = rb.base[j].max(rb.b[j] * fade); } }
            let mut n = dt * sp * if self.calm { 8.0 } else { 70.0 };   // flickers; calm: now and then
            while n > 0.0 {
                if n < 1.0 && self.rng.f() > n { break; }
                let k = (self.rng.f() * ORB_L as f32) as usize % ORB_L;
                let j = (self.rng.f() * self.ribs[k].n as f32) as usize % self.ribs[k].n;
                self.ribs[k].b[j] = self.rng.range(0.5, 1.0);
                n -= 1.0;
            }
            for i in 0..if self.calm { 0 } else { self.packets.len() } {   // packets streaming along the slats; none when calm
                let (k, mut pos, v, len) = self.packets[i];
                pos += v * dt * sp;
                let rn = self.ribs[k].n as i64;
                for j in 0..len {
                    let q = ((pos.floor() as i64 - v.signum() as i64 * j as i64) % rn + rn) % rn;
                    let b = &mut self.ribs[k].b[q as usize];
                    *b = b.max(1.0 - j as f32 / len as f32 * 0.6);
                }
                self.packets[i] = if self.rng.f() < dt * 0.12 { self.packet() } else { (k, pos, v, len) };
            }
        }
        let light = look.light;
        let full = rect(0.0, 0.0, self.w, self.h);
        unsafe { g.dc.DrawBitmap(&self.back, Some(&full), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None); }
        // the racks' status lights: steady, blinking, or flickering with traffic
        let tt = self.t;
        for l in &self.leds {
            let on = match l.kind { 0 => 1.0, 1 => if (tt * l.f + l.p).sin() > -0.2 { 1.0 } else { 0.08 },
                                    _ => if (tt * l.f * 9.0 + l.p).sin() * (tt * l.f * 4.3).sin() > 0.0 { 1.0 } else { 0.15 } };
            let a = l.a * on;
            let c = if l.red { look.acc } else { look.ink };
            self.dest.push(rect(l.x, l.y, l.s, l.s));
            self.tint.push(D2D1_COLOR_F { r: c[0] * a, g: c[1] * a, b: c[2] * a, a });
        }
        let dot = self.dot.clone();
        self.sprites(g, &dot, false)?;
        let (ox, oy, s) = (self.ox, self.oy, self.s);
        unsafe {
            let dc = &g.dc;
            let e = D2D1_ELLIPSE { point: p2(ox, oy), radiusX: s, radiusY: s };
            for b in &self.sphere { dc.FillEllipse(&e, b); }
            dc.DrawEllipse(&D2D1_ELLIPSE { point: p2(ox, oy), radiusX: s - 0.7, radiusY: s - 0.7 }, &self.rim, 1.4, None);
            // the slats: a lit upper lip and a fainter lower one per slat, in four steps of how far each stretch
            // faces us and the light
            let gap = 2.8 / ORB_L as f32;
            let paths: Vec<ID2D1PathGeometry> = (0..4).map(|_| self.factory.CreatePathGeometry()).collect::<Result<_>>()?;
            let sinks: Vec<ID2D1GeometrySink> = paths.iter().map(|p| p.Open()).collect::<Result<_>>()?;
            for rb in &self.ribs {
                for (dphi, dim) in [(gap * 0.42, false), (-gap * 0.3, true)] {
                    let mut prev: Option<(f32, f32, f32, f32)> = None;
                    for i in 0..=ORB_N {
                        let p = self.pt(rb.phi + dphi, (i % ORB_N) as f32 / ORB_N as f32 * TAU);
                        if let Some(q) = prev {
                            if p.2 > 0.0 && q.2 > 0.0 {
                                let b = p.2.min(q.2).powf(0.7) * (0.25 + 0.75 * p.3) * if dim { 0.45 } else { 1.0 };
                                if b > 0.04 {
                                    let st = &sinks[if b < 0.18 { 0 } else if b < 0.36 { 1 } else if b < 0.6 { 2 } else { 3 }];
                                    st.BeginFigure(p2(q.0, q.1), D2D1_FIGURE_BEGIN_HOLLOW);
                                    st.AddLine(p2(p.0, p.1));
                                    st.EndFigure(D2D1_FIGURE_END_OPEN);
                                }
                            }
                        }
                        prev = Some(p);
                    }
                }
            }
            let skin = g.brush(if light { look.bg } else { look.dim })?;
            for (j, al) in [0.04, 0.09, 0.17, 0.3].iter().enumerate() {
                sinks[j].Close()?;
                skin.SetOpacity(*al);
                dc.DrawGeometry(&paths[j], &skin, 0.9, None);
            }
        }
        // the lights: a soft glow under each, then the point itself, narrower toward the edge of the sphere
        let u = (s / 160.0).max(1.0);
        let mut lit = Vec::new();
        for rb in &self.ribs {
            for j in 0..rb.n {
                if rb.b[j] < 0.04 { continue; }
                let p = self.pt(rb.phi, rb.o + j as f32 / rb.n as f32 * TAU);
                if p.2 > 0.04 { lit.push((p.0, p.1, p.2, rb.b[j])); }
            }
        }
        for &(x, y, z, b) in &lit {
            let a = (b * z.sqrt() * 0.9).min(1.0);
            let (gw, gh) = (u * (7.0 + 13.0 * z), u * 10.0);
            self.dest.push(rect(x - gw / 2.0, y - gh / 2.0, gw, gh));
            self.tint.push(D2D1_COLOR_F { r: a, g: a, b: a, a });
        }
        let glow = self.glow.clone();
        self.sprites(g, &glow, !light)?;
        let core = if light { look.acc } else { mixc(look.acc, [1.0, 1.0, 1.0, 1.0], 0.45) };
        for &(x, y, z, b) in &lit {
            let a = (b * z.sqrt() * 1.2).min(1.0);
            let wd = u * (0.8 + 1.6 * z);
            self.dest.push(rect(x - wd / 2.0, y - u * 0.5, wd, u));
            self.tint.push(D2D1_COLOR_F { r: core[0] * a, g: core[1] * a, b: core[2] * a, a });
        }
        self.sprites(g, &dot, !light)?;
        if self.caged { self.cage_layer(g, look, s, ox, oy, u)?; }
        unsafe { g.dc.DrawBitmap(&self.front, Some(&full), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None); }
        Ok(())
    }
}
