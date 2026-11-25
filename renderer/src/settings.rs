//! settings.json, as settings.py has it (piece 2b of docs/native-plan.md): the defaults, the checks on a change
//! from the settings window (apply_changes), what the page is sent (look, tune, opts, masked, corner, mode, the
//! label), and the save: to a temp file swapped in, so the feed never reads a half-written file.
//! A settings.json that can't be read counts as paused, so a bad hand edit never turns a pause off; the running
//! feed keeps the last good settings (Live, server.py's LiveSettings).
//!
//! The theme names, backgrounds, fonts, transitions and each background's options come from looks.js (themes.py).
//! Settings are kept as a JSON object with Python's dict ways: one of each key, a repeated key keeping its first
//! place and its last value.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use crate::json::Json;
use crate::logs::{py_str, py_strip};

pub type Obj = Vec<(String, Json)>;

pub const DEFAULT_PORT: u16 = 47833;
pub const DEFAULT_LABEL: &str = "CLAUDE :: live tool feed";
pub const AMBIENT_LABEL: &str = "AMBIENT :: made-up sessions, nothing real";

/// setting, name, on by default: whose logs live mode reads (only exactly `true` turns one on)
pub const AGENTS: [(&str, &str, bool); 3] =
    [("agent_claude", "Claude Code", true), ("agent_gemini", "Gemini CLI", false), ("agent_codex", "Codex CLI", false)];

/// setting -> (the page's slider name, lowest, highest)
pub const TUNE: [(&str, &str, i64, i64); 6] = [("type_speed", "typeSpeed", 1, 20), ("linger", "linger", 2, 30),
    ("cards", "cards", 1, 6), ("intensity", "rain", 0, 10), ("glass", "glass", 20, 95), ("grain", "grain", 0, 10)];

pub const MODES: [(&str, &str); 3] = [
    ("live", "your coding agents' real tool calls, as they happen"),
    ("ambient", "made-up but realistic coding sessions; reads nothing, no agent needed"),
    ("art", "just the themed background: no cards, no label"),
];
pub const CORNERS: [(&str, &str); 5] = [
    ("off", "no hot corner; open the settings from the right-click menu or the Start menu"),
    ("top-left", "rest the mouse in the top-left corner of a screen to open the settings"),
    ("top-right", "rest the mouse in the top-right corner of a screen to open the settings"),
    ("bottom-left", "rest the mouse in the bottom-left corner of a screen to open the settings"),
    ("bottom-right", "rest the mouse in the bottom-right corner of a screen to open the settings"),
];
pub const MASKED: [(&str, &str); 5] = [   // the redaction always writes •••• in the feed; the wall swaps in one of these
    ("dots", "a row of dots"),
    ("words", "a word in brackets, a different one each time: [REDACTED], [EXPUNGED], [WITHHELD] and so on"),
    ("blocks", "solid blocks, like a censor's marker"),
    ("stars", "asterisks, like a password box"),
    ("noise", "random gibberish, made up fresh, never from the hidden text"),
];
const FLAGS: [&str; 5] = ["paused", "commands_only", "mask_user", "mask_email", "tray"];
/// how it's installed, what it shows, stream-safety: kept by RESET ALL
const KEEP_ON_RESET: [&str; 5] = ["port", "screens", "mode", "paused", "renderer"];
/// not in the settings window's "defaults" (command-line only)
pub const NOT_RESETTABLE: [&str; 4] = ["port", "host", "screens", "renderer"];

// ---------------------------------------------------------------- Python's dict and value ways
/// The last value of a key (dict.get).
pub fn get<'a>(o: &'a [(String, Json)], k: &str) -> Option<&'a Json> { o.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v) }

/// d[k] = v: a key already there keeps its place.
pub fn set(o: &mut Obj, k: &str, v: Json) {
    match o.iter_mut().find(|(n, _)| n == k) { Some(e) => e.1 = v, None => o.push((k.to_string(), v)) }
}

/// An object as json.loads makes a dict of it: one of each key, first place, last value.
pub fn dict(m: &[(String, Json)]) -> Obj {
    let mut o = Obj::new();
    for (k, v) in m { set(&mut o, k, v.clone()); }
    o
}

