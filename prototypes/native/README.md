# Native renderer prototype (phosphor)

A question in code form: what would glitchwall cost if it drew the wallpaper itself instead of in WebView2?

[PhosphorNative.cs](PhosphorNative.cs) draws the phosphor theme with Direct2D and DirectWrite, in one window behind the desktop
icons (the same trick as `winhost.py`):
- the pixel rain at a third of the resolution, upscaled with hard edges;
- scanlines, vignette and grain as one bitmap made at start;
- up to four cards with typed commands and output, a fake glow and a CRT-line arrival.

It's built with the C# compiler that comes with Windows (.NET Framework 4.8), so nothing is downloaded. The build command
is at the top of the file.

## Measured

Both versions awake behind the icons, 60 s each, two rounds, on a 1920x1080 200 Hz screen (Radeon 780M). WebView2 is
glitchwall d4a280d in ambient mode with phosphor. CPU, GPU and memory cover each version's whole process tree.

| | WebView2 (glitchwall today) | Native prototype |
|---|---|---|
| CPU | 18.1-19.4% of one core | **4.2%** |
| GPU (3D engine) | 6.1-6.5% | **3.4-3.9%** |
| Private memory | 331-347 MB | **87 MB** |
| Processes | 7 | 1 |

## What the prototype leaves out (so the real thing would cost a bit more)

- **Card glass blur.** Direct2D 1.0 has no blur; it needs Direct2D 1.1 effects on a DXGI swap chain. It costs about 2% in WebView2.
- **The bundled fonts.** It uses Consolas. Loading the bundled OFL fonts needs a DirectWrite custom font loader.
- **Real glow.** The glow is four offset copies of the text, not a blurred shadow.
- **Everything but one theme.** No other themes or transitions, no multi-monitor, no settings, no feed: the events are made up.
- **The feed.** glitchwall's Python feed (log tailing, redaction, about 1-1.5% and 68 MB) would still run next to it,
  unless it is ported too.

## What it suggests

- **Most of WebView2's cost is Chromium, not the drawing.** The native version does the same work for about a quarter of
  the CPU, half the GPU and a quarter of the memory.
- **A full port is a rewrite of the look.** Each of the 19 themes is canvas JavaScript plus CSS animations, and each
  would need redoing.
- **The settings window can stay a web page.** It's only open briefly.
- **The rain itself can get cheaper.** About 3,700 one-pixel draw calls a frame are the prototype's main CPU cost, and
  most of that can go.
