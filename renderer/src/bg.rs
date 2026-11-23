//! Backgrounds: each one draws into the full-screen scene bitmap that the cards' glass blurs.
//! Ported so far: the page's "pixel", "cascade", "broadsheet", "halftone", "stop", "grid", "record", "notebook", "textmode", "sunprint", "draft", "topo", "radar", "orb", "scope", "oracle", "signs", "chalk", and the eight no theme uses: "stars", "snow", "static", "code", "jukebox", "starburst", "testcard" and "print". All 27; an unknown name gets a plain fill.
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
        "broadsheet" => Box::new(Broadsheet::new(g, fonts, look, o)?),
        "halftone" => Box::new(Halftone::new(g, o)?),
        "stop" => Box::new(Stop::new(g, fonts, look)?),
        "grid" => Box::new(Grid { ph: 0.0, still: false, sun: true, stripes: true, scroll: true, hz: 0.6 }),
        "record" => Box::new(Record::new(g, fonts, look)?),
        "notebook" => Box::new(Notebook::new(g)?),
        "textmode" => Box::new(Textmode::new(g, fonts, look)?),
        "sunprint" => Box::new(Sunprint::new(g, o)?),
        "draft" => Box::new(Draft::new(g, fonts, look)?),
        "topo" => Box::new(Topo::new(g, fonts, look, o)?),
        "radar" => Box::new(Radar::new(g, fonts, look, o)?),
        "orb" => Box::new(Orb::new(g, fonts, look)?),
        "scope" => Box::new(Scope::new(g, fonts, look, o)?),
        "oracle" => Box::new(Oracle::new(g, fonts, look, o)?),
        "signs" => Box::new(Signs::new(g, fonts, look, o)?),
        "chalk" => Box::new(Chalk::new(g, fonts, look, o)?),
        "stars" => Box::new(Stars::new()),
        "snow" => Box::new(Snow::new(g)),
        "static" => Box::new(Static::new(g)?),
        "code" => { let mut c = Code(Cascade::new(g, fonts, &look.mono.family, &Json::parse(r#"{"layers":"one"}"#).unwrap_or(Json::Null))?, 1.0); c.opts(o); Box::new(c) }   // the page's katakana rain: cascade, one depth
        "jukebox" => Box::new(Jukebox::new(g, o)?),
        "starburst" => Box::new(Starburst::new(g)?),
        "testcard" => Box::new(Testcard::new(g, fonts, look)?),
        "print" => Box::new(Print::new(g, o)?),
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
    ink: f32, built: (bool, usize, bool),   // options: the ink's strength; in the layout: photos, columns, rules
}

enum Sheet { Photo(f32, f32, f32, f32), Bar(f32, f32, f32, f32, f32) }   // x, y, w, h (and alpha)

const SET: u8 = 0;
const HOLD: u8 = 1;
const OUT: u8 = 2;

impl Broadsheet {
    fn built(o: &Json) -> (bool, usize, bool) {
        (o.get("photos").str() != Some("off"), match o.get("columns").str() { Some("four") => 4, Some("three") => 3, _ => 6 },
         o.get("rules").str() != Some("off"))
    }
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Broadsheet> {
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
            ink: 1.0, built: Self::built(o),
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
        let cols = if w > h { self.built.1 } else { self.built.1.min(3) };
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
        if !self.built.0 { photos.clear(); }
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
            for c in 1..if self.built.2 { cols } else { 1 } {
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
    fn opts(&mut self, o: &Json) -> bool {
        self.ink = match o.get("ink").str() { Some("faint") => 0.6, Some("bold") => 1.8, _ => 1.0 };
        Self::built(o) != self.built
    }
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
            let a = (if look.light { 0.13 } else { 0.11 } * self.fade * self.ink).min(1.0);
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
                  fs: f32, w: f32, h: f32, clock: std::time::Instant,
                  bright: bool, caret: u8, short: bool, centre: bool }   // options: brighter text, cursor blink/steady/off, two lines, centred

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
        Ok(Stop { title, tw: m.widthIncludingTrailingWhitespace, lines, cursor, fs, w, h, clock: std::time::Instant::now(),
                  bright: false, caret: 0, short: false, centre: false })
    }
}

impl Background for Stop {
    fn fps(&self) -> f32 { if self.caret == 0 { 1.0 / 0.55 } else { 0.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.bright = o.get("text").str() == Some("bright");
        self.caret = match o.get("cursor").str() { Some("steady") => 1, Some("off") => 2, _ => 0 };
        self.short = o.get("message").str() == Some("short");
        self.centre = o.get("place").str() == Some("centre");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, _: f32, _: f32) -> Result<()> {
        let on = match self.caret { 1 => true, 2 => false, _ => (self.clock.elapsed().as_secs_f32() / 0.55) as u64 % 2 == 0 };
        let (fs, x) = (self.fs, self.w * 0.06);
        let lines = &self.lines[..if self.short { 3 } else { self.lines.len() }];   // short: the first two and a blank
        let mut y = if self.centre { (self.h - fs * (2.4 + (lines.len() + 1) as f32 * 1.35)) / 2.0 } else { self.h * 0.12 };
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
            let ink = g.brush(a(look.ink, if self.bright { 0.32 } else { 0.17 }))?;   // faint, so cards on top stay readable
            for l in lines { dc.DrawTextLayout(D2D_POINT_2F { x, y }, l, &ink, o); y += fs * 1.35; }
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
pub struct Notebook { doodles: Vec<Doodle>, wait: f32, rng: Rng, round: ID2D1StrokeStyle, factory: ID2D1Factory,
                     paper: u8, margin: bool, pen: u8, holes: bool }   // options: ruled/grid/blank, margin, pencil/ballpoint/none, holes

struct Doodle { strokes: Vec<Vec<(f32, f32)>>, n: usize, x: f32, y: f32, rot: f32, t: f32, dur: f32, hold: f32 }

const NB_LINE: f32 = 30.0;

impl Notebook {
    fn new(g: &Gfx) -> Result<Notebook> {
        let factory = unsafe { g.dc.GetFactory()? };
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND,
                                                   dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
        let round = unsafe { factory.CreateStrokeStyle(&props, None)? };
        Ok(Notebook { doodles: Vec::new(), wait: 1.0, rng: Rng(0x0b00c_5ce7c4_0001), round, factory, paper: 0, margin: true, pen: 0, holes: true })
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
    fn opts(&mut self, o: &Json) -> bool {
        self.paper = match o.get("paper").str() { Some("grid") => 1, Some("blank") => 2, _ => 0 };
        self.margin = o.get("margin").str() != Some("off");
        self.pen = match o.get("doodles").str() { Some("ballpoint") => 1, Some("off") => 2, _ => 0 };
        self.holes = o.get("holes").str() != Some("off");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let level = if self.pen == 2 { 0.0 } else { level };   // no doodles: as if the rain were off
        let (w, h) = (g.w as f32, g.h as f32);
        let with = |c: Color, a: f32| [c[0], c[1], c[2], a];
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let rule = g.brush(with(look.dim, 0.75))?;
            let mut y = if self.paper == 1 { 0.5 } else if self.paper == 2 { h } else { NB_LINE * 3.0 + 0.5 };
            while y < h { dc.DrawLine(D2D_POINT_2F { x: 0.0, y }, D2D_POINT_2F { x: w, y }, &rule, 1.0, None); y += NB_LINE; }
            let mut x = if self.paper == 1 { 0.5 } else { w };   // squared paper
            while x < w { dc.DrawLine(D2D_POINT_2F { x, y: 0.0 }, D2D_POINT_2F { x, y: h }, &rule, 1.0, None); x += NB_LINE; }
            let margin = g.brush(with(look.acc, 0.55))?;
            let mx = (w * 0.08).round() + 0.5;
            if self.margin { for x in [mx, mx + 3.0] { dc.DrawLine(D2D_POINT_2F { x, y: 0.0 }, D2D_POINT_2F { x, y: h }, &margin, 1.0, None); } }
            let hole = g.brush([0.0, 0.0, 0.0, 0.07])?;
            let hr = w.min(h) * 0.016;
            for f in if self.holes { &[0.18, 0.5, 0.82][..] } else { &[][..] } {
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
        let pencil = g.brush(if self.pen == 1 { look.ink } else { look.hi })?;
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
                      mx: i32, my: i32, goal: Option<(i32, i32)>, wait: f32, rng: Rng, w: f32, h: f32,
                      clock: u8, bars: bool, shade: f32, cursor: bool }   // options: 24h/12h/off, the bars, the shade's strength, the mouse

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
                      rng: Rng(0x7e47_30de_0000_0001), w, h, clock: 0, bars: true, shade: 0.5, cursor: true })
    }
}

impl Background for Textmode {
    fn fps(&self) -> f32 { if self.goal.is_some() { 12.0 } else { 1.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.clock = match o.get("clock").str() { Some("12h") => 1, Some("off") => 2, _ => 0 };
        self.bars = o.get("bars").str() != Some("off");
        self.shade = match o.get("shade").str() { Some("medium") => 0.8, Some("dark") => 1.0, _ => 0.5 };
        self.cursor = o.get("cursor").str() != Some("off");
        false
    }
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
            let shade = g.brush([look.dim[0], look.dim[1], look.dim[2], self.shade])?;
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.mask, &shade, Some(&rect(0.0, 0.0, w, h)), None);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let (bar_b, on_b, key_b) = (g.brush(bar)?, g.brush(on)?, g.brush(look.err)?);
            let text = |s: &str, col: f32, y: f32, b: &ID2D1SolidColorBrush| {
                let t: Vec<u16> = s.encode_utf16().collect();
                dc.DrawText(&t, &self.fmt, &rect(col * cw, y, cw * (t.len() as f32 + 1.0), fs * 1.5), b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            };
            // the menu bar: first letters picked out, a clock at the right
            if self.bars {
            dc.FillRectangle(&rect(0.0, 0.0, w, fs), &bar_b);
            let mut x = 2.0;
            for m in TM_MENU { text(&m[..1], x, 0.0, &key_b); text(&m[1..], x + 1.0, 0.0, &on_b); x += m.len() as f32 + 3.0; }
            let t = windows::Win32::System::SystemInformation::GetLocalTime();
            let clock = if self.clock == 1 { format!("{:02}:{:02}:{:02} {}", (t.wHour + 11) % 12 + 1, t.wMinute, t.wSecond, if t.wHour < 12 { "AM" } else { "PM" }) }
                        else { format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond) };
            if self.clock != 2 { text(&clock, (self.cols - clock.len() as i32 - 2) as f32, 0.0, &on_b); }
            // the status bar
            let y = (self.rows - 1) as f32 * fs;
            dc.FillRectangle(&rect(0.0, y, w, h - y), &bar_b);
            let mut x = 1.0;
            for (k, l) in TM_KEYS { text(k, x, y, &key_b); text(l, x + k.len() as f32 + 1.0, y, &on_b); x += (k.len() + l.len() + 3) as f32; }
            }
            // the mouse: an inverted cell
            if self.cursor { dc.FillRectangle(&rect(self.mx as f32 * cw, self.my as f32 * fs, cw, fs), &g.brush([look.hi[0], look.hi[1], look.hi[2], 0.85])?); }
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
                      k: f32, phase: u8, t: f32, still: bool,
                      made: (u8, u8, bool), pace: f32 }   // options: in the print, plants (mixed/ferns/leaves), count (normal/few/many), crisp edges; how fast it develops

fn turn(a: f32, x: f32, y: f32) -> windows::Foundation::Numerics::Matrix3x2 {
    let (s, c) = a.sin_cos();
    windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: x, M32: y }
}
fn p2(x: f32, y: f32) -> D2D_POINT_2F { D2D_POINT_2F { x, y } }

impl Sunprint {
    fn made(o: &Json) -> (u8, u8, bool) {
        (match o.get("plants").str() { Some("ferns") => 1, Some("leaves") => 2, _ => 0 },
         match o.get("count").str() { Some("few") => 1, Some("many") => 2, _ => 0 }, o.get("edges").str() == Some("crisp"))
    }
    fn new(g: &Gfx, o: &Json) -> Result<Sunprint> {
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
                               k: 1.0, phase: HOLD, t: 0.0, still: false, made: Self::made(o), pace: 1.0 };   // the first print is already developed
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
            let (plants, count, crisp) = self.made;
            let r = self.rng.f();
            let nf = if plants == 2 { 0 } else { [2 + (r * 2.0) as usize, 1, 4 + (r * 2.0) as usize][count as usize] };
            let nl = if plants == 1 { 0 } else { [4 + (r * 4.0) as usize, 2, 10 + (r * 4.0) as usize][count as usize] };
            for _ in 0..nf {
                let (x, y) = (self.rng.range(0.1, 0.9) * w, self.rng.range(0.15, 0.95) * h);
                let (a, l) = (self.rng.range(-pi, 0.0) - 0.3, self.rng.range(0.35, 0.6) * u);
                self.frond(x, y, a, l)?;
            }
            for _ in 0..nl {
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
            if !crisp { self.pdc.DrawImage(&self.blur.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER); }
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
    fn opts(&mut self, o: &Json) -> bool {
        self.pace = match o.get("develop").str() { Some("slow") => 0.5, Some("fast") => 2.0, _ => 1.0 };
        Self::made(o) != self.made
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;   // 0 stills it: the print as it is stays up
        if !self.still {
            let sp = (0.4 + level * 1.6) * self.pace;
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
                 turns: (f32, f32), caged: bool, calm: bool,
                 fmt: IDWriteTextFormat, fs: f32, cy: f32, calls: Vec<OrbCall> }   // the readouts pointing at parts of the sphere   // how fast the sphere and the cage turn, from the options: 1 right, -1 left, 0 still

struct Led { x: f32, y: f32, s: f32, a: f32, red: bool, kind: u8, f: f32, p: f32 }
struct Rib { phi: f32, n: usize, o: f32, base: Vec<f32>, b: Vec<f32> }

/// a readout: which of the three (each keeps to its own band of slats), the slat, where on it, its age and life,
/// which label, its drifting value and an id
struct OrbCall { i: usize, k: usize, th: f32, age: f32, life: f32, lab: usize, v: f32, id: String }

const ORB_LABELS: [&str; 6] = ["DIVERGENCE", "RECALCULATING", "LOAD", "VARIANCE", "THREAD", "CONVERGENCE"];

fn orb_value(lab: usize, v: f32) -> String {
    match lab {
        0 => format!("Δ {:.4}", v * 0.09),
        1 => format!("SECTOR {}", (v * 96.0) as u32 + 1),
        2 => format!("{:.1} %", 60.0 + v * 39.0),
        3 => format!("σ {:.3}", v * 0.3),
        4 => format!("0x{:04X}", (v * 65535.0) as u32),
        _ => format!("{:.2} %", 90.0 + v * 9.9),
    }
}

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
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Orb> {
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
        let fs = (s * 0.045).round().max(10.0);
        let mut o = Orb { fmt: fonts.format(&look.mono.family, fs, 400)?, fs, cy, calls: Vec::new(), back, front, glow, dot, batch, dc3, factory, sphere, rim, leds, ribs, packets: Vec::new(), rng,
                          s, ox, oy, w, h, t: 0.0, spin: 0.0, still: false, dest: Vec::new(), tint: Vec::new(), net, tris, mesh, mdc, mdc3, mbatch, hole, holes, edge, cage: 0.0, turns: (1.0, 1.0), caged: true, calm: false };
        for i in 0..3 { let a = o.rng.range(0.15, 0.5); let c = o.callout(i, a); o.calls.push(c); }
        o.burn();
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
    /// a readout on one part of the sphere: each of the three keeps to its own band of slats, so they never collide
    fn callout(&mut self, i: usize, age: f32) -> OrbCall {
        let used: Vec<usize> = self.calls.iter().filter(|c| c.i != i).map(|c| c.lab).collect();
        let pool: Vec<usize> = (1..ORB_LABELS.len()).filter(|l| !used.contains(l)).collect();
        let band = [(22usize, 26usize), (18, 21), (14, 16)][i];
        let k = (band.0 + (self.rng.f() * (band.1 - band.0 + 1) as f32) as usize).min(band.1).min(self.ribs.len() - 1);
        let life = self.rng.range(9.0, 15.0);
        let lab = if i == 0 { 0 } else { pool[(self.rng.f() * pool.len() as f32) as usize % pool.len()] };
        OrbCall { i, k, th: -self.spin + self.rng.range(-0.9, 0.45), age: age * life, life, lab, v: self.rng.f(),
                  id: format!("#{:04x}", (self.rng.f() * 65535.0) as u32) }
    }

    /// where a readout points, a run of lights on that slat burns together, pulsing
    fn burn(&mut self) {
        for c in &self.calls {
            let rb = &mut self.ribs[c.k];
            for j in 0..rb.n {
                let d = ((rb.o + j as f32 / rb.n as f32 * TAU - c.th + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI).abs();
                if d < 0.2 { rb.b[j] = rb.b[j].max(0.75 + 0.25 * (self.t * 4.0 + j as f32).sin()); }
            }
        }
    }

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
            for i in 0..self.calls.len() {   // readouts come and go; the numbers drift while they show
                self.calls[i].age += dt * sp;
                if self.calls[i].age > self.calls[i].life { let c = self.callout(i, 0.0); self.calls[i] = c; }
                else if self.rng.f() < dt * 0.6 { let v = self.calls[i].v + self.rng.range(-0.03, 0.03); self.calls[i].v = v.clamp(0.0, 1.0); }
            }
            self.burn();
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
        // the readouts, each pointing at its part of the sphere while that part faces us
        let (fs, ss) = (self.fs, self.s);
        let acc = g.brush(look.acc)?;
        let ink = g.brush(look.ink)?;
        for c in &self.calls {
            let (mx, my, mz, _) = self.pt(self.ribs[c.k].phi, c.th);
            let f = ((mz - 0.2) * 4.0).min(c.age / 0.8).min((c.life - c.age) / 0.8).clamp(0.0, 1.0);
            if f <= 0.0 || my > self.cy { continue; }
            let side = if mx < self.ox { -1.0 } else { 1.0 };
            let x2 = self.ox + side * (ss * 1.05).max((ss * 1.4).min(self.w / 2.0 - fs * 16.0));
            let ty = my - ss * 0.1 * (c.i as f32 - 1.0);
            unsafe {
                let dc = &g.dc;
                acc.SetOpacity(0.8 * f);
                dc.DrawLine(p2(mx, my), p2(x2, ty), &acc, 1.0, None);
                dc.DrawLine(p2(x2, ty), p2(x2 + side * fs * 3.0, ty), &acc, 1.0, None);
                dc.FillRectangle(&rect(mx - 2.0, my - 2.0, 4.0, 4.0), &acc);
                let tx = x2 + side * fs * 3.6;
                let _ = self.fmt.SetTextAlignment(if side < 0.0 { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING });
                let _ = self.fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let at = |y: f32| if side < 0.0 { rect(tx - 600.0, y - fs, 600.0, fs * 2.0) } else { rect(tx, y - fs, 600.0, fs * 2.0) };
                let l1: Vec<u16> = ORB_LABELS[c.lab].encode_utf16().collect();
                let l2: Vec<u16> = format!("{}   {}", orb_value(c.lab, c.v), c.id).encode_utf16().collect();
                ink.SetOpacity(0.65 * f);
                dc.DrawText(&l1, &self.fmt, &at(ty - fs * 0.7), &ink, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                acc.SetOpacity(0.95 * f);
                dc.DrawText(&l2, &self.fmt, &at(ty + fs * 0.7), &acc, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
        Ok(())
    }
}

/// a lab oscilloscope: a graticule on dark glass, one beam sweeping it, the trace fading by its age; the signal
/// changes every few sweeps (or stays on one, by option). Either side, where a wide screen leaves room: a spectrum of
/// the signal and a second channel rolling. The glass is one mask (tinted each frame); each sweep's pieces of trace
/// are kept as geometry and drawn at the brightness their age allows. 30 fps; still at 0 (one whole trace).
pub struct Scope { mask: ID2D1Bitmap1, factory: ID2D1Factory, round: ID2D1StrokeStyle, fmt: IDWriteTextFormat,
                   div: f32, gx: f32, gy: f32, gw: f32, gh: f32, cy0: f32, w: f32, h: f32, fs: f32,
                   panels: Option<(f32, f32, f32)>, trail: Vec<(ID2D1PathGeometry, f32)>, last: Option<D2D_POINT_2F>,
                   bars: [f32; 32], roll: std::collections::VecDeque<f32>, ch2: f32,
                   t: f32, clk: f32, u: f32, m: usize, sweeps: u32, rng: Rng, still: bool,
                   fade: f32, signal: usize, built: (bool, bool) }   // options: afterglow, signal (0 cycles); in the glass: graticule, side panels

struct ScopeMode { ch: &'static str, tb: &'static str, f: &'static str, pk: &'static [(f32, f32)] }

const SCOPE_MODES: [ScopeMode; 4] = [
    ScopeMode { ch: "0.50 V", tb: "2.00 ms", f: "1.502 kHz", pk: &[(3.0, 1.0), (9.0, 0.3)] },
    ScopeMode { ch: "1.00 V", tb: "500 µs", f: "12.48 kHz", pk: &[(12.0, 0.45), (13.0, 0.8), (14.0, 0.45)] },
    ScopeMode { ch: "0.20 V", tb: "1.00 ms", f: "2.000 kHz", pk: &[(4.0, 1.0), (12.0, 0.33), (20.0, 0.2), (28.0, 0.14)] },
    ScopeMode { ch: "2.00 V", tb: "5.00 µs", f: "6.250 MHz", pk: &[(29.0, 0.5), (30.0, 0.8), (31.0, 1.0)] },
];

/// the signal of mode m at sweep position u (0-1), in divisions
fn scope_y(m: usize, u: f32, t: f32) -> f32 {
    match m {
        0 => 2.3 * (TAU * 3.0 * u + t * 0.12).sin() + 0.55 * (TAU * 9.0 * u + t * 0.5).sin(),
        1 => if u < 0.14 { 0.0 } else { 3.1 * (-(u - 0.14) * 6.0).exp() * (TAU * 13.0 * (u - 0.14)).sin() },
        2 => { let q = (u * 4.0) % 1.0; let e = q.min((q - 0.5).abs()); let s = if q < 0.5 { 1.0 } else { -1.0 };
               2.0 * s + s * 0.9 * (-e * 60.0).exp() * (e * 260.0).cos() }
        _ => 3.0 * (TAU * 31.0 * u + t * 0.7).sin() * (-((u - 0.5) / 0.17).powi(2)).exp(),
    }
}

impl Scope {
    fn built(o: &Json) -> (bool, bool) { (o.get("graticule").str() != Some("off"), o.get("sides").str() != Some("off")) }
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Scope> {
        let (w, h) = (g.w as f32, g.h as f32);
        let built = Self::built(o);
        let light = look.light;
        let div = (w / 10.8).min(h / 8.8).floor();
        let (gw, gh) = (div * 10.0, div * 8.0);
        let (gx, gy) = (((w - gw) / 2.0).round(), ((h - gh) / 2.0).round());
        let mask = g.surface(w, h)?;
        let factory = unsafe { g.dc.GetFactory()? };
        unsafe {   // the glass: a solid frame, dotted divisions, the centre axes ticked in fifths
            let dc = &g.dc;
            let old = dc.GetTarget()?;
            dc.SetTarget(&mask);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let white = g.brush([1.0, 1.0, 1.0, 1.0])?;
            white.SetOpacity(if light { 0.3 } else { 0.22 });
            dc.DrawRectangle(&rect(gx + 0.5, gy + 0.5, gw, gh), &white, 1.0, None);
            if built.0 {
                white.SetOpacity(if light { 0.26 } else { 0.2 });
                for i in 1..10 { let mut y = gy; while y <= gy + gh { dc.FillRectangle(&rect(gx + i as f32 * div, y.round(), 1.0, 1.0), &white); y += div / 5.0; } }
                for j in 1..8 { let mut x = gx; while x <= gx + gw { dc.FillRectangle(&rect(x.round(), gy + j as f32 * div, 1.0, 1.0), &white); x += div / 5.0; } }
            }
            white.SetOpacity(if light { 0.32 } else { 0.26 });
            let (mx, my, tk) = (gx + 5.0 * div + 0.5, gy + 4.0 * div + 0.5, (div * 0.07).max(3.0));
            let mut x = gx; while x <= gx + gw + 0.1 { let xx = x.round() + 0.5; dc.DrawLine(p2(xx, my - tk), p2(xx, my + tk), &white, 1.0, None); x += div / 5.0; }
            let mut y = gy; while y <= gy + gh + 0.1 { let yy = y.round() + 0.5; dc.DrawLine(p2(mx - tk, yy), p2(mx + tk, yy), &white, 1.0, None); y += div / 5.0; }
            dc.EndDraw(None, None)?;
            dc.SetTarget(&old);
        }
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND,
                                                   dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
        let round = unsafe { factory.CreateStrokeStyle(&props, None)? };
        let fs = (div * 0.15).round().max(11.0);
        let fmt = fonts.format(&look.mono.family, fs, 400)?;
        let side = gx - div * 0.6;
        let pw = (side * 0.7).min(div * 2.6);
        let panels = if side > div * 1.2 && built.1 { Some((pw, (gx - div * 0.6 - pw).round(), (gx + gw + div * 0.6).round())) } else { None };
        let mut rng = Rng(0x5c0e_0f00_0000_0001);
        let mut s = Scope { mask, factory, round, fmt, div, gx, gy, gw, gh, cy0: gy + gh / 2.0, w, h, fs, panels, trail: Vec::new(), last: None,
                            bars: [0.0; 32], roll: std::collections::VecDeque::new(), ch2: rng.range(0.0, 100.0),
                            t: rng.range(0.0, 100.0), clk: 0.0, u: 0.0, m: 0, sweeps: 0, rng, still: false, fade: 1.1, signal: 0, built };
        s.opts(o);
        Ok(s)
    }

    /// where the beam is at sweep position u in the current mode
    fn at(&mut self, u: f32) -> D2D_POINT_2F {
        p2(self.gx + u * self.gw, self.cy0 - scope_y(self.m, u, self.t) * self.div + self.rng.range(-0.4, 0.4))
    }

    /// the trace from u0 to u1, kept as one piece of the persistence
    fn piece(&mut self, u0: f32, u1: f32) -> Result<()> {
        let n = ((u1 - u0) * self.gw / 1.5).ceil().max(2.0) as usize;
        let mut pts = Vec::with_capacity(n + 2);
        if u0 > 0.0 { if let Some(l) = self.last { pts.push(l); } }
        for i in pts.len()..=n { let p = self.at(u0 + (u1 - u0) * i as f32 / n as f32); pts.push(p); }
        self.last = pts.last().copied();
        unsafe {
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLines(&pts[1..]);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            self.trail.push((path, self.clk));
        }
        Ok(())
    }

    fn label(&self, g: &Gfx, s: &str, x: f32, base: f32, right: bool, b: &ID2D1SolidColorBrush) {
        let t: Vec<u16> = s.encode_utf16().collect();
        let fs = self.fs;
        unsafe {
            let _ = self.fmt.SetTextAlignment(if right { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING });
            let r = if right { rect(x - 600.0, base - fs * 1.05, 600.0, fs * 1.5) } else { rect(x, base - fs * 1.05, 600.0, fs * 1.5) };
            g.dc.DrawText(&t, &self.fmt, &r, b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }
}

impl Background for Scope {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.fade = match o.get("afterglow").str() { Some("short") => 2.6, Some("long") => 0.45, _ => 1.1 };
        self.signal = match o.get("signal").str() { Some("sine") => 1, Some("square") => 2, Some("burst") => 3, _ => 0 };
        if self.signal > 0 { self.m = [0, 0, 2, 3][self.signal]; }
        Self::built(o) != self.built
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        let was = self.still;
        self.still = level <= 0.0;
        if self.still && !was && self.trail.is_empty() { self.piece(0.0, 1.0)?; self.u = 1.0; }   // stilled: one whole trace
        if !self.still {
            let sp = 0.3 + level * 1.2;
            self.t += dt * sp; self.clk += dt * sp;
            let (clk, fade) = (self.clk, self.fade);
            self.trail.retain(|(_, b)| (clk - b) * fade < 4.0);   // faded out: gone for good
            // the side instruments: spectrum bars ease toward the signal's harmonics, channel 2 rolls on
            for i in 0..32 {
                let mut v = 0.03 + self.rng.f() * 0.05;
                for &(hh, a) in SCOPE_MODES[self.m].pk { v += a * 0.85 * (-((i as f32 + 1.0) - hh).powi(2) / 1.2).exp(); }
                let target = (v * (0.9 + self.rng.f() * 0.2)).min(1.0);
                self.bars[i] += (target - self.bars[i]) * (dt * 8.0).min(1.0);
            }
            self.ch2 += dt * sp;
            let c = self.ch2;
            let c2 = 0.55 * (c * 1.3).sin() * (c * 0.21).sin() + 0.2 * (c * 4.7).sin() + self.rng.range(-0.04, 0.04);
            self.roll.push_front(c2.clamp(-1.0, 1.0));
            if self.roll.len() > 181 { self.roll.pop_back(); }
            let u1 = (self.u + dt * sp / 1.5).min(1.0);
            self.piece(self.u, u1)?;
            self.u = u1;
            if self.u >= 1.0 {   // retrace, and every few sweeps a new signal
                self.u = 0.0; self.last = None;
                self.sweeps += 1;
                if self.signal == 0 && self.sweeps >= 4 + (self.rng.f() * 3.0) as u32 {
                    self.sweeps = 0;
                    self.m = (self.m + 1 + (self.rng.f() * 3.0) as usize) % 4;
                }
            }
        }
        let (w, h, d) = (self.w, self.h, self.div);
        let light = look.light;
        let blend = if light { D2D1_PRIMITIVE_BLEND_SOURCE_OVER } else { D2D1_PRIMITIVE_BLEND_ADD };
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let rt: &ID2D1RenderTarget = dc;
            let ink = look.ink;
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { r: ink[0], g: ink[1], b: ink[2], a: if light { 0.05 } else { 0.045 } } },
                         D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { r: ink[0], g: ink[1], b: ink[2], a: 0.0 } }];
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let rr = (w * w + h * h).sqrt() * 0.55;
            let glow = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(w / 2.0, h / 2.0), gradientOriginOffset: p2(0.0, 0.0),
                                                                                         radiusX: rr, radiusY: rr }, None, &coll)?;
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &glow);
            let inkb = g.brush(look.ink)?;
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);   // FillOpacityMask needs it
            dc.FillOpacityMask(&self.mask, &inkb, Some(&rect(0.0, 0.0, w, h)), None);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            // the persistence: a wide soft pass and then the line, each piece as bright as its age allows
            let dim = g.brush(look.dim)?;
            dc.SetPrimitiveBlend(blend);
            for (lw, a) in [((d * 0.09).max(5.0), 0.08), (1.6, 0.75)] {
                for (p, b) in &self.trail {
                    let k = (-(self.clk - b) * self.fade).exp();
                    if k < 0.02 { continue; }
                    dim.SetOpacity(a * k);
                    dc.DrawGeometry(p, &dim, lw, &self.round);
                }
            }
            dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            // the side instruments
            if let Some((pw, lx, rx)) = self.panels {
                let (gy, gh) = (self.gy, self.gh);
                for x in [lx, rx] {
                    inkb.SetOpacity(0.18); dc.DrawRectangle(&rect(x + 0.5, gy + 0.5, pw, gh), &inkb, 1.0, None);
                    inkb.SetOpacity(0.1);
                    if x == lx { for k in 1..4 { dc.FillRectangle(&rect(x + pw * k as f32 / 4.0, gy, 1.0, gh), &inkb); } }
                    else { dc.FillRectangle(&rect(x + pw / 2.0, gy, 1.0, gh), &inkb); }
                }
                dc.SetPrimitiveBlend(blend);
                let bh = gh / 32.0;
                dim.SetOpacity(0.5);
                for (i, v) in self.bars.iter().enumerate() { dc.FillRectangle(&rect(lx + 1.0, gy + i as f32 * bh + bh * 0.2, (v * (pw - 2.0)).max(1.0), bh * 0.6), &dim); }
                if self.roll.len() > 1 {
                    let step = gh / 180.0;
                    let pts: Vec<D2D_POINT_2F> = self.roll.iter().enumerate().map(|(i, v)| p2(rx + pw / 2.0 + v * pw * 0.42, gy + i as f32 * step)).collect();
                    let path = self.factory.CreatePathGeometry()?;
                    let sink = path.Open()?;
                    sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddLines(&pts[1..]);
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    sink.Close()?;
                    for (lw, a) in [((d * 0.07).max(4.0), 0.07), (1.4, 0.7)] { dim.SetOpacity(a); dc.DrawGeometry(&path, &dim, lw, &self.round); }
                }
                dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            }
            // the beam itself: a short white-hot head on the trace
            if level > 0.0 && self.u < 1.0 {
                dc.SetPrimitiveBlend(blend);
                let hi = g.brush(look.hi)?;
                let (n, back) = (14, 0.025);
                let mut prev = self.at((self.u - back).max(0.0));
                for i in 1..=n {
                    let q = self.at((self.u - back * (1.0 - i as f32 / n as f32)).max(0.0));
                    hi.SetOpacity(0.9 * i as f32 / n as f32);
                    dc.DrawLine(prev, q, &hi, 1.0 + 1.6 * i as f32 / n as f32, &self.round);
                    prev = q;
                }
                let hc = look.hi;
                let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { r: hc[0], g: hc[1], b: hc[2], a: 0.55 } },
                             D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { r: hc[0], g: hc[1], b: hc[2], a: 0.0 } }];
                let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                let hb = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: prev, gradientOriginOffset: p2(0.0, 0.0),
                                                                                           radiusX: d * 0.22, radiusY: d * 0.22 }, None, &coll)?;
                dc.FillRectangle(&rect(prev.x - d * 0.22, prev.y - d * 0.22, d * 0.44, d * 0.44), &hb);
                dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            }
            // the readouts round the graticule
            let md = &SCOPE_MODES[self.m];
            let (fs, pad) = (self.fs, self.fs * 0.8);
            let (top, bot) = (self.gy - pad, self.gy + self.gh + pad + fs);
            let txt = g.brush([ink[0], ink[1], ink[2], 0.55])?;
            self.label(g, &format!("CH1  {}/div   DC", md.ch), self.gx, top, false, &txt);
            self.label(g, &format!("M  {}/div", md.tb), self.gx, bot, false, &txt);
            self.label(g, &format!("f  {}", md.f), self.gx + self.gw, bot, true, &txt);
            self.label(g, "TRIG'D", self.gx + self.gw, top, true, &g.brush([look.acc[0], look.acc[1], look.acc[2], 0.85])?);
            if let Some((_, lx, rx)) = self.panels {
                self.label(g, "FFT  CH1", lx, top, false, &txt); self.label(g, "0 dB", lx, bot, false, &txt);
                self.label(g, "CH2  ROLL", rx, top, false, &txt); self.label(g, "5.00 s/div", rx, bot, false, &txt);
            }
        }
        Ok(())
    }
}