/// bool(v)
pub fn truthy(v: &Json) -> bool {
    match v { Json::Arr(a) => !a.is_empty(), Json::Obj(m) => !m.is_empty(), v => v.truthy() }
}

/// int(v), or None where Python raises: whole numbers from bools, numbers (cut toward zero) and strings like " -12 "
/// or "1_000". Out-of-range values saturate (they are clamped next anyway).
fn py_int(v: &Json) -> Option<i64> {
    match v {
        Json::Bool(b) => Some(*b as i64),
        Json::Num(f) if f.is_nan() => None,
        Json::Num(f) => Some(f.trunc().clamp(i64::MIN as f64, i64::MAX as f64) as i64),
        Json::Str(s) => {
            let t = py_strip(s);
            let (neg, digits) = match t.as_bytes().first() {
                Some(b'-') => (true, &t[1..]), Some(b'+') => (false, &t[1..]), _ => (false, t),
            };
            // digits, single underscores only between two of them
            let b = digits.as_bytes();
            if b.is_empty() || b[0] == b'_' || b[b.len() - 1] == b'_' || digits.contains("__") { return None; }
            let mut n: i128 = 0;
            for c in b {
                match c { b'_' => {}, b'0'..=b'9' => n = (n * 10 + (c - b'0') as i128).min(i64::MAX as i128 + 1), _ => return None }
            }
            let n = if neg { -n } else { n };
            Some(n.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
        }
        _ => None,
    }
}

fn has(list: &[(&str, &str)], v: &str) -> bool { list.iter().any(|(k, _)| *k == v) }
fn in_keys(o: &Json, v: &str) -> bool { o.entries().iter().any(|(k, _)| k == v) }
fn str_obj(list: &[(&str, &str)]) -> Json { Json::Obj(list.iter().map(|(k, v)| (k.to_string(), Json::Str(v.to_string()))).collect()) }

// ---------------------------------------------------------------- what looks.js offers (themes.py)
pub struct Schema {
    /// theme, palette, rain, font, transition -> {name: about}
    pub kinds: Obj,
    /// <background>_<option> -> {choice: about}; the first choice is the default
    pub options: Obj,
    /// background -> [its option settings]
    pub options_of: Obj,
    /// each theme's own background
    pub theme_rain: Obj,
    pub defaults: Obj,
}

impl Schema {
    pub fn from_file(looks_js: &Path) -> Result<Schema, String> { Ok(Schema::new(&crate::looks::Looks::load(looks_js)?.all)) }

    pub fn new(looks: &Json) -> Schema {
        let about = |m: &Json, field: &str| -> Obj { dict(m.entries()).into_iter().map(|(k, t)| (k, t.get(field).clone())).collect() };
        let with_theme = |first: &str, rest: Obj| -> Json {
            let mut o = vec![("theme".to_string(), Json::Str(first.into()))];
            for (k, v) in rest { set(&mut o, &k, v); }
            Json::Obj(o)
        };
        let (themes, bgs) = (looks.get("themes"), looks.get("backgrounds"));
        let kinds = vec![
            ("theme".to_string(), Json::Obj(about(themes, "about"))),
            ("palette".to_string(), with_theme("the theme's own colors", about(themes, "palette"))),
            ("rain".to_string(), with_theme("whatever the theme uses", about(bgs, "about"))),
            ("font".to_string(), with_theme("the theme's own pair (title + text)", about(looks.get("fonts"), "about"))),
            ("transition".to_string(), with_theme("whatever the theme uses", about(looks.get("transitions"), "about"))),
        ];
        let (mut options, mut options_of) = (Obj::new(), Obj::new());
        for (bg, b) in dict(bgs.entries()) {
            let opts = dict(b.get("options").entries());
            for (name, choices) in &opts { set(&mut options, &format!("{bg}_{name}"), Json::Obj(dict(choices.entries()))); }
            if !opts.is_empty() {
                set(&mut options_of, &bg, Json::Arr(opts.iter().map(|(n, _)| Json::Str(format!("{bg}_{n}"))).collect()));
            }
        }
        let n = |v: i64| Json::Num(v as f64);
        let s = |v: &str| Json::Str(v.into());
        let mut defaults: Obj = vec![
            ("port".into(), n(DEFAULT_PORT as i64)), ("commands_only".into(), Json::Bool(false)), ("paused".into(), Json::Bool(false)),
            ("mask_user".into(), Json::Bool(true)), ("mask_email".into(), Json::Bool(true)), ("masked".into(), s("dots")),
            ("label".into(), s(DEFAULT_LABEL)), ("mode".into(), s("live")), ("theme".into(), s("phosphor")),
            ("palette".into(), s("theme")), ("rain".into(), s("theme")), ("font".into(), s("theme")), ("transition".into(), s("theme")),
            ("type_speed".into(), n(5)), ("linger".into(), n(7)), ("cards".into(), n(4)), ("intensity".into(), n(5)),
            ("glass".into(), n(58)), ("grain".into(), n(5)), ("corner".into(), s("top-left")), ("screens".into(), s("all")),
            ("renderer".into(), s("native")), ("tray".into(), Json::Bool(true)),
        ];
        for (k, _, on) in AGENTS { defaults.push((k.into(), Json::Bool(on))); }
        for (k, choices) in &options {
            if let Some((first, _)) = choices.entries().first() { set(&mut defaults, k, Json::Str(first.clone())); }
        }
        let theme_rain = about(themes, "rain");
        Schema { kinds, options, options_of, theme_rain, defaults }
    }

    pub fn default(&self, k: &str) -> Json { get(&self.defaults, k).cloned().unwrap_or(Json::Null) }

    /// s.get(k, DEFAULTS[k])
    fn val(&self, s: &Json, k: &str) -> Json { get(s.entries(), k).cloned().unwrap_or_else(|| self.default(k)) }

    /// The settings: the defaults with settings.json on top (`saved`: None when it can't be read, which counts as
    /// paused). A background option whose choices changed since it was saved goes back to its default.
    pub fn load(&self, saved: Option<&Json>) -> Json {
        let mut s = self.defaults.clone();
        match saved {
            Some(Json::Obj(m)) => for (k, v) in dict(m) { set(&mut s, &k, v); },
            _ => set(&mut s, "paused", Json::Bool(true)),
        }
        for (k, choices) in &self.options {
            let ok = matches!(get(&s, k), Some(Json::Str(v)) if in_keys(choices, v));
            if !ok { set(&mut s, k, self.default(k)); }
        }
        Json::Obj(s)
    }

    /// Changes from the settings window: only known keys, only valid values. Port and screens stay command-line
    /// only. Returns the new settings and the keys refused.
    pub fn apply_changes(&self, s: &Json, changes: &Json) -> (Json, Vec<String>) {
        let (mut out, mut refused) = (dict(s.entries()), Vec::new());
        for (k, v) in dict(changes.entries()) {
            let st = v.str();
            let ok = if let Some(names) = get(&self.kinds, &k) {
                st.is_some_and(|x| in_keys(names, x))
            } else if k == "mode" {
                st.is_some_and(|x| has(&MODES, x))
            } else if FLAGS.contains(&k.as_str()) || AGENTS.iter().any(|(a, _, _)| *a == k) {
                matches!(v, Json::Bool(_))
            } else if let Some((_, _, lo, hi)) = TUNE.iter().find(|(t, ..)| *t == k) {
                // an int, not a bool (JSON can't tell 3 from 3.0 here: a whole number is taken as an int)
                matches!(v, Json::Num(f) if f.fract() == 0.0 && (*lo as f64..=*hi as f64).contains(&f))
            } else if let Some(choices) = get(&self.options, &k) {
                st.is_some_and(|x| in_keys(choices, x))
            } else if k == "corner" {
                st.is_some_and(|x| has(&CORNERS, x))
            } else if k == "masked" {
                st.is_some_and(|x| has(&MASKED, x))
            } else if k == "label" {
                st.is_some_and(|x| x.chars().count() <= 80 && !x.contains('\n'))
            } else { false };
            if ok { set(&mut out, &k, v); } else { refused.push(k); }
        }
        (Json::Obj(out), refused)
    }

    /// RESET ALL: everything back to the defaults except how it is installed, the mode and pause.
    pub fn reset(&self, s: &Json) -> Json {
        let mut out = self.defaults.clone();
        for k in KEEP_ON_RESET { if let Some(v) = get(s.entries(), k) { set(&mut out, k, v.clone()); } }
        Json::Obj(out)
    }

    /// What settings.json gets: the known settings, in their order, as json.dumps(indent=2) writes them.
    pub fn file_text(&self, s: &Json) -> String {
        let o: Obj = self.defaults.iter().filter_map(|(k, _)| get(s.entries(), k).map(|v| (k.clone(), v.clone()))).collect();
        let mut t = String::new();
        dump_indent(&Json::Obj(o), 0, &mut t);
        t + "\n"
    }

    // ------------------------------------------------ what the page is sent
    /// The slider settings as the page names them, each kept in its range.
    pub fn tune(&self, s: &Json) -> Json {
        Json::Obj(TUNE.iter().map(|(k, name, lo, hi)| {
            let v = py_int(&self.val(s, k)).map(|v| v.clamp(*lo, *hi)).unwrap_or_else(|| self.default(k).num().unwrap_or(0.0) as i64);
            (name.to_string(), Json::Num(v as f64))
        }).collect())
    }

    /// Each background's own options: {"orb": {"spin": "both", ...}}.
    pub fn opts(&self, s: &Json) -> Json {
        Json::Obj(self.options_of.iter().map(|(bg, keys)| {
            (bg.clone(), Json::Obj(keys.items().iter().filter_map(|k| k.str()).map(|k| {
                let v = self.val(s, k);
                let ok = matches!(&v, Json::Str(x) if get(&self.options, k).is_some_and(|c| in_keys(c, x)));
                (k[bg.len() + 1..].to_string(), if ok { v } else { self.default(k) })
            }).collect()))
        }).collect())
    }

    /// The theme/palette/rain/font/transition part, cleaned up (themes.look).
    pub fn look(&self, s: &Json) -> Json {
        Json::Obj(self.kinds.iter().map(|(k, names)| {
            let v = text(s, k).to_lowercase();
            let v = if in_keys(names, &v) { v } else if k == "theme" { "phosphor".into() } else { "theme".into() };
            (k.clone(), Json::Str(v))
        }).collect())
    }

    /// The corner label. A label you set always wins; the stock one says which mode is on.
    pub fn display_label(&self, s: &Json) -> Json {
        let label = get(s.entries(), "label").filter(|v| truthy(v)).cloned().unwrap_or(Json::Str(DEFAULT_LABEL.into()));
        if label != Json::Str(DEFAULT_LABEL.into()) { return label; }
        Json::Str(match mode(s).as_str() { "ambient" => AMBIENT_LABEL.into(), "art" => String::new(), _ => self.live_label(s) })
    }

    /// The stock live label names the agents being read, so it never claims one that's off.
    pub fn live_label(&self, s: &Json) -> String {
        let on: Vec<String> = self.agents_on(s).iter()
            .map(|k| AGENTS.iter().find(|(a, ..)| a == k).unwrap().1.split_whitespace().next().unwrap_or("").to_uppercase()).collect();
        if on.is_empty() { return "LIVE :: no agent turned on (settings > AGENTS)".into(); }
        on.join(" + ") + " :: live tool feed"
    }

    /// The agents whose logs live mode reads: on only when the setting is exactly true (missing: its default).
    pub fn agents_on(&self, s: &Json) -> Vec<&'static str> {
        AGENTS.iter().filter(|(k, ..)| self.val(s, k) == Json::Bool(true)).map(|(k, ..)| *k).collect()
    }

    /// The settings window's answer: the settings, what was refused, and what it may offer.
    pub fn window(&self, s: &Json, refused: &[String], restarting: bool) -> Json {
        let o = |k: &str, v: Json| (k.to_string(), v);
        let mut choices = self.kinds.clone();
        choices.extend(self.options.iter().cloned());
        choices.extend([o("mode", str_obj(&MODES)), o("corner", str_obj(&CORNERS)), o("masked", str_obj(&MASKED))]);
        Json::Obj(vec![
            o("settings", Json::Obj(self.defaults.iter().map(|(k, d)| (k.clone(), get(s.entries(), k).cloned().unwrap_or(d.clone()))).collect())),
            o("refused", Json::Arr(refused.iter().map(|k| Json::Str(k.clone())).collect())),
            o("defaults", Json::Obj(self.defaults.iter().filter(|(k, _)| !NOT_RESETTABLE.contains(&k.as_str())).cloned().collect())),
            o("choices", Json::Obj(choices)),
            o("options", Json::Obj(self.options_of.clone())),
            o("theme_rain", Json::Obj(self.theme_rain.clone())),
            o("ranges", Json::Obj(TUNE.iter().map(|(k, _, lo, hi)| (k.to_string(), Json::Arr(vec![Json::Num(*lo as f64), Json::Num(*hi as f64)]))).collect())),
            o("running", Json::Bool(true)),
            o("restarting", Json::Bool(restarting)),
        ])
    }
}

