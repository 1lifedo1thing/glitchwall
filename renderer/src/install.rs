//! `glitchwall-render install|uninstall` (piece 4c): what cli.cmd_install / cmd_uninstall and platforms.py do, with
//! the autostart entry pointing straight at this exe's `run` (supervise.rs) instead of `pythonw -m glitchwall run`.
//! It touches only: the state folder, one HKCU Run value, the Start menu shortcut and the desktop right-click entry.
//! GLITCHWALL_REG_ROOT (a key under HKCU, for tests) moves every registry write under that key; the Start menu
//! follows APPDATA, as in platforms.py.
use crate::cli::{self, list2cmdline};
use crate::json::Json;
use crate::server::state_dir;
use crate::settings::{self as cfg, Schema};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::{HSTRING, Interface, PCWSTR};
use windows::Win32::System::Registry::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "glitchwall";
const MENU_KEY: &str = r"Software\Classes\DesktopBackground\Shell\glitchwall";
const EXE: &str = "glitchwall-render.exe";

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str) }

// ---------------------------------------------------------------- registry (HKCU only)
fn hkcu(sub: &str) -> String {
    match std::env::var("GLITCHWALL_REG_ROOT") { Ok(r) if !r.is_empty() => format!("{r}\\{sub}"), _ => sub.to_string() }
}

/// sets string values in HKCU\sub, creating the key; ("", v) is the key's default value
fn reg_set(sub: &str, values: &[(&str, &str)]) -> bool {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(hkcu(sub)), 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE,
                           KEY_SET_VALUE, None, &mut k, None).is_err() { return false; }
        let mut ok = true;
        for (name, v) in values {
            let w: Vec<u16> = v.encode_utf16().chain([0]).collect();
            let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
            ok &= RegSetValueExW(k, &HSTRING::from(*name), 0, REG_SZ, Some(bytes)).is_ok();
        }
        let _ = RegCloseKey(k);
        ok
    }
}

fn reg_delete_value(sub: &str, name: &str) -> bool {
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(hkcu(sub)), 0, KEY_SET_VALUE, &mut k).is_err() { return false; }
        let ok = RegDeleteValueW(k, &HSTRING::from(name)).is_ok();
        let _ = RegCloseKey(k);
        ok
    }
}

fn reg_delete_key(sub: &str) -> bool { unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(hkcu(sub))).is_ok() } }

// ---------------------------------------------------------------- where things are
/// The Python glitchwall this exe ships in (`<tool>\Lib\site-packages\glitchwall\bin\` -> `<tool>\Scripts\pythonw.exe`):
/// its command, as platforms.live_command gives it. None for a standalone exe.
fn python_glitchwall(exe: &Path) -> Option<Vec<String>> {
    let root = exe.parent()?.parent()?.parent()?.parent()?.parent()?;
    let w = root.join("Scripts").join("pythonw.exe");
    w.is_file().then(|| vec![w.to_string_lossy().into_owned(), "-m".into(), "glitchwall".into()])
}

/// glitchwall's own command: --glitchwall JSON, else the Python one this exe ships in
fn glitchwall_command(args: &[String], exe: &Path) -> Option<Vec<String>> {
    match arg(args, "--glitchwall") {
        Some(c) => Json::parse(c).ok().map(|j| j.items().iter().filter_map(|v| v.str().map(String::from)).collect()),
        None => python_glitchwall(exe),
    }
}

fn bin_dir() -> PathBuf { state_dir().join("bin") }

fn shortcut_file() -> Option<PathBuf> {
    let a = std::env::var("APPDATA").ok().filter(|a| !a.is_empty())?;
    Some(PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs\glitchwall settings.lnk"))
}

fn json_list(items: &[String]) -> String {
    let mut o = String::from("[");
    for (i, a) in items.iter().enumerate() { if i > 0 { o.push_str(", "); } cfg::ascii_str(a, &mut o); }
    o + "]"
}

/// the `run` command autostart and install start: host reads the port from the settings each time
pub fn run_command(exe: &Path, screens: &str, glitchwall: Option<&[String]>, wallpaper: &Path) -> Vec<String> {
    let mut c = vec![exe.to_string_lossy().into_owned(), "run".into(), "--which".into(), screens.into()];
    if let Some(g) = glitchwall { c.extend(["--glitchwall".into(), json_list(g)]); }
    c.extend(["--wallpaper".into(), wallpaper.to_string_lossy().into_owned()]);
    c
}

// ---------------------------------------------------------------- the pieces
fn add_shortcut(cmd: &[String], icon: &Path) -> Option<PathBuf> {
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    let f = shortcut_file()?;
    let _ = std::fs::create_dir_all(f.parent()?);
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.SetPath(&HSTRING::from(cmd[0].as_str())).ok()?;
        link.SetArguments(&HSTRING::from(list2cmdline(&cmd[1..]))).ok()?;
        link.SetDescription(&HSTRING::from("glitchwall settings")).ok()?;
        link.SetWorkingDirectory(&HSTRING::from(home)).ok()?;
        link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0).ok()?;
        link.cast::<IPersistFile>().ok()?.Save(&HSTRING::from(f.as_os_str()), true).ok()?;
    }
    f.exists().then_some(f)
}