/// one great ring of thin jagged data strokes, woven into a band that swells and pinches as it slowly turns. Every
/// stroke has its own write head running round it, rewriting the data as it passes (the bright stretch just behind
/// it); now and then a few strokes diverge in the accent, with a reading. Each stroke is one path a frame. 20 fps;
/// still at 0.
pub struct Oracle { factory: ID2D1Factory, fmt: IDWriteTextFormat, r: f32, ox: f32, oy: f32, w: f32, h: f32, fs: f32,
                    cos: Vec<f32>, sin: Vec<f32>, env: Vec<f32>, t: f32, rot: f32, read: f32,
                    tr: Vec<OrTr>, anom: Option<OrAnom>, wait: f32, rng: Rng, still: bool,
                    wild: f32, dir: f32, red: bool, scales: bool }   // options: stroke amplitude, turning, divergences, scales

struct OrTr { k: usize, off: f32, amp: f32, v: Vec<f32>, head: f32, sp: f32, a: f32, w: f32 }
struct OrAnom { c: usize, w: i32, k: usize, n: usize, pull: f32, age: f32, life: f32, val: f32, id: String }

const OR_M: usize = 480;
const OR_K: usize = 34;

impl Oracle {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Oracle> {
        let (w, h) = (g.w as f32, g.h as f32);
        let r = w.min(h) * 0.32;
        let fs = (r * 0.032).round().max(10.0);
        let mut rng = Rng(0x0ac1_e000_0000_0001);
        let cos = (0..OR_M).map(|i| (i as f32 / OR_M as f32 * TAU).cos()).collect();
        let sin = (0..OR_M).map(|i| (i as f32 / OR_M as f32 * TAU).sin()).collect();
        let tr = (0..OR_K).map(|k| {
            let u = k as f32 / (OR_K - 1) as f32;
            OrTr { k, off: (u - 0.5) * 0.36 + rng.range(-0.02, 0.02), amp: rng.range(0.025, 0.075), v: vec![0.0; OR_M], head: rng.range(0.0, OR_M as f32),
                   sp: rng.range(0.04, 0.12) * OR_M as f32 * if rng.f() < 0.35 { -1.0 } else { 1.0 }, a: rng.range(0.1, 0.38),
                   w: if rng.f() < 0.18 { 1.25 } else { 0.7 } }
        }).collect();
        let mut s = Oracle { factory: unsafe { g.dc.GetFactory()? }, fmt: fonts.format(&look.mono.family, fs, 400)?, r, ox: w / 2.0, oy: h / 2.0, w, h, fs,
                             cos, sin, env: vec![0.0; OR_M], t: rng.range(0.0, 500.0), rot: rng.range(0.0, TAU), read: 0.99 + rng.range(0.0, 0.009),
                             tr, anom: None, wait: 0.0, rng, still: false, wild: 1.0, dir: 1.0, red: true, scales: true };
        s.opts(o);
        for k in 0..OR_K { for i in 0..OR_M { let v = s.field(k, i); s.tr[k].v[i] = v; } }
        if s.red { let a = s.rng.range(0.12, 0.3); s.diverge(a); }   // one already showing, so even a stilled ring has its red
        Ok(s)
    }