/// str(s.get(k, ""))
fn text(s: &Json, k: &str) -> String { get(s.entries(), k).map(py_str).unwrap_or_default() }

pub fn mode(s: &Json) -> String {
    let m = text(s, "mode").to_lowercase();
    if has(&MODES, &m) { m } else { "live".into() }
}

pub fn corner(s: &Json) -> String {
    let c = text(s, "corner").to_lowercase();
    if has(&CORNERS, &c) { c } else { "top-left".into() }
}

pub fn masked(s: &Json) -> String {
    let m = text(s, "masked");
    if has(&MASKED, &m) { m } else { "dots".into() }
}

// ---------------------------------------------------------------- the file
/// What settings.json holds: {} if there is none yet, None if it's there but unreadable (a bad hand edit).
pub fn read_saved(file: &Path) -> Option<Json> {
    match std::fs::read_to_string(file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Json::Obj(Vec::new())),
        Err(_) => None,
        Ok(t) => Json::parse(t.strip_prefix('\u{feff}').unwrap_or(&t)).ok().filter(|j| matches!(j, Json::Obj(_))),
    }
}

static TMP: AtomicU64 = AtomicU64::new(0);

/// Writes a file through a temp file swapped in, so the feed never reads a half-written settings file (which
/// would read as the defaults: not paused). Windows refuses the swap while a reader has the file open for a moment.
pub fn private_write(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let written = o.open(&tmp).and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    if let Err(e) = written { let _ = std::fs::remove_file(&tmp); return Err(e); }
    let mut tries = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if tries >= 39 || e.kind() == std::io::ErrorKind::NotFound => { let _ = std::fs::remove_file(&tmp); return Err(e); }
            Err(_) => { tries += 1; std::thread::sleep(Duration::from_millis(50)); }
        }
    }
}

