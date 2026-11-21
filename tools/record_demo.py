"""Record docs/demo.gif from demo mode (fake data only) with headless Edge/Chrome.

Dev-only helper, not part of the package. Needs Pillow and websockets:
    uv run --no-project --with pillow --with websockets python tools/record_demo.py

It starts `glitchwall demo` on a spare port, opens it in a headless browser via the
DevTools protocol, screenshots it in real time and writes an optimized GIF.
"""
from __future__ import annotations

import asyncio
import base64
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

import websockets
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
DEMO_PORT, CDP_PORT = 47840, 9333
W, H = 1280, 720          # browser viewport
OUT_W = 880               # GIF width
WARMUP, SECONDS, FPS = 7.0, 14.0, 12


def browser() -> str:
    for c in (r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
              r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
              shutil.which("chromium") or "", shutil.which("google-chrome") or "", shutil.which("msedge") or ""):
        if c and Path(c).exists():
            return c
    sys.exit("no Edge/Chrome/Chromium found")


async def record(ws_url: str) -> list[tuple[Image.Image, float]]:
    async with websockets.connect(ws_url, max_size=64 * 1024 * 1024) as ws:
        n = 0

        async def call(method, **params):
            nonlocal n
            n += 1
            await ws.send(json.dumps({"id": n, "method": method, "params": params}))
            while True:
                msg = json.loads(await ws.recv())
                if msg.get("id") == n:
                    return msg.get("result", {})

        await call("Emulation.setDeviceMetricsOverride", width=W, height=H, deviceScaleFactor=1, mobile=False)
        await call("Page.navigate", url=f"http://127.0.0.1:{DEMO_PORT}/")
        await asyncio.sleep(WARMUP)
        frames, t0, step = [], time.monotonic(), 1 / FPS
        while time.monotonic() - t0 < SECONDS:
            t = time.monotonic()
            shot = await call("Page.captureScreenshot", format="png", fromSurface=True)
            img = Image.open(io.BytesIO(base64.b64decode(shot["data"]))).convert("RGB")
            frames.append((img, t))
            await asyncio.sleep(max(0, step - (time.monotonic() - t)))
        return frames


def main() -> None:
    env = dict(os.environ, PYTHONPATH=str(ROOT / "src"))
    demo = subprocess.Popen([sys.executable, "-m", "glitchwall", "demo", "--port", str(DEMO_PORT), "--pace", "0.55"],
                            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    prof = tempfile.mkdtemp(prefix="glitchwall-rec-")
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
        frames = asyncio.run(record(ws_url))
    finally:
        br.terminate()
        demo.terminate()
        shutil.rmtree(prof, ignore_errors=True)

    out_h = round(H * OUT_W / W)
    small = [f.resize((OUT_W, out_h), Image.LANCZOS) for f, _ in frames]
    durations = [max(20, round((frames[i + 1][1] - frames[i][1]) * 1000)) for i in range(len(frames) - 1)] + [1000 // FPS]
    # one shared palette from a few sample frames keeps the GIF small and flicker-free
    sample = Image.new("RGB", (OUT_W, out_h * 4))
    for k, i in enumerate(range(0, len(small), max(1, len(small) // 4))[:4]):
        sample.paste(small[i], (0, out_h * k))
    pal = sample.quantize(colors=128, method=Image.Quantize.MEDIANCUT)
    gif = [im.quantize(palette=pal, dither=Image.Dither.NONE) for im in small]
    out = ROOT / "docs" / "demo.gif"
    gif[0].save(out, save_all=True, append_images=gif[1:], duration=durations, loop=0, optimize=True, disposal=1)
    print(f"{out}  {len(gif)} frames  {out.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
