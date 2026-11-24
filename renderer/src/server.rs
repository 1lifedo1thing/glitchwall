//! The local feed, in Rust: a port of server.py's /feed (piece 2a of docs/native-plan.md).
//!   glitchwall-render feed [--port N] [--seed FILE]
//!
//! An HTTP endpoint on 127.0.0.1 that only the wallpaper may read, with the same rules as the Python one:
//!   1. Host must be 127.0.0.1:<port> or localhost:<port>   (blocks DNS rebinding)
//!   2. Origin must be absent, "null" (a page opened from a file) or the feed's own address
//!   3. ?key= must equal the per-install key (<state dir>/key), compared in constant time
//!
//! Events are redacted on the way in, so unredacted text is never kept or served. While paused (or in art mode) new
//! events are dropped and everything so far is put out of reach: resuming never replays a pause.
//! Its source, as server.py's follow_mode: in live mode Claude Code's logs (logs.rs). Ambient mode's made-up sessions
//! are not ported yet (2c).
//! Not here yet (pieces 2b-2d): the settings API, the look/tune/opts in the answer, the page.
//! --seed (tests only): follow a file of JSON events, one per line, adding each as it appears, instead of the sources.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::json::Json;
use crate::redact::Redactor;

pub const KEEP: usize = 300;   // events kept in memory
pub const BATCH: usize = 60;   // most events per answer
const MODES: [&str; 3] = ["live", "ambient", "art"];
// every answer: never sniffed into another type, never framed by another page, no address passed on in a Referer
const SAFE_HEADERS: [(&str, &str); 4] = [("X-Content-Type-Options", "nosniff"), ("X-Frame-Options", "SAMEORIGIN"),
    ("Content-Security-Policy", "frame-ancestors 'self'"), ("Referrer-Policy", "no-referrer")];

/// glitchwall's state folder, as settings.py has it (GLITCHWALL_HOME, else ~/.glitchwall)
pub fn state_dir() -> PathBuf {
    match std::env::var("GLITCHWALL_HOME") {
        Ok(h) if !h.is_empty() => PathBuf::from(h),
        _ => PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()).join(".glitchwall"),
    }
}

// ---------------------------------------------------------------- the events
type Ev = Vec<(String, Json)>;

pub struct Store { pub redact: Redactor, masks: Option<(bool, bool)>, events: Vec<Ev>, pub seq: u64, floor: u64 }

fn chars(s: &str, n: usize) -> String { s.chars().take(n).collect() }

impl Store {
    pub fn new(redact: Redactor) -> Store { Store { redact, masks: None, events: Vec::new(), seq: 0, floor: 0 } }

    /// An event, redacted (text and project), numbered, the oldest past KEEP dropped. The caller checks the pause.
    pub fn add(&mut self, ev: &[(String, Json)]) {
        let field = |k: &str| match ev.iter().rev().find(|(x, _)| x == k) {   // str(ev.get(k, ""))
            None => String::new(),
            Some((_, v)) => crate::logs::py_str(v),
        };
        let text = chars(&self.redact.redact(&field("text")), 1600);
        let proj = chars(&self.redact.redact(&field("proj")), 60);
        let mut out: Ev = Vec::new();
        for (k, v) in ev {   // one of each key, the last one winning, as a dict has it
            if let Some(i) = out.iter().position(|(x, _)| x == k) { out.remove(i); }
            out.push((k.clone(), v.clone()));
        }
        for (k, v) in [("text", text), ("proj", proj)] {
            match out.iter_mut().find(|(x, _)| x == k) {
                Some(e) => e.1 = Json::Str(v),
                None => out.push((k.into(), Json::Str(v))),
            }
        }
        self.seq += 1;
        out.retain(|(k, _)| k != "id");
        out.push(("id".into(), Json::Num(self.seq as f64)));
        self.events.push(out);
        let extra = self.events.len().saturating_sub(KEEP);
        self.events.drain(..extra);
    }

    pub fn hide_all(&mut self) { self.floor = self.seq; }

    pub fn since(&self, n: i64, commands_only: bool) -> Vec<Ev> {
        let n = n.max(self.floor as i64);
        let id = |e: &Ev| e.iter().find(|(k, _)| k == "id").and_then(|(_, v)| v.num()).unwrap_or(0.0) as i64;
        let out: Vec<Ev> = self.events.iter().filter(|e| id(e) > n)
            .filter(|e| !commands_only || !matches!(e.iter().find(|(k, _)| k == "kind").and_then(|(_, v)| v.str()), Some("out" | "err")))
            .cloned().collect();
        out[out.len().saturating_sub(BATCH)..].to_vec()
    }
}

