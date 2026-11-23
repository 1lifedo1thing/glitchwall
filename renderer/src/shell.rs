//! Commands you type in PowerShell, and what they print, as wallpaper events: a port of shell_history.py's
//! ShellTailer. glitchwall's terminal hook (shell-hook.ps1) writes one JSON line per command when you press Enter,
//! and lines with its output (live ones while it runs, a final one when it is done), to
//! <state dir>/shell/<process id>.jsonl. This follows those files read-only, only what is appended, and nothing older
//! than the feed: a file that was there before it started is read from its end. Redaction happens in the store.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::json::Json;
use crate::logs::{clip, output_text, py_splitlines, py_str, py_strip};

pub const STALE: Duration = Duration::from_secs(24 * 3600);   // a shell's file this long untouched is cleared out
type Ev = Vec<(String, Json)>;

/// A running command's output as the wall shows it: its newest 14 lines (a finished one shows its first ones).
pub fn last_lines(text: &str) -> String {
    let lines = py_splitlines(py_strip(text));
    let n = 14;
    let out = lines[lines.len().saturating_sub(n)..].join("\n");
    let before = if lines.len() > n { format!("… {} lines before\n", lines.len() - n) } else { String::new() };
    clip(&(before + &out), 900)
}

pub struct ShellTailer {
    pub root: PathBuf, offsets: HashMap<PathBuf, u64>, pub started: SystemTime,
    cmds: Vec<(String, String)>,                            // WSL commands waiting for their output: (ref, text)
    pub screen_output: fn(&str, &str) -> Option<String>,   // (command, prompt) -> what it printed (tests swap it)
}

impl ShellTailer {
    pub fn new(root: PathBuf) -> ShellTailer { ShellTailer { root, offsets: HashMap::new(), started: SystemTime::now(), cmds: Vec::new(), screen_output: output_of } }

    /// One pass over the files. While enabled is false it keeps up with them but shows nothing (nothing piles up).
    pub fn scan(&mut self, enabled: bool, emit: &mut impl FnMut(Ev)) {
        let Ok(dir) = std::fs::read_dir(&self.root) else { return };
        let now = SystemTime::now();
        for e in dir.flatten() {
            let f = e.path();
            if f.extension().is_none_or(|x| x != "jsonl") { continue; }
            let Ok(st) = std::fs::metadata(&f) else { continue };
            if st.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age > STALE) {   // that shell is long gone
                let _ = std::fs::remove_file(&f);
                self.offsets.remove(&f);
                continue;
            }
            let size = st.len();
            let mut pos = match self.offsets.get(&f) {
                Some(&p) => p,
                // first sight: a shell started since the feed is read whole, an older one from its end
                None => if st.created().is_ok_and(|c| c + Duration::from_secs(1) >= self.started) { 0 } else { size },
            };
            if size < pos { pos = 0; }   // the hook cut it back
            if size == pos { self.offsets.insert(f, pos); continue; }
            let mut chunk = Vec::new();
            let read = std::fs::File::open(&f).and_then(|mut h| { h.seek(SeekFrom::Start(pos))?; h.read_to_end(&mut chunk) });
            if read.is_err() { continue; }
            let Some(nl) = chunk.iter().rposition(|&b| b == b'\n') else { continue };
            self.offsets.insert(f.clone(), pos + nl as u64 + 1);
            if !enabled { continue; }
            let shell = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            for line in chunk[..nl].split(|&b| b == b'\n') {
                let (cmds, read) = (&mut self.cmds, self.screen_output);
                parse(line, &shell, &mut |mut ev: Ev| {
                    wsl_output(&mut ev, cmds, read);
                    emit(ev)
                });
            }
        }
    }
}

/// A WSL command is remembered; when its output record comes, what it printed is read off its window's screen.
fn wsl_output(ev: &mut Ev, cmds: &mut Vec<(String, String)>, read: fn(&str, &str) -> Option<String>) {
    let get = |k: &str| ev.iter().find(|(x, _)| x == k).and_then(|(_, v)| v.str()).map(str::to_string);
    if get("proj").as_deref() != Some("wsl") { return; }
    let (r, kind, prompt) = (get("ref").unwrap_or_default(), get("kind").unwrap_or_default(), get("prompt"));
    ev.retain(|(k, _)| k != "prompt");
    if kind == "$" {
        cmds.push((r, get_text(ev)));
        if cmds.len() > 50 { cmds.remove(0); }
    } else if let Some(i) = cmds.iter().position(|(x, _)| *x == r) {
        let (_, cmd) = cmds.remove(i);
        if let Some(out) = read(&cmd, &prompt.unwrap_or_default())
            && let Some(t) = ev.iter_mut().find(|(k, _)| k == "text") { t.1 = Json::Str(out); }
    }
}

