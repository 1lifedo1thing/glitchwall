//! glitchwall-render: glitchwall's wallpaper drawn natively (Direct2D), one window per screen behind the desktop icons,
//! fed by glitchwall's own feed. glitchwall starts it (renderer = native); it can also run by hand:
//!   GLITCHWALL_KEY=<feed key> glitchwall-render --port 47833 [--which all|primary] [--wallpaper DIR] [--parent PID] [--scale X] [--peek [--peek-size WxH]]
//! --wallpaper is the folder with looks.js, index.html and the fonts (default: the one next to this exe).
//! `glitchwall-render feed [--port N] [--wallpaper DIR]` runs the feed instead (server.rs and settings.rs; pieces 2a-2b,
//! not used by glitchwall yet); `glitchwall-render demo` runs demo.py's made-up feed (piece 2c).
//! `glitchwall-render host [--wallpaper DIR] [--parent PID] [--which ..]` runs both in one process (piece 3b): the
//! feed on threads, the windows rebuilt in place when the screens change instead of the process ending; with
//! --glitchwall JSON (glitchwall's own command, as a list) it has the hot corner and the tray icon too (3c, 3d).
//! `glitchwall-render run [host's options]` keeps `host` running with no Python, and hands over to the Python feed
//! after 3 starts in a minute (supervise.rs, piece 4a).
//! `glitchwall-render install|uninstall` (install.rs, piece 4c) adds or removes autostart (pointing at `run`), the
//! Start menu shortcut and the desktop right-click entry.
#![windows_subsystem = "windows"]

mod ambient;
mod anim;
mod cli;
mod bg;
mod codex;
mod corner;
mod desk;
mod feed;
mod fonts;
mod gemini;
mod gfx;
mod install;
mod json;
mod logs;
mod looks;
mod redact;
mod scene;
mod server;
mod settings;
mod supervise;
mod tray;
mod tail;

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use windows::core::*;

struct Screen { n: u32, rect: desk::Rect, scale: f32, hwnd: windows::Win32::Foundation::HWND, scene: scene::Scene, feed: Option<std::sync::mpsc::Receiver<feed::Msg>> }

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
    if let Some(cmd @ ("config" | "themes" | "reset" | "export" | "install" | "uninstall")) = args.get(1).map(String::as_str) {   // piece 4b
        cli::attach_console();
        let wp = server::wallpaper_dir(&args);
        let schema = settings::Schema::from_file(&wp.join("looks.js")).map_err(|e| Error::new(HRESULT(-1), e.as_str()))?;
        let code = match cmd {
            "config" => cli::config(&schema, &args),
            "themes" => cli::themes(&schema),
            "reset" => cli::reset(&schema),
            "install" => install::install(&schema, &wp, &args),   // piece 4c
            "uninstall" => install::uninstall(),
            _ => cli::export(&schema, &wp, &args),
        };
        std::process::exit(code);
    }
    if args.get(1).map(String::as_str) == Some("run") {   // what start and autostart leave running (piece 4a)
        return supervise::run(&args).map_err(|e| Error::new(HRESULT(-1), e.as_str()));
    }
    if args.get(1).map(String::as_str) == Some("demo") {
        return server::run_demo(&args).map_err(|e| Error::new(HRESULT(-1), e.as_str()));
    }
    if args.get(1).map(String::as_str) == Some("host") {
        let (port, key) = server::start_in_background(&args).map_err(|e| Error::new(HRESULT(-1), e.as_str()))?;
        loop {   // screens changed: new windows for the new layout, the feed carries on
            if draw(&args, Some(port), &key, args.iter().any(|a| a == "--peek"))? == Ended::Quit { return Ok(()); }
        }
    }
    let port: Option<u16> = arg(&args, "--port").and_then(|p| p.parse().ok());
    let key = std::env::var("GLITCHWALL_KEY").unwrap_or_default();
    draw(&args, port, &key, args.iter().any(|a| a == "--peek")).map(|_| ())
}

#[derive(PartialEq)]
enum Ended { Quit, ScreensChanged }

/// The wallpaper windows, one per screen, until they close, glitchwall's process ends, or the screens change.
fn draw(args: &[String], port: Option<u16>, key: &str, peek: bool) -> Result<Ended> {
    // --peek-size: the peek window's size, for screenshots at a set resolution (default 960x560)
    let peek_size: (u32, u32) = arg(args, "--peek-size").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    }).filter(|&(w, h)| (320..=3840).contains(&w) && (200..=2160).contains(&h)).unwrap_or((960, 560));
    let which = arg(args, "--which").unwrap_or_else(|| "all".into());
    let parent: Option<u32> = arg(args, "--parent").and_then(|p| p.parse().ok());
    let force_scale: Option<f32> = arg(args, "--scale").and_then(|s| s.parse().ok()).filter(|s: &f32| (0.5..=4.0).contains(s));
    let wp = arg(args, "--wallpaper").map(PathBuf::from).unwrap_or_else(|| {
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
        let feed = port.map(|p| feed::start(p, key.to_string(), n, peek));
        screens.push(Screen { n, rect: r, scale, hwnd, scene, feed });
    }

    let clock = Instant::now();
    let mut checked = 0.0;
    let mut looked = 0.0;
    let ours: Vec<u32> = screens.iter().map(|s| s.n).collect();
    loop {
        if !desk::pump() { return Ok(Ended::Quit); }
        let now = clock.elapsed().as_secs_f64();
        if now - looked > 0.5 && !peek {   // twice a second: which of our screens nobody can see (covered, locked)
            looked = now;
            let asleep = desk::asleep(&ours);
            for s in &mut screens { s.scene.covered = asleep.contains(&s.n); }
        }
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
            if parent.is_some_and(|p| !desk::alive(p)) { return Ok(Ended::Quit); }   // glitchwall is gone: so are we
            // screens changed, or their scaling: windows for the new layout (the drawing goes first, then its window)
            if m != layout || (force_scale.is_none() && screens.iter().any(|s| (desk::scale(s.rect) - s.scale).abs() > 0.01)) {
                for s in screens { let h = s.hwnd; drop(s); desk::close(h); }
                return Ok(Ended::ScreensChanged);
            }
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
