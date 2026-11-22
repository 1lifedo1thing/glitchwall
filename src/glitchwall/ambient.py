"""Ambient mode: made-up but believable coding sessions, forever. Reads nothing on this machine.

For people who want the look without the live feed. Each "episode" is a small, coherent piece of
work in a fictional project (fix a bug, add an endpoint, tidy a build, ship a release), so what
scrolls by reads like real code rather than noise. Names, files and numbers are mixed each time.
"""
from __future__ import annotations

import random
import secrets
import threading
from datetime import datetime, timezone

from .server import EventStore

PROJECTS = ["orbit", "lantern", "mossgate", "tidepool", "quill", "ferrous", "nimbus", "kestrel", "driftwood", "parcel"]
PY_MODS = ["api/routes", "jobs/sync", "core/cache", "auth/session", "billing/invoice", "search/index", "util/retry"]
TS_MODS = ["components/Header", "lib/fetcher", "pages/settings", "hooks/useQueue", "store/cart", "lib/format"]
RS_MODS = ["parser", "codec", "scheduler", "net/conn", "fs/watch"]
GO_MODS = ["handler", "queue", "metrics", "storage"]
NOUNS = ["user", "order", "invoice", "track", "item", "session", "job", "note", "device", "event"]
BRANCHES = ["main", "dev", "fix/retry", "feat/export", "chore/deps"]
HOSTS = ["api.example.test", "staging.example.test", "registry.example.test", "status.example.test"]
DOCS = [
    "https://docs.python.org/3/library/asyncio-task.html",
    "https://developer.mozilla.org/en-US/docs/Web/API/AbortController",
    "https://doc.rust-lang.org/std/collections/struct.HashMap.html",
    "https://pkg.go.dev/context",
    "https://www.postgresql.org/docs/current/sql-createindex.html",
    "https://docs.docker.com/compose/compose-file/",
]
AGENT_TASKS = ["Survey error handling across {m}/", "Find every caller of {f}()", "Check {n} migrations for missing indexes",
               "Summarize open TODOs in src/", "Compare the two retry helpers"]


def _h(n=7):
    return secrets.token_hex(4)[:n]


def _ms():
    return f"{random.uniform(.08, 3.9):.2f}s"


def _pick(*xs):
    return random.choice(xs)


# --- episodes: each returns a list of (kind, text, output, is_error) ------------------------------

def py_bugfix(p):
    m = random.choice(PY_MODS)
    n = random.choice(NOUNS)
    f = f"src/{m}.py"
    t = f"tests/test_{m.split('/')[-1]}.py"
    fn = _pick(f"list_{n}s", f"get_{n}", f"load_{n}s", f"total_for_{n}")
    total = random.randint(14, 120)
    old, new = _pick(
        ("    return {'items': rows}", "    return {'items': rows, 'total': len(rows)}"),
        (f"    if not {n}_id:", f"    if {n}_id is None:"),
        ("    for row in rows[1:]:", "    for row in rows:"),
        ("    timeout = 5", "    timeout = settings.get('timeout', 30)"),
    )
    return [
        ("$", "git status -sb", f"## {random.choice(BRANCHES)}...origin\n M {f}", False),
        ("read", f"~/code/{p}/{f}", "", False),
        ("$", f"pytest -q {t}", f"...F..\nFAILED {t}::test_{fn} - AssertionError\n1 failed, {random.randint(4, 9)} passed in {_ms()}", True),
        ("edit", f"~/code/{p}/{f}\n- {old}\n+ {new}", "", False),
        ("$", "pytest -q", "." * min(total, 60) + f"\n{total} passed in {_ms()}", False),
        ("$", f'git commit -am "{fn}: {_pick("handle empty input", "fix off-by-one", "return totals", "respect timeout")}"',
         f"[{_pick('main', 'dev')} {_h()}] {fn}\n 2 files changed, {random.randint(2, 12)} insertions(+), {random.randint(1, 6)} deletions(-)", False),
    ]


def ts_feature(p):
    m = random.choice(TS_MODS)
    f = f"src/{m}.tsx" if "components" in m or "pages" in m else f"src/{m}.ts"
    comp = m.split("/")[-1]
    return [
        ("find", f"{comp}  in src/", f"src/{m}.ts:12: export function {comp}(\nsrc/app.tsx:44:   <{comp} />", False),
        ("read", f"~/code/{p}/{f}", "", False),
        ("edit", f"~/code/{p}/{f}\n- const res = await fetch(url)\n+ const res = await fetch(url, {{ signal: ctrl.signal }})\n+ if (!res.ok) throw new HttpError(res.status)", "", False),
        ("$", "npm run lint", f"> {p}@{random.randint(0, 3)}.{random.randint(0, 20)}.0 lint\n> eslint src --ext .ts,.tsx\n\n✔ no problems", False),
        ("$", "npm test -- --run", f" ✓ src/{m}.test.ts ({random.randint(3, 14)} tests) {random.randint(8, 90)}ms\n\n Test Files  {random.randint(6, 30)} passed\n      Tests  {random.randint(40, 300)} passed", False),
        ("$", "npm run build", f"vite v5.4.2 building for production...\n✓ {random.randint(200, 1400)} modules transformed.\ndist/assets/index-{_h(8)}.js   {random.randint(90, 420)}.{random.randint(10, 99)} kB │ gzip: {random.randint(30, 140)}.{random.randint(10, 99)} kB\n✓ built in {_ms()}", False),
    ]


