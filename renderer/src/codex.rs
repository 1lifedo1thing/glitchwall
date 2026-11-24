//! Read Codex CLI session logs ("rollouts") and turn tool calls into wallpaper events: a port of glitchwall's
//! codex_logs.py, kept to the same events line for line (tests/fixtures/codex.json, recorded from the Python, is
//! the yardstick).
//!
//! Codex CLI appends one JSON record per line to $CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl
//! (CODEX_HOME defaults to ~/.codex). Each line is {"timestamp", "type", "payload"}; tool calls are "response_item"
//! payloads and their results separate lines with the same call_id. Compressed (.jsonl.zst) finished sessions are
//! never read. Files are opened read-only, only appended data is read, nothing is written there.
//! See docs/other-agents.md.
#![allow(dead_code)]

use crate::json::Json;
use crate::logs::{clip, dumps, py_splitlines, py_str, py_strip, truthy, when, Event, MAX_OUT_LINES};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const HIDDEN: [&str; 5] = ["update_plan", "write_stdin", "request_permissions", "request_user_input", "tool_search"];
const SHELL_FLAGS: [&str; 6] = ["-c", "-lc", "-Command", "-command", "/c", "/C"];
const HEADER: [&str; 8] = ["Chunk ID: ", "Wall time", "Process exited with code ", "Process running with session ID ",
                           "Original token count: ", "Total output lines: ", "Exit code: ", "Script "];
const QUOTES: [char; 3] = ['"', '\'', '`'];
const RUNNING: &str = "Script running with cell ID ";
static EMPTY: Json = Json::Obj(Vec::new());

pub fn logs_root() -> PathBuf {
    match std::env::var_os("CODEX_HOME") {
        Some(b) if !b.is_empty() => PathBuf::from(b),
        _ => PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".codex"),
    }.join("sessions")
}

/// "" for null, the string itself, else str() of the value.
fn s_of(v: &Json) -> String { match v { Json::Null => String::new(), Json::Str(s) => s.clone(), v => py_str(v) } }

/// The value as text if it is truthy, else "" (Python's `str(v or "")`).
fn or_empty(v: &Json) -> String { if truthy(v) { py_str(v) } else { String::new() } }

/// A command given as an argv list: the script itself when it is `bash -lc "<script>"` and the like.
pub fn shell_text(argv: &Json) -> String {
    match argv {
        Json::Str(s) => s.clone(),
        Json::Arr(a) => {
            let a: Vec<String> = a.iter().map(s_of).collect();
            if a.len() >= 3 && SHELL_FLAGS.contains(&a[a.len() - 2].as_str()) { a[a.len() - 1].clone() } else { a.join(" ") }
        }
        _ => String::new(),
    }
}

/// apply_patch's input, shown as the files it touches and its diff lines.
pub fn patch_text(p: &str) -> String {
    let heads = [("*** Update File: ", ""), ("*** Add File: ", "new: "), ("*** Delete File: ", "deleted: "), ("*** Move to: ", "moved to: ")];
    let mut out: Vec<String> = Vec::new();
    for line in py_splitlines(p) {
        if matches!(line, "*** Begin Patch" | "*** End Patch" | "*** End of File") { continue; }
        out.push(heads.iter().find_map(|(h, show)| line.strip_prefix(h).map(|r| show.to_string() + r)).unwrap_or_else(|| line.to_string()));
    }
    clip(&out.join("\n"), 600)
}

fn is_hex(s: &str) -> bool { s.bytes().all(|b| b.is_ascii_hexdigit()) }

