//! `glitchwall-render run [host's options]` (piece 4a): what `glitchwall start` and autostart leave running, with no
//! Python. It notes its pid where glitchwall's stop and status look (feed.pid), runs `host` as its child (ended with
//! it, by --parent), starts it again when it closes, and after 3 starts in a minute hands over to the Python feed
//! (glitchwall's own command + "run", GLITCHWALL_PYTHON_FEED=1) and steps aside, so the wallpaper never just stops.
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const DETACHED: u32 = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;   // DETACHED_PROCESS, NEW_PROCESS_GROUP, NO_WINDOW

/// 3 starts within a minute: it can't be kept running
pub fn gave_up(starts: &[Instant], now: Instant) -> bool {
    starts.len() >= 3 && now.duration_since(starts[starts.len() - 3]) < Duration::from_secs(60)
}

fn pid_file() -> PathBuf { crate::server::state_dir().join("feed.pid") }

fn arg(args: &[String], name: &str) -> Option<String> { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned()) }

pub fn run(args: &[String]) -> Result<(), String> {
    let me = std::process::id();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(crate::server::state_dir());
    std::fs::write(pid_file(), me.to_string()).map_err(|e| format!("feed.pid: {e}"))?;
    let rest = &args[2..];   // host's own options, passed on as they are
    // noted for `config` and `reset`, which start it again the same way (cli.rs)
    let mut noted = String::from("[");
    for (i, a) in rest.iter().enumerate() { if i > 0 { noted.push_str(", "); } crate::settings::ascii_str(a, &mut noted); }
    let _ = std::fs::write(crate::server::state_dir().join("run-args.json"), noted + "]");
    let mut starts: Vec<Instant> = Vec::new();
    loop {
        let now = Instant::now();
        if gave_up(&starts, now) { break; }
        starts.push(now);
        let child = Command::new(&exe).arg("host").args(["--parent", &me.to_string()]).args(rest).creation_flags(DETACHED).spawn();
        match child {
            Ok(mut c) => { let _ = c.wait(); }
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
    // the Python feed and windows instead (server.py's run_feed): it writes its own pid
    if std::fs::read_to_string(pid_file()).is_ok_and(|p| p.trim() == me.to_string()) { let _ = std::fs::remove_file(pid_file()); }
    let base: Vec<String> = arg(rest, "--glitchwall").and_then(|c| crate::json::Json::parse(&c).ok())
        .map(|j| j.items().iter().filter_map(|v| v.str().map(String::from)).collect()).unwrap_or_default();
    if let Some((cmd, more)) = base.split_first() {
        let _ = Command::new(cmd).args(more).arg("run").env("GLITCHWALL_PYTHON_FEED", "1").creation_flags(DETACHED).spawn();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_starts_in_a_minute() {
        let t = Instant::now();
        let s = |secs: &[u64]| secs.iter().map(|x| t + Duration::from_secs(*x)).collect::<Vec<_>>();
        assert!(!gave_up(&s(&[0, 1]), t + Duration::from_secs(2)));
        assert!(gave_up(&s(&[0, 1, 2]), t + Duration::from_secs(3)));
        assert!(!gave_up(&s(&[0, 1, 2]), t + Duration::from_secs(61)));
        assert!(!gave_up(&s(&[0, 100, 101]), t + Duration::from_secs(102)));   // only the last three count
        assert!(gave_up(&s(&[0, 100, 101, 102]), t + Duration::from_secs(103)));
    }
}