// ---------------------------------------------------------------- who may read
/// True if this request may read the feed (server.py's check_access).
pub fn check_access(host: Option<&str>, origin: Option<&str>, given: &str, key: &str, port: u16, extra: &[String]) -> bool {
    let h = host.unwrap_or("");
    if h != format!("127.0.0.1:{port}") && h != format!("localhost:{port}") { return false; }
    if let Some(o) = origin && o != "null" && !extra.iter().any(|e| e == o) { return false; }
    if key.is_empty() || given.is_empty() { return false; }
    same(given.as_bytes(), key.as_bytes())
}

/// Constant-time comparison (hmac.compare_digest): every byte is looked at whatever the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    let mut d = (a.len() != b.len()) as u8;
    for (i, x) in a.iter().enumerate() { d |= x ^ b.get(i).copied().unwrap_or(!x); }
    d == 0
}

// ---------------------------------------------------------------- settings (what 2a needs of them)
/// settings.json, read again whenever the file changes, so pause and mode act without a restart.
pub struct Live { file: PathBuf, mtime: Option<SystemTime>, s: Json }

impl Live {
    pub fn new(file: PathBuf) -> Live {
        let mut l = Live { file, mtime: Some(SystemTime::UNIX_EPOCH), s: Json::Obj(Vec::new()) };   // never a file's time: read at once
        l.get();
        l
    }
    pub fn get(&mut self) -> &Json {
        let m = std::fs::metadata(&self.file).and_then(|m| m.modified()).ok();
        if m != self.mtime {
            self.mtime = m;
            self.s = std::fs::read_to_string(&self.file).ok().and_then(|t| Json::parse(t.trim_start_matches('\u{feff}')).ok())
                .filter(|j| matches!(j, Json::Obj(_))).unwrap_or(Json::Obj(Vec::new()));
        }
        &self.s
    }
    pub fn flag(&mut self, k: &str, default: bool) -> bool {
        match self.get().get(k) { Json::Null => default, v => v.truthy() }
    }
    pub fn mode(&mut self) -> String {
        let m = match self.get().get("mode") { Json::Null => String::new(), v => crate::logs::py_str(v).to_lowercase() };
        if MODES.contains(&m.as_str()) { m } else { "live".into() }
    }
}

// ---------------------------------------------------------------- HTTP
pub struct Feed { pub store: Mutex<Store>, pub live: Mutex<Live>, key: String, port: u16, extra: Vec<String>, refusals: Option<PathBuf> }

struct Req { method: String, path: String, query: String, headers: Vec<(String, String)> }

impl Req {
    /// The first header of this name (as Python's headers.get).
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    /// The first value of a query parameter (parse_qs), decoded.
    fn q(&self, name: &str) -> Option<String> {
        self.query.split(['&', ';']).filter_map(|p| p.split_once('=')).find(|(k, _)| unquote(k) == name).map(|(_, v)| unquote(v))
            .filter(|v| !v.is_empty())
    }
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => { out.push(v); i += 2; }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads a request's head (and no more: the feed takes no bodies yet). None for anything that isn't one.
fn read_req(s: &mut TcpStream) -> Option<(Req, Option<i64>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut chunk).ok()?;
        if n == 0 || buf.len() > 16 * 1024 { return None; }
        buf.extend_from_slice(&chunk[..n]);
    }
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let (path, query) = match target.split_once('?') { Some((p, q)) => (p, q), None => (target.as_str(), "") };
    let path = path.split('#').next().unwrap_or("").to_string();
    let query = query.split('#').next().unwrap_or("").to_string();
    let req = Req { method, path, query, headers };
    let len = req.header("Content-Length").map(|v| v.trim().parse::<i64>().unwrap_or(-1));
    Some((req, len))
}

fn reason(code: u16) -> &'static str {
    match code { 200 => "OK", 400 => "Bad Request", 403 => "Forbidden", 404 => "Not Found", 405 => "Method Not Allowed", _ => "Error" }
}