/// The JavaScript string literal starting at src[i] (a quote), unescaped; None if it never closes.
pub fn js_string(src: &str, i: usize) -> Option<String> {
    let b = src.as_bytes();
    let q = b[i];
    let (mut out, mut i) = (String::new(), i + 1);
    while i < b.len() {
        let c = src[i..].chars().next()?;
        if c as u32 == q as u32 { return Some(out); }
        if c == '\\' && i + 1 < b.len() {
            let e = src[i + 1..].chars().next()?;
            i += 1 + e.len_utf8();
            let simple = match e { 'n' => Some("\n"), 't' => Some("\t"), 'r' => Some("\r"), 'b' => Some("\u{8}"), 'f' => Some("\u{c}"),
                                   'v' => Some("\u{b}"), '0' => Some("\0"), '\n' => Some(""), _ => None };
            if let Some(t) = simple { out.push_str(t); continue; }
            let mut h: Option<u32> = None;
            if e == 'u' && b.get(i) == Some(&b'{') {
                if let Some(rel) = src[i..].find('}') {
                    let close = i + rel;
                    let inner = &src[i + 1..close];
                    if close - i > 1 && inner.chars().count() + 1 <= 7 && is_hex(inner) {
                        h = u32::from_str_radix(inner, 16).ok();
                        i = close + 1;
                    }
                }
            } else if e == 'u' || e == 'x' {
                let w = if e == 'u' { 4 } else { 2 };
                if let Some(t) = src.get(i..i + w) {
                    if is_hex(t) { h = u32::from_str_radix(t, 16).ok(); i += w; }
                }
            }
            match h {
                Some(h) => out.push(char::from_u32(h).unwrap_or('\u{fffd}')),
                None => out.push(e),
            }
            continue;
        }
        out.push(c);
        i += c.len_utf8();
    }
    None
}

/// The cmd strings a code-mode script passes to tools.exec_command({cmd: "..."}).
pub fn script_commands(src: &str) -> Vec<String> {
    let (mut out, mut pos, needle) = (Vec::new(), 0, "tools.exec_command(");
    let b = src.as_bytes();
    while let Some(rel) = src[pos..].find(needle) {
        let start = pos + rel + needle.len();
        let end = src[start..].find("tools.").map_or(src.len(), |r| start + r);
        let mut k = start;
        while let Some(rc) = src[k..end].find("cmd") {
            let c = k + rc;
            let mut a = c + 3;
            if a < end && (b[a] == b'"' || b[a] == b'\'') { a += 1; }
            while a < end && (b[a] == b' ' || b[a] == b'\t') { a += 1; }
            if a < end && b[a] == b':' {
                a += 1;
                while a < end && matches!(b[a], b' ' | b'\t' | b'\r' | b'\n') { a += 1; }
                if a < end && QUOTES.contains(&(b[a] as char)) {
                    if let Some(lit) = js_string(src, a) { out.push(lit); break; }
                }
            }
            k = c + 3;
        }
        pos = start;
    }
    out
}

/// An apply_patch text written into a code-mode script, if there is one.
pub fn script_patch(src: &str) -> Option<String> {
    let i = src.find("*** Begin Patch")?;
    if i > 0 && QUOTES.contains(&(src.as_bytes()[i - 1] as char)) {
        if let Some(lit) = js_string(src, i - 1) { return Some(lit); }
    }
    Some(match src[i..].find("*** End Patch") { Some(e) => src[i..i + e + "*** End Patch".len()].to_string(), None => src[i..].to_string() })
}

/// Codex's code mode: one `exec` tool whose input is a short JavaScript calling tools.exec_command,
/// tools.apply_patch and the like. Shown as the edit and the commands it runs; anything else as the script.
pub fn describe_script(src: &str) -> (&'static str, String) {
    let cmds: Vec<String> = script_commands(src).iter().map(|c| clip(c, 300)).collect();
    if let Some(patch) = script_patch(src) {
        let mut t = patch_text(&patch);
        if !cmds.is_empty() { t += &("\n".to_string() + &cmds.iter().map(|c| format!("$ {c}")).collect::<Vec<_>>().join("\n")); }
        return ("edit", t);
    }
    if !cmds.is_empty() { return ("$", cmds.join("\n")); }
    ("tool", "exec ".to_string() + &clip(src, 200))
}

/// A Codex function call -> (kind, text) as it should appear on the wall. kind None = skip.
pub fn describe(name: &str, args: &Json, raw: &str) -> (Option<&'static str>, String) {
    let g = |k: &str| match args.get(k) { Json::Str(s) => s.clone(), Json::Null => String::new(), v => dumps(v) };
    match name {
        "exec_command" => (Some("$"), g("cmd")),
        "shell" => (Some("$"), shell_text(args.get("command"))),
        "shell_command" => (Some("$"), g("command")),
        "apply_patch" => (Some("edit"), patch_text(&g("input"))),
        "view_image" => (Some("read"), g("path")),
        _ if HIDDEN.contains(&name) => (None, String::new()),
        _ => (Some("tool"), name.to_string() + " " + &clip(&if args.entries().is_empty() { raw.to_string() } else { dumps(args) }, 200)),
    }
}

