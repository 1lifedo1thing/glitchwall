//! glitchwall's typed commands in Rust (cli.py, piece 4b): `config`, `themes`, `reset`, `export`, printing what the
//! Python ones print. Plus what they need: whether glitchwall is running (feed.pid), stopping it, and starting it
//! again the way it was last started (supervise.rs notes its options in run-args.json).
use crate::json::Json;
use crate::server::state_dir;
use crate::settings::{self as cfg, Schema};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// "on", "yes", "1", "true" and their opposites, as cli.py's ON_OFF
fn on_off(v: &str) -> Option<bool> {
    match v.to_lowercase().as_str() { "on" | "true" | "yes" | "1" => Some(true), "off" | "false" | "no" | "0" => Some(false), _ => None }
}

/// a value as Python's str() shows it
pub fn py(v: &Json) -> String {
    match v {
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Json::Num(n) => format!("{n}"),
        Json::Str(s) => s.clone(),
        Json::Null => "None".into(),
        Json::Arr(_) | Json::Obj(_) => "...".into(),
    }
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str) }

fn settings_file() -> PathBuf { state_dir().join("settings.json") }

pub fn load(schema: &Schema) -> Json { schema.load(cfg::read_saved(&settings_file()).as_ref()) }

pub fn save(schema: &Schema, s: &Json) -> Result<(), String> {
    let _ = std::fs::create_dir_all(state_dir());
    cfg::private_write(&settings_file(), &schema.file_text(s)).map_err(|e| format!("settings.json: {e}"))
}

// ---------------------------------------------------------------- running or not
fn pid_file() -> PathBuf { state_dir().join("feed.pid") }

/// the pid in feed.pid, if that process is alive and is glitchwall (python or glitchwall-*): platforms.running_pid
pub fn running_pid() -> Option<u32> {
    let pid: u32 = std::fs::read_to_string(pid_file()).ok()?.trim().parse().ok()?;
    let name = process_name(pid)?.to_lowercase();
    (name.contains("python") || name.contains("glitchwall")).then_some(pid)
}

fn process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::*;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        let mut code = 0u32;
        if !ok { return None; }
        let h2 = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let alive = GetExitCodeProcess(h2, &mut code).is_ok() && code == 259;   // STILL_ACTIVE
        let _ = windows::Win32::Foundation::CloseHandle(h2);
        alive.then(|| String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().unwrap_or("").to_string())
    }
}

pub fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).is_ok()
}

/// platforms.stop_background: end the process in feed.pid (its host follows within 2 s). True if it was running.
pub fn stop() -> bool {
    use windows::Win32::System::Threading::*;
    let Some(pid) = running_pid() else { return false };
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) else { return false };
        let ok = TerminateProcess(h, 1).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if !ok { return false; }
    }
    let _ = std::fs::remove_file(pid_file());
    true
}

/// cli.cmd_start: `run` with the options it was last started with, and the port from the settings now
pub fn start(schema: &Schema) -> i32 {
    let Ok(exe) = std::env::current_exe() else { return 1 };
    launch(schema, None, &exe)
}

/// start `exe run OPTS` in the background and wait for the feed; OPTS None: the ones noted in run-args.json
pub fn launch(schema: &Schema, opts: Option<Vec<String>>, exe: &Path) -> i32 {
    let port = load(schema).get("port").num().map(|p| p as u16).unwrap_or(cfg::DEFAULT_PORT);
    if let Some(pid) = running_pid() { println!("feed already running (pid {pid}) on 127.0.0.1:{port}"); return 0; }
    if port_open(port) { println!("port {port} is in use by another program. Pick another: glitchwall config --port N"); return 1; }
    let Some(mut opts) = opts.or_else(|| std::fs::read_to_string(state_dir().join("run-args.json")).ok().and_then(|t| Json::parse(&t).ok())
        .map(|j| j.items().iter().filter_map(|v| v.str().map(String::from)).collect::<Vec<_>>())) else {
        println!("start it with: glitchwall start");
        return 1;
    };
    if opts.first().is_some_and(|o| o == "run") { opts.remove(0); }
    if let Some(i) = opts.iter().position(|a| a == "--port") { if i + 1 < opts.len() { opts[i + 1] = port.to_string(); } }
    use std::os::windows::process::CommandExt;
    if std::process::Command::new(exe).arg("run").args(&opts).creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000).spawn().is_err() {
        println!("feed did not come up");
        return 1;
    }
    for _ in 0..40 {
        if port_open(port) { println!("feed running on 127.0.0.1:{port} (pid {})", running_pid().map_or("?".into(), |p| p.to_string())); return 0; }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("feed did not come up; run `glitchwall run` to see the error");
    1
}

