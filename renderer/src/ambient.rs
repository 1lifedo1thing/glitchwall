//! Ambient mode and the demo's script, in Rust (piece 2c of docs/native-plan.md): ports of ambient.py and demo.py's
//! generator. Made-up but believable coding sessions, forever; nothing on this machine is read.
//!
//! Each "episode" is a small, coherent piece of work in a fictional project (fix a bug, add an endpoint, tidy a
//! build, ship a release), so what scrolls by reads like real code rather than noise. Names, files and numbers are
//! mixed each time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::json::Json;

pub type Ev = Vec<(String, Json)>;
/// (kind, text, output, is_error)
pub type Step = (&'static str, String, String, bool);

// ---------------------------------------------------------------- randomness
/// n bytes from the system's secure generator (secrets.token_bytes).
pub fn os_random(n: usize) -> Vec<u8> {
    use windows::Win32::Security::Cryptography::{BCRYPT_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
    let mut b = vec![0u8; n];
    let st = unsafe { BCryptGenRandom(BCRYPT_ALG_HANDLE::default(), &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    assert!(st.is_ok(), "BCryptGenRandom failed: {st:?}");   // never silently weaker: a key made from zeros would be guessable
    b
}

/// secrets.token_hex(n)
pub fn token_hex(n: usize) -> String { os_random(n).iter().map(|b| format!("{b:02x}")).collect() }

/// secrets.token_urlsafe(n): base64url of n random bytes, no padding
pub fn token_urlsafe(n: usize) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let b = os_random(n);
    let mut o = String::new();
    for c in b.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..c.len() + 1 { o.push(A[(v >> (18 - 6 * i) & 63) as usize] as char); }
    }
    o
}

/// The made-up numbers and names (Python's random module): a xorshift seeded from the system generator.
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Rng {
        let s = u64::from_le_bytes(os_random(8).try_into().unwrap());
        Rng(s | 1)
    }
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    /// random.random(): [0, 1)
    pub fn random(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
    /// random.uniform(a, b)
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.random() }
    /// random.randint(a, b): both ends included
    pub fn randint(&mut self, a: i64, b: i64) -> i64 { a + (self.next() % (b - a + 1) as u64) as i64 }
    /// random.choice
    pub fn choice<'a, T>(&mut self, xs: &'a [T]) -> &'a T { &xs[self.next() as usize % xs.len()] }
}

// ---------------------------------------------------------------- the fictional world
pub const PROJECTS: [&str; 10] = ["orbit", "lantern", "mossgate", "tidepool", "quill", "ferrous", "nimbus", "kestrel", "driftwood", "parcel"];
const PY_MODS: [&str; 7] = ["api/routes", "jobs/sync", "core/cache", "auth/session", "billing/invoice", "search/index", "util/retry"];
const TS_MODS: [&str; 6] = ["components/Header", "lib/fetcher", "pages/settings", "hooks/useQueue", "store/cart", "lib/format"];
const RS_MODS: [&str; 5] = ["parser", "codec", "scheduler", "net/conn", "fs/watch"];
const GO_MODS: [&str; 4] = ["handler", "queue", "metrics", "storage"];
const NOUNS: [&str; 10] = ["user", "order", "invoice", "track", "item", "session", "job", "note", "device", "event"];
const BRANCHES: [&str; 5] = ["main", "dev", "fix/retry", "feat/export", "chore/deps"];
const HOSTS: [&str; 4] = ["api.example.test", "staging.example.test", "registry.example.test", "status.example.test"];
const DOCS: [&str; 6] = [
    "https://docs.python.org/3/library/asyncio-task.html",
    "https://developer.mozilla.org/en-US/docs/Web/API/AbortController",
    "https://doc.rust-lang.org/std/collections/struct.HashMap.html",
    "https://pkg.go.dev/context",
    "https://www.postgresql.org/docs/current/sql-createindex.html",
    "https://docs.docker.com/compose/compose-file/",
];
const AGENT_TASKS: [&str; 5] = ["Survey error handling across {m}/", "Find every caller of {f}()", "Check {n} migrations for missing indexes",
                                "Summarize open TODOs in src/", "Compare the two retry helpers"];