pub fn project_label(cwd: &Json) -> String {
    let Some(c) = cwd.str() else { return "codex".into() };
    let name = c.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("");
    let last = name.rsplit('-').next().unwrap_or("");
    if last.is_empty() { "codex".into() } else { last.to_string() }
}

fn joined(v: &Json) -> String {
    match v {
        Json::Arr(a) => a.iter().filter_map(|p| p.get("text").str()).collect::<Vec<_>>().join("\n"),
        Json::Str(s) => s.clone(),
        _ => String::new(),
    }
}

/// The cell id when a code-mode script is still running (its output comes later, from a `wait` call).
pub fn running_cell(v: &Json) -> Option<String> {
    let t = joined(v);
    let first = t.split('\n').next().unwrap_or("");
    first.strip_prefix(RUNNING).map(py_strip).filter(|c| !c.is_empty()).map(str::to_string)
}

/// A tool's output as the wall shows it, and whether it failed (a nonzero exit code).
pub fn output_text(v: &Json) -> (String, bool) {
    let mut text = joined(v);
    let mut failed: Option<bool> = None;
    let d = if text.starts_with('{') { Json::parse(&text).ok() } else { None };
    if let Some(d @ Json::Obj(_)) = &d {
        if let Some(o) = d.get("output").str() {   // older shell: {"output", "metadata": {"exit_code"}}
            failed = d.get("metadata").get("exit_code").num().map(|c| c != 0.0);
            text = o.to_string();
        }
    }
    let old_form = matches!(&d, Some(d @ Json::Obj(_)) if d.get("output").str().is_some());
    if !old_form {   // exec_command: a few header lines, then "Output:" and the output
        let lines: Vec<&str> = text.split('\n').collect();
        let mut i = 0;
        while i < lines.len() && HEADER.iter().any(|h| lines[i].starts_with(h)) {
            for h in ["Process exited with code ", "Exit code: "] {
                if let Some(r) = lines[i].strip_prefix(h) {
                    let digits = r.strip_prefix('-').unwrap_or(r);
                    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) { failed = Some(digits.bytes().any(|b| b != b'0')); }
                }
            }
            i += 1;
        }
        if i > 0 && i < lines.len() && lines[i].starts_with("Output:") {
            text = std::iter::once(&lines[i]["Output:".len()..]).chain(lines[i + 1..].iter().copied()).collect::<Vec<_>>().join("\n");
        } else {
            failed = None;
        }
    }
    let mut out = py_strip(&text).to_string();
    if out.is_empty() { out = "(no output)".into(); }
    let ls = py_splitlines(&out);
    if ls.len() > MAX_OUT_LINES {
        out = ls[..MAX_OUT_LINES].join("\n") + &format!("\n… {} more lines", ls.len() - MAX_OUT_LINES);
    }
    (clip(&out, 900), failed == Some(true))
}

/// Follows Codex CLI's rollouts and hands each new tool call, and each result, to emit.
pub struct Tailer {
    pub root: PathBuf,
    follow: crate::tail::Follow,
    seen: HashSet<String>,   // "u"+id once the call is shown, "r"+id once its result is
    proj: HashMap<PathBuf, String>,
    side: HashMap<PathBuf, bool>,
    cells: HashMap<(PathBuf, String), String>,   // a code-mode script still running: its cell -> its call id
    waits: HashMap<String, String>,              // a `wait` call -> the call whose output it brings
}

impl Tailer {
    pub fn new(root: PathBuf) -> Tailer {
        Tailer { root, follow: crate::tail::Follow::new(), seen: HashSet::new(), proj: HashMap::new(), side: HashMap::new(), cells: HashMap::new(), waits: HashMap::new() }
    }

    fn send(&mut self, key: String, d: &Json, f: &Path, kind: &str, r: &str, text: String, emit: &mut impl FnMut(Event)) {
        if self.seen.contains(&key) || py_strip(&text).is_empty() { return; }
        self.seen.insert(key);
        if self.seen.len() > 5000 { self.seen.clear(); }
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let tail: Vec<char> = r.chars().collect();
        emit(Event {
            ts: s_of(d.get("timestamp")),
            proj: self.proj.get(f).cloned().unwrap_or_else(|| "codex".into()),
            session: stem.rsplit('-').next().unwrap_or("").chars().take(4).collect(),
            side: self.side.get(f).copied().unwrap_or(false),
            kind: kind.to_string(),
            r#ref: tail[tail.len().saturating_sub(12)..].iter().collect(),
            text,
        });
    }

