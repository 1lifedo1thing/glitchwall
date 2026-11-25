//! The local feed, in Rust: a port of server.py's feed (pieces 2a and 2b of docs/native-plan.md).
//!   glitchwall-render feed [--port N] [--wallpaper DIR] [--seed FILE]
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
//!
//! It also serves the page and the settings window from --wallpaper (default: the wallpaper folder next to this exe,
//! where looks.js is read from too): static files to the right Host only, the key never in them (the page finds it
//! after #key= in its address, which is never sent). /settings reads and changes settings.json (settings.rs) behind
//! the same checks as /feed; a change must also be a JSON POST from the feed's own address.
//! --seed (tests only): follow a file of JSON events, one per line, adding each as it appears, instead of the sources.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::json::Json;
use crate::redact::Redactor;
use crate::settings::{self as cfg, Live, Schema};

pub const KEEP: usize = 300;   // events kept in memory
pub const BATCH: usize = 60;   // most events per answer
const SCAN: usize = 4000;      // text redacted per event (server.py's SCAN): only 1600 characters show
const MAX_BODY: i64 = 8192;    // the most a settings change may send
/// what /config.js is when the feed serves the page itself: the key stays out of every file
const CONFIG_JS: &str = "// the key is in the page address\n";
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
        let text = chars(&self.redact.redact(&crate::logs::clip(&field("text"), SCAN)), 1600);   // cut at a word break first
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

// ---------------------------------------------------------------- HTTP
pub struct Feed {
    pub store: Mutex<Store>, pub live: Mutex<Live>, key: String, port: u16, extra: Vec<String>, refusals: Option<PathBuf>,
    schema: Arc<Schema>,
    settings_file: PathBuf,
    /// the page, the settings window, looks.js and the fonts
    static_dir: Option<PathBuf>,
    /// one settings change at a time: each reads the file, changes it and writes it back
    saving: Mutex<()>,
    /// the settings window's API (off in the demo)
    pub editable: bool,
    /// what /config.js is
    pub config_js: String,
    /// the demo's own look, mode and label, whatever the settings say
    pub fixed: Option<Fixed>,
}

/// What `glitchwall demo` shows: set when it starts (demo.py's run_demo passes look, mode and label to the handler).
pub struct Fixed { pub look: Json, pub mode: String, pub label: Json }

struct Req { method: String, path: String, query: String, headers: Vec<(String, String)>, body: Vec<u8> }

impl Req {
    /// The first header of this name (as Python's headers.get).
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    /// The first value of a query parameter (parse_qs: only & separates, as since Python 3.9.2), decoded.
    fn q(&self, name: &str) -> Option<String> {
        self.query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| unquote(k) == name).map(|(_, v)| unquote(v))
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

/// Reads a request's head and what came in after it. None for anything that isn't one.
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
    let mut path = path.split('#').next().unwrap_or("").to_string();
    if path.starts_with("//") { path = format!("/{}", path.trim_start_matches('/')); }   // as http.server does: never a host
    let query = query.split('#').next().unwrap_or("").to_string();
    let req = Req { method, path, query, headers, body: buf[end + 4..].to_vec() };
    let len = req.header("Content-Length").map(|v| v.trim().parse::<i64>().unwrap_or(-1));
    Some((req, len))
}

fn reason(code: u16) -> &'static str {
    match code { 200 => "OK", 400 => "Bad Request", 403 => "Forbidden", 404 => "Not Found", 405 => "Method Not Allowed", 500 => "Internal Server Error", _ => "Error" }
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

/// The type of a file the feed serves (the wallpaper folder holds only these kinds).
fn content_type(f: &Path) -> &'static str {
    match f.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html", Some("js") => "text/javascript", Some("css") => "text/css",
        Some("json") => "application/json", Some("txt") => "text/plain", Some("png") => "image/png",
        Some("ico") => "image/vnd.microsoft.icon", Some("svg") => "image/svg+xml", Some("ttf") => "font/ttf",
        Some("otf") => "font/otf", Some("woff") => "font/woff", Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