def rust_build(p):
    m = random.choice(RS_MODS)
    ok = random.random() < .5
    out = [
        ("read", f"~/code/{p}/src/{m}.rs", "", False),
        ("edit", f"~/code/{p}/src/{m}.rs\n- fn next(&mut self) -> Option<Token> {{\n+ fn next(&mut self) -> Option<Token<'a>> {{", "", False),
    ]
    if not ok:
        out.append(("$", "cargo check", f"error[E0106]: missing lifetime specifier\n  --> src/{m}.rs:{random.randint(20, 200)}:{random.randint(5, 40)}\n   |\n   = help: consider introducing a named lifetime parameter", True))
        out.append(("edit", f"~/code/{p}/src/{m}.rs\n- impl Iterator for Lexer {{\n+ impl<'a> Iterator for Lexer<'a> {{", "", False))
    out += [
        ("$", "cargo test", f"   Compiling {p} v0.{random.randint(1, 9)}.{random.randint(0, 9)}\n    Finished `test` profile in {_ms()}\ntest result: ok. {random.randint(12, 140)} passed; 0 failed", False),
        ("$", "cargo build --release", f"   Compiling {p} v0.{random.randint(1, 9)}.0\n    Finished `release` profile [optimized] target(s) in {random.uniform(8, 60):.2f}s", False),
    ]
    return out


def go_service(p):
    m = random.choice(GO_MODS)
    return [
        ("read", f"~/code/{p}/internal/{m}/{m}.go", "", False),
        ("edit", f"~/code/{p}/internal/{m}/{m}.go\n- ctx := context.Background()\n+ ctx, cancel := context.WithTimeout(r.Context(), 3*time.Second)\n+ defer cancel()", "", False),
        ("$", "go vet ./... && go test ./...", f"ok  \t{p}/internal/{m}\t{_ms()}\nok  \t{p}/internal/api\t{_ms()}\n?   \t{p}/cmd/{p}\t[no test files]", False),
        ("$", f"go build -o bin/{p} ./cmd/{p}", "", False),
        ("$", f"./bin/{p} --version", f"{p} v1.{random.randint(0, 12)}.{random.randint(0, 9)} ({_h()})", False),
    ]


def docker_deploy(p):
    tag = _h()
    pods = random.randint(2, 6)
    return [
        ("$", f"docker build -t {p}:{tag} .", f"[+] Building {random.uniform(6, 50):.1f}s (14/14) FINISHED\n => exporting to image\n => naming to docker.io/library/{p}:{tag}", False),
        ("$", "docker compose up -d", f"[+] Running 3/3\n ✔ Container {p}-db-1     Healthy\n ✔ Container {p}-cache-1  Started\n ✔ Container {p}-api-1    Started", False),
        ("$", f"kubectl -n {p} rollout status deploy/{p}-api", f"Waiting for deployment \"{p}-api\" rollout to finish: {pods - 1} of {pods} updated replicas are available...\ndeployment \"{p}-api\" successfully rolled out", False),
        ("$", f"kubectl -n {p} get pods", "NAME          READY  STATUS   AGE\n" + "\n".join(
            f"api-{_h(5)}-{_h(3)}  1/1    Running  {random.randint(5, 90)}s" for _ in range(min(pods, 3))), False),
        ("$", f"curl -s https://{random.choice(HOSTS)}/healthz", '{"status":"ok","db":"ok","version":"' + tag + '"}', False),
    ]


def sql_work(p):
    n = random.choice(NOUNS)
    c = random.randint(800, 250000)
    mig = f"{random.randint(20, 80):04d}_{n}s_owner_idx.sql"   # the same file in every step
    return [
        ("$", f"psql -d {p} -c \"select count(*) from {n}s where created_at > now() - interval '7 days'\"", f" count\n-------\n {c:>5}\n(1 row)", False),
        ("$", f"psql -d {p} -c \"explain analyze select * from {n}s where owner_id = 42\"",
         f"Seq Scan on {n}s  (cost=0.00..{random.randint(1000, 9000)}.00 rows={random.randint(3, 40)})\nExecution Time: {random.uniform(40, 900):.3f} ms", False),
        ("write", f"~/code/{p}/migrations/{mig}\nCREATE INDEX CONCURRENTLY {n}s_owner_idx\n    ON {n}s (owner_id);", "", False),
        ("$", "make migrate", f"applying {mig} ... ok", False),
        ("$", f"psql -d {p} -c \"explain analyze select * from {n}s where owner_id = 42\"",
         f"Index Scan using {n}s_owner_idx on {n}s\nExecution Time: {random.uniform(.05, 2):.3f} ms", False),
    ]


