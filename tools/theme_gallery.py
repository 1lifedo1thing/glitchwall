"""Screenshot every theme in demo mode (fake data only) and build docs/themes.png.

Dev-only helper, not part of the package. Needs Pillow and websockets:
    uv run --no-project --with pillow --with websockets python tools/theme_gallery.py [OUTDIR]

Full-size shots go to OUTDIR (default: a temp folder); the gallery (4 per row) goes to docs/themes.png.
Browser console errors are printed, so it doubles as a smoke test for every theme.
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
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).resolve().parent))
from record_demo import browser  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from glitchwall.themes import THEMES  # noqa: E402

DEMO_PORT, CDP_PORT = 47841, 9334
W, H = 1280, 720
SETTLE = 9.0


async def shoot(ws_url: str, outdir: Path) -> dict[str, Path]:
    shots = {}
    async with websockets.connect(ws_url, max_size=64 * 1024 * 1024) as ws:
        n = 0
        errors = []

        async def call(method, **params):
            nonlocal n
            n += 1
            await ws.send(json.dumps({"id": n, "method": method, "params": params}))
            while True:
                msg = json.loads(await ws.recv())
                if msg.get("method") == "Runtime.exceptionThrown":
                    errors.append(msg["params"]["exceptionDetails"].get("exception", {}).get("description", "?"))
                if msg.get("method") == "Runtime.consoleAPICalled" and msg["params"]["type"] == "error":
                    errors.append(" ".join(str(a.get("value", a.get("description"))) for a in msg["params"]["args"]))
                if msg.get("id") == n:
                    return msg.get("result", {})

        await call("Runtime.enable")
        await call("Emulation.setDeviceMetricsOverride", width=W, height=H, deviceScaleFactor=1, mobile=False)
        for name in THEMES:
            errors.clear()
            await call("Page.navigate", url=f"http://127.0.0.1:{DEMO_PORT}/?still&theme={name}")
            await asyncio.sleep(SETTLE)
            shot = await call("Page.captureScreenshot", format="png", fromSurface=True)
            p = outdir / f"theme-{name}.png"
            p.write_bytes(base64.b64decode(shot["data"]))
            shots[name] = p
            print(f"{name:10} {p}" + (f"   ERRORS: {errors}" if errors else ""))
    return shots


def gallery(shots: dict[str, Path]) -> Path:
    tw, th, pad, cap = 420, 236, 10, 26
    cols = 4
    rows = -(-len(shots) // cols)
    img = Image.new("RGB", (cols * tw + (cols + 1) * pad, rows * (th + cap) + (rows + 1) * pad), "#0b0b0b")
    d = ImageDraw.Draw(img)
    font_path = ROOT / "src" / "glitchwall" / "wallpaper" / "DepartureMono-Regular.otf"
    font = ImageFont.truetype(str(font_path), 16)
    for i, (name, p) in enumerate(shots.items()):
        x, y = pad + (i % cols) * (tw + pad), pad + (i // cols) * (th + cap + pad)
        img.paste(Image.open(p).convert("RGB").resize((tw, th), Image.LANCZOS), (x, y))
        d.text((x + 2, y + th + 5), name, font=font, fill="#9be7a9")
    out = ROOT / "docs" / "themes.png"
    img.save(out, optimize=True)
    return out


def main() -> None:
    outdir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(tempfile.mkdtemp(prefix="glitchwall-themes-"))
    outdir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PYTHONPATH=str(ROOT / "src"), GLITCHWALL_HOME=tempfile.mkdtemp(prefix="gw-home-"))
    demo = subprocess.Popen([sys.executable, "-m", "glitchwall", "demo", "--port", str(DEMO_PORT), "--pace", "0.35"],
                            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    prof = tempfile.mkdtemp(prefix="glitchwall-gal-")
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
        shots = asyncio.run(shoot(ws_url, outdir))
    finally:
        br.terminate()
        demo.terminate()
    print(gallery(shots))


if __name__ == "__main__":
    main()