/// json.dumps(v, indent=2): ASCII only (ensure_ascii), as Python writes settings.json.
fn dump_indent(v: &Json, level: usize, o: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        Json::Arr(a) if !a.is_empty() => {
            o.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                if i > 0 { o.push_str(",\n"); }
                o.push_str(&pad(level + 1));
                dump_indent(x, level + 1, o);
            }
            o.push('\n'); o.push_str(&pad(level)); o.push(']');
        }
        Json::Obj(m) if !m.is_empty() => {
            o.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 { o.push_str(",\n"); }
                o.push_str(&pad(level + 1));
                ascii_str(k, o);
                o.push_str(": ");
                dump_indent(x, level + 1, o);
            }
            o.push('\n'); o.push_str(&pad(level)); o.push('}');
        }
        Json::Str(s) => ascii_str(s, o),
        other => o.push_str(&crate::logs::dumps(other)),
    }
}

/// The file the wallpaper loads to find the feed (settings.config_js). Holds the key: never publish it.
pub fn config_js(port: u16, key: &str, label: &str) -> String {
    let (mut k, mut l) = (String::new(), String::new());
    ascii_str(key, &mut k);
    ascii_str(label, &mut l);
    format!("window.FEED = {{\"port\": {port}, \"key\": {k}, \"label\": {l}}};\n")
}

