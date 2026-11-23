//! One screen's wallpaper: background, cards, overlay and status label, following the page's rules
//! (wallpaper/index.html: newCard, place, kill, typeInto, next). Draws only when something changed.
use crate::anim::{self, Anim, Ctx, Ease, Pose};
use crate::bg::{self, Background, Rng};
use crate::fonts::Fonts;
use crate::gfx::{self, Gfx, rect, rgba};
use crate::json::Json;
use crate::looks::{Color, Look, Looks, with_alpha};
use std::collections::{HashMap, VecDeque};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::*;

const FPS: f64 = 30.0;
const ANIM_FPS: f64 = 60.0;   // only while a card arrives, leaves or glides in the stack
const STACK_GLIDE: f64 = 0.45;   // the page's "transition: bottom .45s cubic-bezier(.2,.8,.2,1)"

/// where a stacked card is now, gliding from its old place to its new one
fn stack_bottom(b: Option<(f32, f32, f64)>, now: f64) -> f32 {
    let Some((from, to, t0)) = b else { return 0.0 };
    let u = ((now - t0) / STACK_GLIDE).clamp(0.0, 1.0) as f32;
    from + (to - from) * Ease::Bezier(0.2, 0.8, 0.2, 1.0).at(u)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind { Cmd, Out, Err }

struct Part { kind: Kind, prefix: String, text: Vec<u16>, masks: Vec<(usize, usize)>, shown: usize }   // masks: (start, length) in text

struct Card {
    id: u64, x: f32, y: f32, w: f32, maxh: f32, h: f32,
    proj: String, time: String,
    parts: Vec<Part>,
    born: f64, dying: Option<f64>, kill_at: f64,
    vars: Ctx,                          // its CSS variables: where dock and slide bring it in from
    bottom: Option<(f32, f32, f64)>,    // in the stack: distance from the screen's bottom (from, to, since when)
    dirty: bool, cursor: Option<usize>,   // the part being typed into
    content: Option<ID2D1Bitmap1>, comp: Option<ID2D1Bitmap1>, size: (u32, u32),
}

struct Typing { card: u64, part: usize, t0: f64, cps: f64, after: f64, out: bool, busy: bool }

/// the sliders: the feed sends them from `glitchwall config` and the settings window
struct Tune { cps: f64, rain: f32, linger: f64, cards: usize, glass: f32, grain: f32 }

pub struct Scene {
    g: Gfx, fonts: std::rc::Rc<Fonts>, css: std::rc::Rc<String>,
    look: Look, look_key: String, bg: Box<dyn Background>, opts: Json,
    scene: ID2D1Bitmap1, overlay: Option<ID2D1Bitmap1>,
    anim_in: Option<Anim>, anim_out: Option<Anim>, place_mode: String,
    mono: IDWriteTextFormat, bar: IDWriteTextFormat, status_fmt: IDWriteTextFormat, line_h: f32, bar_h: f32,
    fx: Effects,
    cards: Vec<Card>, queue: VecDeque<Json>, typing: Option<Typing>, wait_until: Option<f64>, busy_queue: bool,
    by_ref: HashMap<String, u64>, current: Option<u64>, next_id: u64,
    tune: Tune, rng: Rng, masked: String,
    pub asleep: bool, pub ignore_sleep: bool, slept_at: Option<f64>, hidden: bool, art: bool,
    label: String, live_until: f64, blink: bool,
    last_frame: f64, last_bg: f64, bg_drawn: bool, changed: bool,
}

struct Effects { crop: ID2D1Effect, blur: ID2D1Effect, sat: ID2D1Effect, matrix: ID2D1Effect,
                 shadow1: ID2D1Effect, shadow2: ID2D1Effect,
                 pmatrix: ID2D1Effect, psat: ID2D1Effect, pblur: ID2D1Effect }

fn now_hms() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

fn mat_mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21, M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21, M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31, M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}
const IDENTITY: Matrix3x2 = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 };
fn translate(x: f32, y: f32) -> Matrix3x2 { Matrix3x2 { M31: x, M32: y, ..IDENTITY } }

impl Scene {
    pub fn new(g: Gfx, fonts: std::rc::Rc<Fonts>, css: std::rc::Rc<String>, looks: &Looks) -> Result<Scene> {
        let look = looks.resolve(&Json::Null);
        let scene = g.surface(g.w as f32, g.h as f32)?;
        let fx = Effects {
            crop: g.effect(&CLSID_D2D1Crop)?, blur: g.effect(&CLSID_D2D1GaussianBlur)?, sat: g.effect(&CLSID_D2D1Saturation)?,
            matrix: g.effect(&CLSID_D2D1ColorMatrix)?, shadow1: g.effect(&CLSID_D2D1Shadow)?, shadow2: g.effect(&CLSID_D2D1Shadow)?,
            pmatrix: g.effect(&CLSID_D2D1ColorMatrix)?, psat: g.effect(&CLSID_D2D1Saturation)?, pblur: g.effect(&CLSID_D2D1GaussianBlur)?,
        };
        unsafe {   // the glass: crop the scene under the card -> blur -> saturate -> brightness
            fx.crop.SetInput(0, &g.back.cast::<ID2D1Image>()?, true);   // replaced by the scene before use
            fx.blur.SetInput(0, &fx.crop.GetOutput()?, true);
            fx.sat.SetInput(0, &fx.blur.GetOutput()?, true);
            fx.matrix.SetInput(0, &fx.sat.GetOutput()?, true);
            fx.psat.SetInput(0, &fx.pmatrix.GetOutput()?, true);
            fx.pblur.SetInput(0, &fx.psat.GetOutput()?, true);
        }
        gfx::set_bool(&fx.matrix, 2, true)?;   // clamp
        gfx::set_bool(&fx.pmatrix, 2, true)?;
        let mono = fonts.format("Consolas", 16.0, 400)?;
        let mut s = Scene {
            bg: bg::make(&look.rain, &g, &fonts, &look, &Json::Null)?, opts: Json::Null, scene, overlay: None, anim_in: None, anim_out: None, place_mode: "any".into(),
            bar: mono.clone(), status_fmt: mono.clone(), mono, line_h: 18.4, bar_h: 24.0,
            g, fonts, css, look, look_key: String::new(), fx,
            cards: Vec::new(), queue: VecDeque::new(), typing: None, wait_until: None, busy_queue: false,
            by_ref: HashMap::new(), current: None, next_id: 1,
            tune: Tune { cps: 240.0, rain: 0.5, linger: 7.0, cards: 4, glass: 0.58, grain: 1.0 }, rng: Rng(0x2545_f491_4f6c_dd1d), masked: "dots".into(),
            asleep: false, ignore_sleep: false, slept_at: None, hidden: false, art: false, label: "live tool feed".into(), live_until: 0.0, blink: false,
            last_frame: 0.0, last_bg: 0.0, bg_drawn: false, changed: true,
        };
        s.set_look(looks, &Json::Null)?;
        Ok(s)
    }