fn get_text(ev: &Ev) -> String { ev.iter().find(|(k, _)| k == "text").and_then(|(_, v)| v.str()).unwrap_or("").to_string() }

/// One record of the hook's -> an event: "PS>" for a command, out/err for its output (live while it runs).
pub fn parse(line: &[u8], shell: &str, emit: &mut impl FnMut(Ev)) {
    let text = String::from_utf8_lossy(line);
    let Ok(d) = Json::parse(text.trim_start_matches('\u{feff}')) else { return };
    let Json::Obj(m) = &d else { return };
    let get = |k: &str| m.iter().rev().find(|(x, _)| x == k).map(|(_, v)| v);
    let s = |k: &str| get(k).map(py_str).unwrap_or_default();   // str(d.get(k, ""))
    let t = get("t").and_then(Json::str);
    if t == Some("cmd") && py_strip(&s("text")).is_empty() { return; }
    let id = get("id").map(py_str).unwrap_or_else(|| "None".into());
    let bash = get("sh").and_then(Json::str) == Some("bash");   // WSL's bash hook (shell-hook.bash)
    let mut ev: Ev = vec![("ts".into(), Json::Str(s("ts"))), ("proj".into(), Json::Str(if bash { "wsl" } else { "terminal" }.into())),
        ("session".into(), Json::Str("you".into())), ("side".into(), Json::Bool(false)), ("ref".into(), Json::Str(format!("sh{shell}-{id}")))];
    match t {
        Some("cmd") => {
            ev.push(("kind".into(), Json::Str(if bash { "$" } else { "PS>" }.into())));
            ev.push(("text".into(), Json::Str(s("text"))));
        }
        Some("out") => {
            let live = get("live").is_some_and(Json::truthy);
            let ok = !matches!(get("ok"), Some(Json::Bool(false)));
            ev.push(("kind".into(), Json::Str(if ok { "out" } else { "err" }.into())));
            ev.push(("live".into(), Json::Bool(live)));
            if bash { ev.push(("prompt".into(), Json::Str(s("prompt")))); }   // for wsl_output, which takes it out again
            let raw = s("text");
            ev.push(("text".into(), Json::Str(if live { last_lines(&raw) } else { output_text(&Json::Str(raw)) })));
        }
        _ => return,
    }
    emit(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct T { dir: PathBuf, t: ShellTailer, got: Vec<Ev> }
    impl T {
        fn new(name: &str) -> T {
            let dir = std::env::temp_dir().join(format!("gw-shell-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            T { t: ShellTailer::new(dir.clone()), dir, got: Vec::new() }
        }
        fn add(&self, f: &str, recs: &[&str]) {
            let mut h = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(f)).unwrap();
            for r in recs { writeln!(h, "{r}").unwrap(); }
        }
        fn scan(&mut self, on: bool) { let got = &mut self.got; self.t.scan(on, &mut |e| got.push(e)); }
        fn field(&self, k: &str) -> Vec<String> {
            self.got.iter().map(|e| e.iter().find(|(x, _)| x == k).and_then(|(_, v)| v.str()).unwrap_or("").to_string()).collect()
        }
    }
    impl Drop for T { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); } }

    // tests/test_shell.py's TestShellTailer, case for case
    #[test]
    fn command_then_its_output() {
        let mut t = T::new("a");
        t.add("100.jsonl", &[r#"{"t": "cmd", "id": 1, "text": "ping -n 1 x"}"#]);
        t.scan(true);
        t.add("100.jsonl", &[r#"{"t": "out", "id": 1, "ok": true, "text": "Reply from x\n"}"#, r#"{"t": "cmd", "id": 2, "text": "bad"}"#,
                             r#"{"t": "out", "id": 2, "ok": false, "text": "not found"}"#]);
        t.scan(true);
        assert_eq!(t.field("kind"), ["PS>", "out", "PS>", "err"]);
        assert_eq!(t.field("text"), ["ping -n 1 x", "Reply from x", "bad", "not found"]);
        assert_eq!(t.field("ref"), ["sh100-1", "sh100-1", "sh100-2", "sh100-2"]);
        assert!(t.field("proj").iter().all(|p| p == "terminal"));
    }

    #[test]
    fn wsl_commands() {
        let mut t = T::new("g");
        t.add("wsl-12.jsonl", &[r#"{"sh": "bash", "t": "cmd", "id": 4, "text": "ls /x"}"#, r#"{"sh": "bash", "t": "out", "id": 4, "ok": false, "text": "exit 2"}"#]);
        t.scan(true);
        assert_eq!(t.field("kind"), ["$", "err"]);
        assert_eq!(t.field("proj"), ["wsl", "wsl"]);
        assert_eq!(t.field("text"), ["ls /x", "exit 2"]);
        assert_eq!(t.field("ref"), ["shwsl-12-4", "shwsl-12-4"]);
    }

    #[test]
    fn wsl_output_from_the_screen() {
        let screen: Vec<String> = ["me$ echo old", "old", "me$ ls /tmp", "a.txt", "b.txt", "me$"].iter().map(|s| s.to_string()).collect();
        assert_eq!(find_output(&screen, "ls /tmp", "me$").as_deref(), Some("a.txt\nb.txt"));
        assert_eq!(find_output(&screen, "nope", "me$"), None);
        let mut t = T::new("h");
        t.t.screen_output = |cmd, prompt| Some(format!("printed by {cmd} before {prompt}"));
        t.add("wsl-1.jsonl", &[r#"{"sh": "bash", "t": "cmd", "id": 1, "text": "ls"}"#,
                               r#"{"sh": "bash", "t": "out", "id": 1, "ok": true, "text": "exit 0", "prompt": "p$"}"#,
                               r#"{"sh": "bash", "t": "out", "id": 9, "ok": false, "text": "exit 1", "prompt": "p$"}"#]);
        t.scan(true);
        assert_eq!(t.field("text"), ["ls", "printed by ls before p$", "exit 1"]);
        assert!(t.got.iter().all(|e| e.iter().all(|(k, _)| k != "prompt")));
    }

    #[test]
    fn empty_output_says_so() {
        let mut t = T::new("b");
        t.add("1.jsonl", &[r#"{"t": "cmd", "id": 1, "text": "cls"}"#, r#"{"t": "out", "id": 1, "ok": true, "text": ""}"#]);
        t.scan(true);
        assert_eq!(t.field("text").last().unwrap(), "(no output)");
    }

    #[test]
    fn older_shell_read_from_its_end() {
        let mut t = T::new("c");
        t.add("7.jsonl", &[r#"{"t": "cmd", "id": 1, "text": "old"}"#]);
        t.t.started = SystemTime::now() + Duration::from_secs(5);   // as if the feed started after this shell wrote
        t.scan(true);
        t.add("7.jsonl", &[r#"{"t": "cmd", "id": 2, "text": "new"}"#]);
        t.scan(true);
        assert_eq!(t.field("text"), ["new"]);
    }

    #[test]
    fn off_shows_nothing_and_saves_nothing_up() {
        let mut t = T::new("d");
        t.add("2.jsonl", &[r#"{"t": "cmd", "id": 1, "text": "hidden"}"#]);
        t.scan(false);
        t.add("2.jsonl", &[r#"{"t": "cmd", "id": 2, "text": "shown"}"#]);
        t.scan(true);
        assert_eq!(t.field("text"), ["shown"]);
    }

    #[test]
    fn partial_line_and_garbage() {
        let mut t = T::new("e");
        std::fs::write(t.dir.join("3.jsonl"), "not json\n{\"t\": \"cmd\", \"id\": 1, \"te").unwrap();
        t.scan(true);
        assert!(t.got.is_empty());
        let mut h = std::fs::OpenOptions::new().append(true).open(t.dir.join("3.jsonl")).unwrap();
        writeln!(h, "xt\": \"done\"}}").unwrap();
        t.scan(true);
        assert_eq!(t.field("text"), ["done"]);
    }

    #[test]
    fn stale_files_cleared() {
        let mut t = T::new("f");
        t.add("9.jsonl", &[r#"{"t": "cmd", "id": 1, "text": "x"}"#]);
        let old = SystemTime::now() - STALE - Duration::from_secs(10);
        std::fs::File::options().write(true).open(t.dir.join("9.jsonl")).unwrap().set_modified(old).unwrap();
        t.scan(true);
        assert!(!t.dir.join("9.jsonl").exists());
    }

    #[test]
    fn live_output_keeps_the_newest_lines() {
        let mut got = Vec::new();
        let text: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let rec = Json::Obj(vec![("t".into(), Json::Str("out".into())), ("id".into(), Json::Num(3.0)), ("live".into(), Json::Bool(true)),
                                 ("text".into(), Json::Str(text.join("\n")))]);
        parse(crate::logs::dumps(&rec).as_bytes(), "5", &mut |e| got.push(e));
        let t = got[0].iter().find(|(k, _)| k == "text").unwrap().1.str().unwrap().to_string();
        assert!(t.starts_with("… 6 lines before\nline 7\n") && t.ends_with("line 20"), "{t}");
        assert!(matches!(got[0].iter().find(|(k, _)| k == "live").unwrap().1, Json::Bool(true)));
    }
}

// ---------------------------------------------------------------- WSL output, off the window's screen (wslscreen.py)

/// The lines a command printed: below its own line(s), above the next prompt. None if it isn't on this screen.
pub fn find_output(screen: &[String], cmd: &str, prompt: &str) -> Option<String> {
    let first = cmd.split('\n').next().unwrap_or("").trim_end();
    if first.is_empty() { return None; }
    let at = (0..screen.len()).rev().find(|&i| screen[i].contains(first))?;   // its latest showing
    let (p, mut out) = (prompt.trim_end(), Vec::new());
    for line in screen.iter().skip(at + cmd.matches('\n').count() + 1) {
        if !prompt.trim().is_empty() && line.trim_end().starts_with(p) { break; }   // the next prompt
        out.push(line.trim_end());
    }
    Some(out.join("\n").trim_matches('\n').to_string())
}

/// Each open WSL window's screen, read by a copy of this exe (`glitchwall-render wslscreen`) so the feed itself never
/// attaches to a console. Empty when there is none or it fails.
pub fn screens() -> Vec<Vec<String>> {
    use std::os::windows::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return Vec::new() };
    let Ok(out) = std::process::Command::new(exe).arg("wslscreen").creation_flags(0x0800_0000).output() else { return Vec::new() };
    match Json::parse(&String::from_utf8_lossy(&out.stdout)) {
        Ok(Json::Arr(a)) => a.iter().map(|s| s.items().iter().filter_map(|l| l.str().map(str::to_string)).collect()).collect(),
        _ => Vec::new(),
    }
}

pub fn output_of(cmd: &str, prompt: &str) -> Option<String> { screens().iter().find_map(|s| find_output(s, cmd, prompt)) }

/// `glitchwall-render wslscreen`: attach to each wsl.exe's console in turn, read the screen (read-only), print JSON.
pub fn wslscreen_main() {
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleScreenBufferInfo, ReadConsoleOutputCharacterW, CONSOLE_SCREEN_BUFFER_INFO, COORD};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let mut pids = Vec::new();
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut pe).is_ok();
            while ok {
                let n = pe.szExeFile.iter().position(|&c| c == 0).unwrap_or(pe.szExeFile.len());
                if String::from_utf16_lossy(&pe.szExeFile[..n]).eq_ignore_ascii_case("wsl.exe") { pids.push(pe.th32ProcessID); }
                ok = Process32NextW(snap, &mut pe).is_ok();
            }
            let _ = CloseHandle(snap);
        }
    }
    let mut seen: Vec<Vec<String>> = Vec::new();
    for pid in pids {
        unsafe {
            let _ = FreeConsole();
            if AttachConsole(pid).is_err() { continue; }
            if let Ok(h) = CreateFileW(windows::core::w!("CONOUT$"), GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) {
                let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
                if GetConsoleScreenBufferInfo(h, &mut info).is_ok() {
                    let (w, end) = (info.dwSize.X.max(1) as usize, info.dwCursorPosition.Y);
                    let mut lines = Vec::new();
                    for y in (end - 400).max(0)..=end {
                        let mut buf = vec![0u16; w];
                        let mut n = 0u32;
                        let _ = ReadConsoleOutputCharacterW(h, &mut buf, COORD { X: 0, Y: y }, &mut n);
                        lines.push(String::from_utf16_lossy(&buf[..n as usize]).trim_end().to_string());
                    }
                    if !seen.contains(&lines) { seen.push(lines); }   // a window's wsl.exe processes share one console
                }
                let _ = CloseHandle(h);
            }
            let _ = FreeConsole();
        }
    }
    let j = Json::Arr(seen.into_iter().map(|s| Json::Arr(s.into_iter().map(Json::Str).collect())).collect());
    use std::io::Write;
    let _ = std::io::stdout().write_all(crate::logs::dumps(&j).as_bytes());
}
