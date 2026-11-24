//! glitchwall-render: glitchwall's wallpaper drawn natively (Direct2D), one window per screen behind the desktop icons,
//! fed by glitchwall's own feed. glitchwall starts it (renderer = native); it can also run by hand:
//!   GLITCHWALL_KEY=<feed key> glitchwall-render --port 47833 [--which all|primary] [--wallpaper DIR] [--parent PID] [--scale X] [--peek [--peek-size WxH]]
//! --wallpaper is the folder with looks.js, index.html and the fonts (default: the one next to this exe).
//! `glitchwall-render feed [--port N] [--wallpaper DIR]` runs the feed instead (server.rs and settings.rs; pieces 2a-2b,
//! not used by glitchwall yet).
#![windows_subsystem = "windows"]

mod anim;
mod bg;
mod codex;
mod desk;
mod feed;
mod fonts;
mod gemini;
mod gfx;
mod json;
mod logs;
mod looks;
mod redact;
mod scene;
mod server;
mod settings;
mod tail;

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use windows::core::*;

struct Screen { rect: desk::Rect, scale: f32, hwnd: windows::Win32::Foundation::HWND, scene: scene::Scene, feed: Option<std::sync::mpsc::Receiver<feed::Msg>> }

fn arg(args: &[String], name: &str) -> Option<String> { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned()) }

/// glitchwall's state folder (as settings.py has it): where the renderer notes why it stopped
fn log(msg: &str) {
    let dir = std::env::var("GLITCHWALL_HOME").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".glitchwall"));
    let _ = std::fs::write(dir.join("render.log"), format!("{msg}\n"));
}

fn main() {
    if let Err(e) = run() { log(&format!("glitchwall-render stopped: {e}")); std::process::exit(1); }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("feed") {
        return server::run(&args).map_err(|e| Error::new(HRESULT(-1), e.as_str()));
    }
    let peek = args.iter().any(|a| a == "--peek");
    // --peek-size: the peek window's size, for screenshots at a set resolution (default 960x560)
    let peek_size: (u32, u32) = arg(&args, "--peek-size").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    }).filter(|&(w, h)| (320..=3840).contains(&w) && (200..=2160).contains(&h)).unwrap_or((960, 560));
    let port: Option<u16> = arg(&args, "--port").and_then(|p| p.parse().ok());
    let which = arg(&args, "--which").unwrap_or_else(|| "all".into());
    let key = std::env::var("GLITCHWALL_KEY").unwrap_or_default();
    let parent: Option<u32> = arg(&args, "--parent").and_then(|p| p.parse().ok());
    let force_scale: Option<f32> = arg(&args, "--scale").and_then(|s| s.parse().ok()).filter(|s: &f32| (0.5..=4.0).contains(s));
    let wp = arg(&args, "--wallpaper").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("wallpaper"))).unwrap_or_default()
    });
    let looks = looks::Looks::load(&wp.join("looks.js")).map_err(|e| Error::new(HRESULT(-1), e))?;
    let css = Rc::new(std::fs::read_to_string(wp.join("index.html")).unwrap_or_default());
    let fonts = Rc::new(fonts::Fonts::load(&wp)?);
    desk::dpi_aware();

    let mut mons = desk::monitors();
    if which == "primary" || peek { mons.truncate(1); }
    let layout = mons.clone();
    let mut screens = Vec::new();
    for (n, r) in mons {
        let hwnd = desk::window(r, peek.then_some(peek_size))?;
        let (w, h) = if peek { peek_size } else { (r.w as u32, r.h as u32) };
        let scale = force_scale.unwrap_or_else(|| desk::scale(r));   // --scale: try another display scaling
        let g = gfx::Gfx::new(hwnd, w, h, scale)?;
        let mut scene = scene::Scene::new(g, fonts.clone(), css.clone(), &looks)?;
        scene.ignore_sleep = peek;   // the peek window is the thing being looked at, whatever covers the desktop
        let feed = port.map(|p| feed::start(p, key.clone(), n, peek));
        screens.push(Screen { rect: r, scale, hwnd, scene, feed });
    }

    let clock = Instant::now();
    let mut checked = 0.0;
    loop {
        if !desk::pump() { return Ok(()); }
        let now = clock.elapsed().as_secs_f64();
        for s in &mut screens {
            while let Some(m) = s.feed.as_ref().and_then(|f| f.try_recv().ok()) {
                match m {
                    feed::Msg::Reply(r) => s.scene.on_reply(&looks, &r.data, r.events, now)?,
                    feed::Msg::Down => s.scene.down(),
                }
            }
        }
        if now - checked > 2.0 && !peek {   // every 2 s: screens unchanged, still behind the icons
            checked = now;
            let mut m = desk::monitors();
            if which == "primary" { m.truncate(1); }
            if m != layout { return Ok(()); }   // screens changed: glitchwall starts a fresh renderer for the new layout
            if force_scale.is_none() && screens.iter().any(|s| (desk::scale(s.rect) - s.scale).abs() > 0.01) { return Ok(()); }   // scaling too
            if parent.is_some_and(|p| !desk::alive(p)) { return Ok(()); }   // glitchwall's feed is gone: so are we
            for s in &screens { desk::keep_behind_icons(s.hwnd, s.rect); }
        }
        let mut next = now + 0.25;
        for s in &mut screens {
            if s.scene.due(now) <= now + 0.001 { s.scene.frame(now)?; }
            next = next.min(s.scene.due(clock.elapsed().as_secs_f64()));
        }
        desk::sleep_until(next.min(clock.elapsed().as_secs_f64() + 0.25), &clock);
    }
}
