//! The scan loop the Gemini CLI and Codex CLI readers share: a port of glitchwall's tail.py. Each second only the
//! logs being followed and the recent ones are looked at, and every log once a minute (SWEEP); a log that goes
//! quiet stops being followed, and an old log that grows has only what was added read. Files are opened read-only.
#![allow(dead_code)]

use crate::logs::{ACTIVE, FIRST_TAIL};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SWEEP: f64 = 60.0;

pub struct Follow {
    pub offsets: HashMap<PathBuf, u64>,
    pub idle: HashMap<PathBuf, u64>,
    pub swept: f64,
}

pub fn now_secs() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() }

/// Seconds since a file or folder last changed (0 when that can't be told, as if just now).
pub fn age(m: &std::fs::Metadata, now: f64) -> f64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |d| now - d.as_secs_f64())
}

impl Follow {
    pub fn new() -> Follow { Follow { offsets: HashMap::new(), idle: HashMap::new(), swept: -SWEEP } }

    /// True when it's time to look at every log again (and counts that look as done).
    pub fn due(&mut self, now: f64) -> bool {
        if now - self.swept < SWEEP { return false; }
        self.swept = now;
        true
    }

    pub fn scan(&mut self, files: Vec<PathBuf>, everything: bool, now: f64, parse: &mut impl FnMut(&[u8], &Path, Option<f64>)) {
        let files: BTreeSet<PathBuf> = files.into_iter().chain(self.offsets.keys().cloned()).collect();
        if everything { self.idle.retain(|f, _| files.contains(f)); }
        for f in files {
            let st = std::fs::metadata(&f).ok().filter(|m| m.is_file());
            let Some(st) = st else { self.offsets.remove(&f); self.idle.remove(&f); continue };
            let size = st.len();
            let old = age(&st, now) > ACTIVE;
            let mut newer_than = None;
            let mut pos = match self.offsets.get(&f).copied() {
                None => {
                    let p = if old {
                        let was = self.idle.insert(f.clone(), size);
                        match was { Some(w) if size > w => w, _ => continue }   // an old log that grew: only what was added
                    } else {
                        newer_than = Some(now - ACTIVE);   // first sight: only its recent calls
                        size.saturating_sub(FIRST_TAIL)
                    };
                    self.idle.remove(&f);
                    p
                }
                Some(p) => {
                    if everything && size == p && old {   // gone quiet: stop following, remember its size
                        self.offsets.remove(&f);
                        self.idle.insert(f, size);
                        continue;
                    }
                    p
                }
            };
            if size < pos { pos = 0; }   // rewritten in place
            if size == pos { self.offsets.insert(f, pos); continue; }
            let mut chunk = Vec::new();
            let read = std::fs::File::open(&f)   // read-only
                .and_then(|mut h| { h.seek(SeekFrom::Start(pos))?; h.read_to_end(&mut chunk) });
            if read.is_err() { continue; }   // locked, or gone since: try again next time
            let Some(last_nl) = chunk.iter().rposition(|&b| b == b'\n') else {
                if newer_than.is_none() { self.offsets.insert(f, pos); }
                continue;
            };
            self.offsets.insert(f.clone(), pos + last_nl as u64 + 1);
            for line in chunk[..last_nl].split(|&b| b == b'\n') { parse(line, &f, newer_than); }
        }
    }
}

fn dirs_of(d: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    v
}

/// The n newest folders `depth` levels down, by name (Codex's sessions/YYYY/MM/DD), newest first.
pub fn newest_dirs(root: &Path, depth: usize, n: usize) -> Vec<PathBuf> {
    fn walk(d: &Path, level: usize, depth: usize, n: usize, out: &mut Vec<PathBuf>) {
        for p in dirs_of(d) {
            if out.len() >= n { return; }
            if level == depth { out.push(p); } else { walk(&p, level + 1, depth, n, out); }
        }
    }
    let mut out = Vec::new();
    walk(root, 1, depth, n, &mut out);
    out
}

/// The *.jsonl files in a folder.
pub fn jsonl_in(d: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(f: &Path, text: &[&str]) { std::fs::write(f, text.iter().map(|t| format!("{t}\n")).collect::<String>()).unwrap(); }
    fn set_old(f: &Path) {
        let t = SystemTime::now() - std::time::Duration::from_secs_f64(ACTIVE + 60.0);
        std::fs::File::options().write(true).open(f).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn old_logs_quiet_logs_and_newest_days() {
        let root = std::env::temp_dir().join(format!("gw-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["2025/12/31", "2026/10/01", "2026/11/23", "2026/11/24"] { std::fs::create_dir_all(root.join(d)).unwrap(); }
        let names: Vec<String> = newest_dirs(&root, 3, 2).iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["2026/11/24", "2026/11/23"]);

        let mut got: Vec<String> = Vec::new();
        let mut parse = |l: &[u8], _: &Path, _: Option<f64>| got.push(String::from_utf8_lossy(l).into_owned());
        let mut fw = Follow::new();
        // an old log: skipped, then followed once it grows, from where it was
        let f = root.join("2026/11/24/old.jsonl");
        lines(&f, &["before"]);
        set_old(&f);
        let now = now_secs();
        fw.scan(vec![f.clone()], fw.swept + SWEEP <= now, now, &mut parse);
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"after\n").unwrap();
        set_old(&f);
        fw.scan(vec![f.clone()], false, now_secs(), &mut parse);
        // followed, gone quiet at a sweep, then grows again
        set_old(&f);
        fw.scan(vec![], true, now_secs(), &mut parse);
        assert!(!fw.offsets.contains_key(&f));
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"again\n").unwrap();
        set_old(&f);
        fw.scan(vec![f.clone()], false, now_secs(), &mut parse);
        drop(parse);
        assert_eq!(got, ["after", "again"]);
        let _ = std::fs::remove_dir_all(&root);
    }
    use std::io::Write;
}