    /// the data: a few slow waves around the ring (shared by neighbouring strokes, so the band flows), plus jag and spikes
    fn field(&mut self, k: usize, i: usize) -> f32 {
        let a = i as f32 / OR_M as f32 * TAU;
        let (t, p) = (self.t, k as f32 * 0.12);
        let mut v = (3.0 * a + t * 0.35 + p).sin() * 0.45 + (7.0 * a - t * 0.5 + p * 1.7).sin() * 0.3 + (17.0 * a + t * 0.9 - p * 2.3).sin() * 0.15;
        v += (self.rng.f() - 0.5) * 0.45;
        if self.rng.f() < 0.03 { v += if self.rng.f() < 0.5 { -1.0 } else { 1.0 } * self.rng.range(0.8, 2.0); }
        v
    }

    fn rad(&self, tr: &OrTr, i: usize) -> f32 { let e = self.env[i]; self.r * (1.0 + tr.off * e + tr.amp * self.wild * e * tr.v[i]) }

    fn diverge(&mut self, age: f32) {   // a few neighbouring strokes pulled out of true for a while, with a reading
        let life = self.rng.range(8.0, 14.0);
        let val = self.rng.range(0.012, 0.09);
        self.anom = Some(OrAnom { c: (self.rng.f() * OR_M as f32) as usize % OR_M, w: self.rng.range(14.0, 28.0) as i32, k: (self.rng.f() * OR_K as f32) as usize % OR_K,
                                  n: 3 + (self.rng.f() * 3.0) as usize, pull: self.rng.range(0.05, 0.1), age: age * life, life, val,
                                  id: format!("#{:04x}", (self.rng.f() * 65535.0) as u32) });
        self.read = 1.0 - val * self.rng.range(0.05, 0.15);
    }

    /// a polyline along a stroke, n samples from `from`, in ring coordinates
    fn path(&self, pts: impl Iterator<Item = D2D_POINT_2F>) -> Result<ID2D1PathGeometry> {
        unsafe {
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let mut first = true;
            for p in pts { if first { sink.BeginFigure(p, D2D1_FIGURE_BEGIN_HOLLOW); first = false; } else { sink.AddLine(p); } }
            if !first { sink.EndFigure(D2D1_FIGURE_END_OPEN); }
            sink.Close()?;
            Ok(path)
        }
    }
    fn trace(&self, tr: &OrTr, from: i64, n: i64, dir: i64) -> Result<ID2D1PathGeometry> {
        let m = OR_M as i64;
        self.path((0..=n).map(|j| { let i = ((from + dir * j) % m + m) as usize % OR_M; let r = self.rad(tr, i); p2(r * self.cos[i], r * self.sin[i]) }))
    }

    fn say(&self, g: &Gfx, s: &str, x: f32, y: f32, al: i32, b: &ID2D1SolidColorBrush) {   // al: -1 right of x, 0 centred, 1 left; y the middle
        let t: Vec<u16> = s.encode_utf16().collect();
        let fs = self.fs;
        unsafe {
            let _ = self.fmt.SetTextAlignment(match al { -1 => DWRITE_TEXT_ALIGNMENT_TRAILING, 0 => DWRITE_TEXT_ALIGNMENT_CENTER, _ => DWRITE_TEXT_ALIGNMENT_LEADING });
            let _ = self.fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let r = match al { -1 => rect(x - 600.0, y - fs, 600.0, fs * 2.0), 0 => rect(x - 300.0, y - fs, 600.0, fs * 2.0), _ => rect(x, y - fs, 600.0, fs * 2.0) };
            g.dc.DrawText(&t, &self.fmt, &r, b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }
}

impl Background for Oracle {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 20.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.wild = match o.get("strokes").str() { Some("calm") => 0.45, Some("wild") => 2.2, _ => 1.0 };
        self.dir = if o.get("direction").str() == Some("anticlockwise") { -1.0 } else { 1.0 };
        self.red = o.get("red").str() != Some("off");
        if !self.red { self.anom = None; }
        self.scales = o.get("scales").str() != Some("off");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if !self.still {
            let sp = 0.4 + level * 1.6;
            self.t += dt * sp; self.rot += dt * 0.016 * sp * self.dir;
            let m = OR_M as i64;
            for k in 0..OR_K {   // each head rewrites the samples it passes
                let (from, dir) = (self.tr[k].head.floor() as i64, self.tr[k].sp.signum() as i64);
                self.tr[k].head += self.tr[k].sp * dt * sp;
                let n = (self.tr[k].head.floor() as i64 - from).abs();
                for j in 1..=n { let i = ((from + dir * j) % m + m) as usize % OR_M; let v = self.field(k, i); self.tr[k].v[i] = v; }
                self.tr[k].head = self.tr[k].head.rem_euclid(OR_M as f32);
            }
            let mut ended = false;
            if let Some(a) = &mut self.anom { a.age += dt * sp; ended = a.age > a.life; }
            else if self.red { self.wait -= dt * sp; if self.wait <= 0.0 { self.diverge(0.0); } }
            if ended { self.anom = None; self.wait = self.rng.range(1.5, 4.0); }
        }
        let (r, ox, oy, w, h, t) = (self.r, self.ox, self.oy, self.w, self.h, self.t);
        for i in 0..OR_M {   // the band's thickness: swells and pinches around the ring, drifting slowly
            let a = i as f32 / OR_M as f32 * TAU;
            self.env[i] = 0.14 + 0.86 * (0.5 + 0.5 * (2.0 * a + t * 0.06).sin()).powf(2.2) * (0.7 + 0.3 * (5.0 * a - t * 0.11).sin());
        }
        let ink = look.ink;
        let ic = |a: f32| D2D1_COLOR_F { r: ink[0], g: ink[1], b: ink[2], a };
        unsafe {
            let dc = &g.dc;
            let rt: &ID2D1RenderTarget = dc;
            dc.Clear(Some(&rgba(look.bg)));
            // the parts that never move: a faint halo where the band sits, a vignette, guide circles, a centre mark
            // the page's gradient runs from R .6 to R 1.55; here from the centre, so its stops are moved out to match
            let (r0, r1) = (r * 0.6, r * 1.55);
            let st = |f: f32| (r0 + (r1 - r0) * f) / r1;
            let halo = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: ic(0.0) }, D2D1_GRADIENT_STOP { position: st(0.0), color: ic(0.0) },
                                                          D2D1_GRADIENT_STOP { position: st(0.4), color: ic(if look.light { 0.05 } else { 0.04 }) },
                                                          D2D1_GRADIENT_STOP { position: 1.0, color: ic(0.0) }], D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let hb = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(ox, oy), gradientOriginOffset: p2(0.0, 0.0), radiusX: r1, radiusY: r1 }, None, &halo)?;
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &hb);
            if !look.light {
                let (v0, v1) = (r * 1.1, (w * w + h * h).sqrt() * 0.62);
                let vg = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F::default() },
                                                            D2D1_GRADIENT_STOP { position: v0 / v1, color: D2D1_COLOR_F::default() },
                                                            D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 } }],
                                                          D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                let vb = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(ox, oy), gradientOriginOffset: p2(0.0, 0.0), radiusX: v1, radiusY: v1 }, None, &vg)?;
                dc.FillRectangle(&rect(0.0, 0.0, w, h), &vb);
            }
            let inkb = g.brush(ink)?;
            for (rr, a) in [(r * 0.62, 0.09), (r * 0.6, 0.04), (r * 1.4, 0.06), (r * 1.47, 0.03)] {
                inkb.SetOpacity(a);
                dc.DrawEllipse(&D2D1_ELLIPSE { point: p2(ox, oy), radiusX: rr, radiusY: rr }, &inkb, 1.0, None);
            }
            inkb.SetOpacity(0.22);
            dc.DrawLine(p2(ox - 7.0, oy), p2(ox + 7.0, oy), &inkb, 1.0, None);
            dc.DrawLine(p2(ox, oy - 7.0), p2(ox, oy + 7.0), &inkb, 1.0, None);
            // scales: fine ticks outside turning against the ring, coarse ones inside turning with it, faster
            if self.scales {
                for (r0, n, l, rot, al) in [(r * 1.4, 180, 5.0f32, -self.rot * 0.6, 0.22), (r * 0.62, 60, -6.0, self.rot * 1.7, 0.2)] {
                    inkb.SetOpacity(al);
                    for k in 0..n {
                        let a = rot + k as f32 / n as f32 * TAU;
                        let ll = if k % 15 != 0 { l } else { l * 2.4 };
                        let (s, c) = a.sin_cos();
                        dc.DrawLine(p2(ox + c * r0, oy + s * r0), p2(ox + c * (r0 + ll), oy + s * (r0 + ll)), &inkb, 1.0, None);
                    }
                }
            }
            let (s, c) = self.rot.sin_cos();
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: ox, M32: oy });
            let line = g.brush(if look.light { ink } else { look.dim })?;   // on paper the faint colour vanishes: draw in ink
            for tr in &self.tr {
                line.SetOpacity(tr.a);
                let p = self.trace(tr, 0, OR_M as i64, 1)?;
                dc.DrawGeometry(&p, &line, tr.w, None);
            }
            // the freshly written stretch behind every head, brightest nearest it
            let hi = g.brush(look.hi)?;
            for tr in &self.tr {
                let (hd, dir) = (tr.head.floor() as i64, -(tr.sp.signum() as i64));
                for (o, n, al) in [(0i64, 10i64, 0.75), (10, 16, 0.4), (26, 24, 0.16)] {
                    hi.SetOpacity(al);
                    let p = self.trace(tr, hd + dir * o, n, dir)?;
                    dc.DrawGeometry(&p, &hi, 1.0, None);
                }
            }
            // a divergence: a few strokes pulled out of true, in the accent
            let acc = g.brush(look.acc)?;
            if let Some(an) = &self.anom {
                let f = (an.age / 0.9).min((an.life - an.age) / 0.9).clamp(0.0, 1.0);
                let m = OR_M as i64;
                for s in 0..an.n {
                    let tr = &self.tr[(an.k + s) % OR_K];
                    let p = self.path((-an.w..=an.w).map(|j| {
                        let i = ((an.c as i64 + j as i64) % m + m) as usize % OR_M;
                        let bump = (j as f32 / an.w as f32 * std::f32::consts::PI / 2.0).cos().powi(2) * f;
                        let rr = self.rad(tr, i) + r * an.pull * bump * if s % 2 == 1 { -0.6 } else { 1.0 } * (1.0 + 0.3 * (j as f32 * 0.45 + s as f32 * 1.9).sin());
                        p2(rr * self.cos[i], rr * self.sin[i])
                    }))?;
                    let a = (0.95 - s as f32 * 0.12) * f;
                    acc.SetOpacity(a * 0.25); dc.DrawGeometry(&p, &acc, 5.0, None);   // its glow
                    acc.SetOpacity(a); dc.DrawGeometry(&p, &acc, 1.2, None);
                }
            }
            dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
            // its leader line and reading, upright, outside the ring
            let fs = self.fs;
            if let Some(an) = &self.anom {
                let f = (an.age / 0.9).min((an.life - an.age) / 0.9).clamp(0.0, 1.0);
                let a = self.rot + an.c as f32 / OR_M as f32 * TAU;
                let (s, c) = a.sin_cos();
                let (x1, y1, x2, y2) = (ox + c * r * 1.22, oy + s * r * 1.22, ox + c * r * 1.36, oy + s * r * 1.36);
                let side = if c < 0.0 { -1.0 } else { 1.0 };
                acc.SetOpacity(0.8 * f);
                dc.DrawLine(p2(x1, y1), p2(x2, y2), &acc, 1.0, None);
                dc.DrawLine(p2(x2, y2), p2(x2 + side * fs * 3.0, y2), &acc, 1.0, None);
                dc.FillRectangle(&rect(x1 - 2.0, y1 - 2.0, 4.0, 4.0), &acc);
                let tx = x2 + side * fs * 3.6;
                let al = if side < 0.0 { -1 } else { 1 };
                inkb.SetOpacity(0.55 * f);
                self.say(g, "DIVERGENCE", tx, y2 - fs * 0.7, al, &inkb);
                acc.SetOpacity(0.9 * f);
                self.say(g, &format!("Δ {:.4}   {}", an.val, an.id), tx, y2 + fs * 0.7, al, &acc);
            }
            // the centre: one quiet reading
            inkb.SetOpacity(0.38);
            self.say(g, "P R O J E C T I O N", ox, oy - fs * 2.2, 0, &inkb);
            inkb.SetOpacity(0.7);
            self.say(g, &format!("{:.5}", self.read), ox, oy + fs * 2.2, 0, &inkb);
        }
        Ok(())
    }
}

