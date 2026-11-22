"""Record the README's feature GIFs into docs/gifs/ (made-up data only, your own settings untouched).

Dev-only helper, not part of the package. Needs Pillow and websockets:
    uv run --no-project --with pillow --with websockets python tools/showcase.py            # all of them
    uv run --no-project --with pillow --with websockets python tools/showcase.py pause modes  # some

How: a private feed (its own state folder, its own port) serves the real wallpaper page and the real settings
window to headless Edge/Chrome, which is driven over the DevTools protocol: settings are changed the way the
app changes them, the settings window gets real pointer drags and clicks. Frames are captured in real time.
Things that happen on the desktop rather than in a page (the hot corner, docking, two screens, the settings
window fading while you hold a control) are put together from those real captures plus a drawn cursor.
"""
from __future__ import annotations

import asyncio
import base64
import io
import itertools
import json
import math
import os
import random
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import websockets
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
os.environ["GLITCHWALL_HOME"] = tempfile.mkdtemp(prefix="glitchwall-show-")   # before glitchwall is imported
sys.path.insert(0, str(ROOT / "src"))
sys.path.insert(0, str(ROOT / "tools"))

from glitchwall import ambient, demo  # noqa: E402
from glitchwall import settings as cfg  # noqa: E402
from glitchwall.redact import Redactor  # noqa: E402
from glitchwall.server import EventStore, LiveSettings, make_handler, serve  # noqa: E402
from record_demo import browser, stop  # noqa: E402

OUT = ROOT / "docs" / "gifs"
PORT, CDP_PORT = 47851, 9341
W, H = 1280, 720               # the wallpaper's viewport
SW, SH = 560, 900              # the settings window's
OUT_W, FPS = 800, 10
CALM = 3   # MOTION in clips about something else than the background: smaller GIFs
FONT = ROOT / "src" / "glitchwall" / "wallpaper" / "DepartureMono-Regular.otf"


# ---------------------------------------------------------------- the private feed
class Feed:
    def __init__(self):
        self.store = EventStore(Redactor(home="/home/octo", user="octo"))
        self.live = LiveSettings()
        self.key = "showcase-" + os.urandom(12).hex()
        self.stop_src = None
        me = (f"http://127.0.0.1:{PORT}", f"http://localhost:{PORT}")
        h = make_handler(self.store, self.key, PORT, self.live, static_dir=ROOT / "src" / "glitchwall" / "wallpaper",
                         config_js="// the key is in the page address\n", extra_origins=me, editable=True)
        self.httpd = serve(h, PORT)
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()
        self.bump = itertools.count(1)

    def set(self, **kw) -> None:
        s = cfg.load_settings()
        s.update(kw)
        cfg.save_settings(s)
        t = time.time() + next(self.bump)   # a new mtime every time, so the feed re-reads it
        os.utime(cfg.settings_file(), (t, t))

    def source(self, gen=None, pace: float = 1.0) -> None:
        if self.stop_src:
            self.stop_src.set()
        self.stop_src = threading.Event()
        if gen:
            threading.Thread(target=gen, args=(self.store, self.stop_src, pace), daemon=True).start()

    def say(self, kind: str, text: str, out: str = "", err: bool = False, proj: str = "orbit") -> None:
        """One scripted call (and its output a moment later)."""
        ref, ts = os.urandom(6).hex(), datetime.now(timezone.utc).isoformat()
        common = {"proj": proj, "session": "5e7a", "side": False, "ts": ts, "ref": ref}
        self.store.add({**common, "kind": kind, "text": text})
        if out:
            threading.Timer(1.6, lambda: self.store.add({**common, "kind": "err" if err else "out", "text": out})).start()

    def url(self, page: str = "", query: str = "") -> str:
        return f"http://127.0.0.1:{PORT}/{page}{'?' + query if query else ''}#key={self.key}"


