//! Mask secrets and personal paths before anything reaches the wallpaper: a port of glitchwall's redact.py, kept
//! to the same output case for case (tests/fixtures/redact.json, recorded from the Python, is the yardstick).
//!
//! Three layers, applied in order:
//!   1. known secret formats (cloud keys, vendor tokens, JWTs, private keys, ...)
//!   2. "name = value" style assignments, env exports, CLI flags and HTTP headers whose name says the value is
//!      secret (password, token, api_key, ...)
//!   3. a high-entropy catch for anything token-shaped that slipped through
//!
//! Plus: the home directory becomes "~", the user name and email local parts are masked.
//!
//! The goal is "never show a secret", not "never show anything odd", so this errs toward masking; a pattern that
//! gives up partway (fancy-regex's backtracking limit) masks the rest of the text.
#![allow(dead_code)]

use fancy_regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const MASK: &str = "••••";

/// Terminal escapes and invisible format characters out, as redact.py's clean(): junk on the wall, and one inside
/// a secret would split it so the patterns miss it. Tabs and newlines stay; a carriage return becomes a newline.
pub fn clean(text: &str) -> String {
    static ESC: OnceLock<Regex> = OnceLock::new();
    let esc = ESC.get_or_init(|| Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])?").unwrap());
    let t = sub(esc, text, |_| String::new()).replace("\r\n", "\n").replace('\r', "\n");
    t.chars().filter(|&c| !invisible(c)).collect()
}

fn invisible(c: char) -> bool {
    matches!(c, '\x00'..='\x08' | '\x0b' | '\x0c' | '\x0e'..='\x1f' | '\x7f'..='\u{9f}' | '\u{ad}'
        | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

// ---------------------------------------------------------------- 1. known formats
// Python's re.S is (?s) here, and its \Z is \z.
const KNOWN: &[&str] = &[
    // private key blocks (PEM, OpenSSH, PGP, encrypted). Unterminated blocks mask to the end.
    r"-----BEGIN[A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END[A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\z)",
    r"PuTTY-User-Key-File-\d+:.*?(?:Private-MAC:\s*\S+|\z)",
    r"\bAGE-SECRET-KEY-1[0-9A-Z]{50,}",
    // AWS access key ids
    r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ABIA|ACCA)[0-9A-Z]{16}\b",
    // Google API key, OAuth client secret / refresh token
    r"\bAIza[0-9A-Za-z_-]{35}\b",
    r"\bGOCSPX-[0-9A-Za-z_-]{20,}",
    r"\b1//0[0-9A-Za-z_-]{30,}",
    r"\bya29\.[0-9A-Za-z_-]{20,}",
    // Azure storage / service bus keys and SAS signatures
    r"(?i)(?<=AccountKey=)[A-Za-z0-9+/=]{20,}",
    r"(?i)(?<=SharedAccessKey=)[A-Za-z0-9+/=]{20,}",
    r"(?i)(?<=[?&]sig=)[A-Za-z0-9%+/=]{16,}",
    // GitHub
    r"\bgh[pousr]_[A-Za-z0-9]{30,}\b",
    r"\bgithub_pat_[A-Za-z0-9_]{40,}\b",
    // GitLab
    r"\bgl(?:pat|dt|rt|ptt|cbt|oas|soat|ft|imt|agent|wt)-[A-Za-z0-9_-]{20,}",
    // Slack tokens and webhooks
    r"\bxox[abposre]-[A-Za-z0-9-]{10,}",
    r"\bxapp-\d-[A-Za-z0-9-]{10,}",
    r"hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9/_-]+",
    // Discord webhook and bot token
    r"discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_-]+",
    r"\b[MN][A-Za-z\d_-]{23,25}\.[A-Za-z\d_-]{6}\.[A-Za-z\d_-]{27,}\b",
    // Stripe
    r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{10,}\b",
    r"\bwhsec_[A-Za-z0-9]{20,}\b",
    // AI providers (Anthropic, OpenAI, ...)
    r"\bsk-(?:ant-|proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}",
    r"\bhf_[A-Za-z0-9]{30,}\b",
    r"\br8_[A-Za-z0-9]{30,}\b",
    r"\bgsk_[A-Za-z0-9]{40,}\b",
    // package registries
    r"\bnpm_[A-Za-z0-9]{36}\b",
    r"\bpypi-AgE[A-Za-z0-9_-]{50,}",
    r"\boy2[a-z0-9]{43}\b", // NuGet
    r"\bdckr_pat_[A-Za-z0-9_-]{20,}",
    // infra and SaaS
    r"\bdop_v1_[a-f0-9]{64}\b",
    r"\bdo[ar]_v1_[a-f0-9]{64}\b",
    r"\bhvs\.[A-Za-z0-9_-]{24,}",
    r"\bhv[br]\.[A-Za-z0-9_-]{24,}",
    r"\btskey-[A-Za-z0-9-]{10,}",
    r"\bshp(?:at|ss|ca|pa)_[a-fA-F0-9]{32}\b",
    r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
    r"\bkey-[0-9a-f]{32}\b",  // Mailgun
    r"\bSK[0-9a-fA-F]{32}\b", // Twilio
    r"\bdp\.(?:pt|st|ct|sa|scim|audit)\.[A-Za-z0-9]{30,}", // Doppler
    r"\bglc_[A-Za-z0-9+/=_-]{30,}", // Grafana cloud
    r"\bglsa_[A-Za-z0-9_]{30,}",    // Grafana service account
    r"\bnfp_[A-Za-z0-9]{30,}",      // Netlify
    r"\bvercel_[A-Za-z0-9]{20,}",
    r"\bfigd_[A-Za-z0-9_-]{30,}",
    r"\blin_api_[A-Za-z0-9]{30,}",
    r"\bntn_[A-Za-z0-9]{30,}|\bsecret_[A-Za-z0-9]{40,}", // Notion
    r"\bATATT3[A-Za-z0-9_=-]{30,}",                      // Atlassian
    r"\b\d{8,10}:AA[A-Za-z0-9_-]{30,}\b",                // Telegram bot
    r"\bEAA[A-Za-z0-9]{60,}",                            // Facebook
    // JSON Web Tokens (and other eyJ... base64 JSON blobs)
    r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?:\.[A-Za-z0-9_-]*)?",
];

