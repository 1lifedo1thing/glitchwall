//! `glitchwall-render doctor [--yes]` (piece 4d) and the check `install` runs first: prereqs.py's checks (Windows
//! build, .NET Framework, the WebView2 runtime, Claude Code's logs) with the same lines. Like prereqs.py it never
//! downloads anything: for what's missing it can only open Microsoft's download page (with --yes).
use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Registry::*;

const WEBVIEW2_PAGE: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
const DOTNET_PAGE: &str = "https://dotnet.microsoft.com/download/dotnet-framework/net481";
const DOTNET_472: u32 = 461808;   // the .NET Framework 4.7.2 release number; the settings window needs it

pub struct Check { pub name: &'static str, pub ok: bool, pub detail: String, pub required: bool, fix: Option<(&'static str, &'static str)>, hint: String }

fn reg_sz(hive: HKEY, path: &str, value: &str) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut n = (buf.len() * 2) as u32;
    let ok = unsafe { RegGetValueW(hive, &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_SZ, None,
                                   Some(buf.as_mut_ptr().cast()), Some(&mut n)) }.is_ok();
    ok.then(|| String::from_utf16_lossy(&buf[..(n as usize / 2).saturating_sub(1)]))
}

fn reg_dword(hive: HKEY, path: &str, value: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut n = 4u32;
    let ok = unsafe { RegGetValueW(hive, &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_DWORD, None,
                                   Some((&mut v as *mut u32).cast()), Some(&mut n)) }.is_ok();
    ok.then_some(v)
}

pub fn windows_build() -> u32 {
    reg_sz(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber").and_then(|b| b.parse().ok()).unwrap_or(0)
}

pub fn dotnet_release() -> u32 { reg_dword(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full", "Release").unwrap_or(0) }

pub fn webview2_version() -> Option<String> {
    let guid = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    [(HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{guid}")),
     (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{guid}")),
     (HKEY_CURRENT_USER, format!(r"Software\Microsoft\EdgeUpdate\Clients\{guid}"))]
        .into_iter().find_map(|(h, p)| reg_sz(h, &p, "pv").filter(|v| !v.is_empty() && v != "0.0.0.0"))
}

pub fn claude_logs() -> Option<PathBuf> {
    let base = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|d| !d.is_empty()).map(|d| PathBuf::from(d.replace('/', "\\")))
        .unwrap_or_else(|| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".claude"));
    let p = base.join("projects");
    p.is_dir().then_some(p)
}

/// a web page in the default browser
pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL); }
}

/// prereqs.checks without its Python and pywebview lines (this exe needs neither)
pub fn checks(mode: &str) -> Vec<Check> {
    let mut out = Vec::new();
    let b = windows_build();
    out.push(Check { name: "Windows", ok: b >= 17134, detail: format!("build {b} (needs Windows 10 1803 or newer)"), required: true,
                     fix: None, hint: "update Windows".into() });
    let rel = dotnet_release();
    out.push(Check { name: ".NET Framework 4.7.2+", ok: rel >= DOTNET_472, required: true,
                     detail: if rel >= DOTNET_472 { "present".into() } else { "missing: glitchwall's own desktop windows need it (built into Windows 10 1803 and later)".into() },
                     fix: Some(("Open Microsoft's .NET Framework 4.8.1 download page?", DOTNET_PAGE)), hint: DOTNET_PAGE.into() });
    let wv = webview2_version();
    out.push(Check { name: "WebView2 runtime", ok: wv.is_some(), required: true,
                     detail: wv.unwrap_or_else(|| "missing: the engine that draws the wallpaper behind your icons".into()),
                     fix: Some(("Open Microsoft's WebView2 download page? (Install the Evergreen runtime, then run install again.)", WEBVIEW2_PAGE)),
                     hint: WEBVIEW2_PAGE.into() });
    if mode == "live" {
        let logs = claude_logs();
        out.push(Check { name: "Claude Code logs", ok: logs.is_some(), required: false, fix: None, hint: String::new(),
                         detail: logs.map(|p| p.display().to_string()).unwrap_or_else(|| "not found: live mode shows nothing until you use Claude Code here. Ambient mode works without it (--mode ambient)".into()) });
    }
    out
}

/// prereqs.preflight: list the checks, offer the download page for what's missing (yes only with --yes: a windowed
/// exe can't ask at the keyboard), say what's still missing. True if everything required is there.
pub fn preflight(mode: &str, yes: bool) -> bool {
    let found = checks(mode);
    println!("checking what glitchwall needs:");
    for c in &found {
        println!("  {} {}: {}", if c.ok { "ok " } else if !c.required { "-- " } else { "XX " }, c.name, c.detail);
    }
    for c in found.iter().filter(|c| !c.ok) {
        let Some((prompt, url)) = c.fix else { continue };
        if yes {
            println!("{prompt} yes (--yes)");
            open_url(url);
            println!("  {}: still missing", c.name);   // until the person installs it
        } else {
            println!("{prompt} skipped (not interactive; --yes says yes to these)");
        }
    }
    let mut ok = true;
    for c in found.iter().filter(|c| !c.ok && c.required) {
        ok = false;
        println!("missing: {}. {}", c.name, if c.hint.is_empty() { String::new() } else { format!("To fix: {}", c.hint) });
    }
    ok
}

/// `doctor [--yes]`
pub fn doctor(mode: &str, yes: bool) -> i32 {
    let ok = preflight(mode, yes);
    println!("{}", if ok { "everything glitchwall needs is here" } else { "" });
    if ok { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_machine() {
        assert!(windows_build() >= 17134);
        assert!(checks("ambient").iter().all(|c| c.name != "Claude Code logs"));
        assert_eq!(checks("live").len(), 4);
    }
}
