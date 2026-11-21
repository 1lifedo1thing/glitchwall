"""Demo mode: made-up tool calls, no logs read. For screenshots and trying the look.

`agent-wallpaper demo` serves fake events on its own port (47834 by default) and also
serves the wallpaper page itself at http://127.0.0.1:47834/ so you can open it in a browser.
The demo key is random per run and the data is fake, so nothing real is exposed.
"""
from __future__ import annotations

import random
import secrets
import threading
import time
from datetime import datetime, timezone
from importlib import resources
from pathlib import Path

from . import settings as cfg
from .redact import Redactor
from .server import EventStore, make_handler, serve

DEMO_PORT = 47834

# (kind, text, output, is_error). Fictional project; the fake secrets show the masking.
SCRIPT = [
    ("$", "git status -sb", "## main...origin/main [ahead 2]\n M src/api/routes.py\n M tests/test_routes.py\n?? notes/todo.md", False),
    ("read", "~/code/orbit/src/api/routes.py", "", False),
    ("edit", "~/code/orbit/src/api/routes.py\n- return jsonify(items)\n+ return jsonify(items=items, total=len(items))", "", False),
    ("$", "pytest -q tests/test_routes.py", "....F.\nFAILED tests/test_routes.py::test_list_empty - KeyError: 'total'\n1 failed, 5 passed in 0.41s", True),
    ("edit", "~/code/orbit/tests/test_routes.py\n- assert r.json == []\n+ assert r.json == {'items': [], 'total': 0}", "", False),
    ("$", "pytest -q", "..............................\n30 passed in 1.92s", False),
    ("find", "TODO|FIXME  in src/", "src/api/auth.py:41: # TODO rotate session keys\nsrc/jobs/sync.py:12: # FIXME retry on 429", False),
    ("$", "export STRIPE_KEY=sk_" + "live_51HdemoNotARealKey00000000 && ./deploy.sh staging",
     "building image orbit:7f3a1c2 ...\npushing to registry.internal:5000\nrolling out 3/3 pods\ndeploy ok in 38s", False),
    ("PS>", "Get-Service | Where-Object Status -eq Running | Select-Object -First 5",
     "Status   Name               DisplayName\n------   ----               -----------\nRunning  Appinfo            Application Information\nRunning  AudioSrv           Windows Audio\nRunning  BFE                Base Filtering Engine", False),
    ("web", "https://docs.python.org/3/library/http.server.html", "", False),
    ("$", "curl -s -H \"Authorization: Bearer gh" + "p_demoDemoDemoDemoDemoDemoDemo1234\" https://api.example.test/v1/me",
     "{\"login\": \"octo-demo\", \"plan\": \"free\"}", False),
    ("write", "~/code/orbit/notes/todo.md\n- [x] totals in list endpoint\n- [ ] retry on 429\n- [ ] rotate session keys", "", False),
    ("$", "docker compose ps", "NAME        STATUS         PORTS\norbit-api   Up 3 minutes   0.0.0.0:8080->8080/tcp\norbit-db    Up 3 minutes   5432/tcp", False),
    ("agent", "Survey retry logic across jobs/", "", False),
    ("$", "psql postgres://orbit:hunter2@localhost:5432/orbit -c 'select count(*) from orders'", " count\n-------\n  1284\n(1 row)", False),
    ("$", "git commit -am \"list endpoint returns totals\"", "[main 9c1e4b7] list endpoint returns totals\n 2 files changed, 6 insertions(+), 3 deletions(-)", False),
    ("$", "cargo build --release", "   Compiling orbit-cli v0.4.1\n    Finished `release` profile [optimized] target(s) in 12.84s", False),
    ("$", "ls -la ~/code/orbit", "drwxr-xr-x  src\ndrwxr-xr-x  tests\n-rw-r--r--  pyproject.toml\n-rw-r--r--  README.md", False),
]
PROJECTS = ["orbit", "orbit", "orbit", "homelab", "dotfiles"]


def generate(store: EventStore, stop: threading.Event, pace: float = 1.0) -> None:
    session = secrets.token_hex(2)
    i = 0
    while not stop.is_set():
        kind, text, out, err = SCRIPT[i % len(SCRIPT)]
        i += 1
        ref = secrets.token_hex(6)
        common = {"proj": random.choice(PROJECTS), "session": session, "side": kind == "agent" or random.random() < .1}
        store.add({**common, "ts": datetime.now(timezone.utc).isoformat(), "kind": kind, "ref": ref, "text": text})
        if stop.wait((1.2 + random.random() * 1.8) * pace):
            break
        if out:
            store.add({**common, "ts": datetime.now(timezone.utc).isoformat(), "kind": "err" if err else "out", "ref": ref, "text": out})
        if stop.wait((1.0 + random.random() * 2.5) * pace):
            break


def run_demo(port: int = DEMO_PORT, pace: float = 1.0) -> None:
    store = EventStore(Redactor(home="", user="", mask_user=False))
    stop = threading.Event()
    threading.Thread(target=generate, args=(store, stop, pace), daemon=True).start()
    key = secrets.token_urlsafe(24)
    wallpaper = Path(str(resources.files("agent_live_wallpaper") / "wallpaper"))
    label = cfg.load_settings().get("label") or cfg.DEFAULTS["label"]
    handler = make_handler(
        store, key, port, static_dir=wallpaper,
        config_js=cfg.config_js(port, key, label + "  [demo]"),
        extra_origins=(f"http://127.0.0.1:{port}", f"http://localhost:{port}"),
    )
    httpd = serve(handler, port)
    print(f"demo feed (fake data) on http://127.0.0.1:{port}/  - open it in a browser, Ctrl+C to stop")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        stop.set()