pub fn ascii_str(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"), '\x08' => o.push_str("\\b"), '\x0c' => o.push_str("\\f"),
            ' '..='~' => o.push(c),
            c => { let mut b = [0u16; 2]; for u in c.encode_utf16(&mut b) { o.push_str(&format!("\\u{u:04x}")); } }
        }
    }
    o.push('"');
}

// ---------------------------------------------------------------- re-read on change
/// The settings, read again whenever the file changes, so pause and the rest act without a restart (LiveSettings).
/// If the file turns unreadable the last good settings stay; one that's unreadable from the start counts as paused.
pub struct Live { pub schema: Arc<Schema>, file: Option<PathBuf>, mtime: Option<SystemTime>, s: Json, good: bool }

impl Live {
    pub fn new(schema: Arc<Schema>, file: PathBuf) -> Live {
        let s = schema.load(read_saved(&file).as_ref());
        Live { schema, file: Some(file), mtime: None, s, good: false }
    }

    /// Settings that never change (the demo: the defaults, whatever settings.json says).
    pub fn fixed(schema: Arc<Schema>, s: Json) -> Live { Live { schema, file: None, mtime: None, s, good: true } }

    pub fn get(&mut self) -> &Json {
        let Some(file) = self.file.clone() else { return &self.s };
        let m = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        if m != self.mtime {
            self.mtime = m;
            let saved = read_saved(&file);
            if saved.is_some() || !self.good {
                self.s = self.schema.load(saved.as_ref());
                self.good = saved.is_some();
            }
        }
        &self.s
    }