/// neon signs on a wet brick wall in the rain: two or three words and a shape in tube, each in its own colour, a
/// glow thrown on the wall, broken reflections on the ground; now and then a sign stutters, and one letter won't
/// stay lit. Each letter is drawn once (its glyph outline stroked as tube, blurred twice for the glow, a hot core)
/// into its own little bitmap; the wall too. Each frame lays them out with their brightness. 24 fps; still at 0.
pub struct Signs { back: ID2D1Bitmap1, signs: Vec<Sign>, drops: Vec<(f32, f32, f32, f32)>, rng: Rng, u: f32, w: f32, h: f32, fy: f32,
                   still: bool, flick: f32, rain: u8, built: (usize, bool, u8) }   // options: flicker rate, rain; built: signs, plain wall, flicker

struct Sign { letters: Vec<(ID2D1Bitmap1, f32, f32, f32, f32, f32)>, col: Color, x: f32, y: f32, buzz: f32, bad: Option<usize> }   // bitmap, x, y, w, h, on

const SIGN_WORDS: [&str; 8] = ["OPEN", "LIVE", "ALL NIGHT", "NO VACANCY", "ON AIR", "COFFEE", "TATTOO", "ARCADE"];

fn font_face(fonts: &Fonts, fam: &str) -> Result<IDWriteFontFace> {
    let w: Vec<u16> = fam.encode_utf16().chain([0]).collect();
    let (mut i, mut found) = (0u32, windows::Win32::Foundation::BOOL(0));
    unsafe {
        fonts.set.FindFamilyName(PCWSTR(w.as_ptr()), &mut i, &mut found)?;
        let i = if found.as_bool() { i } else { 0 };
        fonts.set.GetFontFamily(i)?.GetFont(0)?.CreateFontFace()?.cast()
    }
}

impl Signs {
    fn built(o: &Json) -> (usize, bool, u8) {
        (match o.get("signs").str() { Some("two") => 2, Some("five") => 5, _ => 4 }, o.get("wall").str() == Some("plain"),
         match o.get("flicker").str() { Some("rare") => 1, Some("often") => 2, _ => 0 })
    }

    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Signs> {
        let (w, h) = (g.w as f32, g.h as f32);
        let u = w.min(h);
        let fy = (h * 0.8).round();
        let light = look.light;
        let built = Self::built(o);
        let mut rng = Rng(0x5195_0000_0000_0001 ^ (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1) | 1));
        let factory = unsafe { g.dc.GetFactory()? };
        let back = g.surface(w, h)?;
        let dc = &g.dc;
        let old = unsafe { dc.GetTarget()? };
        unsafe {   // the wall: courses of brick, barely there; the wet ground below
            dc.SetTarget(&back);
            dc.BeginDraw();
            dc.Clear(Some(&rgba(look.bg)));
            let ink = g.brush(look.ink)?;
            if !built.1 {
                ink.SetOpacity(if light { 0.06 } else { 0.035 });
                let bh = (u / 40.0).round().max(10.0);
                let bw = bh * 3.2;
                let (mut y, mut r) = (0.0, 0);
                while y < fy {
                    dc.DrawLine(p2(0.0, y + 0.5), p2(w, y + 0.5), &ink, 1.0, None);
                    let mut x = (r % 2) as f32 * bw / 2.0;
                    while x < w { let xx = x.round() + 0.5; dc.DrawLine(p2(xx, y), p2(xx, fy.min(y + bh)), &ink, 1.0, None); x += bw; }
                    y += bh; r += 1;
                }
            }
            let rt: &ID2D1RenderTarget = dc;
            let ic = |a: f32| D2D1_COLOR_F { r: look.ink[0], g: look.ink[1], b: look.ink[2], a };
            let coll = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: ic(if light { 0.08 } else { 0.05 }) },
                                                          D2D1_GRADIENT_STOP { position: 1.0, color: ic(0.0) }], D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let gb = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p2(0.0, fy), endPoint: p2(0.0, h) }, None, &coll)?;
            dc.FillRectangle(&rect(0.0, fy, w, h - fy), &gb);
            ink.SetOpacity(0.25);
            dc.FillRectangle(&rect(0.0, fy, w, 1.0), &ink);
            dc.EndDraw(None, None)?;
        }
        // the signs: words and a shape, each in its own colour, laid out so they don't overlap
        let cols = [look.acc, look.ink, look.acc2];
        let mut words: Vec<&str> = SIGN_WORDS.to_vec();
        let mut spots = vec![(0.2f32, 0.22f32), (0.74, 0.3), (0.46, 0.56), (0.18, 0.62), (0.8, 0.66)];
        for i in (1..words.len()).rev() { let j = (rng.f() * (i + 1) as f32) as usize % (i + 1); words.swap(i, j); }
        for i in (1..spots.len()).rev() { let j = (rng.f() * (i + 1) as f32) as usize % (i + 1); spots.swap(i, j); }
        let disp = [("Instrument Serif", true), ("Chakra Petch", false), ("Saira Stencil One", false)];
        let tube = (u / 260.0).max(2.0);
        let bad_p = [0.6, 0.2, 1.0][built.2 as usize];
        let blur = g.effect(&CLSID_D2D1GaussianBlur)?;
        // one letter or shape: `geo` stroked as tube in `col`, glowing, with a hot core; into a bitmap of w x h
        let tubed = |geo: &ID2D1Geometry, bw: f32, bh: f32, m: windows::Foundation::Numerics::Matrix3x2, col: Color, core: Color, core_a: f32| -> Result<ID2D1Bitmap1> {
            let img = g.surface(bw, bh)?;
            let tmp = g.surface(bw, bh)?;
            unsafe {
                dc.SetTarget(&tmp);
                dc.BeginDraw();
                dc.Clear(Some(&D2D1_COLOR_F::default()));
                dc.SetTransform(&m);
                let cb = g.brush(col)?;
                dc.DrawGeometry(geo, &cb, tube / (m.M11.abs().max(1e-3)), None);
                dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                dc.EndDraw(None, None)?;
                dc.SetTarget(&img);
                dc.BeginDraw();
                dc.Clear(Some(&D2D1_COLOR_F::default()));
                blur.SetInput(0, &tmp, true);
                for sd in [tube * 3.0, tube] {   // the canvas's shadowBlur is about twice the deviation
                    crate::gfx::set_f32(&blur, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, sd)?;
                    dc.DrawImage(&blur.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                    dc.DrawBitmap(&tmp, None, 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
                }
                blur.SetInput(0, None, true);
                dc.SetTransform(&m);
                let kb = g.brush(core)?;
                kb.SetOpacity(core_a);
                dc.DrawGeometry(geo, &kb, tube * 0.4 / (m.M11.abs().max(1e-3)), None);
                dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                dc.EndDraw(None, None)?;
            }
            Ok(img)
        };
        let mut signs = Vec::new();
        for i in 0..built.0 {
            let (sx, sy) = spots[i];
            let col = cols[i % 3];
            if i == 3 {   // an outline shape in tube: an arrow, a star, a ring with a bolt, a cocktail glass
                let s = u * 0.09;
                let k = (rng.f() * 4.0) as usize % 4;
                let path = unsafe { factory.CreatePathGeometry()? };
                unsafe {
                    let sk = path.Open()?;
                    let fig = |pts: &[(f32, f32)], closed: bool| {
                        sk.BeginFigure(p2(pts[0].0 * s, pts[0].1 * s), D2D1_FIGURE_BEGIN_HOLLOW);
                        for p in &pts[1..] { sk.AddLine(p2(p.0 * s, p.1 * s)); }
                        sk.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
                    };
                    match k {
                        0 => fig(&[(-1.0, -0.25), (0.3, -0.25), (0.3, -0.6), (1.0, 0.0), (0.3, 0.6), (0.3, 0.25), (-1.0, 0.25)], true),
                        1 => { let p: Vec<(f32, f32)> = (0..=10).map(|i| { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                                                                           let r = if i % 2 == 1 { 0.42 } else { 1.0 }; (a.cos() * r, a.sin() * r) }).collect();
                               fig(&p, false) }
                        2 => { let p: Vec<(f32, f32)> = (0..=48).map(|i| { let a = i as f32 / 48.0 * TAU; (a.cos() * 0.9, a.sin() * 0.9) }).collect();
                               fig(&p, true); fig(&[(0.15, -0.6), (-0.25, 0.05), (0.15, 0.05), (-0.15, 0.6)], false) }
                        _ => { fig(&[(-0.7, -0.6), (0.7, -0.6), (0.0, 0.15)], true); fig(&[(0.0, 0.15), (0.0, 0.8)], false); fig(&[(-0.4, 0.8), (0.4, 0.8)], false) }
                    }
                    sk.Close()?;
                }
                let m = windows::Foundation::Numerics::Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: s * 1.5, M32: s * 1.5 };
                let img = tubed(&path.cast()?, s * 3.0, s * 3.0, m, col, [1.0, 1.0, 1.0, 1.0], 0.5)?;
                signs.push(Sign { letters: vec![(img, sx * w - s * 1.5, sy * fy - s * 1.5, s * 3.0, s * 3.0, 1.0)], col, x: sx * w, y: sy * fy, buzz: 0.0, bad: None });
                continue;
            }
            let word = words[i];
            let (fam, italic) = disp[i % 3];
            let size = u * if word.len() > 6 { 0.07 } else { 0.1 };
            let face = font_face(fonts, fam)?;
            let mut fm = DWRITE_FONT_METRICS::default();
            unsafe { face.GetMetrics(&mut fm); }
            let em = size / fm.designUnitsPerEm as f32;
            let mid = (fm.ascent as f32 - fm.descent as f32) / 2.0 * em;   // the canvas's 'middle': halfway up the em box
            let cps: Vec<u32> = word.chars().map(|c| c as u32).collect();
            let mut idx = vec![0u16; cps.len()];
            let mut gm = vec![DWRITE_GLYPH_METRICS::default(); cps.len()];
            unsafe {
                face.GetGlyphIndices(cps.as_ptr(), cps.len() as u32, idx.as_mut_ptr())?;
                face.GetDesignGlyphMetrics(idx.as_ptr(), idx.len() as u32, gm.as_mut_ptr(), false)?;
            }
            let adv: Vec<f32> = gm.iter().map(|m| m.advanceWidth as f32 * em).collect();
            let total: f32 = adv.iter().sum();
            let pad = size * 0.5;
            let mut x = sx * w - total / 2.0;
            let mut letters = Vec::new();
            for (k, ch) in word.chars().enumerate() {
                let (bw, bh) = ((adv[k] + pad * 2.0).ceil().max(1.0), (size * 1.5 + pad * 2.0).ceil());
                if ch != ' ' {
                    let path = unsafe { factory.CreatePathGeometry()? };
                    unsafe {
                        let sk = path.Open()?;
                        face.GetGlyphRunOutline(size, idx[k..].as_ptr(), None, None, 1, false, false, &sk)?;
                        sk.Close()?;
                    }
                    // the baseline sits `mid` below the middle; italic: the serif slanted, as the browser makes it
                    let m = windows::Foundation::Numerics::Matrix3x2 { M11: 1.0, M12: 0.0, M21: if italic { -0.2 } else { 0.0 }, M22: 1.0,
                                                                       M31: pad, M32: bh / 2.0 + mid };
                    let img = tubed(&path.cast()?, bw, bh, m, col, if light { col } else { [1.0, 1.0, 1.0, 1.0] }, 0.55)?;
                    letters.push((img, x - pad, sy * fy - bh / 2.0, bw, bh, 1.0));
                }
                x += adv[k];
            }
            let n = letters.len();
            let bad = if rng.f() < bad_p && n > 1 { Some(1 + (rng.f() * (n - 1) as f32) as usize % (n - 1)) } else { None };
            signs.push(Sign { letters, col, x: sx * w, y: sy * fy, buzz: 0.0, bad });
        }
        unsafe { dc.SetTarget(&old); }
        let mut s = Signs { back, signs, drops: Vec::new(), rng, u, w, h, fy, still: false, flick: 1.0, rain: 9, built };
        s.opts(o);
        Ok(s)
    }
}

