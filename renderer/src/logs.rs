//! Read Claude Code session logs and turn tool calls into wallpaper events: a port of glitchwall's claude_logs.py,
//! kept to the same events line for line (tests/fixtures/logs.json, recorded from the Python, is the yardstick).
//!
//! Claude Code writes one JSON object per line to ~/.claude/projects/<project>/<session>.jsonl. Files are opened
//! read-only, only appended data is read, and nothing is ever written there.
//!
//! Where Python would have stumbled on a malformed line (an input that isn't an object, say) this reads it as best
//! it can instead; and numbers are kept as floats, so a whole-number float in a tool's input (2.0) shows as 2.
#![allow(dead_code)]

use crate::json::Json;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const ACTIVE: f64 = 30.0 * 60.0;   // only start following logs touched in the last 30 minutes
pub const FIRST_TAIL: u64 = 200_000;   // on first sight of a log, read only its recent tail
pub const MAX_OUT_LINES: usize = 14;

/// One tool call, or its result, as the wall shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Event { pub ts: String, pub proj: String, pub session: String, pub side: bool, pub kind: String, pub r#ref: String, pub text: String }

pub fn logs_root() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(b) if !b.is_empty() => PathBuf::from(b),
        _ => PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".claude"),
    }.join("projects")
}

// ---------------------------------------------------------------- Python's ways with text

/// str.isspace (and so re's \s): Unicode white space, plus the four separators Python counts too.
fn py_space(c: char) -> bool { c.is_whitespace() || ('\x1c'..='\x1f').contains(&c) }

pub(crate) fn py_strip(s: &str) -> &str { s.trim_matches(py_space) }

/// str.splitlines: every kind of line break, \r\n as one.
pub(crate) fn py_splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (b, mut start, mut i) = (s.as_bytes(), 0, 0);
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        let w = c.len_utf8();
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&s[start..i]);
            i += if c == '\r' && b.get(i + 1) == Some(&b'\n') { 2 } else { w };
            start = i;
        } else {
            i += w;
        }
    }
    if start < s.len() { out.push(&s[start..]); }
    out
}

/// A number as Python prints it: whole numbers as ints, the rest as float's repr (1.5, 1e-07, 1e+20).
fn py_num(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e16 { return format!("{}", f as i64); }
    if !f.is_finite() { return if f.is_nan() { "NaN".into() } else if f > 0.0 { "Infinity".into() } else { "-Infinity".into() }; }
    let e = format!("{f:e}");
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if !(-4..16).contains(&exp) { return format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs()); }
    let s = format!("{f}");
    if s.contains('.') { s } else { s + ".0" }
}

/// repr() of a str: single quotes unless it holds one and no double quote.
fn py_repr_str(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut o = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), '\t' => o.push_str("\\t"),
            c if c == q => { o.push('\\'); o.push(c); }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => o.push_str(&format!("\\x{:02x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push(q);
    o
}

fn py_repr(v: &Json) -> String {
    match v {
        Json::Str(s) => py_repr_str(s),
        Json::Arr(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Json::Obj(m) => format!("{{{}}}", m.iter().map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v))).collect::<Vec<_>>().join(", ")),
        other => py_str(other),
    }
}