fn add_context_menu(cmd: &[String], icon: &Path) -> bool {
    let icon = format!("{},0", icon.display());
    reg_set(MENU_KEY, &[("", "glitchwall settings"), ("MUIVerb", "glitchwall settings"), ("Icon", &icon), ("Position", "Bottom")])
        && reg_set(&format!(r"{MENU_KEY}\command"), &[("", &list2cmdline(cmd))])
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

/// how many of our wallpaper windows are showing (winhost.shown_screens)
fn shown_windows() -> usize {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let mut b = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut b) } as usize;
        if String::from_utf16_lossy(&b[..n]) == "glitchwall-render" && unsafe { IsWindowVisible(h) }.as_bool() {
            unsafe { *(l.0 as *mut usize) += 1; }
        }
        BOOL(1)
    }
    let mut n = 0usize;
    unsafe { let _ = EnumChildWindows(GetDesktopWindow(), Some(each), LPARAM(&mut n as *mut usize as isize)); }
    n
}

// ---------------------------------------------------------------- the commands
/// `install [--port N] [--commands-only] [--mode M] [--screens all|primary] [--no-autostart] [--yes] [--no-start]
/// [--glitchwall JSON] [--wallpaper DIR]`
pub fn install(schema: &Schema, wallpaper: &Path, args: &[String]) -> i32 {
    let Ok(me) = std::env::current_exe() else { return 1 };
    let mut s = cli::load(schema);
    let Json::Obj(o) = &mut s else { return 1 };
    if let Some(p) = arg(args, "--port") {
        let Ok(p) = p.parse::<u16>() else { eprintln!("glitchwall install: --port: a whole number, not {p:?}"); return 2 };
        cfg::set(o, "port", Json::Num(p as f64));
    }
    if args.iter().any(|a| a == "--commands-only") { cfg::set(o, "commands_only", Json::Bool(true)); }
    if let Some(m) = arg(args, "--mode") {
        if !cfg::MODES.iter().any(|(k, _)| *k == m) { eprintln!("glitchwall install: --mode: live, ambient or art"); return 2; }
        cfg::set(o, "mode", Json::Str(m.into()));
    }
    let screens = arg(args, "--screens").unwrap_or("all");
    if !matches!(screens, "all" | "primary") { eprintln!("glitchwall install: --screens: all or primary"); return 2; }
    cfg::set(o, "screens", Json::Str(screens.into()));
    if let Err(e) = cli::save(schema, &s) { eprintln!("glitchwall install: {e}"); return 1; }
    if cli::make_key().is_none() { eprintln!("glitchwall install: can't write the feed key"); return 1; }
    let mode = cfg::mode(&s);
    println!("mode: {mode} ({})", cfg::MODES.iter().find(|(k, _)| *k == mode).map_or("", |m| m.1));
    println!("state dir: {}", state_dir().display());
    if !crate::doctor::preflight(&mode, args.iter().any(|a| a == "--yes" || a == "-y")) {   // piece 4d
        println!("install stopped: fix what's missing above, then run install again (glitchwall doctor re-checks)");
        return 1;
    }

    // where it runs from: in place when it ships inside the Python glitchwall (upgrades replace it there), else a
    // copy in the state folder, so autostart doesn't depend on the Downloads folder (platforms.install_binaries)
    let glitchwall = glitchwall_command(args, &me);
    let standalone = python_glitchwall(&me).is_none();
    let (exe, wp) = if standalone { (bin_dir().join(EXE), bin_dir().join("wallpaper")) } else { (me.clone(), wallpaper.to_path_buf()) };
    let run = run_command(&exe, screens, glitchwall.as_deref(), &wp);

    if args.iter().any(|a| a == "--no-autostart") {
        println!("autostart: skipped (--no-autostart)");
    } else if reg_set(RUN_KEY, &[(RUN_VALUE, &list2cmdline(&run))]) {
        println!("autostart: HKCU\\{RUN_KEY}\\{RUN_VALUE}");
    } else {
        println!("autostart: could not add it");
    }
    match &glitchwall {
        Some(g) => {
            let settings: Vec<String> = g.iter().cloned().chain(["settings".to_string()]).collect();
            let icon = wallpaper.join("glitchwall.ico");
            let lnk = add_shortcut(&settings, &icon);
            println!("Start menu: {}", if lnk.is_some() { "glitchwall settings" } else { "could not add the settings shortcut" });
            println!("desktop right-click menu: {}", if add_context_menu(&settings, &icon) { "glitchwall settings" } else { "could not add it" });
        }
        None => println!("Start menu and desktop right-click menu: skipped (the settings window needs glitchwall's Python part for now)"),
    }

    // (re)start it so it uses the current port and code
    cli::stop();
    std::thread::sleep(Duration::from_millis(500));
    if standalone && exe != me {
        let _ = std::fs::create_dir_all(bin_dir());
        match std::fs::copy(&me, &exe) {
            Ok(_) => println!("copied {EXE} to {}", bin_dir().display()),
            Err(e) => { eprintln!("glitchwall install: can't copy {EXE}: {e}"); return 1; }
        }
        let _ = std::fs::remove_dir_all(&wp);
        if let Err(e) = copy_tree(wallpaper, &wp) { eprintln!("glitchwall install: can't copy the wallpaper files: {e}"); return 1; }
        println!("copied wallpaper to {}", bin_dir().display());
    }
    if args.iter().any(|a| a == "--no-start") { return 0; }   // for tests and packagers
    if cli::launch(schema, Some(run[1..].to_vec()), &exe) != 0 { return 1; }

    let want = if screens == "primary" { 1 } else { crate::desk::monitors().len() };
    let mut shown = 0;
    for _ in 0..40 {
        shown = shown_windows();
        if shown >= want { break; }
        std::thread::sleep(Duration::from_millis(250));
    }
    let n = crate::desk::monitors().len();
    if shown > 0 {
        println!("showing on {shown} of {n} screen{}, behind the desktop icons", if n > 1 { "s" } else { "" });
    } else {
        println!("the feed is running but no wallpaper window appeared yet; run `glitchwall status` in a moment");
    }
    println!();
    if standalone {
        println!("done. Commands from now on:  \"{}\" pause | resume | status | uninstall", exe.display());
        println!("You can delete the downloaded copy.");
    } else {
        println!("done. Stream-safe switch:  glitchwall pause   /   glitchwall resume");
    }
    println!("Settings window: glitchwall settings  (also in the Start menu)");
    0
}