impl Background for Signs {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 24.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.flick = match o.get("flicker").str() { Some("rare") => 0.25, Some("often") => 4.0, _ => 1.0 };
        let rain = match o.get("rain").str() { Some("off") => 0, Some("heavy") => 2, _ => 1 };
        if rain != self.rain {   // the drops: thin streaks catching the light
            self.rain = rain;
            let n = if rain == 0 { 0 } else { (self.w / if rain == 2 { 3.5 } else { 9.0 }).round() as usize };
            let (w, h) = (self.w, self.h);
            self.drops = (0..n).map(|_| (self.rng.range(0.0, w), self.rng.range(-h, h), self.rng.range(0.8, 1.2), self.rng.range(0.5, 1.2))).collect();
        }
        Self::built(o) != self.built
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if !self.still {
            let sp = 0.4 + level * 1.6;
            for sg in &mut self.signs {
                if sg.buzz > 0.0 {   // a stutter: the whole sign flickers, then comes back
                    sg.buzz -= dt;
                    let on = if sg.buzz <= 0.0 || self.rng.f() < 0.45 { 1.0 } else { 0.0 };
                    for l in &mut sg.letters { l.5 = on; }
                } else if self.rng.f() < dt * 0.025 * sp * self.flick { sg.buzz = self.rng.range(0.2, 1.1); }
                if let (Some(b), true) = (sg.bad, sg.buzz <= 0.0) {   // the letter that won't stay lit
                    let l = &mut sg.letters[b];
                    let lit = l.5 > 0.5;
                    let keep = self.rng.f() < if lit { 0.93 } else { 0.3 };
                    l.5 = if keep { if lit { 1.0 } else { 0.0 } } else if lit { 0.0 } else { 1.0 };
                    if l.5 == 0.0 && self.rng.f() < 0.02 { l.5 = 0.35; }
                }
            }
            let (u, h, w) = (self.u, self.h, self.w);
            for d in &mut self.drops {
                d.1 += d.2 * u * 1.6 * dt * sp;
                if d.1 > h { d.1 = self.rng.range(-80.0, 0.0); d.0 = self.rng.range(0.0, w + 40.0); }
            }
        }
        let (fy, u) = (self.fy, self.u);
        unsafe {
            let dc = &g.dc;
            let rt: &ID2D1RenderTarget = dc;
            dc.DrawBitmap(&self.back, Some(&rect(0.0, 0.0, self.w, self.h)), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
            dc.SetPrimitiveBlend(if look.light { D2D1_PRIMITIVE_BLEND_SOURCE_OVER } else { D2D1_PRIMITIVE_BLEND_ADD });
            for sg in &self.signs {
                let lit = sg.letters.iter().map(|l| l.5).sum::<f32>() / sg.letters.len().max(1) as f32;
                if lit > 0.05 {   // the glow the sign throws on the wall around it
                    let r = u * 0.28;
                    let c = sg.col;
                    let coll = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: 0.07 * lit } },
                                                                  D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: 0.0 } }],
                                                                D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                    let gb = dc.CreateRadialGradientBrush(&D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: p2(sg.x, sg.y), gradientOriginOffset: p2(0.0, 0.0),
                                                                                               radiusX: r, radiusY: r }, None, &coll)?;
                    dc.FillRectangle(&rect(sg.x - r, sg.y - r, r * 2.0, r * 2.0), &gb);
                }
                for (img, x, y, bw, bh, on) in &sg.letters {
                    dc.DrawBitmap(img, Some(&rect(*x, *y, *bw, *bh)), 0.12 + 0.88 * on, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
                    // and its reflection on the wet ground, stretched and broken up
                    let ry = fy + (fy - y - bh) * 0.35;
                    dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: -0.6, M31: 0.0, M32: ry + bh * 0.6 });
                    dc.DrawBitmap(img, Some(&rect(*x, 0.0, *bw, *bh)), 0.16 * on, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
                    dc.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
                }
            }
            dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            if !self.drops.is_empty() {
                let ink = g.brush(look.ink)?;
                ink.SetOpacity(if look.light { 0.12 } else { 0.1 });
                for d in &self.drops { dc.DrawLine(p2(d.0, d.1), p2(d.0 - d.3 * 2.0, d.1 + d.3 * u * 0.035), &ink, 1.0, None); }
            }
        }
        Ok(())
    }
}

/// a blackboard: smudged with old lessons, a chalk tray along the bottom. Lines of working, a plotted graph and a
/// little flowchart get written up a stroke at a time; when the board is full, an eraser wipes it in bands (or it
/// clears at once), and the writing starts again. The board is drawn once; the chalk lives in its own layer, kept
/// by a second device context, drawn into as the writing goes. 20 fps while writing or wiping; still at 0.
pub struct Chalk { back: ID2D1Bitmap1, layer: ID2D1Bitmap1, pdc: ID2D1DeviceContext, factory: ID2D1Factory, round: ID2D1StrokeStyle,
                   dw: IDWriteFactory5, coll: Option<IDWriteFontCollection>, rng: Rng, u: f32, w: f32, h: f32, hb: f32,
                   used: Vec<(f32, f32, f32, f32)>, on: Vec<String>, job: Option<ChalkJob>, wipe: Option<(f32, f32, f32, f32)>, wait: f32,
                   still: bool, look: Look, scale: f32,
                   content: u8, clean: bool, white: bool, ghosts: bool }   // options: what gets written, wipe at once, white only; ghosts in the board

enum Chalked { Img(ID2D1Bitmap1, f32, f32, f32, f32), Pts(Vec<(f32, f32)>, Color) }
struct ChalkJob { strokes: Vec<Chalked>, i: usize, j: f32, w: f32, done: bool }

fn mix(a: Color, b: Color, t: f32) -> Color { [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0] }