// ---------------------------------------------------------------- 2. by name
// words that mark a value as secret when they appear in a variable / key / flag name
const SECRET_WORD: &str = concat!(
    r"(?:passw(?:or)?d|passwd|pass|pwd|passphrase|secret|token|api[_-]?key|apikey|access[_-]?key|",
    r"private[_-]?key|client[_-]?secret|credentials?|creds|auth(?!or)(?:[_-]?token)?|bearer|session[_-]?(?:id|key|token)|",
    r"cookie|signature|salt|seed[_-]?phrase|mnemonic|pin[_-]?code|otp|totp|webhook[_-]?url|dsn|conn(?:ection)?[_-]?str(?:ing)?|",
    r"license[_-]?key|encryption[_-]?key|master[_-]?key|signing[_-]?key)"
);
const VALUE: &str = r#"(?:"[^"\n]*"|'[^'\n]*'|`[^`\n]*`|[^\s,;&|)}\]"']+)"#;

/// The by-name patterns, with {N} for a key name holding one of the secret words and {V} for its value. Each
/// keeps its first group and masks the rest of the match, plus `tail` (an "@" kept after a URL's password).
const BY_NAME: &[(&str, &str)] = &[
    // name = value / name: value / "name": "value"  (also covers export X=, set X=, $env:X =, ENV X)
    // ":" counts only before a space or quote, so file:line references like auth.py:41: stay readable;
    // the gaps never cross a line break, so a diff's next "+" line isn't taken for the value
    (r#"(?i)(\b{N}["']?[ \t]*(?::=|=>|=|:(?=[\s"'`]))[ \t]*)({V})"#, ""),
    // PowerShell / dotenv / Dockerfile:  $env:X = 'v'   ENV X v   setx X v
    (r"(?i)(\b(?:ENV|setx|set-item\s+env:)\s+{N}\s+)({V})", ""),
    // CLI flags:  --password hunter2   --token=abc   -Password 'x'
    (r"(?i)((?<![\w-])--?{N}(?:=|\s+))({V})", ""),
    // mysql -pSECRET
    (r"(\bmysql\S*\s(?:.*\s)?-p)(\S+)", ""),
    // curl -u user:pass / --user user:pass
    (r#"((?:\s-u|--user)\s+["']?[^:\s"']+:)([^\s"']+)"#, ""),
    // HTTP headers
    (r#"(?i)\b((?:proxy-)?authorization\s*:\s*)(?:(?:bearer|basic|token|digest|negotiate)\s+)?[^\s"',]+"#, ""),
    (r#"(?i)\b((?:set-)?cookie\s*:\s*)[^\n"']+"#, ""),
    (r#"(?i)\b(x-[a-z-]*(?:key|token|secret|auth)[a-z-]*\s*:\s*)[^\s"',]+"#, ""),
    // credentials inside URLs, connection strings:  scheme://user:pass@host
    (r"(?i)\b([a-z][a-z0-9+.-]{0,30}://[^:/?#\s@]*:)[^@\s/]+@", "@"),
    // ADO.NET / ODBC style:  Password=x;  Pwd=x;
    (r#"(?i)\b((?:password|pwd|user\s*password)\s*=\s*)([^;"'\n]+)"#, ""),
    // .netrc:  machine x login y password z
    (r"(?i)(\bpassword\s+)(\S+)", ""),
    // bare Bearer tokens anywhere
    (r"(?i)(\bbearer\s+)[A-Za-z0-9._~+/=-]{8,}", ""),
    // passwords handed to a program as a plain argument
    (r"(\bsshpass\s+(?:-[^p\s]\S*\s+)*-p\s*)({V})", ""),
    (r"(\b(?:docker|podman|nerdctl|oras|helm\s+registry)\s+login\b[^\n|;&]*?\s-p\s+)({V})", ""),
    (r"(?m)(\bhtpasswd\b[^\n|;&]*?\s-\w*b\w*\s[^\n|;&]*\s)({V})(?=[ \t]*(?:$|[|;&]))", ""),
    (r#"(?i)(\bnet\s+user\s+(?:"[^"\n]*"|\S+)\s+)(?!/)({V})"#, ""),
    (r"(?i)(\bConvertTo-SecureString\s+(?:-String\s+)?)({V})", ""),
    // Windows-style switches:  cmdkey /pass:x   tool /password:x
    (r"(?i)((?<![\w/:.])/{N}[:=])({V})", ""),
    // net use Z: \\server\share PASSWORD /user:bob   (* asks for it instead)
    (r"(?i)(\bnet\s+use\s+(?:[a-z]:\s+)?\\\\\S+\s+)(?![/*])({V})", ""),
    // archivers:  7z a -pSECRET   rar a -pSECRET   zip -P SECRET
    (r"(\b(?:7z[ar]?|rar|unrar)\b[^\n|;&]*?\s-p)(\S+)", ""),
    (r"(\bzip\b[^\n|;&]*?\s-P\s+)({V})", ""),
    // netsh wlan show profile ... key=clear
    (r"(?i)(\bKey Content\s*:\s*)(\S.*)", ""),
];

// ---------------------------------------------------------------- 3. entropy, and personal info
const HEX: &str = r"(?<![A-Za-z0-9])[A-Fa-f0-9]{32,}(?![A-Za-z0-9])";
const TOKEN: &str = r"(?<![A-Za-z0-9+/=_-])[A-Za-z0-9+/=_-]{20,}(?![A-Za-z0-9+/=_-])";
const EMAIL: &str = r"\b[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+\.)+[A-Za-z]{2,}\b";

struct Pats { known: Vec<Regex>, by_name: Vec<(Regex, &'static str)>, hex: Regex, token: Regex, email: Regex }

fn pats() -> &'static Pats {
    static P: OnceLock<Pats> = OnceLock::new();
    P.get_or_init(|| {
        let rx = |p: &str| Regex::new(p).unwrap_or_else(|e| panic!("redact pattern {p}: {e}"));
        // at most 64 name characters either side of the word, as redact.py (unbounded, a long a-a-a-... run is slow)
        let name = format!("[A-Za-z0-9_.-]{{0,64}}{SECRET_WORD}[A-Za-z0-9_.-]{{0,64}}");
        Pats {
            known: KNOWN.iter().map(|p| rx(&format!("(?s){p}"))).collect(),
            by_name: BY_NAME.iter().map(|(p, tail)| (rx(&p.replace("{N}", &name).replace("{V}", VALUE)), *tail)).collect(),
            hex: rx(HEX),
            token: rx(TOKEN),
            email: rx(EMAIL),
        }
    })
}

/// Python's re.sub: each match swapped for what `f` makes of it. A pattern that gives up partway masks the rest.
fn sub(rx: &Regex, text: &str, mut f: impl FnMut(&Captures) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for c in rx.captures_iter(text) {
        let Ok(c) = c else { out.push_str(MASK); return out; };
        let m = c.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        out.push_str(&f(&c));
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    for ch in s.chars() { *counts.entry(ch).or_default() += 1; }
    let n = s.chars().count() as f64;
    if n == 0.0 { return 0.0; }
    -counts.values().map(|&c| { let p = c as f64 / n; p * p.log2() }).sum::<f64>()
}

/// 40 base64 characters with a / in them (an AWS secret access key, printed bare). Told from a path by how often
/// the letters flip case: random text flips about every other letter, words in a path about once each.
fn aws_secret_shaped(tok: &str) -> bool {
    if tok.len() != 40 || !tok.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        || !tok.contains('/') || tok.starts_with('/') {
        return false;
    }
    let letters: Vec<u8> = tok.bytes().filter(u8::is_ascii_alphabetic).collect();
    let flips = letters.windows(2).filter(|w| w[0].is_ascii_uppercase() != w[1].is_ascii_uppercase()).count();
    flips >= 10 && tok.bytes().any(|b| b.is_ascii_digit())
}

/// Token-shaped and random enough to be a key, not a word, path or identifier.
pub fn looks_random(tok: &str) -> bool {
    let parts: Vec<&str> = tok.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 && aws_secret_shaped(tok) { return true; }
    if parts.len() > 1 {   // a path: only mask if one of its pieces itself looks random
        return parts.iter().any(|p| p.chars().count() >= 20 && looks_random(p));
    }
    let core = tok.trim_matches('=');
    if core.chars().count() < 20 { return false; }
    let has = |f: fn(&char) -> bool| core.chars().any(|c| f(&c));
    if !(has(char::is_ascii_lowercase) && has(char::is_ascii_uppercase) && has(char::is_ascii_digit)) { return false; }
    // identifiers like Some_Long_Name_2024 are made of words; secrets are not
    let pieces: Vec<&str> = core.split(['_', '-']).collect();
    if pieces.len() > 1 && pieces.iter().map(|p| p.chars().count()).max().unwrap_or(0) < 16 { return false; }
    let digits = core.chars().filter(|c| c.is_numeric()).count();
    entropy(core) >= 3.6 && digits >= 2
}

fn home_variants(home: &str) -> Vec<String> {
    let mut out = vec![home.to_string(), home.replace('\\', "/"), home.replace('\\', "\\\\"), home.replace('/', "\\")];
    let b = home.as_bytes();
    if home.chars().count() > 2 && b.len() > 1 && b[1] == b':' {   // C:\Users\x -> /c/Users/x (Git Bash), C:/Users/x
        let rest = home[2..].replace('\\', "/");
        let drive = home[..1].to_lowercase();
        out.push(format!("/{drive}{rest}"));
        out.push(format!("/mnt/{drive}{rest}"));
    }
    out.retain(|v| !v.is_empty());
    out.sort();
    out.dedup();
    out
}

/// Windows' 8.3 form of the home folder (C:\Users\JOHNSM~1), which temp paths often use. "" elsewhere.
fn short_home(home: &str) -> String {
    if home.is_empty() { return String::new(); }
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    use windows::core::HSTRING;
    let mut buf = [0u16; 520];
    let n = unsafe { GetShortPathNameW(&HSTRING::from(home), Some(&mut buf)) } as usize;
    if n == 0 || n >= 520 { return String::new(); }
    let s = String::from_utf16_lossy(&buf[..n]);
    if s.to_lowercase() == home.to_lowercase() { String::new() } else { s }
}

pub struct Redactor { homes: Vec<Regex>, user: Option<Regex>, mask_email: bool, entropy_check: bool }

impl Redactor {
    pub fn new(home: &str, user: &str, mask_user: bool, mask_email: bool, entropy_check: bool) -> Redactor {
        Redactor::with_short(home, &short_home(home), user, mask_user, mask_email, entropy_check)
    }

    /// As new, but told the home folder's 8.3 short form ("" for none) instead of asking Windows.
    pub fn with_short(home: &str, short: &str, user: &str, mask_user: bool, mask_email: bool, entropy_check: bool) -> Redactor {
        let mut variants = home_variants(home);
        if !short.is_empty() { variants.extend(home_variants(short)); }
        variants.sort();
        variants.dedup();
        variants.sort_by_key(|v| std::cmp::Reverse(v.chars().count()));   // longest first
        let homes = if home.is_empty() { Vec::new() } else {
            variants.iter().map(|h| Regex::new(&format!("(?i){}(?![A-Za-z0-9_.-])", fancy_regex::escape(h))).unwrap()).collect()
        };
        // the user name, and its 8.3 short form (JOHNSM~1); also right after a URL-encoded \ or /
        let short_name = short.rsplit(['\\', '/']).next().unwrap_or("");
        let names: Vec<String> = [user, if short_name.contains('~') { short_name } else { "" }].iter()
            .filter(|n| n.chars().count() >= 4).map(|n| fancy_regex::escape(n).into_owned()).collect();
        let user = (mask_user && !names.is_empty()).then(|| Regex::new(&format!(
            "(?i)(?:(?<![A-Za-z0-9])|(?<=%5C)|(?<=%2F))(?:{})(?![A-Za-z0-9])", names.join("|"))).unwrap());
        Redactor { homes, user, mask_email, entropy_check }
    }

    /// With this machine's home folder and user name.
    pub fn for_machine() -> Redactor { Redactor::for_machine_with(true, true) }

    /// The same, with the user name and email masking as the settings have them.
    pub fn for_machine_with(mask_user: bool, mask_email: bool) -> Redactor {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| home.rsplit(['\\', '/']).next().unwrap_or("").to_string());
        Redactor::new(&home, &user, mask_user, mask_email, true)
    }

    pub fn redact(&self, text: &str) -> String {
        let p = pats();
        if text.is_empty() { return String::new(); }
        let mut t = clean(text);
        for rx in &self.homes { t = sub(rx, &t, |_| "~".into()); }
        for rx in &p.known { t = sub(rx, &t, |_| MASK.into()); }
        for (rx, tail) in &p.by_name { t = sub(rx, &t, |c| format!("{}{MASK}{tail}", &c[1])); }
        if self.entropy_check {
            t = sub(&p.hex, &t, |_| MASK.into());
            t = sub(&p.token, &t, |c| if looks_random(&c[0]) { MASK.into() } else { c[0].to_string() });
        }
        if self.mask_email {
            t = sub(&p.email, &t, |c| format!("{MASK}@{}", c[0].split_once('@').unwrap().1));
        }
        if let Some(rx) = &self.user { t = sub(rx, &t, |_| "user".into()); }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;

    /// Every case recorded from redact.py comes out the same here.
    #[test]
    fn matches_python() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/redact.json");
        // the fake keys carry an invisible marker in the file (tools/fixture_text.py): taken out before use
        let cases = Json::parse(&std::fs::read_to_string(path).unwrap().replace('\u{e000}', "")).unwrap();
        let mut bad = Vec::new();
        for (i, c) in cases.items().iter().enumerate() {
            let s = |k| c.get(k).str().unwrap();
            let r = Redactor::with_short(s("home"), s("short"), s("user"), c.get("mask_user").truthy(), c.get("mask_email").truthy(),
                                  c.get("entropy_check").truthy());
            let got = r.redact(s("input"));
            if got != s("output") { bad.push(format!("case {i}\n  in:   {:?}\n  want: {:?}\n  got:  {:?}", s("input"), s("output"), got)); }
        }
        assert!(cases.items().len() > 100);
        assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.items().len(), bad.join("\n"));
    }
}