# ---------------------------------------------------------------- the browser
class Page:
    """One tab, over its own DevTools socket; calls may overlap (a reader task matches the answers)."""

    def __init__(self, ws):
        self.ws, self.n, self.wait = ws, 0, {}
        self.reader = asyncio.create_task(self._read())

    async def _read(self):
        async for msg in self.ws:
            m = json.loads(msg)
            f = self.wait.pop(m.get("id"), None)
            if f and not f.done():
                f.set_result(m.get("result", {}))

    async def call(self, method, **params):
        self.n += 1
        f = asyncio.get_running_loop().create_future()
        self.wait[self.n] = f
        await self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        return await f

    async def open(self, url: str, w: int, h: int) -> None:
        self.size = (w, h)
        await self.call("Emulation.setDeviceMetricsOverride", width=w, height=h, deviceScaleFactor=1, mobile=False)
        await self.call("Page.navigate", url=url)

    async def shot(self, scale: float = 1.0) -> Image.Image:
        """Lossless (so still parts stay identical frame to frame and the GIF stays small), rendered straight
        at the size it's used at (much faster than capturing full size and shrinking)."""
        w, h = self.size
        r = await self.call("Page.captureScreenshot", format="png", fromSurface=True, optimizeForSpeed=True,
                            clip={"x": 0, "y": 0, "width": w, "height": h, "scale": scale})
        return Image.open(io.BytesIO(base64.b64decode(r["data"]))).convert("RGB")

    async def js(self, expr: str):
        r = await self.call("Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True)
        return r.get("result", {}).get("value")

    async def center(self, selector: str) -> tuple[float, float]:
        r = await self.js(f"(() => {{ const b = document.querySelector({json.dumps(selector)}).getBoundingClientRect();"
                          f" return [b.left + b.width / 2, b.top + b.height / 2, b.width]; }})()")
        return r[0], r[1]

    async def rect(self, selector: str) -> list[float]:
        return await self.js(f"(() => {{ const b = document.querySelector({json.dumps(selector)}).getBoundingClientRect();"
                             f" return [b.left, b.top, b.width, b.height]; }})()")

    async def mouse(self, kind: str, x: float, y: float, pressed: bool = False) -> None:
        await self.call("Input.dispatchMouseEvent", type=kind, x=x, y=y, button="left",
                        buttons=1 if pressed or kind == "mousePressed" else 0, clickCount=1)

    async def key(self, key: str, text: str = "") -> None:
        codes = {"Enter": 13, "ArrowRight": 39, "ArrowLeft": 37, "Backspace": 8}
        base = {"key": key, "windowsVirtualKeyCode": codes.get(key, 0)}
        await self.call("Input.dispatchKeyEvent", type="keyDown", text=text, **base)
        await self.call("Input.dispatchKeyEvent", type="keyUp", **base)

    async def type(self, text: str, gap: float = .06) -> None:
        for ch in text:
            await self.call("Input.insertText", text=ch)
            await asyncio.sleep(gap)


class Browser:
    """Headless Edge/Chrome, one browser per page. Headless keeps only its front page fully running; any other page
    is throttled to a frame or two a second, so every screenshot would have to force a frame and a clip would
    crawl. A browser of its own makes every page the front one."""

    def __init__(self):
        self.pages: list[Page] = []
        self.procs: list[tuple[subprocess.Popen, str]] = []
        self.port = CDP_PORT

    async def page(self) -> Page:
        port, self.port = self.port, self.port + 1
        prof = tempfile.mkdtemp(prefix="glitchwall-show-br-")
        proc = subprocess.Popen([browser(), "--headless=new", f"--remote-debugging-port={port}", f"--user-data-dir={prof}",
                                 "--no-first-run", "--hide-scrollbars", "--mute-audio",
                                 "--enable-gpu", "--use-angle=d3d11", "--ignore-gpu-blocklist",   # draw on the GPU
                                 # never count as hidden (Windows occlusion, background throttling)
                                 "--disable-features=CalculateNativeWinOcclusion", "--disable-renderer-backgrounding",
                                 "--disable-background-timer-throttling", "--disable-backgrounding-occluded-windows",
                                 f"--window-size={W},{H}", "about:blank"],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append((proc, prof))
        for _ in range(60):
            try:
                targets = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list", timeout=1))
                url = next(x["webSocketDebuggerUrl"] for x in targets if x.get("type") == "page")
                break
            except (OSError, StopIteration, ValueError):
                await asyncio.sleep(0.2)
        else:
            sys.exit("browser did not start")
        p = Page(await websockets.connect(url, max_size=64 * 1024 * 1024))
        self.pages.append(p)
        await p.call("Page.enable")
        await p.call("Emulation.setFocusEmulationEnabled", enabled=True)   # counts as the focused, visible page
        return p

    async def close_pages(self):
        for p in self.pages:
            p.reader.cancel()
            await p.ws.close()
        self.pages = []
        for proc, prof in self.procs:
            stop(proc, prof)
            shutil.rmtree(prof, ignore_errors=True)
        self.procs = []

    def close(self):
        for proc, prof in self.procs:
            stop(proc, prof)
            shutil.rmtree(prof, ignore_errors=True)


# ---------------------------------------------------------------- drawing
def font(px: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONT), px)


ARROW = [(0, 0), (0, 17), (4.5, 13), (7.5, 20), (10, 19), (7, 12.5), (13, 12.5)]


def cursor(img: Image.Image, x: float, y: float, down: bool = False, scale: float = 1.1) -> None:
    d = ImageDraw.Draw(img)
    pts = [(x + px * scale, y + py * scale) for px, py in ARROW]
    d.polygon(pts, fill="black")
    inner = [(x + px * scale * .78 + scale, y + py * scale * .78 + 2 * scale) for px, py in ARROW]
    d.polygon(inner, fill="white")
    if down:   # a ring: the button is held
        r = 9 * scale
        d.ellipse([x - r, y - r, x + r, y + r], outline=(255, 255, 255), width=2)


def caption(img: Image.Image, lines: list[str], at: str = "top", hi: tuple = (140, 255, 178)) -> None:
    """A strip along the top (or bottom): the first line is the command or the point, the rest explain."""
    if not lines:
        return
    d = ImageDraw.Draw(img, "RGBA")
    big, small = font(21), font(15)
    h = 18 + 28 + 22 * (len(lines) - 1)
    top = 0 if at == "top" else img.height - h
    d.rectangle([0, top, img.width, top + h], fill=(4, 8, 6, 222))
    edge = top + h if at == "top" else top
    d.line([0, edge, img.width, edge], fill=hi + (255,), width=2)
    y = top + 10
    d.text((18, y), lines[0], font=big, fill=hi)
    for ln in lines[1:]:
        y += 28 if ln is lines[1] else 22
        d.text((18, y), ln, font=small, fill=(214, 226, 218))


def tag(img: Image.Image, text: str, x: float, y: float) -> None:
    d = ImageDraw.Draw(img, "RGBA")
    f = font(15)
    w = d.textlength(text, font=f)
    d.rectangle([x - 6, y - 4, x + w + 6, y + 20], fill=(4, 8, 6, 210))
    d.text((x, y), text, font=f, fill=(140, 255, 178))


def panel(img: Image.Image, shot: Image.Image, box: tuple[float, float, float], alpha: float = 1.0, k: float = 1.0) -> None:
    """The settings window pasted onto the wallpaper: box = (left, top, scale) in wallpaper pixels, drawn at
    `k` (output pixels per wallpaper pixel), with a thin frame and shadow."""
    x, y, s = box[0] * k, box[1] * k, box[2] * k
    p = shot.resize((round(SW * s), round(SH * s)), Image.LANCZOS)
    d = ImageDraw.Draw(img, "RGBA")
    if alpha > .5:
        d.rectangle([x + 6, y + 8, x + p.width + 6, y + p.height + 8], fill=(0, 0, 0, round(110 * alpha)))
    framed = Image.new("RGB", (p.width + 2, p.height + 2), (40, 110, 70))
    framed.paste(p, (1, 1))
    if alpha >= .999:
        img.paste(framed, (round(x), round(y)))
    else:
        region = img.crop((round(x), round(y), round(x) + framed.width, round(y) + framed.height))
        img.paste(Image.blend(region, framed, alpha), (round(x), round(y)))


# ---------------------------------------------------------------- a clip: capture while things happen
class Clip:
    """Frames of one GIF. `state` is read at every frame by `compose`; actions change it as they go."""

    def __init__(self, name: str, wall: Page, settings: Page | None = None, compose=None, out_w: int = OUT_W,
                 full_w: int = W, second_scale: float = .8, wall_every: float = 0):
        """`full_w`: the width of the whole picture in wallpaper pixels (two screens side by side are wider)."""
        self.name, self.wall, self.settings, self.out_w = name, wall, settings, out_w
        self.compose, self.second_scale, self.wall_every = compose, second_scale, wall_every
        self.k = out_w / full_w   # output pixels per wallpaper pixel; pages are captured at this scale
        self.frames: list[tuple[Image.Image, float]] = []
        self.state: dict = {"caption": [], "cursor": None, "down": False, "alpha": 1.0, "cap_at": "top"}
        self.segments: list[int] = [0]   # frame index where a new palette starts

    def new_segment(self) -> None:
        if self.frames and self.segments[-1] != len(self.frames):
            self.segments.append(len(self.frames))

    async def frame(self) -> None:
        if time.monotonic() - self.__dict__.get("woken", 0) > 3:
            # after ~20 s headless decides a page is idle and each screenshot then takes 2 s; this keeps it awake
            self.woken = time.monotonic()
            await asyncio.gather(*(pg.call("Page.bringToFront") for pg in (self.wall, self.settings) if pg))
        if self.wall_every and time.monotonic() - self.__dict__.get("wall_at", 0) < self.wall_every:
            shots = [self.wall_img]   # scenes that are mostly drawn movement: reuse it, keep the motion smooth
        else:
            self.wall_img, self.wall_at = await self.wall.shot(self.k), time.monotonic()
            shots = [self.wall_img]
        if self.settings:   # the settings window changes rarely: a fresh capture every 0.4 s is plenty
            if self.second_scale != 1 and time.monotonic() - self.__dict__.get("sett_at", 0) < .4:
                shots.append(self.sett_img)
            else:
                self.sett_img, self.sett_at = await self.settings.shot(self.k * self.second_scale), time.monotonic()
                shots.append(self.sett_img)
        img = self.compose(self, *shots) if self.compose else shots[0]
        if img.width != self.out_w:
            img = img.resize((self.out_w, round(img.height * self.out_w / img.width)), Image.LANCZOS)
        caption(img, self.state["caption"], self.state["cap_at"])
        self.frames.append((img, time.monotonic()))

    async def run(self, seconds: float, *actions) -> None:
        """Capture for `seconds` while the actions (coroutines) run alongside."""
        end = time.monotonic() + seconds

        async def capture():
            while time.monotonic() < end:
                t = time.monotonic()
                await self.frame()
                await asyncio.sleep(max(0, 1 / FPS - (time.monotonic() - t)))
        await asyncio.gather(capture(), *actions)

    def save(self, colors: int = 96) -> Path:
        OUT.mkdir(parents=True, exist_ok=True)
        imgs = [f for f, _ in self.frames]
        times = [t for _, t in self.frames]
        durations = [max(40, round((times[i + 1] - times[i]) * 1000)) for i in range(len(times) - 1)] + [1500]
        for s in self.segments[1:]:   # the wait while a change lands isn't recorded: don't hold a frame through it
            durations[s - 1] = min(durations[s - 1], round(1000 / FPS))
        gif = []
        bounds = self.segments + [len(imgs)]
        for a, b in zip(bounds, bounds[1:]):   # one palette per segment (per theme), from a few of its frames
            seg = imgs[a:b]
            pick = seg[:: max(1, len(seg) // 5)][:5]
            sample = Image.new("RGB", (seg[0].width, seg[0].height * len(pick)))
            for k, im in enumerate(pick):
                sample.paste(im, (0, seg[0].height * k))
            pal = sample.quantize(colors=colors, method=Image.Quantize.MEDIANCUT)
            gif += [im.quantize(palette=pal, dither=Image.Dither.NONE) for im in seg]
        out = OUT / f"{self.name}.gif"
        gif[0].save(out, save_all=True, append_images=gif[1:], duration=durations, loop=0, optimize=True, disposal=1)
        print(f"  {out.relative_to(ROOT)}  {len(gif)} frames  {out.stat().st_size / 1e6:.2f} MB", flush=True)
        return out


async def glide(clip: Clip, to: tuple[float, float], seconds: float, page: Page | None = None, pressed: bool = False,
                to_page=None) -> None:
    """Move the drawn cursor (and, with `page`, the real mouse in that page) in a smooth arc."""
    x0, y0 = clip.state["cursor"] or to
    steps = max(1, int(seconds * 30))
    for i in range(1, steps + 1):
        k = (1 - math.cos(math.pi * i / steps)) / 2
        x, y = x0 + (to[0] - x0) * k, y0 + (to[1] - y0) * k
        clip.state["cursor"] = (x, y)
        if page and to_page:
            await page.mouse("mouseMoved", *to_page(x, y), pressed=pressed)
        await asyncio.sleep(seconds / steps)


# ---------------------------------------------------------------- the scenes
def defaults(feed: Feed, **kw) -> None:
    s = dict(cfg.DEFAULTS)
    s.update({"type_speed": 9, "linger": 4, **kw})
    cfg.save_settings(s)
    feed.set()


async def warm(wall: Page, feed: Feed, url: str, seconds: float = 6) -> None:
    await wall.open(url, W, H)
    await asyncio.sleep(seconds)


async def scene_themes(feed, br):
    """Every theme, with its own colors, fonts, background and way of bringing cards in and out."""
    defaults(feed, mode="ambient", linger=3, cards=4)
    feed.source(ambient.generate, pace=0.35)
    wall = await br.page()
    await warm(wall, feed, feed.url())
    clip = Clip("themes", wall)
    for t in ["phosphor", "drafting", "newsprint", "nightprint", "neon", "fieldops", "duplicator", "sleeve",
              "notebook", "oracle", "bsod", "cyanotype", "synthwave", "amber", "msdos", "matrix"]:
        feed.set(theme=t)
        await asyncio.sleep(2.2)   # the page picks it up within a second; fonts and textures settle (not recorded)
        clip.new_segment()
        clip.state["caption"] = [f"theme  {t}"]
        await clip.run(3.2)
    return clip


async def scene_mix(feed, br):
    """Five independent choices: start from a theme, swap any part."""
    defaults(feed, mode="ambient", cards=3, linger=3)
    feed.source(ambient.generate, pace=0.4)
    wall = await br.page()
    await warm(wall, feed, feed.url())
    clip = Clip("mix", wall)
    steps = [({}, "glitchwall config --theme phosphor", "the bundle as it comes"),
             ({"palette": "sleeve"}, "glitchwall config --palette sleeve", "...with another theme's colors"),
             ({"rain": "starburst"}, "glitchwall config --rain starburst", "...any of 17 backgrounds"),
             ({"font": "vt323"}, "glitchwall config --font vt323", "...any of 19 fonts, all bundled"),
             ({"transition": "stamp"}, "glitchwall config --transition stamp", "...any of 11 ways to arrive and leave")]
    for change, cmd, why in steps:
        feed.set(**change)
        await asyncio.sleep(1.1)
        clip.new_segment()
        clip.state["caption"] = [cmd, why]
        await clip.run(3.4)
    return clip


RAIN_LOOK = {"pixel": "phosphor", "code": "neon", "stars": "matrix", "static": "amber", "snow": "nightprint",
             "grid": "synthwave", "draft": "drafting", "halftone": "duplicator", "record": "sleeve", "jukebox": "sleeve",
             "starburst": "sleeve", "testcard": "fieldops", "print": "cyanotype", "stop": "bsod",
             "notebook": "notebook", "oracle": "oracle"}


async def scene_backgrounds(feed, br):
    """Art mode, every background (each on a theme that suits it)."""
    defaults(feed, mode="art")
    feed.source(None)
    wall = await br.page()
    await warm(wall, feed, feed.url(), 3)
    clip = Clip("backgrounds", wall, out_w=720)
    for rain, theme in RAIN_LOOK.items():
        feed.set(theme=theme, rain=rain)
        await asyncio.sleep(1.2)
        clip.new_segment()
        clip.state["caption"] = [f"--rain {rain}"]
        await clip.run(1.8)
    return clip


async def scene_modes(feed, br):
    defaults(feed, mode="live", intensity=CALM)
    feed.source(demo.generate, pace=0.45)
    wall = await br.page()
    await warm(wall, feed, feed.url())
    clip = Clip("modes", wall)
    clip.state["caption"] = ["live", "your real Claude Code tool calls, the moment they run (here: a made-up session)"]
    await clip.run(6)
    feed.set(mode="ambient")
    feed.source(ambient.generate, pace=0.45)
    await asyncio.sleep(0.8)
    clip.new_segment()
    clip.state["caption"] = ["glitchwall config --mode ambient", "endless made-up coding sessions; reads nothing, no Claude Code needed"]
    await clip.run(7)
    feed.set(mode="art")
    feed.source(None)
    clip.new_segment()
    clip.state["caption"] = ["glitchwall config --mode art", "just the themed background"]
    await clip.run(5)
    return clip


async def scene_pause(feed, br):
    defaults(feed, mode="ambient", cards=4, intensity=CALM)
    feed.source(ambient.generate, pace=0.35)
    wall = await br.page()
    await warm(wall, feed, feed.url(), 8)
    clip = Clip("pause", wall)
    clip.state["caption"] = ["about to go live?"]
    await clip.run(3.5)
    clip.state["caption"] = ["glitchwall pause", "the feed stops serving; every card is wiped at once"]
    feed.set(paused=True)
    await clip.run(5)
    clip.state["caption"] = ["glitchwall resume", "nothing from the pause is replayed: it picks up from now"]
    feed.set(paused=False)
    await clip.run(6)
    return clip


SECRET_CALLS = [
    ("$", "export STRIPE_KEY=sk_" + "live_51HdemoNotARealKey00000000 && ./deploy.sh",
     "deploying orbit to production ... ok", "sk_live_51Hdemo…"),
    ("$", "curl -H \"Authorization: Bearer gh" + "p_demoDemoDemoDemoDemoDemoDemo1234\" https://api.example.test/me",
     "{\"login\": \"octo-demo\"}", "Bearer ghp_demoDemo…"),
    ("$", "psql postgres://orbit:hunter2@db:5432/orbit -c 'select 1'", " ?column?\n----------\n        1", "orbit:hunter2@db"),
    ("$", "cat .env", "DATABASE_PASSWORD=correct-horse-battery\nOPENAI_API_KEY=sk-proj-" + "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0Uv",
     "correct-horse-battery, sk-proj-Ab1…"),
]


async def scene_redaction(feed, br):
    defaults(feed, mode="live", cards=3, linger=9, type_speed=7, intensity=CALM)
    feed.source(None)
    wall = await br.page()
    await warm(wall, feed, feed.url(), 3)
    clip = Clip("redaction", wall)
    for kind, text, out, raw in SECRET_CALLS:
        feed.say(kind, text, out)
        clip.state["caption"] = [f"the agent saw  {raw}", "masked before it was ever stored: the page only gets ••••"]
        await clip.run(3.6)
    clip.state["caption"] = ["50+ key formats, secret-looking names, random-looking tokens,",
                             "your home folder, user name and email: all masked in the feed itself"]
    await clip.run(4)
    return clip


# --- the settings window, on top of the wallpaper
PANEL = (36, 100, .68)   # where the settings window sits (wallpaper pixels): under the caption, clear of the corner label


def with_panel(clip: Clip, wall: Image.Image, sett: Image.Image) -> Image.Image:
    img, k = wall.copy(), clip.k
    box = clip.state.get("box", PANEL)
    if box:
        panel(img, sett, box, clip.state["alpha"], k)
    if clip.state["cursor"]:
        x, y = clip.state["cursor"]
        cursor(img, x * k, y * k, down=clip.state["down"])
    for text, x, y in clip.state.get("tags", []):
        tag(img, text, x * k, y * k)
    return img


def to_pic(x: float, y: float, box=PANEL) -> tuple[float, float]:
    return box[0] + 1 + x * box[2], box[1] + 1 + y * box[2]


def to_page(x: float, y: float, box=PANEL) -> tuple[float, float]:
    return (x - box[0] - 1) / box[2], (y - box[1] - 1) / box[2]


async def hold(clip: Clip, page: Page, at_page: tuple[float, float], delay: float = .18):
    """Press: the real window fades to 12% after a moment, so the wallpaper behind is the preview."""
    await page.mouse("mousePressed", *at_page)
    clip.state["down"] = True
    await asyncio.sleep(delay)
    clip.state["alpha"] = .12


async def release(clip: Clip, page: Page, at_page: tuple[float, float]):
    await page.mouse("mouseReleased", *at_page)
    clip.state["down"], clip.state["alpha"] = False, 1.0


async def settings_pages(feed, br, **kw):
    defaults(feed, **kw)
    feed.source(ambient.generate, pace=0.35)
    wall, sett = await br.page(), await br.page()
    await wall.open(feed.url(), W, H)
    await sett.open(feed.url("settings.html"), SW, SH)
    await asyncio.sleep(6)
    return wall, sett


async def scene_settings(feed, br):
    wall, sett = await settings_pages(feed, br, mode="ambient", cards=4, intensity=CALM)
    clip = Clip("settings", wall, sett, compose=with_panel)
    clip.state["cursor"] = (W * .45, H * .55)
    clip.state["caption"] = ["glitchwall settings", "every change lands on the real wallpaper as you make it"]
    await clip.run(2.5, glide(clip, to_pic(*await sett.center("#drum")), 1.6))
    # turn the theme drum: hold it (the window fades away) and drag sideways
    x, y = await sett.center("#drum")
    clip.state["caption"] = ["turn the THEME drum", "hold any control and the window fades: the wallpaper is the preview"]

    async def drag_drum():
        await hold(clip, sett, (x, y))
        px = x
        for _ in range(4):
            for _ in range(10):
                px -= 5.2
                clip.state["cursor"] = to_pic(px, y)
                await sett.mouse("mouseMoved", px, y, pressed=True)
                await asyncio.sleep(.05)
            await asyncio.sleep(1.3)
        await release(clip, sett, (px, y))
        await glide(clip, to_pic(x, y), .6)
    await clip.run(9.5, drag_drum())
    # the PAUSE key
    px, py = await sett.center("#pause")
    clip.state["caption"] = ["PAUSE", "the stream-safe switch: one click and the screen is clean"]

    async def pause():
        await glide(clip, to_pic(px, py), 1.0)
        await sett.mouse("mousePressed", px, py)
        await sett.mouse("mouseReleased", px, py)
        await asyncio.sleep(3)
        await sett.mouse("mousePressed", px, py)
        await sett.mouse("mouseReleased", px, py)
    await clip.run(6.5, pause())
    # the corner label
    lx, ly = await sett.center("#label")
    clip.state["caption"] = ["LABEL", "your own words in the corner"]

    async def label():
        await glide(clip, to_pic(lx, ly), 1.0)
        await sett.mouse("mousePressed", lx, ly)
        await sett.mouse("mouseReleased", lx, ly)
        await sett.js("document.querySelector('#label').select()")
        await sett.type("ON AIR :: building in public", .07)
        await sett.key("Enter")
        await glide(clip, (W * .4, H * .5), 1.2)
    await clip.run(7, label())
    return clip


async def scene_sliders(feed, br):
    wall, sett = await settings_pages(feed, br, mode="ambient", cards=4, theme="synthwave")
    clip = Clip("sliders", wall, sett, compose=with_panel)
    clip.state["cursor"] = (W * .45, H * .55)
    plan = [("cards", [1, 6, 4], "CARDS", "how many share the screen, 1 to 6"),
            ("intensity", [0, 10, 5], "MOTION", "how busy the background is: 0 holds it still"),
            ("glass", [20, 95, 58], "GLASS", "see-through or solid cards")
]
    for kind, values, name, why in plan:
        r = await sett.js(f"(() => {{ const f = [...document.querySelectorAll('.fader')].find(f => f.dataset.help === '{kind}');"
                          f" const b = f.querySelector('.track').getBoundingClientRect(); return [b.left, b.top, b.width, b.height]; }})()")
        lo, hi = cfg.TUNE[kind][1], cfg.TUNE[kind][2]
        xs = lambda v: r[0] + 4 + (v - lo) / (hi - lo) * (r[2] - 8)   # noqa: E731
        y = r[1] + r[3] / 2
        cur = cfg.load_settings()[kind]
        clip.state["caption"] = [name, why]

        async def slide():
            await glide(clip, to_pic(xs(cur), y), .9)
            await hold(clip, sett, (xs(cur), y), 0)
            x = xs(cur)
            for v in values:
                tx = xs(v)
                for i in range(1, 19):
                    px = x + (tx - x) * i / 18
                    clip.state["cursor"] = to_pic(px, y)
                    await sett.mouse("mouseMoved", px, y, pressed=True)
                    await asyncio.sleep(.045)
                x = tx
                await asyncio.sleep(1.4)
            await release(clip, sett, (x, y))
        await clip.run(8.5, slide())
    return clip


async def scene_hotcorner(feed, br):
    defaults(feed, mode="ambient", cards=3, intensity=CALM)
    feed.source(ambient.generate, pace=0.4)
    wall, sett = await br.page(), await br.page()
    await wall.open(feed.url(), W, H)
    await sett.open(feed.url("settings.html"), SW, SH)
    await asyncio.sleep(6)
    clip = Clip("hotcorner", wall, sett, compose=with_panel, wall_every=.25)
    clip.state.update(box=None, cursor=(W * .55, H * .6), cap_at="bottom")
    clip.state["caption"] = ["the hot corner", "rest the mouse in the top-left corner of any screen..."]

    async def go():
        await asyncio.sleep(.8)
        await glide(clip, (3, 3), 1.6)
        await asyncio.sleep(.7)
        centre = ((W - SW * .73) / 2, 26, .73)
        clip.state["cap_at"] = "bottom"
        for a in (.25, .5, .75, 1.0):
            clip.state.update(box=centre, alpha=a)
            await asyncio.sleep(.05)
        clip.state["caption"] = ["the hot corner", "...and the settings open, wherever you are"]
        await asyncio.sleep(1.2)
        await glide(clip, (W * .5, H * .62), 1.0)
    await clip.run(7.5, go())
    clip.state["caption"] = ["pick any corner, or OFF", "it never fires while a game or video is fullscreen"]
    await clip.run(2.5)
    return clip


async def scene_dock(feed, br):
    defaults(feed, mode="ambient", cards=3, theme="drafting", intensity=CALM)
    feed.source(ambient.generate, pace=0.4)
    wall, sett = await br.page(), await br.page()
    await wall.open(feed.url(), W, H)
    await sett.open(feed.url("settings.html"), SW, SH)
    await asyncio.sleep(6)
    clip = Clip("dock", wall, sett, compose=with_panel, wall_every=.25)
    s = .6
    pw = SW * s
    start = ((W - pw) / 2, 105, s)
    clip.state.update(box=start, cursor=(start[0] + pw / 2, 117))
    clip.state["caption"] = ["dock it", "drag the settings to the left, right or top edge..."]

    async def go():
        await asyncio.sleep(.6)
        clip.state["down"] = True
        x0 = start[0]
        for i in range(1, 31):   # drag to the right edge
            k = (1 - math.cos(math.pi * i / 30)) / 2
            x = x0 + (W - pw - 2 - x0) * k
            clip.state["box"] = (x, 105, s)
            clip.state["cursor"] = (x + pw / 2, 117)
            await asyncio.sleep(.04)
        clip.state["down"] = False
        await asyncio.sleep(.4)
        await glide(clip, (W * .35, H * .55), 1.0)
        clip.state["caption"] = ["dock it", "...after a few idle seconds it tucks away into a thin strip"]
        await asyncio.sleep(2.2)
        for i in range(1, 9):   # slide away, a strip stays
            clip.state["box"] = (W - pw - 2 + (pw - 4) * i / 8, 105, s)
            await asyncio.sleep(.03)
        await asyncio.sleep(1.4)
        clip.state["caption"] = ["dock it", "touch the strip and it slides back"]
        await glide(clip, (W - 4, H * .4), 1.1)
        for i in range(1, 7):
            clip.state["box"] = (W - 6 - (pw - 4) * i / 6, 105, s)
            await asyncio.sleep(.03)
        await asyncio.sleep(1.8)
    await clip.run(9.5, go())
    return clip


async def scene_screens(feed, br):
    defaults(feed, mode="ambient", cards=4, theme="newsprint")
    feed.source(ambient.generate, pace=0.35)
    a, b = await br.page(), await br.page()
    await a.open(feed.url(query="screen=1"), W, H)
    await b.open(feed.url(query="screen=2"), W, H)
    await asyncio.sleep(7)

    def two(clip, sa, sb):
        gap = round(28 * clip.k)
        img = Image.new("RGB", (sa.width * 2 + gap * 3, sa.height + gap * 2), (22, 24, 26))
        img.paste(sa, (gap, gap))
        img.paste(sb, (sa.width + gap * 2, gap))
        return img
    clip = Clip("screens", a, b, compose=two, out_w=960, full_w=W * 2 + 28 * 3, second_scale=1)
    clip.state["caption"] = ["every screen", "behind the icons on each monitor, same calls, each with its own layout"]
    await clip.run(9)
    return clip


async def still_settings(feed, br):
    """The settings window at full size (docs/shots/settings.png): in the GIFs it's too small to read."""
    defaults(feed, mode="ambient", theme="phosphor")
    feed.source(None)
    sett = await br.page()
    await sett.open(feed.url("settings.html"), SW, SH)
    await asyncio.sleep(4)
    x, y = await sett.center("#drum")
    await sett.mouse("mouseMoved", x, y)   # the readout explains what's under the mouse
    await asyncio.sleep(.8)
    out = ROOT / "docs" / "shots" / "settings.png"
    (await sett.shot(1.0)).save(out, optimize=True)
    print(f"  {out.relative_to(ROOT)}  {out.stat().st_size / 1e3:.0f} kB", flush=True)
    return None


SCENES = {"themes": scene_themes, "mix": scene_mix, "backgrounds": scene_backgrounds, "modes": scene_modes,
          "pause": scene_pause, "redaction": scene_redaction, "settings": scene_settings, "sliders": scene_sliders,
          "hotcorner": scene_hotcorner, "dock": scene_dock, "screens": scene_screens, "settings-still": still_settings}


async def main(names: list[str]) -> None:
    random.seed(7)
    feed, br = Feed(), Browser()
    try:
        for n in names:
            print(n, flush=True)
            clip = await SCENES[n](feed, br)
            await br.close_pages()
            if clip:
                clip.save()
    finally:
        feed.source(None)
        feed.httpd.shutdown()
        br.close()
        shutil.rmtree(os.environ["GLITCHWALL_HOME"], ignore_errors=True)


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1:] or list(SCENES)))