impl Feed {
    pub fn new(key: String, port: u16, live: Live, store: Store, refusals: Option<PathBuf>, settings_file: PathBuf,
               static_dir: Option<PathBuf>) -> Feed {
        let extra = vec![format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
        let schema = live.schema.clone();
        Feed { store: Mutex::new(store), live: Mutex::new(live), key, port, extra, refusals, schema, settings_file, static_dir,
               saving: Mutex::new(()), editable: true, config_js: CONFIG_JS.into(), fixed: None }
    }

    fn host_ok(&self, req: &Req) -> bool {
        let h = req.header("Host").unwrap_or("");
        h == format!("127.0.0.1:{}", self.port) || h == format!("localhost:{}", self.port)
    }

    fn json(&self, s: &mut TcpStream, origin: Option<&str>, body: &Json) {
        let mut h: Vec<(&str, String)> = vec![("Content-Type", "application/json".into())];
        if let Some(o) = origin.filter(|o| !o.is_empty()) { h.push(("Access-Control-Allow-Origin", o.into())); h.push(("Vary", "Origin".into())); }
        h.push(("Cache-Control", "no-store".into()));
        respond(s, 200, &h, crate::logs::dumps(body).as_bytes());
    }

    /// The settings as they are on disk right now (as settings.load_settings: unreadable counts as paused).
    fn on_disk(&self) -> Json { self.schema.load(cfg::read_saved(&self.settings_file).as_ref()) }

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
        self.answer(&mut s, req, len);
        // a graceful close: a body that was refused unread would otherwise make Windows reset the connection, and the
        // client could lose the answer. What is left is read and dropped (a little, briefly).
        let _ = s.shutdown(std::net::Shutdown::Write);
        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
        let (mut chunk, mut left) = ([0u8; 4096], 64 * 1024usize);
        while left > 0 && let Ok(n @ 1..) = s.read(&mut chunk) { left = left.saturating_sub(n); }
    }

