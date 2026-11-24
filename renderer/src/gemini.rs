//! Read Gemini CLI session logs and turn tool calls into wallpaper events: a port of glitchwall's gemini_logs.py,
//! kept to the same events line for line (tests/fixtures/gemini.json, recorded from the Python, is the yardstick).
//!
//! Gemini CLI writes one JSON record per line to ~/.gemini/tmp/<project>/chats/session-*.jsonl (subagents in
//! chats/<parent session>/<id>.jsonl). A message is written again in full each time it changes; a tool call first
//! appears "scheduled" with its args and again once finished, with its status and result. Late results can also
//! come as "$patch" records. Files are opened read-only, only appended data is read, nothing is written there.
//! See docs/other-agents.md.
#![allow(dead_code)]

use crate::json::Json;
use crate::logs::{clip, dumps, py_splitlines, py_str, py_strip, truthy, when, Event, ACTIVE, FIRST_TAIL, MAX_OUT_LINES};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DONE: [&str; 3] = ["success", "error", "cancelled"];
static EMPTY: Json = Json::Obj(Vec::new());

pub fn logs_root() -> PathBuf {
    match std::env::var_os("GEMINI_CLI_HOME") {
        Some(b) if !b.is_empty() => PathBuf::from(b).join(".gemini"),
        _ => PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".gemini"),
    }.join("tmp")
}

fn list(v: &Json) -> &[Json] { v.items() }

/// A Gemini CLI tool call -> (kind, text) as it should appear on the wall. kind None = skip.
pub fn describe(name: &str, args: &Json) -> (Option<&'static str>, String) {
    let g = |k: &str| match args.get(k) { Json::Str(s) => s.clone(), Json::Null => String::new(), v => dumps(v) };
    match name {
        "run_shell_command" => (Some("$"), g("command")),
        "read_file" => (Some("read"), g("file_path")),
        "read_many_files" => (Some("read"), g("include")),
        "list_directory" => (Some("read"), g("dir_path")),
        "write_file" => (Some("write"), g("file_path") + "\n" + &clip(&g("content"), 600)),
        "replace" => (Some("edit"), g("file_path") + "\n- " + &clip(&g("old_string"), 240) + "\n+ " + &clip(&g("new_string"), 360)),
        "glob" | "grep_search" => { let p = g("dir_path"); (Some("find"), g("pattern") + &if p.is_empty() { p } else { "  in ".to_string() + &p }) }
        "google_web_search" => (Some("web"), g("query")),
        "web_fetch" => (Some("web"), clip(&g("prompt"), 200)),
        "invoke_agent" => {
            let t = [g("description"), g("name")].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| "subagent".into());
            (Some("agent"), t)
        }
        "write_todos" | "update_topic" | "ask_user" | "enter_plan_mode" | "exit_plan_mode" | "activate_skill" | "get_internal_docs" => (None, String::new()),
        _ if name.starts_with("tracker_") => (None, String::new()),
        _ => (Some("tool"), name.to_string() + " " + &clip(&dumps(args), 200)),
    }
}

/// <tmp>/<project>/chats/[<parent>/]<session>.jsonl: the project folder is a short name or a long hash.
pub fn project_label(f: &Path) -> String {
    let in_chats = f.parent().and_then(Path::file_name).is_some_and(|n| n == "chats");
    let d = f.ancestors().nth(if in_chats { 2 } else { 3 });
    let name = d.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.len() >= 32 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return "gemini".into(); }
    let last = name.rsplit('-').next().unwrap_or("");
    if last.is_empty() { "gemini".into() } else { last.to_string() }
}

/// What a finished tool call printed: the UI's text if it has one, else the function response.
fn result_text(tc: &Json) -> String {
    let mut out = tc.get("resultDisplay").str().unwrap_or("").to_string();
    if out.is_empty() {
        let r = tc.get("result");
        let parts: &[Json] = if let Json::Arr(a) = r { a } else { std::slice::from_ref(r) };   // a list of parts, or one
        let mut texts: Vec<&str> = Vec::new();
        for p in parts {
            match p {
                Json::Str(s) => texts.push(s),
                Json::Obj(_) => {
                    if let Some(t) = p.get("text").str() { texts.push(t); }
                    let resp = p.get("functionResponse").get("response");
                    if matches!(resp, Json::Obj(_)) {
                        for k in ["output", "error"] { if let Some(t) = resp.get(k).str() { texts.push(t); } }
                    }
                }
                _ => {}
            }
        }
        out = texts.join("\n");
    }
    let mut out = py_strip(&out).to_string();
    if out.is_empty() { out = "(no output)".into(); }
    let lines = py_splitlines(&out);
    if lines.len() > MAX_OUT_LINES {
        out = lines[..MAX_OUT_LINES].join("\n") + &format!("\n… {} more lines", lines.len() - MAX_OUT_LINES);
    }
    clip(&out, 900)
}