/// str() of a parsed JSON value.
pub(crate) fn py_str(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Num(n) => py_num(*n),
        Json::Str(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python's json.dumps(v, ensure_ascii=False).
pub(crate) fn dumps(v: &Json) -> String {
    let mut o = String::new();
    dump(v, &mut o);
    o
}

fn dump(v: &Json, o: &mut String) {
    match v {
        Json::Null => o.push_str("null"),
        Json::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => o.push_str(&py_num(*n)),
        Json::Str(s) => dump_str(s, o),
        Json::Arr(a) => {
            o.push('[');
            for (i, x) in a.iter().enumerate() { if i > 0 { o.push_str(", "); } dump(x, o); }
            o.push(']');
        }
        Json::Obj(m) => {
            o.push('{');
            for (i, (k, x)) in m.iter().enumerate() { if i > 0 { o.push_str(", "); } dump_str(k, o); o.push_str(": "); dump(x, o); }
            o.push('}');
        }
    }
}

fn dump_str(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"), '\x08' => o.push_str("\\b"), '\x0c' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

/// The value as Python would use it in a condition.
pub(crate) fn truthy(v: &Json) -> bool {
    match v { Json::Arr(a) => !a.is_empty(), Json::Obj(m) => !m.is_empty(), other => other.truthy() }
}

/// A log line's ISO timestamp as epoch seconds; 0 (= very old) if missing or unreadable. Takes what
/// datetime.fromisoformat takes from a log: a date, then a time with seconds and fractions, then Z or an offset
/// (a time without one is read as UTC, where Python reads it as local time; Claude Code's logs always carry a Z).
pub fn when(ts: &str) -> f64 {
    parse_iso(ts).unwrap_or(0.0)
}

fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |a: usize, n: usize| -> Option<i64> {
        let t = s.get(a..a + n)?;
        if t.bytes().all(|c| c.is_ascii_digit()) { t.parse().ok() } else { None }
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' { return None; }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let dim = [31, if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&mo) || d < 1 || d > dim[mo as usize - 1] { return None; }
    let (mut h, mut mi, mut sec, mut frac, mut off) = (0, 0, 0, 0.0, 0i64);
    let mut i;
    if b.len() > 10 {
        if !s.is_char_boundary(11) { return None; }
        i = 11;   // any one character between the date and the time
        h = num(i, 2)?; i += 2;
        if b.get(i) == Some(&b':') { mi = num(i + 1, 2)?; i += 3;
            if b.get(i) == Some(&b':') { sec = num(i + 1, 2)?; i += 3;
                if matches!(b.get(i), Some(b'.') | Some(b',')) {
                    let st = i + 1;
                    i = st;
                    while i < b.len() && b[i].is_ascii_digit() { i += 1; }
                    if i == st { return None; }
                    let us = &s[st..i.min(st + 6)];   // any number of digits, kept to microseconds
                    frac = us.parse::<f64>().ok()? / 10f64.powi(us.len() as i32);
                }
            }
        }
        if h > 23 || mi > 59 || sec > 59 { return None; }
        match b.get(i) {
            None => {}
            Some(b'Z') if i + 1 == b.len() => {}
            Some(&c) if c == b'+' || c == b'-' => {
                let r = &s[i + 1..];
                let (oh, om) = if r.len() == 5 && r.as_bytes()[2] == b':' { (num(i + 1, 2)?, num(i + 4, 2)?) }
                               else if r.len() == 4 { (num(i + 1, 2)?, num(i + 3, 2)?) } else if r.len() == 2 { (num(i + 1, 2)?, 0) } else { return None };
                if oh > 23 || om > 59 { return None; }
                off = (oh * 3600 + om * 60) * if c == b'-' { -1 } else { 1 };
            }
            _ => return None,
        }
    }
    // days since 1970-01-01, by the civil-from-days algorithm
    let (yy, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + sec - off) as f64 + frac)
}

// ---------------------------------------------------------------- what the wall shows

/// Shorten to at most n characters without cutting a word in two. Redaction runs later, and a key cut in half is
/// too short to be recognized as one, so its first characters would show.
pub fn clip(s: &str, n: usize) -> String {
    let cs: Vec<char> = s.chars().collect();
    if cs.len() <= n { return s.to_string(); }
    let mut cut: Vec<char> = cs[..n].to_vec();
    if !py_space(cs[n]) {
        // re.sub(r"\S+$", "", cut): the run of non-spaces at the end, or just before a final \n
        let end = if cut.last() == Some(&'\n') { cut.len() - 1 } else { cut.len() };
        let mut st = end;
        while st > 0 && !py_space(cut[st - 1]) { st -= 1; }
        cut.drain(st..end);
    }
    let t: String = cut.into_iter().collect();
    t.trim_end_matches(py_space).to_string() + " …"
}

/// A tool call -> (kind, text) as it should appear on the wall. kind None = skip.
pub fn describe(name: &str, inp: &Json) -> (Option<&'static str>, String) {
    let g = |k: &str| { let v = inp.get(k); if truthy(v) { py_str(v) } else { String::new() } };
    match name {
        "Bash" => return (Some("$"), g("command")),
        "PowerShell" => return (Some("PS>"), g("command")),
        "Read" => return (Some("read"), g("file_path")),
        "Write" => return (Some("write"), g("file_path") + "\n" + &clip(&g("content"), 600)),
        "Edit" => return (Some("edit"), g("file_path") + "\n- " + &clip(&g("old_string"), 240) + "\n+ " + &clip(&g("new_string"), 360)),
        "Grep" | "Glob" => { let p = g("path"); return (Some("find"), g("pattern") + &if p.is_empty() { p } else { "  in ".to_string() + &p }); }
        "WebFetch" | "WebSearch" => { let u = g("url"); return (Some("web"), if u.is_empty() { g("query") } else { u }); }
        "Agent" | "Task" => return (Some("agent"), g("description")),
        _ => {}
    }
    if name.contains("update_plan") || name.contains("wrap_up") || name == "ToolSearch" || name == "TodoWrite" { return (None, String::new()); }
    if name.contains("computer") { return (Some("mouse"), g("action") + " " + &g("action_summary")); }
    let short = name.rsplit("__").next().unwrap_or(name);
    (Some("tool"), short.to_string() + " " + &clip(&dumps(inp), 200))
}

/// Folder names are the project path with separators turned into "-"; show the last piece.
pub fn project_label(f: &Path) -> String {
    let dir = f.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let last = dir.trim_end_matches('-').rsplit('-').next().unwrap_or("").to_string();
    if last.is_empty() { "project".into() } else { last }
}

pub(crate) fn output_text(content: &Json) -> String {
    let joined;
    let text: &str = match content {
        Json::Arr(a) => {
            joined = a.iter().filter(|x| x.get("type").str() == Some("text"))
                .map(|x| x.get("text").str().unwrap_or("")).collect::<Vec<_>>().join("\n");
            &joined
        }
        Json::Str(s) => s,
        _ => "",
    };
    let mut out = py_strip(text).to_string();
    if out.is_empty() { out = "(no output)".into(); }
    let lines = py_splitlines(&out);
    if lines.len() > MAX_OUT_LINES {
        out = lines[..MAX_OUT_LINES].join("\n") + &format!("\n… {} more lines", lines.len() - MAX_OUT_LINES);
    }
    clip(&out, 900)
}

// ---------------------------------------------------------------- following the logs

/// Follows the logs and hands each new tool call, and each matching result, to emit.
pub struct Tailer {
    pub root: PathBuf,
    offsets: HashMap<PathBuf, u64>,
    shown: HashSet<String>,   // tool_use ids we displayed, so their results are shown too
    seen: HashSet<String>,    // already emitted (logs can repeat a message)
}

impl Tailer {
    pub fn new(root: PathBuf) -> Tailer { Tailer { root, offsets: HashMap::new(), shown: HashSet::new(), seen: HashSet::new() } }

    pub fn parse_line(&mut self, line: &[u8], f: &Path, newer_than: Option<f64>, emit: &mut impl FnMut(Event)) {
        let Ok(text) = std::str::from_utf8(line) else { return };
        let Ok(d) = Json::parse(text) else { return };
        if !matches!(d, Json::Obj(_)) { return; }
        if let Some(t) = newer_than {
            let ts = d.get("timestamp");
            if when(&if ts.is_null() { String::new() } else { py_str(ts) }) < t { return; }
        }
        let m = d.get("message");
        let Json::Arr(content) = m.get("content") else { return };
        for c in content {
            if !matches!(c, Json::Obj(_)) { continue; }
            let (kind, text, r): (&str, String, String);
            let id = c.get("tool_use_id");
            if c.get("type").str() == Some("tool_result") && id.str().is_some_and(|i| self.shown.contains(i)) {
                r = id.str().unwrap().to_string();
                kind = if truthy(c.get("is_error")) { "err" } else { "out" };
                text = output_text(c.get("content"));
            } else if c.get("type").str() == Some("tool_use") {
                let name = c.get("name");
                let (k, t) = describe(&if name.is_null() { String::new() } else { py_str(name) }, c.get("input"));
                let i = c.get("id");
                r = if truthy(i) { py_str(i) } else { String::new() };
                if k.is_some() && !r.is_empty() { self.shown.insert(r.clone()); }
                let Some(k) = k else { continue };
                (kind, text) = (k, t);
            } else {
                continue;
            }
            if py_strip(&text).is_empty() { continue; }
            let key = (if kind == "out" || kind == "err" { "r" } else { "u" }).to_string() + &r;
            if !self.seen.insert(key) { continue; }
            if self.seen.len() > 5000 { self.seen.clear(); }
            if self.shown.len() > 5000 { self.shown.clear(); }
            let tail: Vec<char> = r.chars().collect();
            let ts = d.get("timestamp");
            emit(Event {
                ts: if ts.is_null() { String::new() } else { py_str(ts) },
                proj: project_label(f),
                session: f.file_stem().map(|s| s.to_string_lossy().chars().take(4).collect()).unwrap_or_default(),
                side: truthy(d.get("isSidechain")),
                kind: kind.to_string(),
                r#ref: tail[tail.len().saturating_sub(12)..].iter().collect(),
                text,
            });
        }
    }

    pub fn scan(&mut self, emit: &mut impl FnMut(Event)) {
        let now = SystemTime::now();
        let Ok(projects) = std::fs::read_dir(&self.root) else { return };
        for p in projects.flatten() {
            let Ok(files) = std::fs::read_dir(p.path()) else { continue };
            for e in files.flatten() {
                let f = e.path();
                if f.extension().is_none_or(|x| x != "jsonl") { continue; }
                let Ok(st) = std::fs::metadata(&f) else { continue };
                if !st.is_file() { continue; }
                let age = st.modified().ok().and_then(|m| now.duration_since(m).ok()).map_or(0.0, |d| d.as_secs_f64());
                if age > ACTIVE && !self.offsets.contains_key(&f) { continue; }
                let size = st.len();
                // first sight (at start, or a session picked up again after a long idle): show only its recent
                // calls, never a burst of old ones (which may be from a time the feed was paused)
                let first = !self.offsets.contains_key(&f);
                let newer_than = first.then(|| now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() - ACTIVE);
                let mut pos = self.offsets.get(&f).copied().unwrap_or(size.saturating_sub(FIRST_TAIL));
                if size < pos { pos = 0; }
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

    /// Every stream recorded from claude_logs.py gives the same events here, and every clip the same text.
    #[test]
    fn matches_python() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/logs.json");
        // the fake keys carry an invisible marker in the file (tools/fixture_text.py): taken out before use
        let fx = Json::parse(&std::fs::read_to_string(path).unwrap().replace('\u{e000}', "")).unwrap();
        let f = Path::new(fx.get("project").str().unwrap()).join(fx.get("file").str().unwrap());
        for (k, st) in fx.get("streams").items().iter().enumerate() {
            let mut t = Tailer::new(PathBuf::from("."));
            let mut got = Vec::new();
            for line in st.get("lines").items() {
                t.parse_line(line.str().unwrap().as_bytes(), &f, st.get("newer_than").num(), &mut |e| got.push(e));
            }
            let want: Vec<Event> = st.get("events").items().iter().map(event_of).collect();
            for (i, (g, w)) in got.iter().zip(&want).enumerate() { assert_eq!(g, w, "stream {k}, event {i}"); }
            assert_eq!(got.len(), want.len(), "stream {k}: event count");
        }
        for c in fx.get("clips").items() {
            let (s, n) = (c.get("s").str().unwrap(), c.get("n").num().unwrap() as usize);
            assert_eq!(clip(s, n), c.get("out").str().unwrap(), "clip({s:?}, {n})");
        }
    }

    /// test_logs.py's tailer tests, on a real folder.
    struct Dir { root: PathBuf, log: PathBuf }
    impl Dir {
        fn new(name: &str) -> Dir {
            let root = std::env::temp_dir().join(format!("gw-logs-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("C--code-orbit")).unwrap();
            let log = root.join("C--code-orbit").join("abcd1234.jsonl");
            Dir { root, log }
        }
        fn write(&self, lines: &[String]) {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
            for l in lines { writeln!(f, "{l}").unwrap(); }
        }
    }
    impl Drop for Dir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }

    fn now_iso() -> String {
        let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
        // civil date from days since 1970
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
    }

    fn line(content: &str, ts: Option<&str>) -> String {
        format!(r#"{{"timestamp": "{}", "message": {{"content": {content}}}}}"#, ts.map_or_else(now_iso, str::to_string))
    }

    fn scan(t: &mut Tailer) -> Vec<Event> { let mut v = Vec::new(); t.scan(&mut |e| v.push(e)); v }

    #[test]
    fn call_and_output_paired_and_deduped() {
        let d = Dir::new("pair");
        let u = r#"[{"type": "tool_use", "id": "toolu_01ABC", "name": "Bash", "input": {"command": "ls"}}]"#;
        let r = r#"[{"type": "tool_result", "tool_use_id": "toolu_01ABC", "content": "a\nb"}]"#;
        d.write(&[line(u, None), line(u, None), line(r, None), line(r, None)]);
        let ev = scan(&mut Tailer::new(d.root.clone()));
        assert_eq!(ev.iter().map(|e| (e.kind.as_str(), e.text.as_str())).collect::<Vec<_>>(), [("$", "ls"), ("out", "a\nb")]);
        assert_eq!(ev[0].r#ref, ev[1].r#ref);
        assert_eq!(ev[0].proj, "orbit");
    }

    #[test]
    fn partial_line_waits() {
        let d = Dir::new("partial");
        d.write(&[line(r#"[{"type": "tool_use", "id": "t3", "name": "Read", "input": {"file_path": "/x"}}]"#, None)]);
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).open(&d.log).unwrap().write_all(br#"{"half": "#).unwrap();
        assert_eq!(scan(&mut Tailer::new(d.root.clone())).len(), 1);
    }

    #[test]
    fn resumed_session_shows_no_old_calls() {
        // a session idle for hours, then picked up again: only the new call appears, not a burst of old ones
        let d = Dir::new("resume");
        let old: Vec<String> = (0..5).map(|i| line(&format!(r#"[{{"type": "tool_use", "id": "o{i}", "name": "Bash", "input": {{"command": "old {i}"}}}}]"#),
                                                       Some("2026-01-01T00:00:00Z"))).collect();
        d.write(&old);
        let f = std::fs::File::options().write(true).open(&d.log).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3 * 3600)).unwrap();
        drop(f);
        let mut t = Tailer::new(d.root.clone());
        let mut ev = scan(&mut t);
        d.write(&[line(r#"[{"type": "tool_use", "id": "n1", "name": "Bash", "input": {"command": "new"}}]"#, None)]);
        ev.extend(scan(&mut t));
        assert_eq!(ev.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["new"]);
    }

    #[test]
    fn timestamps() {
        assert_eq!(when("2026-11-23T10:00:00.000Z"), 1795428000.0);
        assert_eq!(when("2026-11-23T18:00:00+08:00"), 1795428000.0);
        assert_eq!(when("1970-01-01"), 0.0);
        assert_eq!(when("garbage"), 0.0);
    }

    /// Optional, local only: every real log on this machine gives the same events as the Python did. Make the
    /// events with `uv run python tools/logs_fixtures.py --real OUT`, then GW_REAL_LOGS=OUT cargo test -- --ignored
    #[test]
    #[ignore]
    fn real_logs() {
        let path = std::env::var("GW_REAL_LOGS").expect("GW_REAL_LOGS");
        let all = Json::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let (mut files, mut events, mut bad) = (0, 0, Vec::new());
        for r in all.items() {
            let f = PathBuf::from(r.get("path").str().unwrap());
            let Ok(data) = std::fs::read(&f) else { continue };
            let want: Vec<Event> = r.get("events").items().iter().map(event_of).collect();
            let mut t = Tailer::new(PathBuf::new());
            let mut got = Vec::new();
            if let Some(cut) = data.iter().rposition(|&b| b == b'\n') {
                for line in data[..cut].split(|&b| b == b'\n') { t.parse_line(line, &f, None, &mut |e| got.push(e)); }
            }
            files += 1;
            events += want.len();
            if got.len() < want.len() { continue; }   // the log grew since: compare what both saw
            if let Some(i) = (0..want.len()).find(|&i| got[i] != want[i]) {
                let w = &want[i];
                let g = &got[i];
                let at = w.text.chars().zip(g.text.chars()).position(|(a, b)| a != b).unwrap_or(0);
                bad.push(format!("{} event {i} ({} vs {}): want …{:?}… got …{:?}…", f.display(), w.kind, g.kind,
                                 w.text.chars().skip(at.saturating_sub(20)).take(60).collect::<String>(),
                                 g.text.chars().skip(at.saturating_sub(20)).take(60).collect::<String>()));
            }
        }
        println!("{files} logs, {events} events compared");
        assert!(bad.is_empty(), "{} logs differ:\n{}", bad.len(), bad.join("\n"));
    }
}