    fn answer(&self, s: &mut TcpStream, mut req: Req, len: Option<i64>) {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/feed") => self.feed(s, &req),
            ("GET", "/settings") => self.settings(s, &req, false),
            ("GET", _) if self.static_dir.is_some() => self.static_file(s, &req),
            ("GET", _) => self.refuse(s, &req, 404),
            ("POST", "/settings") => {
                let Some(n) = len.filter(|n| (1..=MAX_BODY).contains(n)) else { return self.refuse(s, &req, 400) };
                let n = n as usize;
                let mut chunk = [0u8; 2048];
                while req.body.len() < n {   // the rest of the body, as Content-Length says
                    match s.read(&mut chunk) { Ok(0) | Err(_) => return, Ok(k) => req.body.extend_from_slice(&chunk[..k]) }
                }
                req.body.truncate(n);
                self.settings(s, &req, true)
            }
            ("POST", _) => self.refuse(s, &req, 404),
            _ => self.refuse(s, &req, 405),
        }
    }

    fn feed(&self, s: &mut TcpStream, req: &Req) {
        let origin = req.header("Origin");
        if !check_access(req.header("Host"), origin, &req.q("key").unwrap_or_default(), &self.key, self.port, &self.extra) {
            return self.refuse(s, req, 403);
        }
        let since = req.q("since").and_then(|v| v.trim().replace('_', "").parse::<i64>().ok()).unwrap_or(0);
        let st = self.live.lock().unwrap().get().clone();
        let paused = cfg::get(st.entries(), "paused").is_some_and(cfg::truthy);
        let commands_only = cfg::get(st.entries(), "commands_only").is_some_and(cfg::truthy);
        let fixed = self.fixed.as_ref();
        let mode = fixed.map(|f| f.mode.clone()).unwrap_or_else(|| cfg::mode(&st));
        let hidden = paused || mode == "art";
        let (events, last) = {
            let mut store = self.store.lock().unwrap();
            if hidden { store.hide_all(); }   // the page clears the screen; what it hadn't shown yet must not come back later
            (if hidden { Vec::new() } else { store.since(since, commands_only) }, store.seq)
        };
        let sc = &self.schema;
        let body = Json::Obj(vec![
            ("events".into(), Json::Arr(events.into_iter().map(Json::Obj).collect())),
            ("paused".into(), Json::Bool(paused)),
            ("last".into(), Json::Num(last as f64)),
            ("look".into(), fixed.map(|f| f.look.clone()).unwrap_or_else(|| sc.look(&st))),
            ("mode".into(), Json::Str(mode)),
            ("label".into(), fixed.map(|f| f.label.clone()).unwrap_or_else(|| sc.display_label(&st))),
            ("tune".into(), sc.tune(&st)),
            ("opts".into(), sc.opts(&st)),
            ("corner".into(), Json::Str(cfg::corner(&st))),   // where the hot-corner icon sits
            ("masked".into(), Json::Str(cfg::masked(&st))),   // how masked secrets show
            ("sleep".into(), Json::Bool(false)),   // the built-in host's "nobody can see this screen": piece 3
        ]);
        self.json(s, origin, &body);
    }

    /// The settings window: GET reads, POST changes. Same key/Host/Origin checks as the feed; a change must also be a
    /// JSON POST from the feed's own address, where the settings window lives (a plain form can't be one, and a file
    /// page or a sandboxed frame, which send Origin: null, can't make one).
    fn settings(&self, s: &mut TcpStream, req: &Req, change: bool) {
        if !self.editable { return self.refuse(s, req, 404); }
        let origin = req.header("Origin");
        if !check_access(req.header("Host"), origin, &req.q("key").unwrap_or_default(), &self.key, self.port, &self.extra) {
            return self.refuse(s, req, 403);
        }
        let mut refused = Vec::new();
        if change {
            let ctype = req.header("Content-Type").unwrap_or("").split(';').next().unwrap_or("");
            if !origin.is_some_and(|o| self.extra.iter().any(|e| e == o)) || crate::logs::py_strip(ctype) != "application/json" {
                return self.refuse(s, req, 403);
            }
            let body = req.body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&req.body);
            let Some(changes) = std::str::from_utf8(body).ok().and_then(|t| Json::parse(t).ok()) else { return self.refuse(s, req, 400) };
            let mut changes = match changes { Json::Obj(m) => cfg::dict(&m), _ => Vec::new() };
            let reset = match changes.iter().position(|(k, _)| k == "reset") {   // RESET ALL: the defaults
                Some(i) => changes.remove(i).1 == Json::Bool(true),
                None => false,
            };
            let _one_at_a_time = self.saving.lock().unwrap();
            let now = self.on_disk();
            let base = if reset { self.schema.reset(&now) } else { now };
            let (new, r) = self.schema.apply_changes(&base, &Json::Obj(changes));
            refused = r;
            if cfg::private_write(&self.settings_file, &self.schema.file_text(&new)).is_err() { return respond(s, 500, &[], b""); }
        }
        // no restart after RESET ALL: this feed has no windows of its own to start afresh (the built-in host is piece 3)
        self.json(s, origin, &self.schema.window(&self.on_disk(), &refused, false));
    }

    /// The page and its files, from the static folder, to the right Host only. /config.js holds no key (but the demo's,
    /// whose key is random per run and reads nothing real).
    fn static_file(&self, s: &mut TcpStream, req: &Req) {
        if !self.host_ok(req) { return self.refuse(s, req, 403); }
        let name = req.path.trim_start_matches('/');
        let name = if name.is_empty() { "index.html" } else { name };
        let (data, ctype) = if name == "config.js" {
            (self.config_js.as_bytes().to_vec(), "text/javascript")
        } else {
            // resolved first: whatever the path says (.., \, C:, links), the file must be inside the folder
            let dir = self.static_dir.as_ref().and_then(|d| d.canonicalize().ok());
            let f = dir.as_ref().and_then(|d| d.join(name).canonicalize().ok()).filter(|f| dir.as_ref().is_some_and(|d| f.starts_with(d) && f != d));
            let Some(data) = f.as_ref().filter(|f| f.is_file()).and_then(|f| std::fs::read(f).ok()) else { return self.refuse(s, req, 404) };
            (data, content_type(f.as_deref().unwrap()))
        };
        respond(s, 200, &[("Content-Type", ctype.into()), ("Cache-Control", "no-store".into())], &data);
    }

    /// Adds an event unless paused (the pause drops what comes in while it lasts).
    /// The masking follows the settings (mask_user, mask_email) as they change, for what comes next.
    pub fn add(&self, ev: &[(String, Json)]) {
        let (paused, masks) = {
            let mut l = self.live.lock().unwrap();
            (l.flag("paused"), (l.flag("mask_user"), l.flag("mask_email")))
        };
        if paused { return; }
        let mut st = self.store.lock().unwrap();
        if self.fixed.is_none() && st.masks != Some(masks) {   // the demo keeps its own (no home, no user name)
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

/// Follow one agent's logs until stop is set.
fn start_reader(agent: &str, feed: Arc<Feed>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let agent = agent.to_string();
    std::thread::spawn(move || {
        let mut add = |e| feed.add(&event_json(e));
        match agent.as_str() {
            "agent_gemini" => crate::gemini::Tailer::new(crate::gemini::logs_root()).run(&s2, &mut add),
            "agent_codex" => crate::codex::Tailer::new(crate::codex::logs_root()).run(&s2, &mut add),
            _ => crate::logs::Tailer::new(crate::logs::logs_root()).run(&s2, &mut add),
        }
    });
    stop
}

/// The mode's own source (server.py's start_source): ambient's made-up sessions; live and art start nothing here
/// (live mode's readers are started one by one, see Sources). Set the flag to stop it.
fn start_source(mode: &str, feed: Arc<Feed>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    if mode == "ambient" {
        let s2 = stop.clone();
        std::thread::spawn(move || crate::ambient::generate(&mut |e| feed.add(&e), &s2, 1.0));
    }
    stop
}

/// What runs, kept in step with the settings (server.py's follow_mode): the mode's own source, and in live mode one
/// reader per agent turned on. Turning an agent on or off starts or stops just that reader; an agent that is off has
/// its folder left alone.
#[derive(Default)]
pub struct Sources { mode: Option<String>, stop: Option<Arc<AtomicBool>>, readers: Vec<(&'static str, Arc<AtomicBool>)> }

impl Sources {
    pub fn step(&mut self, mode: &str, agents: &[&'static str], start_source: &mut dyn FnMut(&str) -> Arc<AtomicBool>,
                start_reader: &mut dyn FnMut(&'static str) -> Arc<AtomicBool>) {
        if self.mode.as_deref() != Some(mode) {
            if let Some(s) = self.stop.take() { s.store(true, Ordering::Relaxed); }
            self.mode = Some(mode.to_string());
            self.stop = Some(start_source(mode));
        }
        let want: &[&str] = if mode == "live" { agents } else { &[] };
        self.readers.retain(|(a, s)| { let keep = want.contains(a); if !keep { s.store(true, Ordering::Relaxed); } keep });
        for (a, _, _) in cfg::AGENTS {   // in AGENTS' order, as follow_mode starts them
            if want.contains(&a) && !self.readers.iter().any(|(r, _)| *r == a) { self.readers.push((a, start_reader(a))); }
        }
    }

    #[cfg(test)]
    pub fn stop_all(&mut self) {
        for s in self.stop.take().into_iter().chain(self.readers.drain(..).map(|(_, s)| s)) { s.store(true, Ordering::Relaxed); }
    }
}

/// Follows the settings once a second. `seeded` (the tests' --seed): the seed file stands in for the agents' logs,
/// so none is read; the modes' own sources still follow the settings.
fn follow_mode(feed: Arc<Feed>, seeded: bool) {
    std::thread::spawn(move || {
        let mut src = Sources::default();
        loop {
            let (mode, agents) = { let mut l = feed.live.lock().unwrap(); (l.mode(), if seeded { Vec::new() } else { l.agents_on() }) };
            src.step(&mode, &agents, &mut |m| start_source(m, feed.clone()), &mut |a| start_reader(a, feed.clone()));
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
    let wp = wallpaper_dir(args);
    let schema = Arc::new(Schema::from_file(&wp.join("looks.js"))?);
    let file = dir.join("settings.json");
    let mut live = Live::new(schema, file.clone());
    let port = arg(args, "--port").and_then(|p| p.parse().ok())
        .or_else(|| live.get().get("port").num().map(|p| p as u16)).unwrap_or(cfg::DEFAULT_PORT);
    let key = load_key(&dir).ok_or("no feed key yet: run glitchwall once to make one")?;
    let feed = Arc::new(Feed::new(key, port, live, Store::new(Redactor::for_machine()), Some(dir.join("last-refused.txt")),
                                  file, Some(wp)));
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let seed = arg(args, "--seed");
    if let Some(f) = seed.clone() { let fd = feed.clone(); std::thread::spawn(move || follow_seed(fd, PathBuf::from(f))); }
    follow_mode(feed.clone(), seed.is_some());
    serve(&listener, feed);
    Ok(())
}

fn serve(listener: &TcpListener, feed: Arc<Feed>) {
    for conn in listener.incoming().flatten() {
        let fd = feed.clone();
        std::thread::spawn(move || fd.handle(conn));
    }
}

pub const DEMO_PORT: u16 = 47834;

/// `glitchwall-render demo [--port N] [--pace X] [--mode M] [--theme ..] [--palette ..] [--rain ..] [--font ..]
/// [--transition ..] [--wallpaper DIR]` (demo.py's run_demo): made-up tool calls, no logs read, and the wallpaper page
/// itself at http://127.0.0.1:<port>/ to open in a browser. The key is random per run and sits in /config.js: the
/// data is fake, so nothing real is exposed. The settings window's API is off; the look comes from the settings and
/// the options given, and live mode here is the scripted demo session.
pub fn run_demo(args: &[String]) -> Result<(), String> {
    let wp = wallpaper_dir(args);
    let schema = Arc::new(Schema::from_file(&wp.join("looks.js"))?);
    let port = arg(args, "--port").and_then(|p| p.parse().ok()).unwrap_or(DEMO_PORT);
    let pace = arg(args, "--pace").and_then(|p| p.parse::<f64>().ok()).filter(|p| p.is_finite() && *p >= 0.0).unwrap_or(1.0);
    let mode = arg(args, "--mode").filter(|m| cfg::MODES.iter().any(|(k, _)| k == m)).unwrap_or_else(|| "live".into());
    let file = state_dir().join("settings.json");
    let s = schema.load(cfg::read_saved(&file).as_ref());
    let mut with = cfg::dict(s.entries());
    cfg::set(&mut with, "mode", Json::Str(mode.clone()));
    let label = schema.display_label(&Json::Obj(with.clone()));
    for (k, _) in &schema.kinds { if let Some(v) = arg(args, &format!("--{k}")) { cfg::set(&mut with, k, Json::Str(v)); } }
    let look = schema.look(&Json::Obj(with));
    let shown = crate::logs::py_str(&label) + "  [demo]";
    let key = crate::ambient::token_urlsafe(24);
    let mut feed = Feed::new(key.clone(), port, Live::fixed(schema.clone(), Json::Obj(schema.defaults.clone())),
                             Store::new(Redactor::new("", "", false, true, true)), None, file, Some(wp));
    feed.editable = false;
    feed.config_js = cfg::config_js(port, &key, &shown);
    feed.fixed = Some(Fixed { look: look.clone(), mode: mode.clone(), label: Json::Str(if cfg::truthy(&label) { shown } else { String::new() }) });
    let feed = Arc::new(feed);
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    if mode != "art" {
        let fd = feed.clone();
        let m = mode.clone();
        std::thread::spawn(move || {
            let stop = AtomicBool::new(false);
            let mut add = |e: crate::ambient::Ev| fd.add(&e);
            if m == "ambient" { crate::ambient::generate(&mut add, &stop, pace) } else { crate::ambient::demo_generate(&mut add, &stop, pace) }
        });
    }
    let l = |k: &str| look.get(k).str().unwrap_or("").to_string();
    println!("demo feed (fake data) on http://127.0.0.1:{port}/  - open it in a browser, Ctrl+C to stop");
    println!("mode: {mode}");
    println!("look: {} theme, {} rain, {} font  (try ?theme=amber&rain=stars)", l("theme"), l("rain"), l("font"));
    serve(&listener, feed);
    Ok(())
}

/// --wallpaper, or the wallpaper folder next to this exe: the page, looks.js and the fonts.
fn wallpaper_dir(args: &[String]) -> PathBuf {
    arg(args, "--wallpaper").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("wallpaper"))).unwrap_or_default()
    })
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
    fn agents_off_until_turned_on() {
        let dir = std::env::temp_dir().join(format!("gw-agents-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        let schema = cfg::test_schema();
        let on = |text: &str| { std::fs::write(&f, text).unwrap(); let mut l = Live::new(schema.clone(), f.clone()); l.agents_on() };
        assert_eq!(on("{}"), ["agent_claude"]);
        assert_eq!(on(r#"{"agent_gemini": true, "agent_claude": false}"#), ["agent_gemini"]);
        assert_eq!(on(r#"{"agent_codex": "yes", "agent_gemini": 1, "agent_claude": null}"#), Vec::<&str>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_decoding() {
        let r = Req { method: "GET".into(), path: "/feed".into(), query: "key=a%2Bb+c&since=5&x&k2=1;y=2".into(), headers: vec![], body: vec![] };
        assert_eq!(r.q("key").as_deref(), Some("a+b c"));
        assert_eq!(r.q("k2").as_deref(), Some("1;y=2"));   // ; is not a separator (parse_qs since 3.9.2)
        assert_eq!(r.q("y"), None);
        assert_eq!(r.q("since").as_deref(), Some("5"));
        assert_eq!(r.q("nope"), None);
        assert_eq!(unquote("100%"), "100%");
        assert_eq!(unquote("%zz"), "%zz");
    }

    // test_modes.py's TestFollowMode, on Sources::step
    fn run_steps(states: &[(&str, Vec<&'static str>)]) -> (Vec<String>, Vec<&'static str>, Vec<Arc<AtomicBool>>, Sources) {
        let (mut modes, mut readers, flags) = (Vec::new(), Vec::new(), std::cell::RefCell::new(Vec::new()));
        let flag = || { let f = Arc::new(AtomicBool::new(false)); flags.borrow_mut().push(f.clone()); f };
        let mut src = Sources::default();
        for (mode, agents) in states {
            src.step(mode, agents, &mut |m| { modes.push(m.to_string()); flag() }, &mut |a| { readers.push(a); flag() });
        }
        (modes, readers, flags.into_inner(), src)
    }

    #[test]
    fn switches_source_when_mode_changes() {
        let (modes, _, flags, _) = run_steps(&[("live", vec![]), ("live", vec![]), ("ambient", vec![]), ("art", vec![]), ("art", vec![])]);
        assert_eq!(modes, ["live", "ambient", "art"]);
        assert_eq!(flags.iter().map(|f| f.load(Ordering::Relaxed)).collect::<Vec<_>>(), [true, true, false]);   // the old ones stopped
    }

    #[test]
    fn agents_start_and_stop_one_by_one() {
        // the agents' lists as agents_on gives them for {}, {gemini}, {gemini, claude off}, {gemini: "yes"}, then art
        let (_, readers, flags, mut src) = run_steps(&[("live", vec!["agent_claude"]), ("live", vec!["agent_claude", "agent_gemini"]),
            ("live", vec!["agent_gemini"]), ("live", vec!["agent_claude"]), ("art", vec!["agent_claude", "agent_codex"])]);
        assert_eq!(readers, ["agent_claude", "agent_gemini", "agent_claude"]);
        src.stop_all();
        assert!(flags.iter().all(|f| f.load(Ordering::Relaxed)));
    }

    #[test]
    fn config_js_holds_port_key_label() {
        assert_eq!(cfg::config_js(47834, "k-_1", "é  [demo]"), "window.FEED = {\"port\": 47834, \"key\": \"k-_1\", \"label\": \"\\u00e9  [demo]\"};\n");
    }
}