/// `uninstall`: stop it and remove exactly what install added
pub fn uninstall() -> i32 {
    let mut removed: Vec<String> = Vec::new();
    if cli::stop() { removed.push("stopped the feed".into()); }
    if reg_delete_value(RUN_KEY, RUN_VALUE) { removed.push("removed autostart entry".into()); }
    if shortcut_file().is_some_and(|f| std::fs::remove_file(f).is_ok()) { removed.push("removed the Start menu shortcut".into()); }
    if reg_delete_key(&format!(r"{MENU_KEY}\command")) & reg_delete_key(MENU_KEY) {
        removed.push("removed glitchwall settings from the desktop right-click menu".into());
    }
    let sd = state_dir();
    let programs = bin_dir().exists();
    if sd.exists() {
        std::thread::sleep(Duration::from_millis(300));   // let a stopped run let go of feed.pid and its logs
        let _ = std::fs::remove_dir_all(&sd);
        if sd.exists() { delete_later(&sd); }   // this exe is running from sd\bin
        removed.push(format!("deleted {} (key, settings, logs{})", sd.display(), if programs { ", programs" } else { "" }));
    }
    if removed.is_empty() { removed.push("nothing to remove".into()); }
    for r in removed { println!("{r}"); }
    let me = std::env::current_exe().unwrap_or_default();
    println!("Your agents' logs were never modified.{}",
             if python_glitchwall(&me).is_some() { " To remove the program itself: uv tool uninstall glitchwall" } else { "" });
    0
}

/// remove a folder after this process exits (a running exe can't delete itself): platforms.delete_later
fn delete_later(path: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd")
        .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", path.display()))
        .creation_flags(0x0800_0000 | 0x0000_0008).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_runs_the_exe() {
        let g = vec![r"C:\t\Scripts\pythonw.exe".to_string(), "-m".into(), "glitchwall".into()];
        let c = run_command(Path::new(r"C:\a b\glitchwall-render.exe"), "all", Some(&g), Path::new(r"C:\w"));
        assert_eq!(list2cmdline(&c), r#""C:\a b\glitchwall-render.exe" run --which all --glitchwall "[\"C:\\t\\Scripts\\pythonw.exe\", \"-m\", \"glitchwall\"]" --wallpaper C:\w"#);
    }

    #[test]
    fn registry_writes_stay_under_the_test_root() {
        let root = format!(r"Software\glitchwall-test-{}", std::process::id());
        unsafe { std::env::set_var("GLITCHWALL_REG_ROOT", &root); }
        assert!(reg_set(RUN_KEY, &[(RUN_VALUE, "x")]));
        assert!(reg_delete_value(RUN_KEY, RUN_VALUE));
        assert!(!reg_delete_value(RUN_KEY, RUN_VALUE));
        unsafe { let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(root.as_str())); }
        unsafe { std::env::remove_var("GLITCHWALL_REG_ROOT"); }
    }
}
