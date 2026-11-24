<sub>[← back to the README](../README.md)</sub>

# Performance

One small Python process that checks for new log lines once a second, and one small renderer that draws every
screen at most 30 frames a second, whatever your monitor's refresh rate: about 6.5% of one core and 153 MB awake
(the old WebView2 version took 34% and 699 MB). A screen covered by a maximized or fullscreen window (a game, a
video, your editor) goes to sleep until it's uncovered, and so does every screen while the PC is locked, drawing
nothing: then the whole of glitchwall uses under 1% of one core (memory stays where it was). MOTION at 0 stills
the background entirely.

Nobody publishes like-for-like benchmarks of live wallpapers, so take this as a rough guide. Our numbers
are measured; most of the others are one person's test or a forum post, on different hardware, drawing
different things.

| | What it draws | CPU | Memory | Open source | Where the number comes from |
|---|---|---|---|---|---|
| **glitchwall** | Direct2D, no browser | 6.5% of one core (0.4% of a 16-core CPU) | 153 MB | yes | our test, below |
| glitchwall, asleep (PC locked, or a window covering the desktop) | nothing | under 1% of one core (0.05% of a 16-core CPU) | about the same (162 MB) | yes | our test, below |
| [Lively Wallpaper](https://github.com/rocksdanister/lively) | simple web wallpaper (rain) | under 0.3% of the whole CPU | about 245 MB | yes | [one reviewer](https://www.windowscentral.com/software-apps/i-finally-tried-lively-wallpaper-to-animate-my-desktop-its-a-dream-for-customization) |
| Lively Wallpaper | a YouTube page as wallpaper | about 4.5% of the whole CPU | about 800 MB | yes | same reviewer |
| Wallpaper Engine | 60 fps video, hardware decoding | 1-2% of the whole CPU, ~40% GPU | not given | no | [forum post](https://steamcommunity.com/app/431960/discussions/2/1696045708644625403) |
| [anipaper](https://github.com/Theldus/anipaper) (Linux) | 1080p 30 fps video, hardware decoding | 10.8% of a 4-core CPU (about 43% of one core) | not given | yes | author's benchmark |
| [Hidamari](https://github.com/jeffshee/hidamari) (Linux) | video, Intel integrated GPU | about 1% CPU, 4% GPU | not given | yes | [article author](https://ubuntuhandbook.org/index.php/2024/10/video-animated-wallpaper-ubuntu/) |
| macOS aerial wallpapers | built-in video | 20-100% CPU when it misbehaves | up to GBs | no | [bug reports](https://discussions.apple.com/thread/255180158) |
| glitchwall (old version: web page in WebView2) | animated web page | 34% of one core (2.2%) | 699 MB | yes | our test, below |

Our test: a 16-core Windows 11 desktop, matrix theme, ambient mode, every glitchwall process counted (the
feed and the renderer), the desktop showing, 30 seconds. Native: three runs of 6.2-6.7% (2026-11-23); the
default phosphor theme came out the same, and orb, the busiest background, at 10.3% of one core and 172 MB.
Asleep (2026-11-24, phosphor, live mode): 0.83% of one core over 60 s with the PC locked, and 0.62% over 30 s with a
maximized window covering the desktop; what's left is the feed checking the logs once a second and the renderer
asking it whether to wake. The old WebView2 version's figure is from 2026-11-22.

**What the renderer does.** On Windows, glitchwall draws every background with Direct2D straight from a small
Rust program, with no browser in between: about a fifth of the CPU and a fifth of the memory of the old WebView2
version. That old version is still there as a fallback if the renderer is missing (`glitchwall config --renderer
web` picks it on purpose). In OBS and Wallpaper Engine, the web page is the way to run glitchwall.

**Why the web page is heavier than a video wallpaper.** A video wallpaper hands its work to the graphics
card's video decoder, so the CPU barely notices. glitchwall draws every frame live (falling glyphs, cards,
text), and the page does it in WebView2, a full copy of the Edge browser: a browser process, a GPU process
and a renderer for each page. That's where most of its memory goes; Lively's web wallpapers use the same
engine and land in the same range. Either way, glitchwall caps frame rates per background (10-30 fps, 60 only
while a card slides in), and sleeps entirely while a screen is covered, fullscreen or locked, like Lively and
Wallpaper Engine do.
