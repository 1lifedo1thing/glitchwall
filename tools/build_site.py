"""Build the GitHub Pages site into _site/ (or the folder given).

    uv run python tools/build_site.py [OUT]

site/ holds the landing page. Next to it this puts:
- demo/: the real wallpaper page from src/glitchwall/wallpaper, unchanged, with a config.js that plays made-up
  tool calls (demo.SCRIPT and the ambient episodes) in the visitor's browser. The page asks its feed for events
  with fetch(); config.js answers those calls itself, so the site never contacts anything, not even
  127.0.0.1. Every line goes through glitchwall's own redactor here, at build time, exactly as the feed would.
- img/: the theme screenshots and the settings GIF from docs/.
"""
from __future__ import annotations

import json
import random
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from glitchwall import ambient, demo  # noqa: E402
from glitchwall.redact import Redactor  # noqa: E402
from glitchwall.server import EventStore  # noqa: E402

CSP = ("default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; "
       "img-src 'self' data:; font-src 'self'; frame-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'")

SHIM = """// glitchwall's web demo. The wallpaper page asks its feed for new events with fetch();
// this answers those calls right here with made-up ones, so nothing is ever sent anywhere.
window.FEED = { port: 0, key: 'web-demo', label: 'demo: a made-up session' };
(() => {
  const STEPS = %s;   // [kind, text, output, failed, project, side agent], masked at build time
  const Q = new URLSearchParams(location.search);
  const masked = ['dots', 'words', 'blocks', 'stars', 'noise'].includes(Q.get('masked')) ? Q.get('masked') : 'dots';
  const events = [];
  let id = 0, i = Math.floor(Math.random() * STEPS.length);
  const add = (kind, text, s, ref) => {
    events.push({ id: ++id, ts: new Date().toISOString(), proj: s[4], session: 'demo', side: s[5], kind, ref, text });
    if (events.length > 60) events.shift();
  };
  const wait = ms => new Promise(r => setTimeout(r, ms));
  (async () => {
    for (;;) {
      const s = STEPS[i++ %% STEPS.length], ref = Math.random().toString(16).slice(2, 14);
      add(s[0], s[1], s, ref);
      await wait(1200 + Math.random() * 1800);
      if (s[2]) { add(s[3] ? 'err' : 'out', s[2], s, ref); await wait(1000 + Math.random() * 2500); }
    }
  })();
  const own = window.fetch.bind(window);
  window.fetch = (url, opts) => {
    const u = String(url);
    if (!u.startsWith('http://127.0.0.1:')) return own(url, opts);
    const since = Number(new URL(u).searchParams.get('since')) || 0;
    const body = { events: events.filter(e => e.id > since), last: id, mode: 'live', masked,
                   label: 'demo: a made-up session' };
    return Promise.resolve(new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } }));
  };
})();
"""


def steps() -> list[list]:
    store = EventStore(Redactor(home="", user="", mask_user=False))

    def masked(text: str) -> str:
        store.add({"text": text, "proj": ""})
        return store.events[-1]["text"]

    rng = random.Random(7)   # the same site on every build
    random.seed(7)
    out = [[k, masked(t), masked(o) if o else "", e, rng.choice(demo.PROJECTS), k == "agent"]
           for k, t, o, e in demo.SCRIPT]
    for _ in range(3):
        for ep in ambient.EPISODES:
            p = rng.choice(ambient.PROJECTS)
            out += [[k, masked(t), masked(o) if o else "", e, p, k == "agent"] for k, t, o, e in ep(p)]
    return out


def build(dest: Path) -> None:
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(ROOT / "site", dest)
    shutil.copytree(ROOT / "src" / "glitchwall" / "wallpaper", dest / "demo",
                    ignore=shutil.ignore_patterns("config.js", "settings.html", "__pycache__"))
    data = json.dumps(steps(), ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
    (dest / "demo" / "config.js").write_text(SHIM % data, encoding="utf-8")
    # the browser holds the page to "no requests": only this site's own files, and no connections at all
    page = dest / "demo" / "index.html"
    html = page.read_text(encoding="utf-8")
    head = '<meta charset="utf-8">'
    assert html.count(head) == 1, "the wallpaper page's <head> changed: update build_site.py"
    page.write_text(html.replace(head, head + f'<meta http-equiv="Content-Security-Policy" content="{CSP}">'),
                    encoding="utf-8")
    img = dest / "img"
    img.mkdir()
    for f in (ROOT / "docs" / "shots").glob("*.jpg"):
        shutil.copy2(f, img / f.name)
    shutil.copy2(ROOT / "docs" / "gifs" / "settings.gif", img / "settings.gif")
    shutil.copy2(ROOT / "docs" / "banner.gif", img / "banner.gif")
    (dest / ".nojekyll").write_text("", encoding="utf-8")


if __name__ == "__main__":
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "_site"
    build(out.resolve())
    print(f"site built in {out}")