/// settings.load_key: the feed key, made (random, private) when there is none yet
pub fn make_key() -> Option<String> {
    let f = state_dir().join("key");
    if let Some(k) = std::fs::read_to_string(&f).ok().map(|k| k.trim().to_string()).filter(|k| k.len() >= 32) { return Some(k); }
    let k = crate::ambient::token_urlsafe(32);
    cfg::private_write(&f, &k).ok().map(|_| k)
}

/// subprocess.list2cmdline: one Windows command line from a list
pub fn list2cmdline(args: &[String]) -> String {
    let mut out = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 { out.push(' '); }
        let quote = a.is_empty() || a.contains(' ') || a.contains('\t');
        if quote { out.push('"'); }
        let mut bs = 0;
        for c in a.chars() {
            match c {
                '\\' => bs += 1,
                '"' => { out.push_str(&"\\".repeat(bs * 2 + 1)); out.push('"'); bs = 0; }
                _ => { out.push_str(&"\\".repeat(bs)); bs = 0; out.push(c); }
            }
        }
        out.push_str(&"\\".repeat(if quote { bs * 2 } else { bs }));
        if quote { out.push('"'); }
    }
    out
}

// ---------------------------------------------------------------- the commands
const TUNE_FLAGS: [(&str, &str); 6] = [("type_speed", "--type-speed"), ("linger", "--linger"), ("cards", "--cards"),
                                        ("intensity", "--intensity"), ("glass", "--glass"), ("grain", "--grain")];

/// `config [--commands-only on|off] [--mask-user ..] [--mask-email ..] [--claude|--gemini|--codex ..] [--label T]
/// [--port N] [--renderer R] [--mode M] [--theme ..] [--palette ..] [--rain ..] [--font ..] [--transition ..]
/// [--type-speed N] ..]`: changes what is given, then lists every setting.
pub fn config(schema: &Schema, args: &[String]) -> i32 {
    let mut s = load(schema);
    let mut changes: Vec<(String, Json)> = Vec::new();
    let flags = [("commands_only", "--commands-only".to_string()), ("mask_user", "--mask-user".into()), ("mask_email", "--mask-email".into())]
        .into_iter().chain(cfg::AGENTS.iter().map(|(k, _, _)| (*k, format!("--{}", k.split('_').nth(1).unwrap_or(k)))));
    for (k, flag) in flags {
        if let Some(v) = arg(args, &flag) {
            let Some(b) = on_off(v) else { eprintln!("glitchwall config: {flag}: on or off, not {v:?}"); return 2 };
            changes.push((k.into(), Json::Bool(b)));
        }
    }
    for k in ["label", "renderer", "mode"] {
        if let Some(v) = arg(args, &format!("--{k}")) { changes.push((k.into(), Json::Str(v.into()))); }
    }
    for (k, _) in &schema.kinds {
        if let Some(v) = arg(args, &format!("--{k}")) { changes.push((k.clone(), Json::Str(v.into()))); }
    }
    let mut ints: Vec<(&str, &str)> = vec![("port", "--port")];
    ints.extend(TUNE_FLAGS);
    for (k, flag) in ints {
        if let Some(v) = arg(args, flag) {
            let Ok(n) = v.parse::<i64>() else { eprintln!("glitchwall config: {flag}: a whole number, not {v:?}"); return 2 };
            changes.push((k.into(), Json::Num(n as f64)));
        }
    }
    if !changes.is_empty() {
        let restart = changes.iter().any(|(k, _)| matches!(k.as_str(), "port" | "renderer" | "mask_user" | "mask_email"));
        let refresh = changes.iter().any(|(k, _)| matches!(k.as_str(), "port" | "label" | "mode"));
        let (new, refused) = schema.apply_changes(&s, &Json::Obj(changes));
        if !refused.is_empty() { eprintln!("glitchwall config: not a valid value for {}", refused.join(", ")); return 2; }
        s = new;
        if let Err(e) = save(schema, &s) { eprintln!("glitchwall config: {e}"); return 1; }
        if refresh { refresh_config(schema, &s); }
        if restart && running_pid().is_some() {
            stop();
            std::thread::sleep(Duration::from_millis(300));
            start(schema);
        }
    }
    for (k, _) in &schema.defaults { println!("{k:14} {}", py(s.get(k))); }
    0
}

