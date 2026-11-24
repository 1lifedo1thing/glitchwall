<sub>[← back to the README](../README.md)</sub>

# How glitchwall got here

**The idea (21 November 2026).** It started as `agent-live-wallpaper`: a wallpaper that shows what a coding agent is
doing, live, as it works. A small local feed follows the agent's logs, pairs each call with its output, masks
secrets, and serves it to a web page behind the desktop icons. It was renamed glitchwall the same day, got a
standalone exe, and grew from 8 looks to 15, then the oracle.

**Making it safe.** A wallpaper that shows your terminal is only useful if it never shows a secret. Three review
passes went after the leaks: user names and passwords in paths and URLs, bursts of old calls on start, a race in
saving settings, a stale access key. Later a security pass narrowed which pages may talk to the feed and taught
the redactor more secret shapes. A pause switch blanks everything at once, for going live.

**Owning the desktop.** glitchwall first leaned on Lively Wallpaper to sit behind the icons. It then got a desktop
host of its own: one window per screen, behind the icons, taking no mouse input, snapping back if Windows moves it,
and coming back by itself if the browser engine crashes. Lively support was removed.

**The settings window, five times over.** It was a cipher machine, then green phosphor lamps on the same case, then
a rotary drum of themes, then a green phosphor terminal. One version showed the settings as bars around the edges
of the screen with the real wallpaper live in the middle. That hit a wall: WebView2 paints over any hole cut in its
window, so the middle stayed grey. After proving it against a plain Win32 window, that version was dropped for a
small window that fades away while you hold a slider, so the real wallpaper is the preview.

**The cost problem.** Every theme now had its own animated background, and it showed in Task Manager. The page ran
in WebView2, a full copy of the Edge browser, and drew at the monitor's refresh rate. Capping frame rates and
sleeping while the screen is covered helped, but measured honestly it still took about a third of a CPU core and
700 MB, far more than a wallpaper should.

**Going native.** A prototype drew the phosphor theme with Direct2D, first in C#, then in Rust, and measured
it against WebView2: about 3.5% CPU and 63 MB against 18% and 330 MB. That settled it. The plan:

1. Put every theme, background, font and transition in one file (`looks.js`) that both the page and the new
   renderer read, so a theme is defined once.
2. A small Rust renderer (`glitchwall-render`) that reads the same feed as the page and draws behind the icons.
3. Port the backgrounds one at a time, each checked against the page side by side.
4. Make native the default on Windows, and keep the page for OBS and Wallpaper Engine.

**The hurdles on the way:**

- **Toolchain.** Newer versions of Rust's `windows` crate need extra build tools: either a large download or tools
  that aren't free software. glitchwall stays on version 0.58, the last that builds with the stock toolchain.
- **Neon letters.** The neon signs glow along the shape of each letter, which a browser does with a blurred
  shadow. Natively, each letter is turned into its outline, drawn into its own small picture, blurred twice for the
  glow and lit with a bright core.
- **Chalk.** A chalkboard has to remember everything written on it until it's wiped. Native chalk draws into a
  second surface that keeps the writing, and wipes it the way an eraser would.
- **The orb.** The busiest scene: a ribbed sphere, a triangle-net cage, hundreds of red lights and racks fading
  into haze. It's drawn as layers made once, plus lights drawn in batches each frame.
- **Display scaling, docked cards, stacked cards**: each had to match the page exactly.
- **Measuring honestly.** Early numbers came out impossibly low because a window covered the screen and glitchwall
  had (correctly) gone to sleep. Every number in this README is taken with the desktop showing.

**The payoff (23 November 2026).** All 27 backgrounds run natively, the 18 theme ones with four options each, and native is the
default on Windows. With the same looks, glitchwall now uses 6.5% of one core and 153 MB, down from 34% and 699 MB:
about a fifth of the CPU and a fifth of the memory (see [Performance](PERFORMANCE.md)).
The web page is still there, unchanged, for OBS browser sources and Wallpaper Engine.

**Cutting back (23 November 2026).** Not every idea was a good one. Terminal capture grew from "show my PowerShell
commands" to cmd through Clink and WSL, and finally to reading other console windows, before it was clear that it
didn't belong in a wallpaper at all; it was removed. So was the installer that downloaded WebView2, and the old C#
and Rust prototypes from the going-native work (their plan lives on in [native-plan.md](native-plan.md)).
See [What it deliberately doesn't do](PRIVACY.md#what-it-deliberately-doesnt-do-and-why) for why.