fn id_of(tc: &Json) -> String { let i = tc.get("id"); if truthy(i) { py_str(i) } else { String::new() } }

/// Follows Gemini CLI's logs and hands each new tool call, and each finished one, to emit.
pub struct Tailer {
    pub root: PathBuf,
    offsets: HashMap<PathBuf, u64>,
    seen: HashSet<String>,   // "u"+id once the call is shown, "r"+id once its result is
}

impl Tailer {
    pub fn new(root: PathBuf) -> Tailer { Tailer { root, offsets: HashMap::new(), seen: HashSet::new() } }

    fn send(&mut self, key: String, tc: &Json, f: &Path, kind: &str, r: &str, text: String, emit: &mut impl FnMut(Event)) {
        if self.seen.contains(&key) || py_strip(&text).is_empty() { return; }
        self.seen.insert(key);
        if self.seen.len() > 5000 { self.seen.clear(); }
        let ts = tc.get("timestamp");
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let tail: Vec<char> = r.chars().collect();
        emit(Event {
            ts: if ts.is_null() { String::new() } else { py_str(ts) },
            proj: project_label(f),
            session: stem.rsplit('-').next().unwrap_or("").chars().take(4).collect(),
            side: !f.parent().and_then(Path::file_name).is_some_and(|n| n == "chats"),
            kind: kind.to_string(),
            r#ref: tail[tail.len().saturating_sub(12)..].iter().collect(),
            text,
        });
    }

    fn result(&mut self, tc: &Json, r: &str, f: &Path, emit: &mut impl FnMut(Event)) {
        let kind = if matches!(tc.get("status").str(), Some("error") | Some("cancelled")) { "err" } else { "out" };
        self.send(format!("r{r}"), tc, f, kind, r, result_text(tc), emit);
    }

    fn tool_call(&mut self, tc: &Json, f: &Path, emit: &mut impl FnMut(Event)) {
        let r = id_of(tc);
        let name = tc.get("name");
        let args = match tc.get("args") { a @ Json::Obj(_) => a, _ => &EMPTY };
        let (kind, text) = describe(&if truthy(name) { py_str(name) } else { String::new() }, args);
        let Some(kind) = kind else { return };
        if r.is_empty() { return; }
        self.send(format!("u{r}"), tc, f, kind, &r, text, emit);
        let has = |k: &str| tc.entries().iter().any(|(n, _)| n == k);
        if tc.get("status").str().is_some_and(|s| DONE.contains(&s)) && (has("result") || has("resultDisplay")) {
            self.result(tc, &r, f, emit);
        }
    }

    pub fn parse_line(&mut self, line: &[u8], f: &Path, newer_than: Option<f64>, emit: &mut impl FnMut(Event)) {
        let Ok(text) = std::str::from_utf8(line) else { return };
        let Ok(d) = Json::parse(text) else { return };
        if !matches!(d, Json::Obj(_)) { return; }
        let patch = d.get("$patch");
        if matches!(patch, Json::Obj(_)) {
            // a late result: {"toolCalls": [{"id", "result"}]} or {"updates": [{"toolCalls": [...]}]}
            let groups = std::iter::once(patch).chain(list(patch.get("updates")).iter().filter(|u| matches!(u, Json::Obj(_))));
            for grp in groups {
                for tc in list(grp.get("toolCalls")) {
                    let r = if matches!(tc, Json::Obj(_)) { id_of(tc) } else { String::new() };
                    if !r.is_empty() && self.seen.contains(&format!("u{r}")) { self.result(tc, &r, f, emit); }
                }
            }
            return;
        }
        if d.get("type").str() != Some("gemini") { return; }
        if let Some(t) = newer_than {
            let ts = d.get("timestamp");
            if when(&if ts.is_null() { String::new() } else { py_str(ts) }) < t { return; }
        }
        for tc in list(d.get("toolCalls")) {
            if matches!(tc, Json::Obj(_)) { self.tool_call(tc, f, emit); }
        }
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let jsonl = |p: &Path| p.extension().is_some_and(|x| x == "jsonl");
        let Ok(projects) = std::fs::read_dir(&self.root) else { return out };
        for p in projects.flatten() {
            let Ok(chats) = std::fs::read_dir(p.path().join("chats")) else { continue };
            for e in chats.flatten() {
                let f = e.path();
                if f.is_dir() {
                    let Ok(sub) = std::fs::read_dir(&f) else { continue };
                    out.extend(sub.flatten().map(|e| e.path()).filter(|f| jsonl(f) && f.is_file()));
                } else if jsonl(&f) {
                    out.push(f);
                }
            }
        }
        out
    }