impl Chalk {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look, o: &Json) -> Result<Chalk> {
        let (w, h) = (g.w as f32, g.h as f32);
        let u = w.min(h);
        let light = look.light;
        let tray = (h * 0.045).round();
        let hb = h - tray;
        let mut rng = Rng(0xc4a1_0000_0000_0001 ^ (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1) | 1));
        let ghosts = o.get("ghosts").str() != Some("off");
        let factory = unsafe { g.dc.GetFactory()? };
        let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND,
                                                   dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
        let round = unsafe { factory.CreateStrokeStyle(&props, None)? };
        let back = g.surface(w, h)?;
        let layer = g.surface(w, h)?;
        let pdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        unsafe { pdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); }
        let coll: Option<IDWriteFontCollection> = if fonts.has("Caveat") { fonts.set.cast().ok() } else { None };
        let _ = light;
        let mut c = Chalk { back, layer, pdc, factory, round, dw: fonts.dw.clone(), coll, rng: Rng(0), u, w, h, hb, used: Vec::new(), on: Vec::new(),
                            job: None, wipe: None, wait: 0.0, still: false, look: look.clone(), scale: g.scale,
                            content: 0, clean: false, white: false, ghosts };
        c.opts(o);
        unsafe {   // the board: smudges and ghosts of old writing, then the ledge with chalk and an eraser
            let dc = &c.pdc;
            dc.SetTarget(&c.back);
            dc.BeginDraw();
            dc.Clear(Some(&rgba(look.bg)));
            let ink = dc.CreateSolidColorBrush(&rgba(look.ink), None)?;
            for _ in 0..28 {
                ink.SetOpacity(rng.range(0.008, 0.022));
                let (x, y) = (rng.range(0.0, w), rng.range(0.0, hb));
                let lw = rng.range(0.04, 0.12) * u;
                dc.DrawLine(p2(x, y), p2(x + rng.range(-0.3, 0.3) * w, y + rng.range(-0.06, 0.06) * h), &ink, lw, &c.round);
            }
            if ghosts {
                let f = c.format((u * 0.035).round())?;
                for t in CHALK_GHOSTS {
                    ink.SetOpacity(rng.range(0.02, 0.04));
                    let s: Vec<u16> = t.encode_utf16().collect();
                    let (x, y) = (rng.range(0.0, 0.9) * w, rng.range(0.08, 0.92) * hb);
                    dc.DrawText(&s, &f, &rect(x, y - u * 0.035, w, u * 0.05), &ink, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
            let rt: &ID2D1RenderTarget = dc;
            let ic = |a: f32| D2D1_COLOR_F { r: look.ink[0], g: look.ink[1], b: look.ink[2], a };
            let dust = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: ic(0.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: ic(0.045) }],
                                                       D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let db = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p2(0.0, hb - u * 0.08), endPoint: p2(0.0, hb) }, None, &dust)?;
            dc.FillRectangle(&rect(0.0, hb - u * 0.08, w, u * 0.08), &db);
            dc.FillRectangle(&rect(0.0, hb, w, tray), &dc.CreateSolidColorBrush(&rgba(mix(look.bg, [0.0, 0.0, 0.0, 1.0], 0.45)), None)?);
            for _ in 0..(w / 2.0) as usize {   // dust on the ledge
                ink.SetOpacity(rng.range(0.04, 0.16));
                dc.FillRectangle(&rect(rng.range(0.0, w), hb + rng.range(1.0, tray * 0.55), rng.range(1.0, 2.0), 1.0), &ink);
            }
            ink.SetOpacity(0.12);
            dc.FillRectangle(&rect(0.0, hb, w, 2.0), &ink);
            let (ty, cl, ch) = (hb + tray * 0.45, u * 0.05, (tray * 0.22).max(4.0));
            for (x, col, l) in [(0.62, look.ink, 1.0), (0.66, look.acc, 0.7), (0.69, look.ink, 0.45), (0.73, look.acc2, 0.8)] {
                let b = dc.CreateSolidColorBrush(&rgba(col), None)?;
                b.SetOpacity(0.75);
                dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(w * x, ty - ch / 2.0, cl * l, ch), radiusX: ch / 2.0, radiusY: ch / 2.0 }, &b);
            }
            let felt = dc.CreateSolidColorBrush(&rgba(mix(look.bg, [0x8a as f32 / 255.0, 0x73 as f32 / 255.0, 0x56 as f32 / 255.0, 1.0], 0.7)), None)?;
            felt.SetOpacity(0.7);
            dc.FillRectangle(&rect(w * 0.2, ty - tray * 0.32, u * 0.12, tray * 0.42), &felt);
            let edge = dc.CreateSolidColorBrush(&rgba(mix(look.bg, look.ink, 0.35)), None)?;
            edge.SetOpacity(0.7);
            dc.FillRectangle(&rect(w * 0.2, ty + tray * 0.02, u * 0.12, tray * 0.12), &edge);
            dc.EndDraw(None, None)?;
            dc.SetTarget(&c.layer);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            dc.EndDraw(None, None)?;
        }
        c.rng = rng;
        for _ in 0..5 {   // start with a board in use
            let Some(mut j) = c.next()? else { break };
            c.step(&mut j, 1e9)?;
        }
        Ok(c)
    }

    fn format(&self, fs: f32) -> Result<IDWriteTextFormat> {
        unsafe { self.dw.CreateTextFormat(w!("Caveat"), self.coll.as_ref(), DWRITE_FONT_WEIGHT(500), DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, fs, w!("en-us")) }
    }
    fn width(&self, s: &str, fs: f32) -> Result<f32> {
        let t: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let l = self.dw.CreateTextLayout(&t, &self.format(fs)?, 1e5, fs * 2.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            l.GetMetrics(&mut m)?;
            Ok(m.widthIncludingTrailingWhitespace)
        }
    }
    fn col(&self, c: Color) -> Color { if self.white { self.look.ink } else { c } }

    /// find room on the board for a w x h piece
    fn room(&mut self, w: f32, h: f32) -> Option<(f32, f32)> {
        for _ in 0..30 {
            let (x0, x1, y0, y1) = (self.u * 0.04, self.w - w - self.u * 0.04, self.u * 0.04, self.hb - h - self.u * 0.03);
            if x1 < x0 || y1 < y0 { return None; }
            let (x, y) = (self.rng.range(x0, x1), self.rng.range(y0, y1));
            if !self.used.iter().any(|r| x < r.0 + r.2 + 20.0 && x + w + 20.0 > r.0 && y < r.1 + r.3 + 20.0 && y + h + 20.0 > r.1) { self.used.push((x, y, w, h)); return Some((x, y)); }
        }
        None
    }

    /// a word in chalk: powder round the strokes, dust, little gaps; as a bitmap to reveal left to right
    fn text(&mut self, s: &str, x: f32, y: f32, fs: f32, c: Color) -> Result<Chalked> {
        let c = self.col(c);
        let (tw, th) = ((self.width(s, fs)? + fs).ceil(), (fs * 1.5).ceil());
        let img = unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 { pixelFormat: D2D1_PIXEL_FORMAT { format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                                                  dpiX: 96.0 * self.scale, dpiY: 96.0 * self.scale, bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET, colorContext: std::mem::ManuallyDrop::new(None) };
            self.pdc.CreateBitmap(D2D_SIZE_U { width: (tw * self.scale).ceil() as u32, height: (th * self.scale).ceil() as u32 }, None, 0, &props)?
        };
        let t: Vec<u16> = s.encode_utf16().collect();
        let f = self.format(fs)?;
        unsafe {
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let dc = &self.pdc;
            dc.SetTarget(&img);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let b = dc.CreateSolidColorBrush(&rgba(c), None)?;
            let o = (fs * 0.05).max(1.0);
            b.SetOpacity(0.08);
            for (dx, dy) in [(-o, 0.0), (o, 0.0), (0.0, -o), (0.0, o)] {   // powder round the strokes
                dc.DrawText(&t, &f, &rect(fs * 0.2 + dx, dy, tw, th), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            b.SetOpacity(1.0);
            dc.DrawText(&t, &f, &rect(fs * 0.2, 0.0, tw, th), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            for _ in 0..(tw / 5.0) as usize {   // dust
                b.SetOpacity(self.rng.range(0.1, 0.3));
                dc.FillRectangle(&rect(self.rng.range(0.0, tw), self.rng.range(0.0, th), 1.0, 1.0), &b);
            }
            dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);   // chalk grain: little gaps in the strokes
            let gap = dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
            for _ in 0..(tw * th / 40.0) as usize { dc.FillRectangle(&rect(self.rng.range(0.0, tw).floor(), self.rng.range(0.0, th).floor(), 1.0, 1.0), &gap); }
            dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            dc.EndDraw(None, None)?;
        }
        Ok(Chalked::Img(img, x, y - th / 2.0, tw, th))
    }

    /// the next thing to write: a line of working, a plot, or a little flowchart; as strokes to draw in order
    fn next(&mut self) -> Result<Option<ChalkJob>> {
        let u = self.u;
        let k = match self.content { 1 => 0.3, 2 => 0.7, 3 => 0.9, _ => self.rng.f() };
        let lw = (u / 300.0).max(2.0);
        let (ink, acc, acc2) = (self.look.ink, self.look.acc, self.look.acc2);
        let mut strokes = Vec::new();
        let mut wob = |r: &mut Rng, st: &mut Vec<Chalked>, x0: f32, y0: f32, x1: f32, y1: f32, c: Color| {
            let m = (r.range(-2.0, 2.0), r.range(-2.0, 2.0));
            st.push(Chalked::Pts(vec![(x0, y0), ((x0 + x1) / 2.0 + m.0, (y0 + y1) / 2.0 + m.1), (x1, y1)], c));
        };
        if k < 0.62 {   // a line of working, sometimes underlined or boxed
            let fresh: Vec<&str> = CHALK_LINES.iter().copied().filter(|s| !self.on.iter().any(|o| o == s)).collect();
            if fresh.is_empty() { return Ok(None); }
            let s = fresh[(self.rng.f() * fresh.len() as f32) as usize % fresh.len()];
            let fs = (u * self.rng.range(0.026, 0.036)).round();
            let tw = self.width(s, fs)? + fs;
            let Some((x, y)) = self.room(tw, fs * 1.8) else { return Ok(None) };
            self.on.push(s.to_string());
            let c = if self.rng.f() < 0.7 { ink } else if self.rng.f() < 0.5 { acc } else { acc2 };
            let t = self.text(s, x, y + fs * 0.8, fs, c)?;
            strokes.push(t);
            let r = self.rng.f();
            if r < 0.3 { let e = self.rng.range(0.6, 0.95); wob(&mut self.rng, &mut strokes, x, y + fs * 1.55, x + tw * e, y + fs * 1.6, acc); }
            else if r < 0.45 {
                let e = [x - 6.0, y, x + tw, y + fs * 1.7];
                for (a, b, c2, d) in [(e[0], e[1], e[2], e[1]), (e[2], e[1], e[2], e[3]), (e[2], e[3], e[0], e[3]), (e[0], e[3], e[0], e[1])] { wob(&mut self.rng, &mut strokes, a, b, c2, d, acc2); }
            }
        } else if k < 0.8 {   // axes, ticks, and a curve through them
            let pw = u * self.rng.range(0.2, 0.28);
            let ph = pw * 0.62;
            let Some((x, y)) = self.room(pw, ph) else { return Ok(None) };
            let oy = y + ph * self.rng.range(0.48, 0.55);
            wob(&mut self.rng, &mut strokes, x, oy, x + pw, oy, ink);
            wob(&mut self.rng, &mut strokes, x + pw * 0.1, y + ph, x + pw * 0.1, y, ink);
            for i in 1..8 { let tx = x + pw * 0.1 + i as f32 * pw * 0.11; wob(&mut self.rng, &mut strokes, tx, oy - 4.0, tx, oy + 4.0, ink); }
            let kind = (self.rng.f() * 3.0) as usize % 3;
            let c = if self.rng.f() < 0.5 { acc } else { acc2 };
            let pts = (0..=40).map(|i| {
                let t = i as f32 / 40.0;
                let v = match kind { 0 => (t * TAU * 1.5).sin() * 0.4, 1 => (t - 0.5).powi(2) * 3.0 - 0.3, _ => -(-t * 4.0).exp() * (t * 18.0).cos() * 0.45 };
                (x + pw * 0.1 + t * pw * 0.85, oy - v * ph)
            }).collect();
            strokes.push(Chalked::Pts(pts, c));
            let t = self.text(["sin", "x²", "ringing"][kind], x + pw * 0.78, y + ph * 0.12, (u * 0.024).round(), c)?;
            strokes.push(t);
        } else {   // a little flowchart: three boxes and arrows
            let fresh: Vec<&[&str; 3]> = CHALK_FLOWS.iter().filter(|f| !self.on.iter().any(|o| *o == f.join(","))).collect();
            if fresh.is_empty() { return Ok(None); }
            let words = *fresh[(self.rng.f() * fresh.len() as f32) as usize % fresh.len()];
            let (bw, bh, gap) = (u * 0.1, u * 0.046, u * 0.05);
            let Some((x, y)) = self.room(bw * 3.0 + gap * 2.0, bh * 1.6) else { return Ok(None) };
            self.on.push(words.join(","));
            for i in 0..3 {
                let (bx, by) = (x + i as f32 * (bw + gap), y + bh * 0.3);
                for (a, b, c2, d) in [(bx, by, bx + bw, by), (bx + bw, by, bx + bw, by + bh), (bx + bw, by + bh, bx, by + bh), (bx, by + bh, bx, by)] { wob(&mut self.rng, &mut strokes, a, b, c2, d, ink); }
                let t = self.text(words[i], bx + bw * 0.18, by + bh / 2.0, (bh * 0.55).round(), ink)?;
                strokes.push(t);
                if i < 2 {
                    let (ax, ay) = (bx + bw + 6.0, by + bh / 2.0);
                    wob(&mut self.rng, &mut strokes, ax, ay, ax + gap - 12.0, ay, acc);
                    wob(&mut self.rng, &mut strokes, ax + gap - 12.0, ay, ax + gap - 22.0, ay - 7.0, acc);
                    wob(&mut self.rng, &mut strokes, ax + gap - 12.0, ay, ax + gap - 22.0, ay + 7.0, acc);
                }
            }
        }
        Ok(Some(ChalkJob { strokes, i: 0, j: 0.0, w: lw, done: false }))
    }

    /// draw up to `px` pixels' worth of the job into the chalk layer
    fn step(&mut self, job: &mut ChalkJob, mut px: f32) -> Result<()> {
        unsafe {
            let dc = self.pdc.clone();
            dc.SetTarget(&self.layer);
            dc.BeginDraw();
            while px > 0.01 && job.i < job.strokes.len() {
                match &job.strokes[job.i] {
                    Chalked::Img(img, x, y, tw, th) => {   // writing: the word appears left to right
                        let (from, to) = (job.j, tw.min(job.j + px));
                        if to - from < 0.01 && to < *tw { break; }
                        let s = self.scale;
                        dc.DrawBitmap(img, Some(&rect(x + from, *y, to - from, *th)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR,
                                      Some(&rect(from * s, 0.0, (to - from) * s, th * s)), None);
                        px -= to - from; job.j = to;
                        if to >= *tw { job.i += 1; job.j = 0.0; }
                    }
                    Chalked::Pts(p, c) => {
                        let jj = job.j as usize;
                        if jj + 1 >= p.len() { job.i += 1; job.j = 0.0; continue; }
                        let (a, b) = (p[jj], p[jj + 1]);
                        let c = self.col(*c);
                        self.seg(&dc, a, b, c, job.w)?;
                        px -= ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                        job.j += 1.0;
                    }
                }
            }
            dc.EndDraw(None, None)?;
        }
        if job.i >= job.strokes.len() { job.done = true; }
        Ok(())
    }

    /// chalk on the board: a slightly broken line, a little dust
    fn seg(&mut self, dc: &ID2D1DeviceContext, (x0, y0): (f32, f32), (x1, y1): (f32, f32), c: Color, w: f32) -> Result<()> {
        unsafe {
            let b = dc.CreateSolidColorBrush(&rgba(c), None)?;
            b.SetOpacity(0.05);
            dc.DrawLine(p2(x0, y0), p2(x1, y1), &b, w * 3.5, &self.round);   // powder
            let n = (((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() / 4.0).max(1.0) as usize;
            for i in 0..n {
                let (a, e) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
                b.SetOpacity(self.rng.range(0.5, 0.85));
                dc.DrawLine(p2(x0 + (x1 - x0) * a, y0 + (y1 - y0) * a),
                            p2(x0 + (x1 - x0) * e + self.rng.range(-0.4, 0.4), y0 + (y1 - y0) * e + self.rng.range(-0.4, 0.4)), &b, w * self.rng.range(0.75, 1.15), &self.round);
                if self.rng.f() < 0.3 {
                    b.SetOpacity(0.25);
                    dc.FillRectangle(&rect(x0 + (x1 - x0) * a + self.rng.range(-w * 2.0, w * 2.0), y0 + (y1 - y0) * a + self.rng.range(-w * 2.0, w * 2.0), 1.0, 1.0), &b);
                }
            }
        }
        Ok(())
    }
}

impl Background for Chalk {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 20.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.content = match o.get("content").str() { Some("equations") => 1, Some("plots") => 2, Some("flowcharts") => 3, _ => 0 };
        self.clean = o.get("wipe").str() == Some("clean");
        self.white = o.get("colors").str() == Some("white");
        (o.get("ghosts").str() != Some("off")) != self.ghosts
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if !self.still {
            let sp = 0.4 + level * 1.6;
            let u = self.u;
            if let Some((mut x, mut y, h, mut dir)) = self.wipe {   // erasing: a band at a time, back and forth, leaving smudge
                let x0 = x;
                x += dir * u * 1.6 * dt * sp;
                unsafe {
                    let dc = &self.pdc;
                    dc.SetTarget(&self.layer);
                    dc.BeginDraw();
                    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                    dc.FillRectangle(&rect(x0.min(x) - u * 0.06, y, (x - x0).abs() + u * 0.12, h), &dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?);
                    dc.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
                    let rt: &ID2D1RenderTarget = dc;
                    let ic = |a: f32| D2D1_COLOR_F { r: look.ink[0], g: look.ink[1], b: look.ink[2], a };
                    let sm = rt.CreateGradientStopCollection(&[D2D1_GRADIENT_STOP { position: 0.0, color: ic(0.0) }, D2D1_GRADIENT_STOP { position: 0.5, color: ic(0.03) },
                                                                D2D1_GRADIENT_STOP { position: 1.0, color: ic(0.0) }], D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                    let sb = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: p2(0.0, y), endPoint: p2(0.0, y + h) }, None, &sm)?;
                    dc.FillRectangle(&rect(x0.min(x), y, (x - x0).abs(), h), &sb);
                    dc.EndDraw(None, None)?;
                }
                if x > self.w + u * 0.1 || x < -u * 0.1 {
                    y += h; dir = -dir;
                    if y >= self.hb { self.wipe = None; self.used.clear(); self.on.clear(); self.wait = 1.5; } else { self.wipe = Some((x, y, h, dir)); }
                } else { self.wipe = Some((x, y, h, dir)); }
            } else if let Some(mut j) = self.job.take().filter(|j| !j.done) {
                self.step(&mut j, u * 1.1 * dt * sp)?;
                self.job = Some(j);
            } else {
                self.wait -= dt * sp;
                if self.wait <= 0.0 {
                    self.job = self.next()?;
                    self.wait = self.rng.range(1.5, 4.0);
                    if self.job.is_none() {   // full: wipe the board
                        if self.clean {
                            unsafe { self.pdc.SetTarget(&self.layer); self.pdc.BeginDraw(); self.pdc.Clear(Some(&D2D1_COLOR_F::default())); self.pdc.EndDraw(None, None)?; }
                            self.used.clear(); self.on.clear(); self.wait = 1.5;
                        } else { self.wipe = Some((-u * 0.06, 0.0, u * 0.11, 1.0)); }
                    }
                }
            }
        }
        unsafe {
            let dc = &g.dc;
            let full = rect(0.0, 0.0, self.w, self.h);
            dc.DrawBitmap(&self.back, Some(&full), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
            dc.DrawBitmap(&self.layer, Some(&full), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
            if let Some((x, y, h, _)) = self.wipe {   // the eraser, a dark block with a felt edge, moving along its band
                let (ew, eh) = (self.u * 0.12, h * 0.9);
                let felt = g.brush(mix(look.bg, [0x8a as f32 / 255.0, 0x73 as f32 / 255.0, 0x56 as f32 / 255.0, 1.0], 0.7))?;
                felt.SetOpacity(0.9);
                dc.FillRectangle(&rect(x - ew / 2.0, y + (h - eh) / 2.0, ew, eh), &felt);
                let edge = g.brush(mix(look.bg, look.ink, 0.35))?;
                edge.SetOpacity(0.9);
                dc.FillRectangle(&rect(x - ew / 2.0, y + (h - eh) / 2.0 + eh * 0.8, ew, eh * 0.2), &edge);
            }
        }
        Ok(())
    }
}

const CHALK_LINES: [&str; 58] = ["f(x) = x² - 3x + 2", "a² + b² = c²", "O(n log n)", "lim sin(x)/x = 1", "P(A|B) = P(B|A) P(A) / P(B)", "if (tests pass) ship()", "TODO: write the tests!", "y = mx + b", "E = mc²", "2 + 2 = 4", "n! = n (n-1)!", "x = (-b ± √(b² - 4ac)) / 2a", "cache hit / (hit + miss)", "merge, then rebase?", "p(fail) < 0.01", "O(1) amortized", "T(n) = 2T(n/2) + n", "Σ i = n(n+1)/2", "e^(iπ) + 1 = 0", "d/dx sin x = cos x", "∫ x dx = x²/2 + C", "log₂ 1024 = 10", "2¹⁰ = 1024", "0.1 + 0.2 ≠ 0.3", "NaN ≠ NaN", "retry with backoff: 1, 2, 4, 8 s", "latency p99 < 200 ms", "rows / page = 50", "ttl = 3600", "off by one?", "it works on my machine", "never trust user input", "measure, then optimize", "git bisect: good ... bad", "DRY, but not too dry", "a → b → c ⇒ a → c", "¬(A ∧ B) = ¬A ∨ ¬B", "binary search: lo, mid, hi", "hash(k) mod m", "load factor < 0.75", "BFS: queue / DFS: stack", "read the error message", "1 KB = 1024 B", "UTC, always", "v2.3.1 → v2.4.0", "exit code 0 = ok", "200 OK  ·  404 not found", "idempotent: f(f(x)) = f(x)", "n² vs n log n", "cos² + sin² = 1", "mean = Σx / n", "σ² = E[(x - μ)²]", "commit early, commit often", "null check first", "race condition? add a lock", "deadlock: A waits B, B waits A", "tabs or spaces?", "p ⇒ q, ¬q ∴ ¬p"];
const CHALK_GHOSTS: [&str; 12] = ["x + y = ", "∴", "Q.E.D.", "f′(x)", "lim →", "≈ 3.14159", "Δt", "(a + b)²", "λx. x", "mod 7", "k = 1 .. n", "∂/∂x"];
const CHALK_FLOWS: [[&str; 3]; 8] = [["plan", "code", "test"], ["read", "edit", "ship"], ["fetch", "parse", "store"], ["build", "lint", "deploy"], ["request", "cache", "db"], ["input", "validate", "save"], ["red", "green", "refactor"], ["clone", "branch", "merge"]];

/// flying through a starfield: stars coming at you (or falling away); the last frames fade under a translucent fill,
/// so each star leaves a short streak. Options: density, speed, streaks, direction. 30 fps; still at 0.
pub struct Stars { s: Vec<(f32, f32, f32)>, rng: Rng, fresh: bool, still: bool, k: f32, sp: f32, streaks: bool, out: bool }

impl Stars {
    fn new() -> Stars {
        let mut rng = Rng(0x51a7_f1e1_d000_0001);
        let s = (0..1100).map(|_| { let z = rng.range(0.05, 1.0); (rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), z) }).collect();
        Stars { s, rng, fresh: true, still: false, k: 0.64, sp: 1.0, streaks: true, out: false }
    }
}

impl Background for Stars {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.k = match o.get("density").str() { Some("sparse") => 0.3, Some("dense") => 1.0, _ => 0.64 };
        self.sp = match o.get("speed").str() { Some("drift") => 0.4, Some("warp") => 2.5, _ => 1.0 };
        self.streaks = o.get("streaks").str() != Some("off");
        self.out = o.get("direction").str() == Some("out");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        let (w, h) = (g.w as f32, g.h as f32);
        let bg = g.brush(look.bg)?;
        unsafe {
            if self.fresh || self.still || !self.streaks { g.dc.Clear(Some(&rgba(look.bg))); self.fresh = false; }
            else { bg.SetOpacity(0.45); g.dc.FillRectangle(&rect(0.0, 0.0, w, h), &bg); }
        }
        if self.still { self.fresh = true; return Ok(()); }
        let (hi, dim) = (g.brush(look.hi)?, g.brush(look.dim)?);
        let n = (self.s.len() as f32 * self.k * level).round() as usize;
        let f = w.max(h) / 2.0;
        let dz = if self.out { 1.0 } else { -1.0 } * dt * (0.06 + level * 0.14) * self.sp;
        for i in 0..n {
            let (px, py, mut z) = self.s[i];
            z += dz;
            let x = w / 2.0 + px / z * f * 0.5;
            let y = h / 2.0 + py / z * f * 0.5;
            if z <= 0.02 || z > 1.0 || x < 0.0 || x > w || y < 0.0 || y > h {
                let nz = if self.out { self.rng.range(0.03, 0.2) } else { 1.0 };
                self.s[i] = (self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0), nz);
                continue;
            }
            self.s[i].2 = z;
            let sz = (1.0 - z) * 2.6 + 0.4;
            unsafe { g.dc.FillRectangle(&rect(x, y, sz, sz), if z < 0.3 { &hi } else { &dim }); }
        }
        Ok(())
    }
}

/// drifting snow: flakes falling at their own pace, swaying or blown; the big ones bright; it can settle into a
/// drift along the bottom that slowly melts. Options: amount, wind, flakes, settle. 30 fps; still at 0.
pub struct Snow { f: Vec<(f32, f32, f32, f32, f32)>, rng: Rng, t: f32, still: bool, h: f32,
                  k: f32, wind: f32, size: (f32, f32), big: f32, settle: bool, pile: Vec<f32> }

impl Snow {
    fn new(g: &Gfx) -> Snow {
        let mut sn = Snow { f: Vec::new(), rng: Rng(0x5a0f_1a4e_0bad_cafe), t: 0.0, still: false, h: g.h as f32,
                            k: 0.5, wind: 0.0, size: (1.0, 3.4), big: 2.6, settle: false, pile: vec![0.0; (g.w as usize).div_ceil(4)] };
        for _ in 0..1000 { let y = sn.rng.range(0.0, sn.h); let fl = sn.flake(g.w as f32, y); sn.f.push(fl); }
        sn
    }
    /// x, y, size, fall speed, sway phase
    fn flake(&mut self, w: f32, y: f32) -> (f32, f32, f32, f32, f32) {
        let r = self.rng.range(self.size.0, self.size.1);
        (self.rng.range(0.0, w), y, r, 12.0 + r * 14.0, self.rng.range(0.0, 7.0))
    }
}

impl Background for Snow {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 30.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.k = match o.get("amount").str() { Some("light") => 0.22, Some("blizzard") => 1.0, _ => 0.5 };
        self.wind = match o.get("wind").str() { Some("left") => -38.0, Some("right") => 38.0, _ => 0.0 };
        (self.size, self.big) = match o.get("flakes").str() { Some("fine") => ((0.8, 1.8), 1.6), Some("big") => ((2.2, 4.5), 3.6), _ => ((1.0, 3.4), 2.6) };
        self.settle = o.get("settle").str() == Some("on");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        self.t += dt;
        unsafe { g.dc.Clear(Some(&rgba(look.bg))); }
        if self.still { return Ok(()); }
        let (w, h) = (g.w as f32, g.h as f32);
        let (hi, dim) = (g.brush(look.hi)?, g.brush(look.dim)?);
        let n = (self.f.len() as f32 * self.k * level).round() as usize;
        let sway = if self.wind != 0.0 { 5.0 } else { 12.0 };
        let (cols, cap) = (self.pile.len(), h * 0.06);
        for i in 0..n {
            let (mut x, mut y, r, v, ph) = self.f[i];
            y += v * dt;
            x += ((self.t * 0.7 + ph).sin() * sway + self.wind) * dt;
            if x < -6.0 { x += w + 12.0 } else if x > w + 6.0 { x -= w + 12.0 }
            let c = ((x / 4.0).max(0.0) as usize).min(cols - 1);
            if self.settle && y > h - self.pile[c] {
                self.pile[c] = cap.min(self.pile[c] + r * 0.35);
                self.f[i] = self.flake(w, -4.0);
                continue;
            }
            self.f[i] = if y > h + 4.0 { self.flake(w, -4.0) } else { (x, y, r, v, ph) };
            unsafe { g.dc.FillRectangle(&rect(x, y, r, r), if r > self.big { &hi } else { &dim }); }
        }
        if self.settle {   // the drift along the bottom, slowly melting
            for p in self.pile.iter_mut() { *p = (*p - dt * 0.6).max(0.0); }
            for c in 0..cols {
                let hh = (self.pile[c.saturating_sub(1)] + self.pile[c] * 2.0 + self.pile[(c + 1).min(cols - 1)]) / 4.0;
                if hh > 0.5 { unsafe { g.dc.FillRectangle(&rect(c as f32 * 4.0, h - hh, 4.0, hh), &dim); } }
            }
        } else { self.pile.fill(0.0); }
        Ok(())
    }
}