/// a short made-up commit hash (_h)
fn h(n: usize) -> String { let mut t = token_hex(4); t.truncate(n.min(8)); t }

fn step(kind: &'static str, text: impl Into<String>, out: impl Into<String>, err: bool) -> Step { (kind, text.into(), out.into(), err) }

// ---------------------------------------------------------------- the episodes
fn py_bugfix(r: &mut Rng, p: &str) -> Vec<Step> {
    let m = *r.choice(&PY_MODS);
    let n = *r.choice(&NOUNS);
    let f = format!("src/{m}.py");
    let t = format!("tests/test_{}.py", m.rsplit('/').next().unwrap());
    let fns = [format!("list_{n}s"), format!("get_{n}"), format!("load_{n}s"), format!("total_for_{n}")];
    let fname = r.choice(&fns).clone();
    let total = r.randint(14, 120);
    let pairs = [
        ("    return {'items': rows}".to_string(), "    return {'items': rows, 'total': len(rows)}".to_string()),
        (format!("    if not {n}_id:"), format!("    if {n}_id is None:")),
        ("    for row in rows[1:]:".to_string(), "    for row in rows:".to_string()),
        ("    timeout = 5".to_string(), "    timeout = settings.get('timeout', 30)".to_string()),
    ];
    let (old, new) = r.choice(&pairs).clone();
    let branch = *r.choice(&BRANCHES);
    let passed = r.randint(4, 9);
    let (ms1, ms2) = (ms(r), ms(r));
    let msg = *r.choice(&["handle empty input", "fix off-by-one", "return totals", "respect timeout"]);
    let br2 = *r.choice(&["main", "dev"]);
    let (ins, del) = (r.randint(2, 12), r.randint(1, 6));
    vec![
        step("$", "git status -sb", format!("## {branch}...origin\n M {f}"), false),
        step("read", format!("~/code/{p}/{f}"), "", false),
        step("$", format!("pytest -q {t}"), format!("...F..\nFAILED {t}::test_{fname} - AssertionError\n{} failed, {passed} passed in {ms1}", 1), true),
        step("edit", format!("~/code/{p}/{f}\n- {old}\n+ {new}"), "", false),
        step("$", "pytest -q", format!("{}\n{total} passed in {ms2}", ".".repeat(total.min(60) as usize)), false),
        step("$", format!("git commit -am \"{fname}: {msg}\""),
             format!("[{br2} {}] {fname}\n 2 files changed, {ins} insertions(+), {del} deletions(-)", h(7)), false),
    ]
}

fn ms(r: &mut Rng) -> String { format!("{:.2}s", r.uniform(0.08, 3.9)) }

fn ts_feature(r: &mut Rng, p: &str) -> Vec<Step> {
    let m = *r.choice(&TS_MODS);
    let f = if m.contains("components") || m.contains("pages") { format!("src/{m}.tsx") } else { format!("src/{m}.ts") };
    let comp = m.rsplit('/').next().unwrap();
    let (a, b) = (r.randint(0, 3), r.randint(0, 20));
    let (tests, tms, files, all) = (r.randint(3, 14), r.randint(8, 90), r.randint(6, 30), r.randint(40, 300));
    let (mods, hash, kb1, kb2, gz1, gz2) = (r.randint(200, 1400), h(8), r.randint(90, 420), r.randint(10, 99), r.randint(30, 140), r.randint(10, 99));
    let took = ms(r);
    vec![
        step("find", format!("{comp}  in src/"), format!("src/{m}.ts:12: export function {comp}(\nsrc/app.tsx:44:   <{comp} />"), false),
        step("read", format!("~/code/{p}/{f}"), "", false),
        step("edit", format!("~/code/{p}/{f}\n- const res = await fetch(url)\n+ const res = await fetch(url, {{ signal: ctrl.signal }})\n+ if (!res.ok) throw new HttpError(res.status)"), "", false),
        step("$", "npm run lint", format!("> {p}@{a}.{b}.0 lint\n> eslint src --ext .ts,.tsx\n\n✔ no problems"), false),
        step("$", "npm test -- --run", format!(" ✓ src/{m}.test.ts ({tests} tests) {tms}ms\n\n Test Files  {files} passed\n      Tests  {all} passed"), false),
        step("$", "npm run build", format!("vite v5.4.2 building for production...\n✓ {mods} modules transformed.\ndist/assets/index-{hash}.js   {kb1}.{kb2} kB │ gzip: {gz1}.{gz2} kB\n✓ built in {took}"), false),
    ]
}

fn rust_build(r: &mut Rng, p: &str) -> Vec<Step> {
    let m = *r.choice(&RS_MODS);
    let ok = r.random() < 0.5;
    let mut out = vec![
        step("read", format!("~/code/{p}/src/{m}.rs"), "", false),
        step("edit", format!("~/code/{p}/src/{m}.rs\n- fn next(&mut self) -> Option<Token> {{\n+ fn next(&mut self) -> Option<Token<'a>> {{"), "", false),
    ];
    if !ok {
        let (l, c) = (r.randint(20, 200), r.randint(5, 40));
        out.push(step("$", "cargo check", format!("error[E0106]: missing lifetime specifier\n  --> src/{m}.rs:{l}:{c}\n   |\n   = help: consider introducing a named lifetime parameter"), true));
        out.push(step("edit", format!("~/code/{p}/src/{m}.rs\n- impl Iterator for Lexer {{\n+ impl<'a> Iterator for Lexer<'a> {{"), "", false));
    }
    let (v1, v2, took, passed) = (r.randint(1, 9), r.randint(0, 9), ms(r), r.randint(12, 140));
    let (v3, secs) = (r.randint(1, 9), r.uniform(8.0, 60.0));
    out.push(step("$", "cargo test", format!("   Compiling {p} v0.{v1}.{v2}\n    Finished `test` profile in {took}\ntest result: ok. {passed} passed; 0 failed"), false));
    out.push(step("$", "cargo build --release", format!("   Compiling {p} v0.{v3}.0\n    Finished `release` profile [optimized] target(s) in {secs:.2}s"), false));
    out
}

fn go_service(r: &mut Rng, p: &str) -> Vec<Step> {
    let m = *r.choice(&GO_MODS);
    let (t1, t2, minor, patch) = (ms(r), ms(r), r.randint(0, 12), r.randint(0, 9));
    vec![
        step("read", format!("~/code/{p}/internal/{m}/{m}.go"), "", false),
        step("edit", format!("~/code/{p}/internal/{m}/{m}.go\n- ctx := context.Background()\n+ ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)\n+ defer cancel()"), "", false),
        step("$", "go vet ./... && go test ./...", format!("ok  \t{p}/internal/{m}\t{t1}\nok  \t{p}/internal/api\t{t2}\n?   \t{p}/cmd/{p}\t[no test files]"), false),
        step("$", format!("go build -o bin/{p} ./cmd/{p}"), "", false),
        step("$", format!("./bin/{p} --version"), format!("{p} v1.{minor}.{patch} ({})", h(7)), false),
    ]
}

fn docker_deploy(r: &mut Rng, p: &str) -> Vec<Step> {
    let tag = h(7);
    let pods = r.randint(2, 6);
    let build = r.uniform(6.0, 50.0);
    let rows: Vec<String> = (0..pods.min(3)).map(|_| format!("api-{}-{}  1/1    Running  {}s", h(5), h(3), r.randint(5, 90))).collect();
    let host = *r.choice(&HOSTS);
    vec![
        step("$", format!("docker build -t {p}:{tag} ."), format!("[+] Building {build:.1}s (14/14) FINISHED\n => exporting to image\n => naming to docker.io/library/{p}:{tag}"), false),
        step("$", "docker compose up -d", format!("[+] Running 3/3\n ✔ Container {p}-db-1     Healthy\n ✔ Container {p}-cache-1  Started\n ✔ Container {p}-api-1    Started"), false),
        step("$", format!("kubectl -n {p} rollout status deploy/{p}-api"),
             format!("Waiting for deployment \"{p}-api\" rollout to finish: {} of {pods} updated replicas are available...\ndeployment \"{p}-api\" successfully rolled out", pods - 1), false),
        step("$", format!("kubectl -n {p} get pods"), format!("NAME          READY  STATUS   AGE\n{}", rows.join("\n")), false),
        step("$", format!("curl -s https://{host}/healthz"), format!("{{\"status\":\"ok\",\"db\":\"ok\",\"version\":\"{tag}\"}}"), false),
    ]
}

fn sql_work(r: &mut Rng, p: &str) -> Vec<Step> {
    let n = *r.choice(&NOUNS);
    let c = r.randint(800, 250000);
    let mig = format!("{:04}_{n}s_owner_idx.sql", r.randint(20, 80));   // the same file in every step
    let (cost, rows, slow, fast) = (r.randint(1000, 9000), r.randint(3, 40), r.uniform(40.0, 900.0), r.uniform(0.05, 2.0));
    vec![
        step("$", format!("psql -d {p} -c \"select count(*) from {n}s where created_at > now() - interval '7 days'\""), format!(" count\n-------\n {c:>5}\n(1 row)"), false),
        step("$", format!("psql -d {p} -c \"explain analyze select * from {n}s where owner_id = 42\""),
             format!("Seq Scan on {n}s  (cost=0.00..{cost}.00 rows={rows})\nExecution Time: {slow:.3} ms"), false),
        step("write", format!("~/code/{p}/migrations/{mig}\nCREATE INDEX CONCURRENTLY {n}s_owner_idx\n    ON {n}s (owner_id);"), "", false),
        step("$", "make migrate", format!("applying {mig} ... ok"), false),
        step("$", format!("psql -d {p} -c \"explain analyze select * from {n}s where owner_id = 42\""),
             format!("Index Scan using {n}s_owner_idx on {n}s\nExecution Time: {fast:.3} ms"), false),
    ]
}

fn research(r: &mut Rng, p: &str) -> Vec<Step> {
    let mods: Vec<&str> = PY_MODS.iter().chain(RS_MODS.iter()).copied().collect();
    let m = *r.choice(&mods);
    let f = *r.choice(&["retry", "with_backoff", "parse_args"]);
    let n = *r.choice(&NOUNS);
    let task = r.choice(&AGENT_TASKS).replace("{m}", m).replace("{f}", f).replace("{n}", n);
    vec![
        step("agent", task, "", false),
        step("web", *r.choice(&DOCS), "", false),
        step("find", "def retry|fn retry|func Retry", "src/util/retry.py:8: def retry(fn, attempts=3):\nsrc/jobs/sync.py:51:     return retry(lambda: client.push(batch))", false),
        step("read", format!("~/code/{p}/src/util/retry.py"), "", false),
        step("edit", format!("~/code/{p}/src/util/retry.py\n-         time.sleep(1)\n+         time.sleep(min(2 ** i, 30) + random.random())"), "", false),
    ]
}

fn ops_check(r: &mut Rng, p: &str) -> Vec<Step> {
    let up = format!(" {}:{}  up {} days,  load average: {:.2}, {:.2}, {:.2}", r.randint(10, 23), r.randint(10, 59), r.randint(2, 90),
                     r.uniform(0.1, 2.0), r.uniform(0.1, 2.0), r.uniform(0.1, 2.0));
    let df = format!("Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p2  468G  {}G  {}G  {}% /", r.randint(90, 380), r.randint(60, 300), r.randint(20, 80));
    let journal: Vec<String> = (0..4).map(|_| {
        let mon = *r.choice(&["Nov", "Dec"]);
        let (d, hh, mm, ss, pid) = (r.randint(1, 28), r.randint(10, 23), r.randint(10, 59), r.randint(10, 59), r.randint(800, 9000));
        let line = *r.choice(&["GET /v1/items 200 12ms", "worker: batch flushed (128)", "cache hit ratio 0.94", "POST /v1/sync 202 31ms"]);
        format!("{mon} {d} {hh}:{mm}:{ss} host {p}[{pid}]: {line}")
    }).collect();
    let procs: Vec<String> = ["node", "code", "python", "pwsh"].iter().map(|n| format!("{n:<11} {:.2}", r.uniform(20.0, 900.0))).collect();
    let host = *r.choice(&HOSTS);
    let pings: Vec<String> = (1..4).map(|i| format!("64 bytes from 10.0.{}.{}: icmp_seq={i} ttl=63 time={:.1} ms", r.randint(0, 9), r.randint(2, 250), r.uniform(0.3, 30.0))).collect();
    vec![
        step("$", "uptime", up, false),
        step("$", "df -h /", df, false),
        step("$", format!("journalctl -u {p} -n 4 --no-pager"), journal.join("\n"), false),
        step("PS>", "Get-Process | Sort-Object CPU -Descending | Select-Object -First 4 Name, CPU", format!("Name        CPU\n----        ---\n{}", procs.join("\n")), false),
        step("$", format!("ping -c 3 {host}"), format!("{}\n3 packets transmitted, 3 received, 0% packet loss", pings.join("\n")), false),
    ]
}

fn release(r: &mut Rng, p: &str) -> Vec<Step> {
    let v = format!("{}.{}.{}", r.randint(0, 3), r.randint(1, 20), r.randint(0, 9));
    let log: Vec<String> = (0..4).map(|_| format!("{} {}", h(7), r.choice(&["fix: handle empty batch", "feat: export to csv", "docs: usage notes", "chore: bump deps", "perf: cache lookups"]))).collect();
    vec![
        step("$", "git log --oneline -4", log.join("\n"), false),
        step("edit", format!("~/code/{p}/CHANGELOG.md\n+ ## {v}\n+ - export to csv\n+ - retries back off properly"), "", false),
        step("$", format!("git tag -a v{v} -m \"v{v}\" && git push --follow-tags"), format!("To forge.example.test:team/{p}.git\n * [new tag]         v{v} -> v{v}"), false),
        step("$", "python -m build", format!("Successfully built {p}-{v}.tar.gz and {p}-{v}-py3-none-any.whl"), false),
    ]
}

type Episode = fn(&mut Rng, &str) -> Vec<Step>;
/// as ambient.py's EPISODES: bug fixes and features come up twice as often
pub const EPISODES: [(&str, Episode); 11] = [("py_bugfix", py_bugfix), ("py_bugfix", py_bugfix), ("ts_feature", ts_feature),
    ("ts_feature", ts_feature), ("rust_build", rust_build), ("go_service", go_service), ("docker_deploy", docker_deploy),
    ("sql_work", sql_work), ("research", research), ("ops_check", ops_check), ("release", release)];

// ---------------------------------------------------------------- the demo's script (demo.py)
/// (kind, text, output, is_error). Fictional project; the fake secrets show the masking.
pub const SCRIPT: [(&str, &str, &str, bool); 18] = [
    ("$", "git status -sb", "## main...origin/main [ahead 2]\n M src/api/routes.py\n M tests/test_routes.py\n?? notes/todo.md", false),
    ("read", "~/code/orbit/src/api/routes.py", "", false),
    ("edit", "~/code/orbit/src/api/routes.py\n- return jsonify(items)\n+ return jsonify(items=items, total=len(items))", "", false),
    ("$", "pytest -q tests/test_routes.py", "....F.\nFAILED tests/test_routes.py::test_list_empty - KeyError: 'total'\n1 failed, 5 passed in 0.41s", true),
    ("edit", "~/code/orbit/tests/test_routes.py\n- assert r.json == []\n+ assert r.json == {'items': [], 'total': 0}", "", false),
    ("$", "pytest -q", "..............................\n30 passed in 1.92s", false),
    ("find", "TODO|FIXME  in src/", "src/api/auth.py:41: # TODO rotate session keys\nsrc/jobs/sync.py:12: # FIXME retry on 429", false),
    ("$", concat!("export STRIPE_KEY=sk_", "live_51HdemoNotARealKey00000000 && ./deploy.sh staging"),
     "building image orbit:7f3a1c2 ...\npushing to registry.internal:5000\nrolling out 3/3 pods\ndeploy ok in 38s", false),
    ("PS>", "Get-Service | Where-Object Status -eq Running | Select-Object -First 5",
     "Status   Name               DisplayName\n------   ----               -----------\nRunning  Appinfo            Application Information\nRunning  AudioSrv           Windows Audio\nRunning  BFE                Base Filtering Engine", false),
    ("web", "https://docs.python.org/3/library/http.server.html", "", false),
    ("$", concat!("curl -s -H \"Authorization: Bearer gh", "p_demoDemoDemoDemoDemoDemoDemo1234\" https://api.example.test/v1/me"),
     "{\"login\": \"octo-demo\", \"plan\": \"free\"}", false),
    ("write", "~/code/orbit/notes/todo.md\n- [x] totals in list endpoint\n- [ ] retry on 429\n- [ ] rotate session keys", "", false),
    ("$", "docker compose ps", "NAME        STATUS         PORTS\norbit-api   Up 3 minutes   0.0.0.0:8080->8080/tcp\norbit-db    Up 3 minutes   5432/tcp", false),
    ("agent", "Survey retry logic across jobs/", "", false),
    ("$", "psql postgres://orbit:hunter2@localhost:5432/orbit -c 'select count(*) from orders'", " count\n-------\n  1284\n(1 row)", false),
    ("$", "git commit -am \"list endpoint returns totals\"", "[main 9c1e4b7] list endpoint returns totals\n 2 files changed, 6 insertions(+), 3 deletions(-)", false),
    ("$", "cargo build --release", "   Compiling orbit-cli v0.4.1\n    Finished `release` profile [optimized] target(s) in 12.84s", false),
    ("$", "ls -la ~/code/orbit", "drwxr-xr-x  src\ndrwxr-xr-x  tests\n-rw-r--r--  pyproject.toml\n-rw-r--r--  README.md", false),
];
const DEMO_PROJECTS: [&str; 5] = ["orbit", "orbit", "orbit", "homelab", "dotfiles"];

// ---------------------------------------------------------------- feeding them in
/// datetime.now(timezone.utc).isoformat()
pub fn now_iso() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let s = d.as_secs() as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    let z = days + 719468;   // civil date from days since 1970
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let us = d.subsec_micros();
    let frac = if us == 0 { String::new() } else { format!(".{us:06}") };
    format!("{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}{frac}+00:00", rem / 3600, rem / 60 % 60, rem % 60)
}

