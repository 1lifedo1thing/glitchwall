"""The README's "More screenshots": a hand-picked set of themes and modes, 1600x900 JPEGs in docs/shots/.

Dev-only helper, not part of the package. Runs the ambient demo (made-up sessions, nothing real).
    uv run --no-project --with pillow --with websockets python tools/screenshots.py
"""
from __future__ import annotations

import asyncio
import base64
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

import websockets
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
from record_demo import browser, stop  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs" / "shots"
DEMO_PORT, CDP_PORT = 47842, 9335
W, H = 1600, 900         # roomy enough that 4 cards sit apart, small enough that the text reads in a README
SETTLE = 16.0

# (file name, URL query). Order is the order in the README.
SHOTS = [
    ("phosphor", "theme=phosphor"),
    ("drafting", "theme=drafting"),
    ("notebook", "theme=notebook"),
    ("sleeve", "theme=sleeve"),
    ("duplicator", "theme=duplicator"),
    ("newsprint", "theme=newsprint"),
    ("oracle", "theme=oracle"),
    ("art-oracle", "theme=oracle&mode=art"),
    ("orb", "theme=orb"),
    ("art-orb", "theme=orb&mode=art"),
    ("scope", "theme=scope"),
    ("art-scope", "theme=scope&mode=art"),
    ("neon", "theme=neon"),
    ("bsod", "theme=bsod"),
    ("art-synthwave", "theme=synthwave&mode=art"),
    ("art-starburst", "theme=sleeve&rain=starburst&mode=art"),
    ("art-halftone", "theme=duplicator&mode=art"),
    ("art-testcard", "theme=fieldops&rain=testcard&mode=art"),
]


async def shoot(ws_url: str) -> None:
    async with websockets.connect(ws_url, max_size=64 * 1024 * 1024) as ws:
        n = 0
        errors: list[str] = []

        async def call(method, **params):
            nonlocal n
            n += 1
            await ws.send(json.dumps({"id": n, "method": method, "params": params}))
            while True:
                msg = json.loads(await ws.recv())
                if msg.get("method") == "Runtime.exceptionThrown":
                    errors.append(msg["params"]["exceptionDetails"].get("exception", {}).get("description", "?"))
                if msg.get("id") == n:
                    return msg.get("result", {})

        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", width=W, height=H, deviceScaleFactor=1, mobile=False)
        for name, query in SHOTS:
            errors.clear()
            # ?still: cards type instantly and stay, so the screen fills up to the 4-card limit
            await call("Page.navigate", url=f"http://127.0.0.1:{DEMO_PORT}/?still&{query}")
            await asyncio.sleep(SETTLE if "mode=art" not in query else 6)
            shot = await call("Page.captureScreenshot", format="png", fromSurface=True)
            img = Image.open(io.BytesIO(base64.b64decode(shot["data"]))).convert("RGB")
            p = OUT / f"{name}.jpg"
            img.save(p, quality=88, optimize=True, progressive=True)
            print(f"{name:14} {p.stat().st_size // 1024} KB" + (f"   ERRORS: {errors}" if errors else ""))


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PYTHONPATH=str(ROOT / "src"), GLITCHWALL_HOME=tempfile.mkdtemp(prefix="gw-home-"))
    demo = subprocess.Popen([sys.executable, "-m", "glitchwall", "demo", "--port", str(DEMO_PORT), "--pace", "0.3",
                             "--mode", "ambient"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    prof = tempfile.mkdtemp(prefix="glitchwall-shots-")
    br = subprocess.Popen([browser(), "--headless=new", f"--remote-debugging-port={CDP_PORT}", f"--user-data-dir={prof}",
                           "--no-first-run", "--hide-scrollbars", f"--window-size={W},{H}", "about:blank"],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                pages = json.load(urllib.request.urlopen(f"http://127.0.0.1:{CDP_PORT}/json/list", timeout=1))
                ws_url = next(p["webSocketDebuggerUrl"] for p in pages if p.get("type") == "page")
                break
            except Exception:
                time.sleep(0.2)
        else:
            sys.exit("browser did not start")
        asyncio.run(shoot(ws_url))
    finally:
        stop(br, prof)
        stop(demo)


if __name__ == "__main__":
    main()