    pub fn flag(&mut self, k: &str) -> bool { get(self.get().entries(), k).is_some_and(truthy) }
    pub fn mode(&mut self) -> String { mode(self.get()) }
    pub fn agents_on(&mut self) -> Vec<&'static str> { let s = self.get().clone(); self.schema.agents_on(&s) }
}

/// The repo's looks.js, for the tests.
#[cfg(test)]
pub fn test_schema() -> Arc<Schema> {
    Arc::new(Schema::from_file(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/glitchwall/wallpaper/looks.js")).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Arc<Schema> { test_schema() }
    fn j(t: &str) -> Json { Json::parse(t).unwrap() }
    fn with(sc: &Schema, t: &str) -> Json { sc.load(Some(&j(t))) }

    #[test]
    fn defaults_like_python() {
        let sc = schema();
        let d = &sc.defaults;
        assert_eq!(&d[..3], &[("port".into(), Json::Num(47833.0)), ("commands_only".into(), Json::Bool(false)), ("paused".into(), Json::Bool(false))]);
        assert_eq!(get(d, "agent_codex"), Some(&Json::Bool(false)));
        assert_eq!(get(d, "orb_spin").and_then(|v| v.str()).is_some(), get(&sc.options, "orb_spin").is_some());
        assert!(get(&sc.kinds, "theme").unwrap().get("phosphor").str().is_some());
        assert_eq!(get(&sc.kinds, "rain").unwrap().entries()[0].0, "theme");
    }

    #[test]
    fn apply_changes_validates() {   // test_server.py's test_apply_changes_validates, case for case
        let sc = schema();
        let (s, refused) = sc.apply_changes(&Json::Obj(sc.defaults.clone()), &j(r#"{"theme": "phosphor", "mode": "ambient",
            "paused": true, "cards": 3, "label": "hi", "glass": 5, "cards_x": 1, "port": 1, "host": "x", "screens": "primary",
            "theme_bad": 1, "mask_user": "yes", "linger": true, "label2": "x"}"#));
        assert_eq!((s.get("mode").str(), s.get("paused"), s.get("cards").num(), s.get("label").str()),
                   (Some("ambient"), &Json::Bool(true), Some(3.0), Some("hi")));
        assert_eq!(s.get("glass").num(), Some(58.0));
        assert_eq!(refused, ["glass", "cards_x", "port", "host", "screens", "theme_bad", "mask_user", "linger", "label2"]);
        let empty = Json::Obj(vec![]);
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"label": "a\nb"}"#)).1, ["label"]);
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"theme": "no-such-theme"}"#)).1, ["theme"]);
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"corner": "bottom-left"}"#)).0.get("corner").str(), Some("bottom-left"));
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"corner": "middle"}"#)).1, ["corner"]);
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"agent_gemini": true}"#)).0, j(r#"{"agent_gemini": true}"#));
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"agent_codex": "yes"}"#)).1, ["agent_codex"]);
        // values Python would choke on (unhashable) are simply refused
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"mode": [], "corner": {}, "masked": null, "orb_spin": 1}"#)).1, ["mode", "corner", "masked", "orb_spin"]);
        assert_eq!(sc.apply_changes(&empty, &j(r#"{"label": "x", "label": "y"}"#)).0, j(r#"{"label": "y"}"#));
        assert_eq!(sc.apply_changes(&empty, &j(&format!(r#"{{"label": "{}"}}"#, "é".repeat(80)))).1, Vec::<String>::new());
        assert_eq!(sc.apply_changes(&empty, &j(&format!(r#"{{"label": "{}"}}"#, "é".repeat(81)))).1, ["label"]);
    }

    #[test]
    fn tune_is_clamped() {
        let sc = schema();
        let t = sc.tune(&j(r#"{"linger": 99, "cards": 0, "glass": "x", "intensity": 3, "grain": " 7 ", "type_speed": 2.9}"#));
        assert_eq!(t, j(r#"{"typeSpeed": 2, "linger": 30, "cards": 1, "rain": 3, "glass": 58, "grain": 7}"#));
        assert_eq!(sc.tune(&j(r#"{"cards": true, "linger": null, "glass": "1_00", "grain": "1__0"}"#)).get("cards").num(), Some(1.0));
        let t = sc.tune(&j(r#"{"linger": null, "glass": "1_00", "grain": "1__0", "intensity": 1e400}"#));
        assert_eq!((t.get("linger").num(), t.get("glass").num(), t.get("grain").num(), t.get("rain").num()), (Some(7.0), Some(95.0), Some(5.0), Some(10.0)));
    }

    #[test]
    fn labels() {   // test_settings.py's TestLabel
        let sc = schema();
        let label = |t: &str| sc.display_label(&with(&sc, t));
        assert_eq!(label("{}"), j(r#""CLAUDE :: live tool feed""#));
        assert_eq!(label(r#"{"agent_claude": false, "agent_codex": true}"#), j(r#""CODEX :: live tool feed""#));
        assert_eq!(label(r#"{"agent_gemini": true, "agent_codex": true}"#), j(r#""CLAUDE + GEMINI + CODEX :: live tool feed""#));
        assert_eq!(label(r#"{"agent_claude": false, "agent_gemini": "yes"}"#), j(r#""LIVE :: no agent turned on (settings > AGENTS)""#));
        assert_eq!(label(r#"{"label": "MINE", "agent_claude": false}"#), j(r#""MINE""#));
        assert_eq!(label(r#"{"mode": "ambient", "agent_claude": false}"#), Json::Str(AMBIENT_LABEL.into()));
        assert_eq!(label(r#"{"mode": "ART"}"#), j(r#""""#));
        assert_eq!(label(r#"{"label": ""}"#), j(r#""CLAUDE :: live tool feed""#));
        assert_eq!(label(r#"{"label": 5}"#), j("5"));
    }

    #[test]
    fn look_opts_corner_masked() {
        let sc = schema();
        let s = with(&sc, r#"{"theme": "NEON", "palette": "nope", "font": "doto", "corner": "Bottom-Right", "masked": "Words", "orb_spin": "x"}"#);
        assert_eq!(sc.look(&s), j(r#"{"theme": "neon", "palette": "theme", "rain": "theme", "font": "doto", "transition": "theme"}"#));
        assert_eq!((corner(&s), masked(&s), mode(&s)), ("bottom-right".into(), "dots".into(), "live".into()));
        let o = sc.opts(&s);
        assert_eq!(o.get("orb").get("spin"), &sc.default("orb_spin"));
        assert_eq!(sc.look(&j("{}")).get("theme").str(), Some("phosphor"));
    }

    #[test]
    fn agents_off_until_turned_on() {
        let sc = schema();
        assert_eq!(sc.agents_on(&with(&sc, "{}")), ["agent_claude"]);
        assert_eq!(sc.agents_on(&with(&sc, r#"{"agent_gemini": true, "agent_claude": false}"#)), ["agent_gemini"]);
        assert_eq!(sc.agents_on(&with(&sc, r#"{"agent_codex": "yes", "agent_gemini": 1, "agent_claude": null}"#)), Vec::<&str>::new());
    }

    #[test]
    fn reset_keeps_mode_and_pause() {
        let sc = schema();
        let s = with(&sc, r#"{"theme": "neon", "cards": 1, "mode": "ambient", "paused": true, "port": 5000}"#);
        let r = sc.reset(&s);
        assert_eq!((r.get("theme").str(), r.get("cards").num(), r.get("mode").str(), r.get("paused"), r.get("port").num()),
                   (Some("phosphor"), Some(4.0), Some("ambient"), &Json::Bool(true), Some(5000.0)));
    }

    #[test]
    fn file_text_like_json_dumps_indent() {
        let sc = schema();
        let s = j(r#"{"label": "é😀\u0001\"", "port": 1, "junk": 1, "cards": [], "glass": {"a": [1, 2.5]}}"#);
        assert_eq!(sc.file_text(&s), "{\n  \"port\": 1,\n  \"label\": \"\\u00e9\\ud83d\\ude00\\u0001\\\"\",\n  \"cards\": [],\n  \"glass\": {\n    \"a\": [\n      1,\n      2.5\n    ]\n  }\n}\n");
    }

    #[test]
    fn broken_file_counts_as_paused_and_live_keeps_last_good() {
        let sc = schema();
        let dir = std::env::temp_dir().join(format!("gw-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        assert!(!truthy(sc.load(read_saved(&f).as_ref()).get("paused")));   // missing: the defaults
        for bad in ["{not json", "5", "[1, 2]", "{\"a\": NaN}"] {
            std::fs::write(&f, bad).unwrap();
            assert!(truthy(sc.load(read_saved(&f).as_ref()).get("paused")), "{bad}");
        }
        std::fs::write(&f, "\u{feff}{\"theme\": \"neon\"}").unwrap();   // PowerShell's BOM
        assert_eq!(sc.load(read_saved(&f).as_ref()).get("theme").str(), Some("neon"));
        private_write(&f, &sc.file_text(&with(&sc, r#"{"paused": true, "theme": "neon"}"#))).unwrap();
        let mut live = Live::new(sc.clone(), f.clone());
        assert!(live.flag("paused"));
        std::fs::write(&f, "{oops").unwrap();
        let old = std::fs::File::options().write(true).open(&f).unwrap();
        old.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)).unwrap();   // a new mtime, so it's read again
        drop(old);
        assert!(live.flag("paused"));
        assert_eq!(live.get().get("theme").str(), Some("neon"));
        private_write(&f, &sc.file_text(&with(&sc, r#"{"paused": false}"#))).unwrap();
        assert!(!live.flag("paused"));
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn py_int_like_python() {
        let n = |t: &str| py_int(&j(t));
        assert_eq!((n("\" 12 \""), n("\"-3\""), n("\"+4\""), n("\"1_000\""), n("true"), n("-2.7")), (Some(12), Some(-3), Some(4), Some(1000), Some(1), Some(-2)));
        for bad in ["\"\"", "\"3.0\"", "\"_1\"", "\"1_\"", "\"1__0\"", "null", "[]", "\"x\"", "\"- 1\""] { assert_eq!(n(bad), None, "{bad}"); }
    }
}
