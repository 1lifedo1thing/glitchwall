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

### Rust

[../native-rs](../native-rs) is the same picture in Rust (the `windows` crate, 0.58, GNU toolchain). One change: the rain is
drawn into a buffer in memory and copied to the GPU once per frame, instead of thousands of one-pixel draws. The exe is
277 KB and needs no runtime. A second run, all three versions in turn, same method:

| | WebView2 | C# | Rust |
|---|---|---|---|
| CPU | 18.2-18.9% | 4.1-4.4% | **3.3-3.6%** |
| GPU (3D engine) | 7.7-7.8% | 5.2-5.4% | **4.7-5.0%** |
| Private memory | 327-339 MB | 92-93 MB | **63-64 MB** |
| Processes | 7 | 1 | 1 |

Rust saves about another 0.8% CPU and 30 MB over C#. That's mostly the .NET runtime's memory and the cheaper rain,
not the language. Most of what's left is Direct2D and DWM composing a full-screen window 30 times a second.

### Frame rate (Rust, `--fps N`)

Same setup, 45 s per run, two rounds. 200 is this screen's refresh rate.

| fps | 10 | 15 | 20 | 30 | 60 | 200 |
|---|---|---|---|---|---|---|
| CPU | 0.9-1.2% | 1.7-2.1% | 2.1% | 3.3-3.5% | 6.9-7.1% | 14.8-18.9% |
| GPU (3D engine) | 1.3-1.5% | 2.2-2.5% | 2.4-3.5% | 3.6-4.6% | 6.6-11.2% | 17.7% |

The cost follows the frame rate almost exactly. At the monitor's rate, the native renderer costs as much as WebView2
does at 30 fps.

Rust build note: the `windows` crate from 0.59 on links with `raw-dylib`, which on the GNU toolchain needs `dlltool`
plus an assembler that rustup doesn't ship. 0.58 brings its own import libraries, so it builds with nothing else
installed.

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