    pub fn parse_line(&mut self, line: &[u8], f: &Path, newer_than: Option<f64>, emit: &mut impl FnMut(Event)) {
        let Ok(text) = std::str::from_utf8(line) else { return };
        let Ok(d) = Json::parse(text) else { return };
        let p = d.get("payload");
        if !matches!(d, Json::Obj(_)) || !matches!(p, Json::Obj(_)) { return; }
        let typ = d.get("type").str();
        if matches!(typ, Some("session_meta") | Some("turn_context")) {
            if p.get("cwd").str().is_some() { self.proj.insert(f.to_path_buf(), project_label(p.get("cwd"))); }
            if typ == Some("session_meta") {
                let src = p.get("source");
                let sub = truthy(p.get("parent_thread_id")) || src.entries().iter().any(|(n, _)| n == "subagent");
                self.side.insert(f.to_path_buf(), sub);
            }
            return;
        }
        if typ != Some("response_item") { return; }
        if let Some(t) = newer_than { if when(&s_of(d.get("timestamp"))) < t { return; } }
        let mut r = or_empty(p.get("call_id"));
        let (kind, text): (Option<&str>, String) = match p.get("type").str() {
            Some("function_call") => {
                let raw = p.get("arguments").str().unwrap_or("");
                let parsed = Json::parse(raw).ok();
                let args = match &parsed { Some(a @ Json::Obj(_)) => a, _ => &EMPTY };
                let name = or_empty(p.get("name"));
                if name == "wait" {   // code mode polling a running script: its output belongs to that script's call
                    if let Some(target) = self.cells.get(&(f.to_path_buf(), s_of(args.get("cell_id")))).cloned() {
                        if !r.is_empty() {
                            self.waits.insert(r, target);
                            if self.waits.len() > 5000 { self.waits.clear(); }
                        }
                    }
                    return;
                }
                describe(&name, args, raw)
            }
            Some("custom_tool_call") => {
                let (name, inp) = (or_empty(p.get("name")), p.get("input").str().unwrap_or(""));
                if name == "apply_patch" { (Some("edit"), patch_text(inp)) }
                else if name == "exec" { let (k, t) = describe_script(inp); (Some(k), t) }
                else { (Some("tool"), name + " " + &clip(inp, 200)) }
            }
            Some("local_shell_call") => {
                if r.is_empty() { r = or_empty(p.get("id")); }
                (Some("$"), shell_text(p.get("action").get("command")))
            }
            Some("web_search_call") => {
                let a = p.get("action");
                let text = match a.get("type").str() {
                    Some("search") => match a.get("query") {
                        Json::Str(q) if !q.is_empty() => q.clone(),
                        _ => a.get("queries").items().iter().filter_map(Json::str).collect::<Vec<_>>().join(" | "),
                    },
                    Some("open_page") => s_of(a.get("url")),
                    Some("find_in_page") => s_of(a.get("pattern")) + &if truthy(a.get("url")) { "  in ".to_string() + &s_of(a.get("url")) } else { String::new() },
                    _ => String::new(),
                };
                r = or_empty(p.get("id"));
                if r.is_empty() { r = s_of(d.get("timestamp")) + &text; }
                (Some("web"), text)
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                let target = if !r.is_empty() && self.seen.contains(&format!("u{r}")) { Some(r.clone()) } else { self.waits.get(&r).cloned() };
                if let Some(target) = target.filter(|t| self.seen.contains(&format!("u{t}"))) {
                    if let Some(cell) = running_cell(p.get("output")) {   // not done yet: a `wait` call brings the output later
                        self.cells.insert((f.to_path_buf(), cell), target);
                        if self.cells.len() > 5000 { self.cells.clear(); }
                        return;
                    }
                    let (text, failed) = output_text(p.get("output"));
                    self.send(format!("r{target}"), &d, f, if failed { "err" } else { "out" }, &target, text, emit);
                }
                return;
            }
            _ => return,
        };
        if let Some(kind) = kind {
            if !r.is_empty() { self.send(format!("u{r}"), &d, f, kind, &r, text, emit); }
        }
    }