fn event(proj: &str, session: &str, side: bool, kind: &str, r#ref: &str, text: &str) -> Ev {
    let s = |v: &str| Json::Str(v.into());
    vec![("proj".into(), s(proj)), ("session".into(), s(session)), ("side".into(), Json::Bool(side)), ("ts".into(), s(&now_iso())),
         ("kind".into(), s(kind)), ("ref".into(), s(r#ref)), ("text".into(), s(text))]
}

/// Waits `secs`, or less if stop is set: true if it was (Event.wait).
pub fn wait(stop: &AtomicBool, secs: f64) -> bool {
    let end = Instant::now() + Duration::from_secs_f64(secs.max(0.0));
    loop {
        if stop.load(Ordering::Relaxed) { return true; }
        let now = Instant::now();
        if now >= end { return stop.load(Ordering::Relaxed); }
        std::thread::sleep((end - now).min(Duration::from_millis(50)));
    }
}

/// Made-up episodes, one event at a time, until stop is set (ambient.generate). `pace`: slower above 1.
pub fn generate(add: &mut dyn FnMut(Ev), stop: &AtomicBool, pace: f64) {
    let mut r = Rng::new();
    while !stop.load(Ordering::Relaxed) {
        let proj = *r.choice(&PROJECTS);
        let session = token_hex(2);
        let (_, ep) = *r.choice(&EPISODES);
        for (kind, text, out, err) in ep(&mut r, proj) {
            let rf = token_hex(6);
            add(event(proj, &session, kind == "agent", kind, &rf, &text));
            if wait(stop, (1.4 + r.random() * 2.2) * pace) { return; }
            if !out.is_empty() {
                add(event(proj, &session, kind == "agent", if err { "err" } else { "out" }, &rf, &out));
                if wait(stop, (1.2 + r.random() * 2.8) * pace) { return; }
            }
        }
        if wait(stop, (2.0 + r.random() * 4.0) * pace) { return; }
    }
}

/// The demo's scripted session, round and round (demo.generate).
pub fn demo_generate(add: &mut dyn FnMut(Ev), stop: &AtomicBool, pace: f64) {
    let mut r = Rng::new();
    let session = token_hex(2);
    let mut i = 0;
    while !stop.load(Ordering::Relaxed) {
        let (kind, text, out, err) = SCRIPT[i % SCRIPT.len()];
        i += 1;
        let rf = token_hex(6);
        let proj = *r.choice(&DEMO_PROJECTS);
        let side = kind == "agent" || r.random() < 0.1;
        add(event(proj, &session, side, kind, &rf, text));
        if wait(stop, (1.2 + r.random() * 1.8) * pace) { break; }
        if !out.is_empty() { add(event(proj, &session, side, if err { "err" } else { "out" }, &rf, out)); }
        if wait(stop, (1.0 + r.random() * 2.5) * pace) { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    const KINDS: [&str; 8] = ["$", "PS>", "read", "edit", "write", "find", "web", "agent"];

    #[test]
    fn every_episode_is_well_formed() {   // test_modes.py's TestAmbient
        let mut r = Rng::new();
        for (name, ep) in EPISODES {
            for _ in 0..50 {
                for (kind, text, _, _) in ep(&mut r, "orbit") {
                    assert!(KINDS.contains(&kind), "{name}: {kind}");
                    assert!(!text.trim().is_empty(), "{name}");
                }
            }
        }
        for (kind, text, ..) in SCRIPT { assert!(KINDS.contains(&kind) && !text.trim().is_empty()); }
    }

    #[test]
    fn research_fills_its_task() {
        let mut r = Rng::new();
        for _ in 0..50 { assert!(!research(&mut r, "orbit")[0].1.contains('{')); }
    }

    #[test]
    fn generators_feed_and_stop() {
        for g in [generate as fn(&mut dyn FnMut(Ev), &AtomicBool, f64), demo_generate] {
            let stop = Arc::new(AtomicBool::new(false));
            let got = Arc::new(std::sync::Mutex::new(Vec::<Ev>::new()));
            let (s2, g2) = (stop.clone(), got.clone());
            let t = std::thread::spawn(move || g(&mut |e| g2.lock().unwrap().push(e), &s2, 0.005));
            std::thread::sleep(Duration::from_millis(400));
            stop.store(true, Ordering::Relaxed);
            let begun = Instant::now();
            t.join().unwrap();
            assert!(begun.elapsed() < Duration::from_millis(500), "stops promptly");
            let evs = got.lock().unwrap();
            assert!(evs.len() > 2);
            for e in evs.iter() {
                let k = e.iter().find(|(k, _)| k == "kind").unwrap().1.str().unwrap().to_string();
                assert!(KINDS.contains(&k.as_str()) || k == "out" || k == "err", "{k}");
                assert_eq!(e.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["proj", "session", "side", "ts", "kind", "ref", "text"]);
            }
        }
    }

    #[test]
    fn tokens() {
        assert_eq!(token_hex(6).len(), 12);
        let k = token_urlsafe(24);
        assert_eq!(k.len(), 32);
        assert!(k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_ne!(token_urlsafe(24), k);
        assert_eq!(token_urlsafe(1).len(), 2);
        assert_eq!(token_urlsafe(2).len(), 3);
        let t = now_iso();
        assert!(t.ends_with("+00:00") && t.len() >= 25, "{t}");
        let mut r = Rng::new();
        for _ in 0..1000 { let v = r.randint(3, 5); assert!((3..=5).contains(&v)); let f = r.random(); assert!((0.0..1.0).contains(&f)); }
    }
}