fn respond(s: &mut TcpStream, code: u16, extra: &[(&str, String)], body: &[u8]) {
    let mut h = format!("HTTP/1.1 {code} {}\r\nServer: glitchwall\r\nConnection: close\r\nContent-Length: {}\r\n", reason(code), body.len());
    for (k, v) in extra { h.push_str(&format!("{k}: {v}\r\n")); }
    for (k, v) in SAFE_HEADERS { h.push_str(&format!("{k}: {v}\r\n")); }
    h.push_str("\r\n");
    let _ = s.write_all(h.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

impl Feed {
    pub fn new(key: String, port: u16, live: Live, store: Store, refusals: Option<PathBuf>) -> Feed {
        let extra = vec![format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
        Feed { store: Mutex::new(store), live: Mutex::new(live), key, port, extra, refusals }
    }

    fn refuse(&self, s: &mut TcpStream, req: &Req, code: u16) {
        if code == 403 && let Some(f) = &self.refusals {   // who was turned away (never the key), to help debug a new host
            let referer = req.header("Referer").unwrap_or("").split('?').next().unwrap_or("").to_string();
            let when = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = std::fs::write(f, format!("{when} host={} origin={} referer={referer}\n",
                req.header("Host").unwrap_or("None"), req.header("Origin").unwrap_or("None")));
        }
        respond(s, code, &[], b"");
    }

    pub fn handle(&self, mut s: TcpStream) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let Some((req, len)) = read_req(&mut s) else { return };
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/feed") => self.feed(&mut s, &req),
            ("GET", _) => self.refuse(&mut s, &req, 404),   // the settings API and the page: not here yet
            ("POST", "/settings") if !matches!(len, Some(n) if n > 0 && n <= 8192) => self.refuse(&mut s, &req, 400),
            ("POST", _) => self.refuse(&mut s, &req, 404),
            _ => self.refuse(&mut s, &req, 405),
        }
    }

    fn feed(&self, s: &mut TcpStream, req: &Req) {
        let origin = req.header("Origin");
        if !check_access(req.header("Host"), origin, &req.q("key").unwrap_or_default(), &self.key, self.port, &self.extra) {
            return self.refuse(s, req, 403);
        }
        let since = req.q("since").and_then(|v| v.trim().replace('_', "").parse::<i64>().ok()).unwrap_or(0);
        let (paused, mode, commands_only) = {
            let mut l = self.live.lock().unwrap();
            (l.flag("paused", false), l.mode(), l.flag("commands_only", false))
        };
        let hidden = paused || mode == "art";
        let (events, last) = {
            let mut st = self.store.lock().unwrap();
            if hidden { st.hide_all(); }   // the page clears the screen; what it hadn't shown yet must not come back later
            (if hidden { Vec::new() } else { st.since(since, commands_only) }, st.seq)
        };
        let body = Json::Obj(vec![
            ("events".into(), Json::Arr(events.into_iter().map(Json::Obj).collect())),
            ("paused".into(), Json::Bool(paused)),
            ("last".into(), Json::Num(last as f64)),
            ("mode".into(), Json::Str(mode)),
            ("sleep".into(), Json::Bool(false)),
        ]);
        let body = crate::logs::dumps(&body).into_bytes();
        let mut h: Vec<(&str, String)> = vec![("Content-Type", "application/json".into())];
        if let Some(o) = origin.filter(|o| !o.is_empty()) { h.push(("Access-Control-Allow-Origin", o.into())); h.push(("Vary", "Origin".into())); }
        h.push(("Cache-Control", "no-store".into()));
        respond(s, 200, &h, &body);
    }

    /// Adds an event unless paused (the pause drops what comes in while it lasts).
    /// The masking follows the settings (mask_user, mask_email) as they change, for what comes next.
    pub fn add(&self, ev: &[(String, Json)]) {
        let (paused, masks) = {
            let mut l = self.live.lock().unwrap();
            (l.flag("paused", false), (l.flag("mask_user", true), l.flag("mask_email", true)))
        };
        if paused { return; }
        let mut st = self.store.lock().unwrap();
        if st.masks != Some(masks) {
            st.redact = Redactor::for_machine_with(masks.0, masks.1);
            st.masks = Some(masks);
        }
        st.add(ev);
    }
}

/// Follows a file of JSON events, one per line (--seed, for the tests): read from the start, then what is appended.
fn follow_seed(feed: Arc<Feed>, f: PathBuf) {
    let mut pos = 0u64;
    loop {
        if let Ok(data) = std::fs::read(&f) {
            if (data.len() as u64) < pos { pos = 0; }
            let chunk = &data[pos as usize..];
            if let Some(nl) = chunk.iter().rposition(|&b| b == b'\n') {
                for line in chunk[..nl].split(|&b| b == b'\n') {
                    if let Ok(Json::Obj(ev)) = Json::parse(String::from_utf8_lossy(line).trim()) { feed.add(&ev); }
                }
                pos += nl as u64 + 1;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn event_json(e: crate::logs::Event) -> Ev {
    vec![("ts".into(), Json::Str(e.ts)), ("proj".into(), Json::Str(e.proj)), ("session".into(), Json::Str(e.session)),
         ("side".into(), Json::Bool(e.side)), ("kind".into(), Json::Str(e.kind)), ("ref".into(), Json::Str(e.r#ref)),
         ("text".into(), Json::Str(e.text))]
}

/// The source (server.py's follow_mode): Claude Code's logs while the mode is live, started and stopped as the mode
/// changes.
fn start_sources(feed: Arc<Feed>) {
    std::thread::spawn(move || {
        let (mut mode, mut stop) = (String::new(), None::<Arc<AtomicBool>>);
        loop {
            let m = feed.live.lock().unwrap().mode();
            if m != mode {
                if let Some(s) = stop.take() { s.store(true, Ordering::Relaxed); }
                if m == "live" {
                    let (s, fd) = (Arc::new(AtomicBool::new(false)), feed.clone());
                    let s2 = s.clone();
                    std::thread::spawn(move || crate::logs::Tailer::new(crate::logs::logs_root()).run(&s2, &mut |e| fd.add(&event_json(e))));
                    stop = Some(s);
                }
                mode = m;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn arg(args: &[String], name: &str) -> Option<String> { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned()) }

fn load_key(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("key")).ok().map(|k| k.trim().to_string()).filter(|k| k.len() >= 32)
}

/// `glitchwall-render feed`: runs until killed. Errors (no key yet, port taken) go back to main's log.
pub fn run(args: &[String]) -> Result<(), String> {
    let dir = state_dir();
    let mut live = Live::new(dir.join("settings.json"));
    let port = arg(args, "--port").and_then(|p| p.parse().ok())
        .or_else(|| live.get().get("port").num().map(|p| p as u16)).unwrap_or(47833);
    let key = load_key(&dir).ok_or("no feed key yet: run glitchwall once to make one")?;
    let feed = Arc::new(Feed::new(key, port, live, Store::new(Redactor::for_machine()), Some(dir.join("last-refused.txt"))));
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    match arg(args, "--seed") {
        Some(f) => { let fd = feed.clone(); std::thread::spawn(move || follow_seed(fd, PathBuf::from(f))); }
        None => start_sources(feed.clone()),
    }
    for conn in listener.incoming().flatten() {
        let fd = feed.clone();
        std::thread::spawn(move || fd.handle(conn));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";
    const P: u16 = 47899;

    fn ok(host: Option<&str>, origin: Option<&str>, key: &str) -> bool { check_access(host, origin, key, KEY, P, &[]) }

    #[test]
    fn access_like_python() {   // tests/test_server.py's TestCheckAccess, case for case
        let h = format!("127.0.0.1:{P}");
        assert!(ok(Some(&h), None, KEY));
        assert!(ok(Some(&format!("localhost:{P}")), None, KEY));
        assert!(ok(Some(&h), Some("null"), KEY));
        let me = format!("http://127.0.0.1:{P}");
        assert!(check_access(Some(&h), Some(&me), KEY, KEY, P, &[me.clone()]));
        for k in ["", &"x".repeat(43), &KEY[..42]] { assert!(!ok(Some(&h), None, k), "{k}"); }
        assert!(!check_access(Some(&h), None, "", "", P, &[]));
        for bad in ["evil.example:47899", "127.0.0.1:47900", "127.0.0.1", "", "0.0.0.0:47899", "127.0.0.1.evil.example:47899"] {
            assert!(!ok(Some(bad), None, KEY), "{bad}");
        }
        assert!(!ok(None, None, KEY));
        for o in ["https://evil.example", "http://127.0.0.1:8000", "https://localhost.evil.example", "https://evil.example/.localhost",
                  "file://", "https://x.localhost.evil.example", "", "https://a1b2c3.localhost", "http://localhost:8080",
                  "http://localhost:47900", "NULL"] {
            assert!(!ok(Some(&h), Some(o), KEY), "{o}");
        }
    }

    #[test]
    fn store_keeps_floors_and_filters() {
        let mut s = Store::new(Redactor::new("", "", false, false, true));
        let ev = |kind: &str, text: &str| vec![("kind".to_string(), Json::Str(kind.into())), ("text".to_string(), Json::Str(text.into()))];
        s.add(&ev("$", &format!("export API_TOKEN={}", "abc123secretvalue")));
        s.add(&ev("out", "ok"));
        let all = s.since(0, false);
        assert_eq!(all.len(), 2);
        assert!(!crate::logs::dumps(&Json::Obj(all[0].clone())).contains("abc123secretvalue"));
        assert_eq!(s.since(0, true).len(), 1);
        s.hide_all();
        assert!(s.since(0, false).is_empty());
        for i in 0..400 { s.add(&ev("$", &i.to_string())); }
        assert_eq!(s.events.len(), KEEP);
        let b = s.since(0, false);
        assert_eq!(b.len(), BATCH);
        assert_eq!(b.last().unwrap().iter().find(|(k, _)| k == "id").unwrap().1.num(), Some(402.0));
    }

    #[test]
    fn query_decoding() {
        let r = Req { method: "GET".into(), path: "/feed".into(), query: "key=a%2Bb+c&since=5&x".into(), headers: vec![] };
        assert_eq!(r.q("key").as_deref(), Some("a+b c"));
        assert_eq!(r.q("since").as_deref(), Some("5"));
        assert_eq!(r.q("nope"), None);
        assert_eq!(unquote("100%"), "100%");
        assert_eq!(unquote("%zz"), "%zz");
    }
}
