//! The hot corner (winhost.py's watch_corner, piece 3c): rest the mouse in the chosen corner of any screen for half
//! a second and the settings open, or come forward if they are open. Never while a fullscreen app is in front (a
//! game, a video), and once per visit to the corner.
use crate::desk::{self, Rect};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// seconds the mouse must rest there; pixels from the exact corner that count
pub const DWELL: f64 = 0.5;
pub const REACH: i32 = 3;

/// The screen whose `corner` (top-left, top-right, bottom-left, bottom-right) the point is in, if any.
pub fn hit(corner: &str, x: i32, y: i32, mons: &[(u32, Rect)]) -> Option<u32> {
    if !matches!(corner, "top-left" | "top-right" | "bottom-left" | "bottom-right") { return None; }
    mons.iter().find(|(_, m)| {
        let cx = if corner.ends_with("left") { m.x } else { m.x + m.w - 1 };
        let cy = if corner.starts_with("top") { m.y } else { m.y + m.h - 1 };
        (x - cx).abs() <= REACH && (y - cy).abs() <= REACH
    }).map(|(n, _)| *n)
}

/// Screens where the app in front covers the whole screen (winhost.fullscreen_screens).
pub fn fullscreen(mons: &[(u32, Rect)]) -> Vec<u32> {
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() || desk::is_desktop(h) { return Vec::new(); }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == std::process::id() { return Vec::new(); }   // our own wallpaper window can get the focus
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        mons.iter().filter(|(_, m)| desk::covers((r.left, r.top, r.right, r.bottom), *m)).map(|(n, _)| *n).collect()
    }
}

/// If the settings window is open, put it in front (and un-hide a docked mini). True if it was there.
pub fn bring_settings_forward() -> bool {
    unsafe {
        let Ok(h) = FindWindowW(PCWSTR::null(), w!("glitchwall settings")) else { return false };
        if h.0.is_null() { return false; }
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetForegroundWindow(h);
        true
    }
}

/// Runs for good (on its own thread): `which` is the corner setting now, `open` opens the settings.
pub fn watch(which: impl Fn() -> String, open: impl Fn()) {
    let (mut since, mut fired): (Option<Instant>, bool) = (None, false);
    let (mut mons, mut corner, mut looked): (Vec<(u32, Rect)>, String, Option<Instant>) = (Vec::new(), "off".into(), None);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if looked.is_none_or(|t| t.elapsed().as_secs_f64() > 2.0) {   // screens and setting rarely change: every 2 s
            mons = desk::monitors();
            corner = which();
            looked = Some(Instant::now());
        }
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() { continue; }
        let Some(n) = hit(&corner, pt.x, pt.y, &mons) else { since = None; fired = false; continue };
        let t = *since.get_or_insert_with(Instant::now);
        if !fired && t.elapsed().as_secs_f64() >= DWELL && !fullscreen(&mons).contains(&n) {
            fired = true;
            open();
        }
    }
}

/// The settings: brought forward if open, else started with `cmd` (glitchwall's own command for them, given by
/// glitchwall when it starts `host`; the Python settings window until piece 5).
pub fn open_settings(cmd: &[String]) {
    if bring_settings_forward() { return; }
    if let Some((exe, rest)) = cmd.split_first() {
        let _ = std::process::Command::new(exe).args(rest).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_like_winhost() {
        let mons = [(1, Rect { x: 0, y: 0, w: 1920, h: 1080 }), (2, Rect { x: 1920, y: 0, w: 1280, h: 1024 })];
        assert_eq!(hit("top-left", 0, 0, &mons), Some(1));
        assert_eq!(hit("top-left", 3, 3, &mons), Some(1));
        assert_eq!(hit("top-left", 4, 0, &mons), None);
        assert_eq!(hit("bottom-right", 1919, 1079, &mons), Some(1));
        assert_eq!(hit("bottom-right", 3199, 1023, &mons), Some(2));
        assert_eq!(hit("top-left", 1920, 0, &mons), Some(2));
        assert_eq!(hit("off", 0, 0, &mons), None);
        assert_eq!(hit("middle", 0, 0, &mons), None);
    }
}