/// cli._refresh_config: the installed wallpaper's config.js (macOS/Linux style installs; none on Windows by default)
fn refresh_config(schema: &Schema, s: &Json) {
    let dest = state_dir().join("wallpaper");
    if !dest.exists() { return; }
    let Some(key) = std::fs::read_to_string(state_dir().join("key")).ok().map(|k| k.trim().to_string()) else { return };
    let port = s.get("port").num().unwrap_or(cfg::DEFAULT_PORT as f64) as u16;
    let label = schema.display_label(s);
    if std::fs::write(dest.join("config.js"), cfg::config_js(port, &key, label.str().unwrap_or(""))).is_ok() {
        println!("updated {} (reload the wallpaper to pick it up)", dest.join("config.js").display());
    }
}

/// `themes`: every look and what it is, the current ones starred
pub fn themes(schema: &Schema) -> i32 {
    let look = schema.look(&load(schema));
    for (k, names) in &schema.kinds {
        println!("{k}  (glitchwall config --{k} NAME)");
        let cur = look.get(k).str().unwrap_or("");
        if let Json::Obj(names) = names {
            for (n, about) in names { println!("  {} {n:10} {}", if n == cur { '*' } else { ' ' }, py(about)); }
        }
        println!();
    }
    println!("Changes reach the running wallpaper within a second.");
    println!("Preview any mix: glitchwall demo --open --theme synthwave --rain stars");
    0
}

/// `reset`: every setting back to the default (mode, pause and how it's installed kept), started fresh
pub fn reset(schema: &Schema) -> i32 {
    let s = schema.reset(&load(schema));
    if let Err(e) = save(schema, &s) { eprintln!("glitchwall reset: {e}"); return 1; }
    println!("settings back to the defaults (mode, pause and install choices kept)");
    if running_pid().is_some() {
        if stop() { println!("feed stopped"); }
        let port = s.get("port").num().map(|p| p as u16).unwrap_or(cfg::DEFAULT_PORT);
        for _ in 0..20 { if !port_open(port) { break; } std::thread::sleep(Duration::from_millis(250)); }
    }
    start(schema)
}

/// `export DIR`: the wallpaper and its config.js, for other wallpaper apps
pub fn export(schema: &Schema, wallpaper: &Path, args: &[String]) -> i32 {
    let Some(dir) = args.get(2) else { eprintln!("glitchwall export: which folder?"); return 2 };
    let s = load(schema);
    let Some(key) = std::fs::read_to_string(state_dir().join("key")).ok().map(|k| k.trim().to_string()) else {
        eprintln!("no feed key yet: run glitchwall once to make one");
        return 1;
    };
    let dest = std::path::absolute(dir).unwrap_or_else(|_| PathBuf::from(dir));
    if dest.exists() && std::fs::remove_dir_all(&dest).is_err() { eprintln!("can't replace {}", dest.display()); return 1; }
    if let Err(e) = copy_tree(wallpaper, &dest) { eprintln!("glitchwall export: {e}"); return 1; }
    let port = s.get("port").num().unwrap_or(cfg::DEFAULT_PORT as f64) as u16;
    let label = schema.display_label(&s);
    if let Err(e) = std::fs::write(dest.join("config.js"), cfg::config_js(port, &key, label.str().unwrap_or(""))) {
        eprintln!("glitchwall export: {e}");
        return 1;
    }
    let shown = dest.canonicalize().map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap_or(dest);
    println!("wallpaper written to {}  (config.js holds your feed key: keep it private)", shown.display());
    println!("open: {}", file_uri(&shown.join("index.html")));
    0
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if name == "config.js" || name == "__pycache__" { continue; }
        if e.file_type()?.is_dir() { copy_tree(&e.path(), &to.join(&name))?; } else { std::fs::copy(e.path(), to.join(&name))?; }
    }
    Ok(())
}

/// pathlib's as_uri on Windows: file:///C:/a%20b/index.html
fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) { out.push(b as char) } else { out.push_str(&format!("%{b:02X}")) }
    }
    out
}

/// A windowed exe has no console of its own: print to the terminal it was typed in (pipes need nothing).
pub fn attach_console() {
    use windows::Win32::System::Console::*;
    unsafe { let _ = AttachConsole(ATTACH_PARENT_PROCESS); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_as_python_prints_them() {
        assert_eq!([py(&Json::Bool(true)), py(&Json::Num(47833.0)), py(&Json::Str("a b".into())), py(&Json::Null)],
                   ["True", "47833", "a b", "None"]);
        assert_eq!((on_off("ON"), on_off("no"), on_off("maybe")), (Some(true), Some(false), None));
        assert_eq!(file_uri(Path::new(r"C:\a b\index.html")), "file:///C:/a%20b/index.html");
        let l = |v: &[&str]| list2cmdline(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(l(&[r"C:\a b\x.exe", "", r#"say "hi""#, r"end\", r"sp ace\"]), r#""C:\a b\x.exe" "" "say \"hi\"" end\ "sp ace\\""#);
    }
}