/// dead-channel noise at a quarter of the resolution, upscaled with hard edges, with a brighter band rolling slowly
/// down. Drawn into memory and handed to the GPU. 15 fps; still at 0.
pub struct Static { lw: usize, lh: usize, buf: Vec<u32>, small: ID2D1Bitmap1, rng: Rng, band: f32, still: bool,
                    blk: usize, roll: bool, tint: bool, slow: bool }   // options: grain, band, tint, flicker

impl Static {
    fn new(g: &Gfx) -> Result<Static> {
        let (lw, lh) = ((g.w as usize).div_ceil(4), (g.h as usize).div_ceil(4));
        Ok(Static { lw, lh, buf: vec![0xff00_0000; lw * lh], small: g.bitmap(lw as u32, lh as u32, None, false)?,
                    rng: Rng(0x0d15_ea5e_57a7_1c00), band: 0.0, still: false, blk: 1, roll: true, tint: false, slow: false })
    }
}

impl Background for Static {
    fn fps(&self) -> f32 { if self.still { 0.0 } else if self.slow { 6.0 } else { 15.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.blk = if o.get("grain").str() == Some("coarse") { 2 } else { 1 };
        self.roll = o.get("band").str() != Some("off");
        self.tint = o.get("tint").str() == Some("tinted");
        self.slow = o.get("flicker").str() == Some("slow");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if self.still { unsafe { g.dc.Clear(Some(&rgba(look.bg))); } return Ok(()); }
        self.band = (self.band + dt * self.lh as f32 * 0.12) % (self.lh as f32 + 40.0);
        let (b, d, a) = (px(look.bg), px(look.dim), px(look.acc));
        let ch = |c: u32, s: u32| ((c >> s) & 255) as f32;
        let p = 0.02 + level * 0.16;
        let blk = self.blk;
        for y in (0..self.lh).step_by(blk) {
            let boost = if self.roll && (y as f32 - self.band).abs() < 14.0 { 1.8 } else { 1.0 };
            for x in (0..self.lw).step_by(blk) {
                let m = if self.rng.f() < p * boost { (self.rng.f() * boost).min(1.0) } else { 0.0 };
                let c = if self.tint && m > 0.0 && self.rng.f() < 0.25 { a } else { d };
                let mix = |s: u32| (ch(b, s) + (ch(c, s) - ch(b, s)) * m) as u32;
                let v = 0xff00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0);
                for yy in y..(y + blk).min(self.lh) { for xx in x..(x + blk).min(self.lw) { self.buf[yy * self.lw + xx] = v; } }
            }
        }
        unsafe {
            self.small.CopyFromMemory(None, self.buf.as_ptr() as *const _, (self.lw * 4) as u32)?;
            g.dc.DrawBitmap(&self.small, Some(&rect(0.0, 0.0, (self.lw * 4) as f32, (self.lh * 4) as f32)), 1.0,
                            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
        }
        Ok(())
    }
}

/// the page's "code" rain: cascade kept to one depth. Its options go to cascade's own (density, glyphs, the heads:
/// bright is cascade's theme-colored heads, accent its accent ones), plus a speed of its own.
pub struct Code(Cascade, f32);
impl Background for Code {
    fn fps(&self) -> f32 { self.0.fps() }
    fn opts(&mut self, o: &Json) -> bool {
        self.1 = match o.get("speed").str() { Some("slow") => 0.5, Some("fast") => 1.7, _ => 1.0 };
        let s = |k: &str| Json::Str(o.get(k).str().unwrap_or("").to_string());
        let heads = if o.get("heads").str() == Some("accent") { "accent" } else { "green" };
        let c = Json::Obj(vec![("layers".into(), Json::Str("one".into())), ("density".into(), s("density")),
                               ("glyphs".into(), s("glyphs")), ("heads".into(), Json::Str(heads.into()))]);
        self.0.opts(&c)
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> { self.0.draw(g, look, level, dt * self.1) }
}

type M3 = windows::Foundation::Numerics::Matrix3x2;
fn ell(x: f32, y: f32, rx: f32, ry: f32) -> D2D1_ELLIPSE { D2D1_ELLIPSE { point: p2(x, y), radiusX: rx, radiusY: ry } }

/// a 50s jukebox arch in the corner: three glowing bands drifting between the two accents, a grille, bubbles rising
/// in the side tubes. The arch shapes are made once; each band is drawn wide and faint (its glow) and then narrow.
/// 20 fps; still at 0.
pub struct Jukebox { arches: Vec<ID2D1PathGeometry>, x: f32, y: f32, w: f32, h: f32, t: f32, bub: Vec<(bool, f32, f32, f32)>,
                     rng: Rng, still: bool, centre: bool, shift: bool, bubbles: bool, grille: bool }   // options: place, bands, bubbles, grille

impl Jukebox {
    fn new(g: &Gfx, o: &Json) -> Result<Jukebox> {
        let (sw, sh) = (g.w as f32, g.h as f32);
        let w = sw.min(sh) * 0.46;
        let h = w * 1.55;
        let centre = o.get("place").str() == Some("centre");
        let (x, y) = (if centre { (sw - w) / 2.0 } else { sw * 0.05 }, sh - h * 0.9);
        let band = w * 0.07;
        let factory = unsafe { g.dc.GetFactory()? };
        let mut arches = Vec::new();
        for i in 0..3 {
            let inset = band * (i as f32 + 0.5);
            let (ax, aw, ay, bottom) = (x + inset, w - inset * 2.0, y + inset, y + h + 40.0);
            unsafe {
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                sink.BeginFigure(p2(ax, bottom), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(p2(ax, ay + aw / 2.0));
                sink.AddArc(&D2D1_ARC_SEGMENT { point: p2(ax + aw, ay + aw / 2.0), size: D2D_SIZE_F { width: aw / 2.0, height: aw / 2.0 },
                                                rotationAngle: 0.0, sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_SMALL });
                sink.AddLine(p2(ax + aw, bottom));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                arches.push(path);
            }
        }
        let mut rng = Rng(0x00ce_11a5_b0b5_1e55);
        let bub = (0..28).map(|i| (i % 2 == 1, rng.f(), rng.range(1.5, 4.5), rng.range(0.05, 0.14))).collect();
        let mut j = Jukebox { arches, x, y, w, h, t: 0.0, bub, rng, still: false, centre, shift: true, bubbles: true, grille: true };
        j.opts(o);
        Ok(j)
    }
}

impl Background for Jukebox {
    fn fps(&self) -> f32 { if self.still { 0.0 } else { 20.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.shift = o.get("bands").str() != Some("still");
        self.bubbles = o.get("bubbles").str() != Some("off");
        self.grille = o.get("grille").str() != Some("off");
        (o.get("place").str() == Some("centre")) != self.centre   // the arches are made in place: remake when it moves
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        if self.shift { self.t += dt * (0.15 + level * 0.9); }
        let sh = g.h as f32;
        let band = self.w * 0.07;
        let (a, b) = (look.acc, look.acc2);
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            for (i, p) in self.arches.iter().enumerate() {
                let k = if self.shift { ((self.t + i as f32 * 1.3).sin() + 1.0) / 2.0 } else { i as f32 / 2.0 };
                let c = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k, 1.0];
                let br = g.brush(c)?;
                br.SetOpacity(0.12);
                dc.DrawGeometry(p, &br, band * 1.5, None);
                br.SetOpacity(0.5);
                dc.DrawGeometry(p, &br, band * 0.75, None);
            }
            let ink = g.brush(look.ink)?;
            ink.SetOpacity(0.18);   // the grille
            let (gx, gw) = (self.x + band * 3.4, self.w - band * 6.8);
            let mut yy = self.y + self.w * 0.55;
            while self.grille && yy < sh { dc.DrawLine(p2(gx, yy), p2(gx + gw, yy), &ink, 1.0, None); yy += 9.0; }
            ink.SetOpacity(0.45);   // the bubble tubes
            let (top, bot) = (self.y + self.w * 0.45, sh.min(self.y + self.h));
            for i in 0..if self.bubbles { self.bub.len() } else { 0 } {
                let (side, mut f, r, v) = self.bub[i];
                if !self.still { f += v * dt; }
                if f > 1.0 { self.bub[i] = (side, 0.0, self.rng.range(1.5, 4.5), self.rng.range(0.05, 0.14)); f = 0.0; } else { self.bub[i].1 = f; }
                let x = if side { self.x + self.w - band * 1.1 } else { self.x + band * 1.1 };
                dc.DrawEllipse(&ell(x, bot - (bot - top) * f, r, r), &ink, 1.0, None);
            }
        }
        Ok(())
    }
}

/// atomic-age starbursts, boomerangs and atoms scattered over the screen, each slowly turning. The boomerang
/// shape is made once; everything is drawn turned in place. 20 fps; still at 0.
pub struct Starburst { items: Vec<Item>, boom: ID2D1PathGeometry, still: bool,
                       kind: Option<u8>, cnt: usize, spin: f32, one: bool }   // options: shapes, count, spin, colors
struct Item { x: f32, y: f32, s: f32, a: f32, spin: f32, kind: u8, n: usize, c: usize }

impl Starburst {
    fn new(g: &Gfx) -> Result<Starburst> {
        let mut rng = Rng(0xa70e_1c_a9e5_0050);
        let (w, h) = (g.w as f32, g.h as f32);
        let items = (0..64).map(|_| Item { x: rng.range(0.0, w), y: rng.range(0.0, h), s: rng.range(14.0, 46.0), a: rng.range(0.0, TAU),
            spin: rng.range(-0.25, 0.25), kind: [0u8, 0, 1, 2][(rng.f() * 4.0) as usize % 4], n: 8 + ((rng.f() * 6.0) as usize % 6) * 2,
            c: (rng.f() * 4.0) as usize % 4 }).collect();
        // a unit boomerang (L = 1), scaled to each item when drawn
        let factory = unsafe { g.dc.GetFactory()? };
        let boom = unsafe {
            let path = factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(p2(-1.0, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: p2(0.0, -0.95), point2: p2(1.0, 0.0) });
            sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: p2(0.0, -0.4), point2: p2(-1.0, 0.0) });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            path
        };
        Ok(Starburst { items, boom, still: false, kind: None, cnt: 34, spin: 1.0, one: false })
    }
}

