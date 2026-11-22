"""Draw glitchwall's icon: a pixel-art G in phosphor green with an RGB split and one glitched slice.

    uv run --no-project --with pillow python tools/make_icon.py

Drawn on a 16x16 grid and scaled by whole numbers, so every size stays sharp. Writes
src/glitchwall/wallpaper/glitchwall.ico (16-256 px) and glitchwall.png (256 px, the settings page's tab icon).
"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "src" / "glitchwall" / "wallpaper"
BG, EDGE = (6, 16, 9, 255), (22, 74, 36, 255)
GREEN, MAGENTA, CYAN = (0, 255, 65, 255), (255, 43, 214, 255), (0, 229, 255, 255)

G = [   # row -> filled columns
    (3, [*range(6, 11)]),
    (4, [*range(5, 12)]),
    (5, [4, 5, 6, 10, 11]),
    (6, [4, 5]),
    (7, [4, 5, 8, 9, 10, 11]),
    (8, [4, 5, 8, 9, 10, 11]),
    (9, [4, 5, 10, 11]),
    (10, [4, 5, 6, 10, 11]),
    (11, [*range(5, 12)]),
    (12, [*range(6, 11)]),
]
SLICE = {7: 1, 8: 1}   # rows pushed sideways: the glitch


def grid() -> dict[tuple[int, int], tuple]:
    px = {}
    for y in range(16):
        for x in range(16):
            corner = (x in (0, 15)) and (y in (0, 15))
            if not corner:
                px[x, y] = EDGE if x in (0, 15) or y in (0, 15) else BG
    rows = {y: [x + SLICE.get(y, 0) for x in xs] for y, xs in G}
    for y, xs in rows.items():   # the RGB split: magenta just left of each row, cyan just right
        px[min(xs) - 1, y] = MAGENTA
        if max(xs) > 8:   # not where the row ends inside the letter's opening
            px[max(xs) + 1, y] = CYAN
        for x in xs:
            px[x, y] = GREEN
    return px


def render(size: int) -> Image.Image:
    k = size // 16
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    for (x, y), c in grid().items():
        d.rectangle([x * k, y * k, (x + 1) * k - 1, (y + 1) * k - 1], fill=c)
    if k >= 8:   # faint scanlines, only where there's room for them
        for y in range(k, size - k, k // 2):
            d.line([(k, y), (size - k - 1, y)], fill=(0, 0, 0, 40))
    return img


if __name__ == "__main__":
    big = render(256)
    big.save(OUT / "glitchwall.png")
    sizes = [16, 32, 48, 64, 128, 256]
    big.save(OUT / "glitchwall.ico", sizes=[(s, s) for s in sizes],
             append_images=[render(s) for s in sizes[:-1]])
    preview = Image.new("RGBA", (16 + 32 + 48 + 64 + 256 + 60, 256), (32, 32, 32, 255))
    x = 0
    for s in (16, 32, 48, 64, 256):
        preview.paste(render(s), (x, 256 - s))
        x += s + 12
    preview.save(Path(__file__).resolve().parent.parent / "build" / "icon-preview.png")
    print("wrote", OUT / "glitchwall.ico", OUT / "glitchwall.png")