    fn set_look(&mut self, looks: &Looks, want: &Json) -> Result<()> {
        let key = format!("{want:?}");
        if key == self.look_key { return Ok(()); }
        self.look_key = key;
        let old_rain = self.look.rain.clone();
        let old_mono = self.look.mono.family.clone();
        self.look = looks.resolve(want);
        let l = &self.look;
        let mono = &l.mono;
        self.line_h = 16.0 * mono.scale * 1.15;
        self.mono = self.fonts.format(&mono.family, 16.0 * mono.scale, 400)?;
        unsafe {
            self.mono.SetWordWrapping(DWRITE_WORD_WRAPPING_CHARACTER)?;
            self.mono.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, self.line_h, self.line_h * 0.8)?;
        }
        let d = &l.display;
        self.bar = self.fonts.format(&d.family, 12.0 * d.scale, d.weight)?;
        self.status_fmt = self.fonts.format(&d.family, 13.0 * d.scale, d.weight)?;
        unsafe {
            self.bar.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            self.status_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            self.status_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        }
        self.bar_h = 12.0 * d.scale * 1.25 + 8.0 + 1.0;
        let t = looks.all.get("transitions").get(&l.transition);
        self.anim_in = t.get("in").str().and_then(|s| anim::parse(&self.css, s));
        self.anim_out = t.get("out").str().and_then(|s| anim::parse(&self.css, s));
        self.place_mode = t.get("place").str().unwrap_or("any").to_string();   // dock | stack | any, as the page's PLACE
        if l.rain != old_rain || l.mono.family != old_mono || !self.bg_drawn {
            self.bg = bg::make(&l.rain, &self.g, &self.fonts, &l, self.opts.get(&l.rain))?;
            self.bg.opts(self.opts.get(&l.rain));
        }
        self.overlay = Some(self.make_overlay()?);
        self.bg_drawn = false;
        for c in &mut self.cards { c.dirty = true; }
        self.changed = true;
        Ok(())
    }

    /// texture (scanlines, tube, halftone) and grain, made once per look, drawn over everything
    fn make_overlay(&self) -> Result<ID2D1Bitmap1> {
        let (w, h) = (self.g.w as usize, self.g.h as usize);
        let l = &self.look;
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let k = self.tune.grain;   // the theme's grain times the slider; past 5 even a grainless theme gets some
        let grain = l.grain * k + (k - 1.0).max(0.0) * 0.08;
        let mut px = vec![0u32; w * h];
        let (gr, gg, gb) = if l.light { (0.4, 0.35, 0.3) } else { (0.55, 0.5, 0.45) };
        for y in 0..h {
            for x in 0..w {
                // black with alpha a, then grain on top: (color c, alpha k); composed premultiplied
                let mut a = 0.0f32;
                match l.texture.as_str() {
                    "scan" => if y % 2 == 1 { a = l.scan; },
                    "tube" => {
                        if y % 3 == 0 { a = 0.34; }
                        let dx = (x as f32 - w as f32 / 2.0) / (w as f32 / 2.0);
                        let dy = (y as f32 - h as f32 / 2.0) / (h as f32 / 2.0);
                        let d = (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2;   // 1 at the far corners
                        let v = ((d - 0.55) / 0.45).clamp(0.0, 1.0) * 0.6;
                        a = 1.0 - (1.0 - a) * (1.0 - v);
                    }
                    _ => {}
                }
                let (mut r, mut g, mut b, mut al) = (0.0f32, 0.0f32, 0.0f32, a);
                if l.texture == "halftone" && x % 5 == 2 && y % 5 == 2 {   // an ink dot every 5 px, multiplied into the paper
                    let k = 0.16 * 0.5;
                    r = l.ink[0] * k; g = l.ink[1] * k; b = l.ink[2] * k; al = k;
                }
                if grain > 0.0 {
                    let k = rng.f() * 0.4 * grain * 0.5;
                    if l.light { al = al + k - al * k; }   // multiply-ish: darker specks
                    else { r = r * (1.0 - k) + gr * k; g = g * (1.0 - k) + gg * k; b = b * (1.0 - k) + gb * k; al = al + k - al * k; }
                }
                px[y * w + x] = ((al.min(1.0) * 255.0) as u32) << 24 | ((r.min(1.0) * 255.0) as u32) << 16 | ((g.min(1.0) * 255.0) as u32) << 8 | (b.min(1.0) * 255.0) as u32;
            }
        }
        self.g.bitmap(w as u32, h as u32, Some(&px), false)
    }

    /// a reply from the feed
    pub fn on_reply(&mut self, looks: &Looks, data: &Json, events: Vec<Json>, now: f64) -> Result<()> {
        let was = self.asleep;
        self.asleep = data.get("sleep").truthy() && !self.ignore_sleep;
        if was && !self.asleep {   // woke: typing picks up where it stopped
            if let (Some(t), Some(at)) = (self.typing.as_mut(), self.slept_at) { t.t0 += now - at; }
            self.changed = true;
        }
        if !was && self.asleep { self.slept_at = Some(now); }
        if !data.get("look").is_null() { self.set_look(looks, data.get("look"))?; }
        let opts = data.get("opts");   // the background's own options, {"orb": {"spin": ...}}
        if !opts.is_null() && *opts != self.opts {
            self.opts = opts.clone();
            let o = self.opts.get(&self.look.rain);
            if self.bg.opts(o) {   // an option drawn in once: make the background again, with it
                self.bg = bg::make(&self.look.rain, &self.g, &self.fonts, &self.look, o)?;
                self.bg.opts(o);
            }
            self.bg_drawn = false;   // a still background shows the change too
        }
        if let Some(m) = data.get("masked").str() { self.masked = m.to_string(); }   // how masked secrets show
        let tune = data.get("tune");
        if let Some(v) = tune.get("typeSpeed").num() { self.tune.cps = 60.0 + v * 40.0; }
        if let Some(v) = tune.get("rain").num() { self.tune.rain = v as f32 / 10.0; }
        if let Some(v) = tune.get("linger").num() { self.tune.linger = v; }
        if let Some(v) = tune.get("cards").num() { self.tune.cards = (v as usize).max(1); }
        if let Some(v) = tune.get("grain").num() {   // the grain slider: 5 is the theme's own
            let k = v as f32 / 5.0;
            if k != self.tune.grain { self.tune.grain = k; self.overlay = Some(self.make_overlay()?); self.bg_drawn = false; }
        }
        if let Some(v) = tune.get("glass").num() { let g = v as f32 / 100.0; if g != self.tune.glass { self.tune.glass = g; self.restyle(); } }
        let paused = data.get("paused").truthy();
        self.art = data.get("mode").str() == Some("art");
        let label = data.get("label").str().unwrap_or("live tool feed").to_string();
        let label = if paused { format!("{label}  [paused]") } else { label };
        if label != self.label { self.label = label; self.changed = true; }
        let hidden = paused || self.art;
        if hidden && !self.hidden {   // stream-safe: clear the screen now
            self.queue.clear();
            let ids: Vec<u64> = self.cards.iter().filter(|c| c.dying.is_none()).map(|c| c.id).collect();
            for id in ids { self.kill(id, now); }
        }
        self.hidden = hidden;
        if !events.is_empty() && !hidden {
            self.live_until = now + 60.0;
            self.queue.extend(events);
            while self.queue.len() > 24 { self.queue.pop_front(); }   // never fall minutes behind: skip the oldest
            if self.typing.is_none() && self.wait_until.is_none() { self.next(now)?; }
        }
        Ok(())
    }

    fn restyle(&mut self) { for c in &mut self.cards { c.dirty = true; } self.changed = true; }

    pub fn down(&mut self) { self.live_until = 0.0; }

    fn kill(&mut self, id: u64, now: f64) {
        if let Some(c) = self.cards.iter_mut().find(|c| c.id == id && c.dying.is_none()) { c.dying = Some(now); self.changed = true; }
    }

    /// where a new card goes, as the page's place(): x, y, width, max height, its variables, its stack position.
    /// stack: a pile in the bottom-right corner, held by its bottom edge. dock: beside the newest card there is room
    /// next to (below, right, above, left), coming out from under it. Otherwise pop-up-ad placement: random size,
    /// best of 80 random tries so the new card covers the live ones least.
    fn place(&mut self) -> (f32, f32, f32, f32, Ctx, Option<(f32, f32, f64)>) {
        let (sw, sh) = (self.g.w as f32, self.g.h as f32);
        if self.place_mode == "stack" {
            let w = (sw * 0.34).min(600.0).round();
            return (sw - w - 24.0, sh - 52.0 - 160.0, w, (sh * 0.42).min(260.0), Ctx::default(), Some((52.0, 52.0, 0.0)));
        }
        let w = (sw * (0.24 + self.rng.f() * 0.14)).round();
        let h = (sh * 0.42).min(220.0);
        let live: Vec<(f32, f32, f32, f32)> = self.cards.iter().filter(|c| c.dying.is_none()).map(|c| (c.x, c.y, c.w, c.h.max(160.0))).collect();
        let hits = |x: f32, y: f32| -> f32 {
            live.iter().map(|&(rx, ry, rw, rh)| {
                let (rx, ry, rw, rh) = (rx - 14.0, ry - 14.0, rw + 28.0, rh + 28.0);   // with a gap around each
                ((x + w).min(rx + rw) - x.max(rx)).max(0.0) * ((y + h).min(ry + rh) - y.max(ry)).max(0.0)
            }).sum()
        };
        let mut vars = Ctx::default();
        let mut best: Option<(f32, f32)> = None;
        if self.place_mode == "dock" {
            'cards: for &(lx, ly, lw, lh) in live.iter().rev() {
                let g = 14.0;
                for (x, y, dx, dy) in [(lx, ly + lh + g, 0.0, -40.0), (lx + lw + g, ly, -40.0, 0.0), (lx, ly - h - g, 0.0, 40.0), (lx - w - g, ly, 40.0, 0.0)] {
                    if x < 16.0 || y < 64.0 || x + w > sw - 16.0 || y + h > sh - 56.0 || hits(x, y) > 0.0 { continue; }
                    best = Some((x, y)); vars.dx = Some(dx); vars.dy = Some(dy);
                    break 'cards;
                }
            }
        }
        let best = best.unwrap_or_else(|| {
            let (mut pick, mut best_hit) = ((16.0, 64.0), f32::INFINITY);
            for _ in 0..80 {
                let x = 16.0 + self.rng.f() * (sw - w - 32.0).max(0.0);
                let y = 64.0 + self.rng.f() * (sh - h - 120.0).max(0.0);
                let hit = hits(x, y);
                if hit < best_hit { best_hit = hit; pick = (x, y); }
                if hit == 0.0 { break; }
            }
            pick
        });
        // slide: in from the nearer side edge, out across the screen to the other one
        let (off_l, off_r) = (-(best.0 + w + 40.0), sw - best.0 + 40.0);
        let left = best.0 + w / 2.0 < sw / 2.0;
        vars.sx = Some(if left { off_l } else { off_r });
        vars.ox = Some(if left { off_r } else { off_l });
        let maxh = (sh * 0.42).min(sh - best.1 - 52.0);   // never grows over the corner label
        (best.0, best.1, w, maxh, vars, None)
    }

    /// the toast pile (stack): newest at the bottom, the rest above it, gliding into place; the oldest goes once
    /// there is no room. As the page's restack(). True if a card moved.
    fn restack(&mut self, now: f64) -> bool {
        let sh = self.g.h as f32;
        let mut b = 52.0;
        let mut drop = Vec::new();
        for c in &mut self.cards {   // cards placed before the look switched to the stack join it from where they are
            if c.dying.is_none() && c.bottom.is_none() { let b0 = sh - c.y - c.h; c.bottom = Some((b0, b0, now)); }
        }
        let live: Vec<usize> = (0..self.cards.len()).filter(|&i| self.cards[i].dying.is_none() && self.cards[i].bottom.is_some()).collect();
        for (k, &i) in live.iter().enumerate().rev() {
            let h = self.cards[i].h.max(48.0);
            if b + h > sh - 64.0 && k < live.len() - 1 { drop.push(self.cards[i].id); continue; }
            let c = &mut self.cards[i];
            if let Some((_, to, _)) = c.bottom {
                if (to - b).abs() > 0.5 { c.bottom = Some((stack_bottom(c.bottom, now), b, now)); }
            }
            b += h + 14.0;
        }
        for id in drop { self.kill(id, now); }
        let mut moved = false;
        for c in &mut self.cards {
            if c.bottom.is_none() { continue; }
            let y = sh - stack_bottom(c.bottom, now) - c.h;
            if (y - c.y).abs() > 0.01 { c.y = y; moved = true; }
        }
        moved
    }

    /// A feed text as the wall shows it: each •••• the redaction wrote swapped for the masked setting's look (made
    /// up fresh, never from the hidden text, always the same length), and where each one sits.
    fn unmask(&mut self, text: &str) -> (Vec<u16>, Vec<(usize, usize)>) {
        const WORDS: [&str; 14] = ["REDACTED", "REMOVED", "HIDDEN", "CENSORED", "DELETED", "ERASED", "EXPUNGED", "EXPURGATED",
                                   "BLACKED OUT", "OBSCURED", "SUPPRESSED", "WITHHELD", "CONCEALED", "OMITTED"];
        const NOISE: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789#%&@$?!*+=~^";
        let (mut out, mut masks) = (Vec::new(), Vec::new());
        for (k, piece) in text.split(crate::redact::MASK).enumerate() {
            if k > 0 {
                let shown = match self.masked.as_str() {
                    "words" => format!("[{}]", WORDS[(self.rng.f() * WORDS.len() as f32) as usize % WORDS.len()]),
                    "blocks" => "██████".into(),
                    "stars" => "********".into(),
                    "noise" => (0..8).map(|_| NOISE[(self.rng.f() * NOISE.len() as f32) as usize % NOISE.len()] as char).collect(),
                    _ => crate::redact::MASK.into(),
                };
                let w = wide(&shown);
                masks.push((out.len(), w.len()));
                out.extend(w);
            }
            out.extend(wide(piece));
        }
        (out, masks)
    }

    fn new_card(&mut self, ev: &Json, now: f64) -> u64 {
        while self.cards.iter().filter(|c| c.dying.is_none()).count() >= self.tune.cards {
            let oldest = self.cards.iter().find(|c| c.dying.is_none()).map(|c| c.id).unwrap();
            self.kill(oldest, now);
        }
        let (x, y, w, maxh, vars, bottom) = self.place();
        let id = self.next_id;
        self.next_id += 1;
        let session = ev.get("session").str().unwrap_or("").to_string();
        let proj = format!("{}{} #{}", ev.get("proj").str().unwrap_or(""), if ev.get("side").truthy() { " / agent" } else { "" }, session);
        self.cards.push(Card { id, x, y, w, maxh, h: 60.0, proj, time: now_hms(), parts: Vec::new(), born: now, dying: None,
                               vars, bottom: bottom.map(|(a, b, _)| (a, b, now)), kill_at: now + 30.0, dirty: true, cursor: None, content: None, comp: None, size: (0, 0) });
        id
    }

    /// the typing queue: commands open a card, output goes under the command it belongs to
    fn next(&mut self, now: f64) -> Result<()> {
        loop {
            let Some(ev) = self.queue.pop_front() else { self.typing = None; return Ok(()); };
            let busy = self.queue.len() > 6;
            let kind = ev.get("kind").str().unwrap_or("tool").to_string();
            let (text, masks) = self.unmask(ev.get("text").str().unwrap_or(""));
            let r = ev.get("ref").str().unwrap_or("").to_string();
            if kind == "out" || kind == "err" {
                // live: a command still running (your terminal), more output to come, so the card stays its target
                let live = ev.get("live").truthy();
                let card = if r.is_empty() { self.current } else if live { self.by_ref.get(&r).copied() } else { self.by_ref.remove(&r) };
                let Some(card) = card.filter(|id| self.cards.iter().any(|c| c.id == *id && c.dying.is_none())) else { continue };
                let linger = self.tune.linger;
                let c = self.cards.iter_mut().find(|c| c.id == card).unwrap();
                if let Some(p) = c.parts.iter_mut().rev().find(|p| p.kind != Kind::Cmd) {
                    // more output for the same command: its newest lines in place of what was there, at once
                    p.kind = if kind == "err" { Kind::Err } else { Kind::Out };
                    p.shown = text.len();
                    (p.text, p.masks) = (text, masks);
                    c.dirty = true;
                    c.kill_at = now + if live { linger.max(15.0) } else { linger };
                    self.changed = true;
                    continue;
                }
                c.parts.push(Part { kind: if kind == "err" { Kind::Err } else { Kind::Out }, prefix: String::new(), text, masks, shown: 0 });
                let part = c.parts.len() - 1;
                self.typing = Some(Typing { card, part, t0: now + 0.26, cps: self.tune.cps * 5.0, after: 0.12, out: true, busy });
                return Ok(());
            }
            let card = self.new_card(&ev, now);
            self.current = Some(card);
            if !r.is_empty() {
                self.by_ref.insert(r, card);
                if self.by_ref.len() > 50 { if let Some(k) = self.by_ref.keys().next().cloned() { self.by_ref.remove(&k); } }
            }
            let prompt = match kind.as_str() {
                "$" => "$", "PS>" => "PS>", "read" => "cat", "write" => "write", "edit" => "edit", "find" => "grep",
                "web" => "curl", "agent" => "spawn", "mouse" => "click", _ => ">",
            };
            let c = self.cards.iter_mut().find(|c| c.id == card).unwrap();
            c.parts.push(Part { kind: Kind::Cmd, prefix: format!("{prompt} "), text, masks, shown: 0 });
            self.typing = Some(Typing { card, part: 0, t0: now + 0.26, cps: self.tune.cps * if busy { 4.0 } else { 1.0 },
                                        after: 0.2, out: false, busy });
            return Ok(());
        }
    }

    /// move time on: typing, lingering, leaving. True if something visible changed.
    fn update(&mut self, now: f64) -> Result<bool> {
        let mut changed = false;
        if let Some(t) = &self.typing {
            if now >= t.t0 {
                let (id, part, t0, cps, after, out, busy) = (t.card, t.part, t.t0, t.cps, t.after, t.out, t.busy);
                let linger = self.tune.linger;
                match self.cards.iter_mut().find(|c| c.id == id && c.dying.is_none()) {
                    None => { self.typing = None; self.wait_until = Some(now); }   // its card went: carry on
                    Some(c) => {
                        let p = &mut c.parts[part];
                        let want = (((now - t0) * cps).round() as usize).max(p.shown + 1).min(p.text.len());
                        if want != p.shown || c.cursor != Some(part) { p.shown = want; c.dirty = true; c.cursor = Some(part); changed = true; }
                        if p.shown >= p.text.len() {
                            c.cursor = None; c.dirty = true;
                            if out { c.kill_at = now + if busy { 1.5 } else { linger }; }
                            self.typing = None;
                            self.wait_until = Some(now + after);
                        }
                    }
                }
            }
        }
        if let Some(w) = self.wait_until { if now >= w { self.wait_until = None; self.next(now)?; } }
        let expired: Vec<u64> = self.cards.iter().filter(|c| c.dying.is_none() && now >= c.kill_at).map(|c| c.id).collect();
        for id in expired { self.kill(id, now); changed = true; }
        if self.place_mode == "stack" { changed |= self.restack(now); }
        let before = self.cards.len();
        self.cards.retain(|c| c.dying.map(|d| now - d < 0.65).unwrap_or(true));
        changed |= self.cards.len() != before;
        let blink = (now * 2.0) as i64 % 2 == 1;
        if blink != self.blink && self.cards.iter().any(|c| c.cursor.is_some()) { changed = true; for c in &mut self.cards { if c.cursor.is_some() { c.dirty = true; } } }
        self.blink = blink;
        Ok(changed)
    }

    fn animating(&self, now: f64) -> bool {
        let din = self.anim_in.as_ref().map(|a| a.dur as f64).unwrap_or(0.0);
        self.cards.iter().any(|c| now - c.born < din || c.dying.is_some() || c.bottom.is_some_and(|(_, _, t0)| now - t0 < STACK_GLIDE))
    }

    /// when this screen next wants a frame
    pub fn due(&self, now: f64) -> f64 {
        if self.asleep { return now + 0.5; }
        if self.animating(now) { return self.last_frame + 1.0 / ANIM_FPS; }
        let bg = self.bg.fps() as f64;
        let mut at = if bg > 0.0 { self.last_frame + 1.0 / bg } else { now + 0.25 };
        if let Some(t) = &self.typing { at = at.min(t.t0.max(self.last_frame + 1.0 / FPS)); }
        if let Some(w) = self.wait_until { at = at.min(w); }
        if self.cards.iter().any(|c| c.cursor.is_some()) { at = at.min(((now * 2.0).floor() + 1.0) / 2.0); }
        for c in &self.cards { if c.dying.is_none() { at = at.min(c.kill_at); } }
        if self.changed { at = now; }
        at
    }

    /// one frame; presents only if something changed
    pub fn frame(&mut self, now: f64) -> Result<()> {
        if self.asleep { return Ok(()); }
        let dt = (now - self.last_frame).min(0.1) as f32;
        self.last_frame = now;
        let mut changed = std::mem::take(&mut self.changed) | self.update(now)?;
        let bgfps = self.bg.fps() as f64;
        if !self.bg_drawn || (bgfps > 0.0 && now - self.last_bg >= 1.0 / bgfps - 0.004) {
            let bdt = if self.bg_drawn { (now - self.last_bg).min(0.1) as f32 } else { 0.0 };
            self.last_bg = now;
            unsafe {
                self.g.dc.SetTarget(&self.scene);
                self.g.dc.BeginDraw();
                self.bg.draw(&self.g, &self.look, self.tune.rain, bdt)?;
                self.g.dc.EndDraw(None, None)?;
            }
            self.bg_drawn = true;
            changed = true;
        }
        let animating = self.animating(now);
        let _ = dt;
        if !(changed || animating) { return Ok(()); }
        for i in 0..self.cards.len() { if self.cards[i].dirty { self.build(i)?; } }
        for i in 0..self.cards.len() { self.glass(i)?; }
        unsafe {
            let dc = &self.g.dc;
            dc.SetTarget(&self.g.back);
            dc.BeginDraw();
            dc.SetTransform(&IDENTITY);
            dc.DrawImage(&self.scene.cast::<ID2D1Image>()?, None, None, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_COMPOSITE_MODE_SOURCE_COPY);
            for i in 0..self.cards.len() { self.draw_card(i, now)?; }
            dc.SetTransform(&IDENTITY);
            self.status(now)?;
            if let Some(o) = &self.overlay {
                let flick = (now % 6.0) < 0.06 && self.look.texture != "none";
                dc.DrawBitmap(o, Some(&gfx::rect(0.0, 0.0, self.g.w as f32, self.g.h as f32)), if flick { 0.86 } else { 1.0 },
                              D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);   // made in CSS pixels, stretched like the page's
            }
            dc.EndDraw(None, None)?;
        }
        self.g.present()
    }

    fn alpha(&self) -> f32 { self.look.solid.unwrap_or(self.tune.glass) }

    /// lay out a card's text and draw its fixed parts (glass tint, bar, edge, text, glow) into its content bitmap
    fn build(&mut self, i: usize) -> Result<()> {
        let l = self.look.clone();
        let (w, maxh, line_h, bar_h) = (self.cards[i].w, self.cards[i].maxh, self.line_h, self.bar_h);
        let inner = w - 24.0;
        // the text blocks: (layout, x offset, height, rule color)
        let mut blocks: Vec<(IDWriteTextLayout, f32, f32, Option<Color>)> = Vec::new();
        let dw = &self.fonts.dw;
        let blink = self.blink;
        let c = &self.cards[i];
        let ink_b = self.g.brush(l.ink)?;
        let acc_b = self.g.brush(l.acc)?;
        let err_b = self.g.brush(l.err)?;
        let mask_b = self.g.brush(l.mask)?;
        let out_b = self.g.brush(with_alpha(l.ink, 0.82))?;
        for (k, p) in c.parts.iter().enumerate() {
            let mut t: Vec<u16> = wide(&p.prefix);
            let pre = t.len() as u32;
            t.extend_from_slice(&p.text[..p.shown]);
            let cursor = c.cursor == Some(k) && !blink;
            if cursor { t.extend(wide("█")); }
            let indent = if p.kind == Kind::Cmd { 0.0 } else { 12.0 };
            let lay = unsafe { dw.CreateTextLayout(&t, &self.mono, (inner - indent).max(10.0), 10_000.0)? };
            unsafe {
                let range = |s: u32, n: u32| DWRITE_TEXT_RANGE { startPosition: s, length: n };
                let main: &ID2D1SolidColorBrush = match p.kind { Kind::Cmd => &ink_b, Kind::Out => &out_b, Kind::Err => &err_b };
                lay.SetDrawingEffect(main, range(0, t.len() as u32))?;
                if pre > 0 { lay.SetDrawingEffect(&acc_b, range(0, pre))?; }
                for &(s, n) in &p.masks {   // masked secrets, in their own color
                    if s < p.shown { lay.SetDrawingEffect(&mask_b, range(pre + s as u32, n.min(p.shown - s) as u32))?; }
                }
                if cursor { lay.SetDrawingEffect(&acc_b, range(t.len() as u32 - 1, 1))?; }
            }
            let mut m = DWRITE_TEXT_METRICS::default();
            unsafe { lay.GetMetrics(&mut m)?; }
            let mut hgt = m.height.max(line_h);
            if p.kind == Kind::Cmd { hgt = hgt.min(line_h * 9.0); }
            let rule = match p.kind { Kind::Cmd => None, Kind::Out => Some(with_alpha(l.line, l.line[3] * 0.82)), Kind::Err => Some(with_alpha(l.err, 0.45)) };
            blocks.push((lay, indent, hgt, rule));
        }
        let content: f32 = blocks.iter().enumerate().map(|(k, b)| b.2 + if k > 0 { 6.0 } else { 0.0 }).sum();
        let total = 2.0 + bar_h + 8.0 + content + 10.0;
        let h = total.min(maxh).max(bar_h + 30.0).round();
        let overflow = (total - h).max(0.0);   // long output shows its newest lines: cut from the top of the outputs
        let (wi, hi) = (w.round() as u32, h as u32);
        let card = &mut self.cards[i];
        card.h = h;
        if card.size != (wi, hi) || card.content.is_none() {
            card.content = Some(self.g.surface(wi as f32, hi as f32)?);
            card.comp = Some(self.g.surface(wi as f32, hi as f32)?);
            card.size = (wi, hi);
        }
        let text_bmp = self.g.surface(wi as f32, hi as f32)?;
        let dc = &self.g.dc;
        unsafe {
            // the text alone first: the glow is a shadow of it
            dc.SetTarget(&text_bmp);
            dc.BeginDraw();
            dc.SetTransform(&IDENTITY);
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let proj: Vec<u16> = wide(&card.proj);
            let time: Vec<u16> = wide(&card.time);
            let time_lay = dw.CreateTextLayout(&time, &self.bar, 200.0, bar_h)?;
            let mut tm = DWRITE_TEXT_METRICS::default();
            time_lay.GetMetrics(&mut tm)?;
            let proj_lay = dw.CreateTextLayout(&proj, &self.bar, (w - 20.0 - tm.width - 12.0).max(10.0), bar_h)?;
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            let sign = dw.CreateEllipsisTrimmingSign(&self.bar)?;
            proj_lay.SetTrimming(&trim, &sign)?;
            let ty = 1.0 + (bar_h - 1.0 - tm.height) / 2.0;
            dc.DrawTextLayout(D2D_POINT_2F { x: 10.0, y: ty }, &proj_lay, &self.g.brush(l.barb)?, D2D1_DRAW_TEXT_OPTIONS_CLIP);
            dc.DrawTextLayout(D2D_POINT_2F { x: w - 10.0 - tm.width, y: ty }, &time_lay, &self.g.brush(l.barfg)?, D2D1_DRAW_TEXT_OPTIONS_NONE);
            let body = rect(1.0, bar_h + 1.0, w - 2.0, h - bar_h - 2.0);
            dc.PushAxisAlignedClip(&body, D2D1_ANTIALIAS_MODE_ALIASED);
            let mut y = 1.0 + bar_h + 8.0;
            let mut cut = overflow;
            let rule_brushes: Vec<Option<ID2D1SolidColorBrush>> = blocks.iter().map(|b| b.3.map(|c| self.g.brush(c).ok()).flatten()).collect();
            for (k, (lay, indent, hgt, _)) in blocks.iter().enumerate() {
                if k > 0 { y += 6.0; }
                let (mut top, mut hh) = (y, *hgt);
                if k > 0 && cut > 0.0 { let take = cut.min(hh - line_h * 2.0).max(0.0); top -= take; hh -= take; cut -= take; }
                let clip = rect(12.0 + indent, y, inner - indent, hh);
                dc.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
                dc.DrawTextLayout(D2D_POINT_2F { x: 12.0 + indent, y: top }, lay, &ink_b, D2D1_DRAW_TEXT_OPTIONS_NONE);
                dc.PopAxisAlignedClip();
                if let Some(b) = &rule_brushes[k] { dc.FillRectangle(&rect(12.0, y, 2.0, hh), b); }
                y += hh;
            }
            dc.PopAxisAlignedClip();
            dc.EndDraw(None, None)?;

            // then the card: tint, bar, rules, edge, glow, text
            let content = card.content.as_ref().unwrap();
            dc.SetTarget(content);
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let alpha = l.solid.unwrap_or(self.tune.glass);
            dc.FillRectangle(&rect(0.0, 0.0, w, h), &self.g.brush(with_alpha(l.surface, alpha))?);
            dc.FillRectangle(&rect(0.0, 0.0, w, bar_h), &self.g.brush(l.bar)?);
            dc.FillRectangle(&rect(1.0, bar_h, w - 2.0, 1.0), &self.g.brush(l.line)?);
            dc.DrawRectangle(&rect(0.5, 0.5, w - 1.0, h - 1.0), &self.g.brush(l.edge)?, 1.0, None);
            if l.glow {   // the page's text-shadow: 0 0 5px ink .8, 0 0 14px ink .45 (a CSS blur radius is about 2 standard deviations)
                let src = text_bmp.cast::<ID2D1Image>()?;
                for (fx, sd, a) in [(&self.fx.shadow2, 7.0f32, 0.45f32), (&self.fx.shadow1, 2.5, 0.8)] {
                    fx.SetInput(0, &src, true);
                    gfx::set_f32(fx, 0, sd)?;
                    gfx::set_floats(fx, 1, D2D1_PROPERTY_TYPE_VECTOR4, &[l.ink[0], l.ink[1], l.ink[2], a])?;
                    dc.DrawImage(&fx.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                }
            }
            dc.DrawBitmap(&text_bmp, None, 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
            dc.EndDraw(None, None)?;
        }
        card.dirty = false;
        Ok(())
    }

    /// the frosted glass: the scene under the card, blurred, desaturated and dimmed, then the card's content on top
    fn glass(&mut self, i: usize) -> Result<()> {
        let c = &self.cards[i];
        let (Some(comp), Some(content)) = (&c.comp, &c.content) else { return Ok(()) };
        let f = &self.look.filter;
        let (x, y, w, h) = (c.x, c.y, c.size.0 as f32, c.size.1 as f32);
        let m = f.blur * 3.0;
        let dc = &self.g.dc;
        unsafe {
            self.fx.crop.SetInput(0, &self.scene.cast::<ID2D1Image>()?, true);
            gfx::set_floats(&self.fx.crop, 0, D2D1_PROPERTY_TYPE_VECTOR4, &[x - m, y - m, x + w + m, y + h + m])?;
            gfx::set_f32(&self.fx.blur, 0, f.blur.max(0.0))?;
            gfx::set_f32(&self.fx.sat, 0, f.saturate)?;
            let b = f.brightness;
            gfx::set_floats(&self.fx.matrix, 0, D2D1_PROPERTY_TYPE_MATRIX_5X4,
                            &[b, 0., 0., 0., 0., b, 0., 0., 0., 0., b, 0., 0., 0., 0., 1., 0., 0., 0., 0.])?;
            dc.SetTarget(comp);
            dc.BeginDraw();
            dc.SetTransform(&IDENTITY);
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let area = rect(x, y, w, h);
            dc.DrawImage(&self.fx.matrix.GetOutput()?, Some(&D2D_POINT_2F { x: -x, y: -y }), Some(&area),
                         D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            dc.DrawBitmap(content, None, 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
            dc.EndDraw(None, None)?;
        }
        Ok(())
    }

    /// the card on screen, posed by its transition
    fn draw_card(&self, i: usize, now: f64) -> Result<()> {
        let c = &self.cards[i];
        let Some(comp) = &c.comp else { return Ok(()) };
        let ctx = Ctx { w: c.size.0 as f32, h: c.size.1 as f32, vw: self.g.w as f32, ..c.vars };
        let pose: Pose = match c.dying {
            Some(d) => self.anim_out.as_ref().map(|a| a.pose((now - d) as f32, &ctx)).unwrap_or(anim::REST),
            None => self.anim_in.as_ref().map(|a| a.pose((now - c.born) as f32, &ctx)).unwrap_or(anim::REST),
        };
        if pose.opacity <= 0.001 || pose.sx.abs() < 1e-4 || pose.sy.abs() < 1e-4 { return Ok(()); }
        let (w, h) = (c.size.0 as f32, c.size.1 as f32);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let rad = pose.rot.to_radians();
        let mut m = translate(-cx, -cy);
        m = mat_mul(&m, &Matrix3x2 { M11: pose.sx, M22: pose.sy, ..IDENTITY });
        m = mat_mul(&m, &Matrix3x2 { M21: pose.skew.to_radians().tan(), ..IDENTITY });
        m = mat_mul(&m, &Matrix3x2 { M11: rad.cos(), M12: rad.sin(), M21: -rad.sin(), M22: rad.cos(), ..IDENTITY });
        m = mat_mul(&m, &translate(c.x + cx + pose.tx, c.y + cy + pose.ty));
        let dc = &self.g.dc;
        unsafe {
            dc.SetTransform(&m);
            let k = pose.clip;
            let clip = D2D_RECT_F { left: w * k[3] / 100.0, top: h * k[0] / 100.0, right: w * (1.0 - k[1] / 100.0), bottom: h * (1.0 - k[2] / 100.0) };
            if clip.right <= clip.left || clip.bottom <= clip.top { return Ok(()); }
            dc.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
            if pose.bright == 1.0 && pose.sat == 1.0 && pose.blur == 0.0 {
                dc.DrawBitmap(comp, None, pose.opacity, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            } else {   // CSS filter: brightness / saturate / blur, and the opacity with them
                let b = pose.bright;
                self.fx.pmatrix.SetInput(0, &comp.cast::<ID2D1Image>()?, true);
                gfx::set_floats(&self.fx.pmatrix, 0, D2D1_PROPERTY_TYPE_MATRIX_5X4,
                                &[b, 0., 0., 0., 0., b, 0., 0., 0., 0., b, 0., 0., 0., 0., pose.opacity, 0., 0., 0., 0.])?;
                gfx::set_f32(&self.fx.psat, 0, pose.sat)?;
                gfx::set_f32(&self.fx.pblur, 0, pose.blur.max(0.0) / 2.0)?;
                dc.DrawImage(&self.fx.pblur.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            }
            dc.PopAxisAlignedClip();
            dc.SetTransform(&IDENTITY);
        }
        Ok(())
    }

    /// bottom right: the live dot and the label
    fn status(&self, now: f64) -> Result<()> {
        if self.art || self.label.is_empty() { return Ok(()); }
        let l = &self.look;
        let (w, h) = (self.g.w as f32, self.g.h as f32);
        let text = wide(&self.label);
        unsafe {
            let lay = self.fonts.dw.CreateTextLayout(&text, &self.status_fmt, w * 0.6, 40.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            lay.GetMetrics(&mut m)?;
            let right = w * 0.975;
            let top = h - 16.0 - m.height;
            self.g.dc.DrawTextLayout(D2D_POINT_2F { x: right - w * 0.6, y: top }, &lay, &self.g.brush(l.status)?, D2D1_DRAW_TEXT_OPTIONS_NONE);
            let live = now < self.live_until;
            let dot = rect(right - m.width - 16.0, top + (m.height - 8.0) / 2.0, 8.0, 8.0);
            self.g.dc.FillRectangle(&dot, &self.g.brush(if live { l.acc } else { l.off })?);
        }
        let _ = self.alpha();
        Ok(())
    }
}