impl Background for Starburst {
    fn fps(&self) -> f32 { if self.still || self.spin == 0.0 { 0.0 } else { 20.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.kind = match o.get("shapes").str() { Some("bursts") => Some(0), Some("boomerangs") => Some(1), Some("atoms") => Some(2), _ => None };
        self.cnt = match o.get("count").str() { Some("few") => 16, Some("many") => 64, _ => 34 };
        self.spin = match o.get("spin").str() { Some("still") => 0.0, Some("lively") => 3.0, _ => 1.0 };
        self.one = o.get("colors").str() == Some("one");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        let n = (self.cnt as f32 * (level * 1.6).min(1.0)).round() as usize;
        let br = [g.brush(look.acc)?, g.brush(look.acc2)?, g.brush(look.ink)?, g.brush(look.muted)?];
        for b in &br { unsafe { b.SetOpacity(0.45); } }
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            for it in self.items.iter_mut().take(n) {
                it.a += it.spin * self.spin * dt;
                let b = &br[if self.one { 0 } else { it.c }];
                match self.kind.unwrap_or(it.kind) {
                    0 => {
                        dc.SetTransform(&turn(it.a, it.x, it.y));
                        for k in 0..it.n {
                            let ang = k as f32 / it.n as f32 * TAU;
                            let l = it.s * if k % 2 == 1 { 0.55 } else { 1.0 };
                            dc.DrawLine(p2(0.0, 0.0), p2(ang.cos() * l, ang.sin() * l), b, 1.4, None);
                            if k % 2 == 0 { dc.FillEllipse(&ell(ang.cos() * it.s, ang.sin() * it.s, 2.4, 2.4), b); }
                        }
                    }
                    1 => {
                        let l = it.s * 1.3;
                        let (s, c) = it.a.sin_cos();
                        dc.SetTransform(&M3 { M11: c * l, M12: s * l, M21: -s * l, M22: c * l, M31: it.x, M32: it.y });
                        dc.FillGeometry(&self.boom, b, None);
                    }
                    _ => {
                        for k in 0..3 {
                            dc.SetTransform(&turn(it.a + k as f32 * std::f32::consts::PI / 3.0, it.x, it.y));
                            dc.DrawEllipse(&ell(0.0, 0.0, it.s, it.s * 0.34), b, 1.4, None);
                        }
                        dc.FillEllipse(&ell(0.0, 0.0, 3.2, 3.2), b);
                    }
                }
            }
            dc.SetTransform(&M3::identity());
        }
        Ok(())
    }
}

/// an old broadcast test card, faded: checker border, bars in a circle, cross, resolution wedges and a name plate,
/// with a slow roll bar and a little snow over it. Drawn whole each frame (a few hundred shapes). 15 fps; still at 0.
pub struct Testcard { roll: f32, rng: Rng, still: bool, title: IDWriteTextFormat, sub: IDWriteTextFormat,
                      rolls: bool, snow: f32, plate: bool, checker: bool }   // options: roll, snow, plate, border

impl Testcard {
    fn new(g: &Gfx, fonts: &Fonts, look: &Look) -> Result<Testcard> {
        let r = (g.w as f32).min(g.h as f32) * 0.38;
        let title = fonts.format("Instrument Serif", (r * 0.12).round(), 400)?;
        let sub = fonts.format(&look.mono.family, (r * 0.04).round().max(8.0), 400)?;
        for f in [&title, &sub] { unsafe { let _ = f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER); let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER); } }
        Ok(Testcard { roll: 0.0, rng: Rng(0x7e57_ca4d_0047_8330), still: false, title, sub, rolls: true, snow: 1.0, plate: true, checker: true })
    }
}

impl Background for Testcard {
    fn fps(&self) -> f32 { if self.still || (!self.rolls && self.snow == 0.0) { 0.0 } else { 15.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.rolls = o.get("roll").str() != Some("off");
        self.snow = match o.get("snow").str() { Some("off") => 0.0, Some("heavy") => 2.5, _ => 1.0 };
        self.plate = o.get("plate").str() != Some("off");
        self.checker = o.get("border").str() != Some("plain");
        false
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        let (w, h) = (g.w as f32, g.h as f32);
        if !self.still { self.roll = (self.roll + dt * h * 0.04) % (h + 80.0); }
        let (cxm, cym, r) = (w / 2.0, h / 2.0, w.min(h) * 0.38);
        unsafe {
            let dc = &g.dc;
            dc.Clear(Some(&rgba(look.bg)));
            let ink = g.brush(look.ink)?;
            ink.SetOpacity(0.32);   // the checker border
            let sq = (w.min(h) / 18.0).round();
            if !self.checker { dc.DrawRectangle(&rect(sq / 2.0, sq / 2.0, w - sq, h - sq), &ink, 2.0, None); }
            let (mut x, mut k) = (0.0, 0);
            while self.checker && x < w { if k % 2 == 1 { dc.FillRectangle(&rect(x, 0.0, sq, sq), &ink); dc.FillRectangle(&rect(x, h - sq, sq, sq), &ink); } x += sq; k += 1; }
            let (mut y, mut k) = (0.0, 0);
            while self.checker && y < h { if k % 2 == 1 { dc.FillRectangle(&rect(0.0, y, sq, sq), &ink); dc.FillRectangle(&rect(w - sq, y, sq, sq), &ink); } y += sq; k += 1; }
            // the bars, clipped to the circle (the circle's chord at the bars' height is wide enough: clip each bar by hand)
            let shades = [look.ink, look.muted, look.faint, look.acc, look.acc2, look.faint];
            let (by, bh) = (cym - r * 0.32, r * 0.64);
            for (i, c) in shades.iter().enumerate() {
                let b = g.brush(*c)?;
                b.SetOpacity(0.28);
                let x0 = cxm - r + i as f32 * r / 3.0;
                // inside the circle at the bars' top and bottom edge
                let half = (r * r - (r * 0.32) * (r * 0.32)).sqrt();
                let (l, rr) = (x0.max(cxm - half), (x0 + r / 3.0).min(cxm + half));
                if rr > l { dc.FillRectangle(&rect(l, by, rr - l, bh), &b); }
            }
            ink.SetOpacity(0.4);
            for rad in [r, r * 0.72] { dc.DrawEllipse(&ell(cxm, cym, rad, rad), &ink, 2.0, None); }
            dc.DrawLine(p2(cxm - r * 1.25, cym), p2(cxm + r * 1.25, cym), &ink, 2.0, None);
            dc.DrawLine(p2(cxm, cym - r * 1.15), p2(cxm, cym + r * 1.15), &ink, 2.0, None);
            for s in [-1.0f32, 1.0] {   // the resolution wedges
                for k in -6..=6 {
                    let k = k as f32;
                    dc.DrawLine(p2(cxm + s * r * 0.78, cym + k * 3.0), p2(cxm + s * r * 1.12, cym + k * 9.0), &ink, 1.0, None);
                    dc.DrawLine(p2(cxm + k * 3.0, cym + s * r * 0.78), p2(cxm + k * 9.0, cym + s * r * 1.05), &ink, 1.0, None);
                }
            }
            let bg = g.brush(look.bg)?;   // the name plate
            let plate = rect(cxm - r * 0.42, cym - r * 0.13, r * 0.84, r * 0.26);
            if self.plate {
            bg.SetOpacity(0.75);
            dc.FillRectangle(&plate, &bg);
            ink.SetOpacity(0.55);
            dc.DrawRectangle(&plate, &ink, 2.0, None);
            let t1: Vec<u16> = "glitchwall".encode_utf16().collect();
            let t2: Vec<u16> = "CHANNEL 47833  ·  TEST TRANSMISSION".encode_utf16().collect();
            dc.DrawText(&t1, &self.title, &rect(cxm - r, cym - r * 0.02 - r * 0.1, r * 2.0, r * 0.2), &ink, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            dc.DrawText(&t2, &self.sub, &rect(cxm - r, cym + r * 0.085 - r * 0.04, r * 2.0, r * 0.08), &ink, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }
            if self.rolls { bg.SetOpacity(0.18); dc.FillRectangle(&rect(0.0, self.roll - 40.0, w, 40.0), &bg); }   // the slow roll bar
            ink.SetOpacity(0.35);   // snow
            for _ in 0..(level * 900.0 * self.snow) as usize {
                dc.FillRectangle(&rect(self.rng.f() * w, self.rng.f() * h, 1.5, 1.5), &ink);
            }
        }
        Ok(())
    }
}

/// mid-century wallpaper: diamonds, dots and four-point stars on a square tile, drifting slowly. The tile is drawn
/// once per look into a small bitmap and laid over the screen with a wrapping bitmap brush. Options: pattern, drift,
/// scale (the tile's size: a change makes it again), ink. 15 fps; still at 0 or with the drift stopped.
pub struct Print { tile: ID2D1Bitmap1, pdc: ID2D1DeviceContext, factory: ID2D1Factory, off: f32, key: [f32; 5], still: bool, scale: f32,
                   ts: f32, pat: u8, drift: bool, acc: bool }

fn print_tile(o: &Json) -> f32 { match o.get("scale").str() { Some("small") => 64.0, Some("large") => 140.0, _ => 96.0 } }

impl Print {
    fn new(g: &Gfx, o: &Json) -> Result<Print> {
        let ts = print_tile(o);
        let px = (ts * g.scale).round() as u32;
        let tile = g.bitmap(px, px, None, true)?;
        let pdc = unsafe { g.dc.GetDevice()?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)? };
        let factory = unsafe { g.dc.GetFactory()? };
        unsafe { pdc.SetDpi(96.0 * g.scale, 96.0 * g.scale); pdc.SetTarget(&tile); }
        let mut p = Print { tile, pdc, factory, off: 0.0, key: [-1.0; 5], still: false, scale: g.scale, ts, pat: 0, drift: true, acc: false };
        p.opts(o);
        Ok(p)
    }

    fn paint(&mut self, look: &Look) -> Result<()> {
        let (t, c) = (self.ts, self.ts / 2.0);
        let shape = |pts: &[(f32, f32)]| -> Result<ID2D1PathGeometry> { unsafe {
            let path = self.factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            sink.BeginFigure(p2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_FILLED);
            for p in &pts[1..] { sink.AddLine(p2(p.0, p.1)); }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            Ok(path)
        } };
        let ink = if self.acc { look.acc } else { look.dim };
        let k = t / 96.0;   // the motifs grow with the tile
        unsafe {
            let dc = &self.pdc;
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let b = dc.CreateSolidColorBrush(&rgba(ink), None)?;
            b.SetOpacity(0.5);
            if self.pat == 0 {   // diamonds
                let diamond = shape(&[(c, c - 18.0 * k), (c + 12.0 * k, c), (c, c + 18.0 * k), (c - 12.0 * k, c)])?;
                dc.DrawGeometry(&diamond, &b, 1.2, None);
                dc.FillEllipse(&ell(c, c, 2.2 * k, 2.2 * k), &b);
            }
            if self.pat != 1 {   // four-point stars on the corners
                let r = if self.pat == 2 { 13.0 } else { 9.0 } * k;
                for (x, y) in [(0.0, 0.0), (t, 0.0), (0.0, t), (t, t)] {
                    let star = shape(&[(x, y - r), (x + 2.0 * k, y - 2.0 * k), (x + r, y), (x + 2.0 * k, y + 2.0 * k), (x, y + r),
                                       (x - 2.0 * k, y + 2.0 * k), (x - r, y), (x - 2.0 * k, y - 2.0 * k)])?;
                    dc.FillGeometry(&star, &b, None);
                }
                if self.pat == 2 { dc.FillEllipse(&ell(c, c, 2.0 * k, 2.0 * k), &b); }
            }
            if self.pat == 1 {   // dots: a big one in the middle, smaller ones around
                dc.FillEllipse(&ell(c, c, 5.0 * k, 5.0 * k), &b);
                for (x, y) in [(0.0, 0.0), (t, 0.0), (0.0, t), (t, t)] { dc.FillEllipse(&ell(x, y, 3.0 * k, 3.0 * k), &b); }
            }
            b.SetOpacity(0.35);
            for (x, y) in [(c, 8.0 * k), (c, t - 8.0 * k), (8.0 * k, c), (t - 8.0 * k, c)] { dc.FillEllipse(&ell(x, y, 1.6 * k, 1.6 * k), &b); }
            dc.EndDraw(None, None)?;
        }
        self.key = [ink[0], ink[1], ink[2], ink[3], self.pat as f32];
        Ok(())
    }
}

impl Background for Print {
    fn fps(&self) -> f32 { if self.still || !self.drift { 0.0 } else { 15.0 } }
    fn opts(&mut self, o: &Json) -> bool {
        self.pat = match o.get("pattern").str() { Some("dots") => 1, Some("stars") => 2, _ => 0 };
        self.drift = o.get("drift").str() != Some("still");
        self.acc = o.get("ink").str() == Some("accent");
        self.key = [-1.0; 5];   // paint the tile again
        print_tile(o) != self.ts
    }
    fn draw(&mut self, g: &Gfx, look: &Look, level: f32, dt: f32) -> Result<()> {
        self.still = level <= 0.0;
        unsafe { g.dc.Clear(Some(&rgba(look.bg))); }
        if self.still { return Ok(()); }
        let ink = if self.acc { look.acc } else { look.dim };
        if self.key != [ink[0], ink[1], ink[2], ink[3], self.pat as f32] { self.paint(look)?; }
        if self.drift { self.off = (self.off + dt * 6.0 * level) % self.ts; }
        unsafe {
            let props = D2D1_BITMAP_BRUSH_PROPERTIES1 { extendModeX: D2D1_EXTEND_MODE_WRAP, extendModeY: D2D1_EXTEND_MODE_WRAP,
                                                        interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR };
            let bb = g.dc.CreateBitmapBrush(&self.tile, Some(&props), None)?;
            // the tile bitmap is in device pixels at 96 dpi: scale it back to the tile's size in DIPs, then drift
            let k = 1.0 / self.scale;
            bb.SetTransform(&M3 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: self.off, M32: self.off * 0.5 });
            bb.SetOpacity(0.4 + level * 0.4);
            g.dc.FillRectangle(&rect(0.0, 0.0, g.w as f32, g.h as f32), &bb);
        }
        Ok(())
    }
}