    pub fn scan(&mut self, emit: &mut impl FnMut(Event)) {
        let now = SystemTime::now();
        for f in self.files() {
            let Ok(st) = std::fs::metadata(&f) else { continue };
            if !st.is_file() { continue; }
            let age = st.modified().ok().and_then(|m| now.duration_since(m).ok()).map_or(0.0, |d| d.as_secs_f64());
            if age > ACTIVE && !self.offsets.contains_key(&f) { continue; }
            let size = st.len();
            let first = !self.offsets.contains_key(&f);
            let newer_than = first.then(|| now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() - ACTIVE);
            let mut pos = self.offsets.get(&f).copied().unwrap_or(size.saturating_sub(FIRST_TAIL));
            if size < pos { pos = 0; }   // rewritten in place (Gemini CLI does that on some resumes)
            if size == pos { self.offsets.insert(f, pos); continue; }
            let mut chunk = Vec::new();
            let read = std::fs::File::open(&f)   // read-only
                .and_then(|mut h| { h.seek(SeekFrom::Start(pos))?; h.read_to_end(&mut chunk) });
            if read.is_err() { continue; }
            let Some(last_nl) = chunk.iter().rposition(|&b| b == b'\n') else { continue };
            self.offsets.insert(f.clone(), pos + last_nl as u64 + 1);
            for line in chunk[..last_nl].split(|&b| b == b'\n') { self.parse_line(line, &f, newer_than, emit); }
        }
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

    fn path_of(j: &Json) -> PathBuf { j.items().iter().map(|p| p.str().unwrap()).collect() }

    /// Every stream recorded from gemini_logs.py gives the same events here, and every folder the same label.
    #[test]
    fn matches_python() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/gemini.json");
        // the fake keys carry an invisible marker in the file (tools/fixture_text.py): taken out before use
        let fx = Json::parse(&std::fs::read_to_string(path).unwrap().replace('\u{e000}', "")).unwrap();
        for (k, st) in fx.get("streams").items().iter().enumerate() {
            let f = path_of(st.get("path"));
            let mut t = Tailer::new(PathBuf::from("."));
            let mut got = Vec::new();
            for line in st.get("lines").items() {
                t.parse_line(line.str().unwrap().as_bytes(), &f, st.get("newer_than").num(), &mut |e| got.push(e));
            }
            let want: Vec<Event> = st.get("events").items().iter().map(event_of).collect();
            for (i, (g, w)) in got.iter().zip(&want).enumerate() { assert_eq!(g, w, "stream {k}, event {i}"); }
            assert_eq!(got.len(), want.len(), "stream {k}: event count");
        }
        for l in fx.get("labels").items() {
            let f = path_of(l.get("path"));
            assert_eq!(project_label(&f), l.get("out").str().unwrap(), "{}", f.display());
        }
    }

    /// test_gemini_logs.py's tailer test, on a real folder: follows appends, waits for a whole line, survives a rewrite.
    #[test]
    fn follows_a_folder() {
        let root = std::env::temp_dir().join(format!("gw-gemini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let chats = root.join("proj").join("chats");
        std::fs::create_dir_all(chats.join("parent")).unwrap();
        let (f, sub) = (chats.join("session-a.jsonl"), chats.join("parent").join("s-b.jsonl"));
        let rec = |id: &str, p: &str| format!(r#"{{"type": "gemini", "timestamp": "{}", "toolCalls": [{{"id": "{id}", "name": "read_file", "args": {{"file_path": "{p}"}}}}]}}"#,
                                              "2999-01-01T00:00:00Z");
        std::fs::write(&f, rec("k1", "one") + "\n").unwrap();
        std::fs::write(&sub, rec("k9", "side") + "\n").unwrap();
        let mut t = Tailer::new(root.clone());
        let mut ev = Vec::new();
        t.scan(&mut |e| ev.push(e));
        use std::io::Write;
        let mut h = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        h.write_all((rec("k2", "two") + "\n" + &rec("k3", "half")).as_bytes()).unwrap();
        drop(h);
        t.scan(&mut |e| ev.push(e));
        std::fs::write(&f, rec("k1", "one") + "\n").unwrap();   // shrank: read again from the start, nothing twice
        t.scan(&mut |e| ev.push(e));
        let _ = std::fs::remove_dir_all(&root);
        let mut texts: Vec<(&str, bool)> = ev.iter().map(|e| (e.text.as_str(), e.side)).collect();
        texts.sort();
        assert_eq!(texts, [("one", false), ("side", true), ("two", false)]);
    }
}