def research(p):
    m = random.choice(PY_MODS + RS_MODS)
    task = random.choice(AGENT_TASKS).format(m=m, f=_pick("retry", "with_backoff", "parse_args"), n=random.choice(NOUNS))
    return [
        ("agent", task, "", False),
        ("web", random.choice(DOCS), "", False),
        ("find", "def retry|fn retry|func Retry", "src/util/retry.py:8: def retry(fn, attempts=3):\nsrc/jobs/sync.py:51:     return retry(lambda: client.push(batch))", False),
        ("read", f"~/code/{p}/src/util/retry.py", "", False),
        ("edit", f"~/code/{p}/src/util/retry.py\n-         time.sleep(1)\n+         time.sleep(min(2 ** i, 30) + random.random())", "", False),
    ]


def ops_check(p):
    return [
        ("$", "uptime", f" {random.randint(10, 23)}:{random.randint(10, 59)}  up {random.randint(2, 90)} days,  load average: {random.uniform(.1, 2):.2f}, {random.uniform(.1, 2):.2f}, {random.uniform(.1, 2):.2f}", False),
        ("$", "df -h /", f"Filesystem      Size  Used Avail Use% Mounted on\n/dev/nvme0n1p2  468G  {random.randint(90, 380)}G  {random.randint(60, 300)}G  {random.randint(20, 80)}% /", False),
        ("$", f"journalctl -u {p} -n 4 --no-pager", "\n".join(
            f"{_pick('Nov', 'Dec')} {random.randint(1, 28)} {random.randint(10, 23)}:{random.randint(10, 59)}:{random.randint(10, 59)} host {p}[{random.randint(800, 9000)}]: "
            + _pick("GET /v1/items 200 12ms", "worker: batch flushed (128)", "cache hit ratio 0.94", "POST /v1/sync 202 31ms")
            for _ in range(4)), False),
        ("PS>", "Get-Process | Sort-Object CPU -Descending | Select-Object -First 4 Name, CPU",
         "Name        CPU\n----        ---\n" + "\n".join(f"{n:<11} {random.uniform(20, 900):.2f}" for n in ("node", "code", "python", "pwsh")), False),
        ("$", f"ping -c 3 {random.choice(HOSTS)}", "\n".join(
            f"64 bytes from 10.0.{random.randint(0, 9)}.{random.randint(2, 250)}: icmp_seq={i} ttl=63 time={random.uniform(.3, 30):.1f} ms" for i in range(1, 4))
         + "\n3 packets transmitted, 3 received, 0% packet loss", False),
    ]


def release(p):
    v = f"{random.randint(0, 3)}.{random.randint(1, 20)}.{random.randint(0, 9)}"
    return [
        ("$", "git log --oneline -4", "\n".join(f"{_h()} {random.choice(['fix: handle empty batch', 'feat: export to csv', 'docs: usage notes', 'chore: bump deps', 'perf: cache lookups'])}" for _ in range(4)), False),
        ("edit", f"~/code/{p}/CHANGELOG.md\n+ ## {v}\n+ - export to csv\n+ - retries back off properly", "", False),
        ("$", f'git tag -a v{v} -m "v{v}" && git push --follow-tags',
         f"To forge.example.test:team/{p}.git\n * [new tag]         v{v} -> v{v}", False),
        ("$", "python -m build", f"Successfully built {p}-{v}.tar.gz and {p}-{v}-py3-none-any.whl", False),
    ]


EPISODES = [py_bugfix, py_bugfix, ts_feature, ts_feature, rust_build, go_service, docker_deploy, sql_work, research, ops_check, release]


def generate(store: EventStore, stop: threading.Event, pace: float = 1.0) -> None:
    """Feed made-up episodes into the store until `stop` is set."""
    while not stop.is_set():
        proj = random.choice(PROJECTS)
        session = secrets.token_hex(2)
        for kind, text, out, err in random.choice(EPISODES)(proj):
            ref = secrets.token_hex(6)
            common = {"proj": proj, "session": session, "side": kind == "agent"}
            store.add({**common, "ts": datetime.now(timezone.utc).isoformat(), "kind": kind, "ref": ref, "text": text})
            if stop.wait((1.4 + random.random() * 2.2) * pace):
                return
            if out:
                store.add({**common, "ts": datetime.now(timezone.utc).isoformat(),
                           "kind": "err" if err else "out", "ref": ref, "text": out})
                if stop.wait((1.2 + random.random() * 2.8) * pace):
                    return
        if stop.wait((2 + random.random() * 4) * pace):
            return