    /// sessions/YYYY/MM/DD/*.jsonl: all of them, or the two newest days' (the others once a minute).
    fn files(&self, _now: f64, everything: bool) -> Vec<PathBuf> {
        if !everything {
            return crate::tail::newest_dirs(&self.root, 3, 2).iter().flat_map(|d| crate::tail::jsonl_in(d)).collect();
        }
        let mut dirs = vec![self.root.clone()];
        for _ in 0..3 {
            dirs = dirs.iter().filter_map(|d| std::fs::read_dir(d).ok())
                .flat_map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>()).collect();
        }
        dirs.iter().flat_map(|d| crate::tail::jsonl_in(d)).collect()
    }

    pub fn scan(&mut self, emit: &mut impl FnMut(Event)) {
        let now = crate::tail::now_secs();
        let mut follow = std::mem::replace(&mut self.follow, crate::tail::Follow::new());
        let everything = follow.due(now);
        let files = self.files(now, everything);
        follow.scan(files, everything, now, &mut |line: &[u8], f: &Path, newer_than: Option<f64>| self.parse_line(line, f, newer_than, emit));
        self.follow = follow;
    }

    /// Scan once a second until stop is set.
    pub fn run(&mut self, stop: &AtomicBool, emit: &mut impl FnMut(Event)) {
        while !stop.load(Ordering::Relaxed) {
            self.scan(emit);
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_of(j: &Json) -> Event {
        let s = |k| j.get(k).str().unwrap().to_string();
        Event { ts: s("ts"), proj: s("proj"), session: s("session"), side: j.get("side").truthy(), kind: s("kind"), r#ref: s("ref"), text: s("text") }
    }

    /// Every stream recorded from codex_logs.py gives the same events here; labels and shell commands too.
    #[test]
    fn matches_python() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/codex.json");
        // the fake keys carry an invisible marker in the file (tools/fixture_text.py): taken out before use
        let fx = Json::parse(&std::fs::read_to_string(path).unwrap().replace('\u{e000}', "")).unwrap();
        for (k, st) in fx.get("streams").items().iter().enumerate() {
            let f: PathBuf = st.get("path").items().iter().map(|p| p.str().unwrap()).collect();
            let mut t = Tailer::new(PathBuf::from("."));
            let mut got = Vec::new();
            for line in st.get("lines").items() {
                t.parse_line(line.str().unwrap().as_bytes(), &f, st.get("newer_than").num(), &mut |e| got.push(e));
            }
            let want: Vec<Event> = st.get("events").items().iter().map(event_of).collect();
            for (i, (g, w)) in got.iter().zip(&want).enumerate() { assert_eq!(g, w, "stream {k}, event {i}"); }
            assert_eq!(got.len(), want.len(), "stream {k}: event count");
        }
        for l in fx.get("labels").items() { assert_eq!(project_label(l.get("cwd")), l.get("out").str().unwrap()); }
        for s in fx.get("shells").items() { assert_eq!(shell_text(s.get("argv")), s.get("out").str().unwrap()); }
    }

    /// test_codex_logs.py's tailer test, on a real folder: follows appends, skips .zst, survives a rewrite.
    #[test]
    fn follows_a_folder() {
        let root = std::env::temp_dir().join(format!("gw-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let day = root.join("2999").join("01").join("02");
        std::fs::create_dir_all(&day).unwrap();
        let f = day.join("rollout-x-aaaa.jsonl");
        let rec = |id: &str, c: &str| format!(r#"{{"timestamp": "2999-01-02T00:00:00Z", "type": "response_item", "payload": {{"type": "function_call", "name": "exec_command", "arguments": "{{\"cmd\": \"{c}\"}}", "call_id": "{id}"}}}}"#);
        std::fs::write(&f, rec("k1", "one") + "\n").unwrap();
        std::fs::write(day.join("rollout-old.jsonl.zst"), rec("z", "compressed") + "\n").unwrap();
        let mut t = Tailer::new(root.clone());
        let mut ev = Vec::new();
        t.scan(&mut |e| ev.push(e));
        use std::io::Write;
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        h.write_all((rec("k2", "two") + "\n" + &rec("k3", "half")).as_bytes()).unwrap();
        drop(h);
        t.scan(&mut |e| ev.push(e));
        std::fs::write(&f, rec("k1", "one") + "\n").unwrap();
        t.scan(&mut |e| ev.push(e));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(ev.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["one", "two"]);
    }
}
