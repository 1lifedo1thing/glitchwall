# Third-party notices

The palettes of the first eight glitchwall themes and the grain, scanline and halftone overlay
textures are adapted from SUB/WAVE (https://github.com/perminder-klair/subwave), under this license:

```
MIT License

Copyright (c) 2026 Parminder Klair

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Fonts: see src/glitchwall/wallpaper/OFL.txt and src/glitchwall/wallpaper/fonts/licenses/ (SIL Open Font License 1.1).

`glitchwall-render.exe`, the native renderer shipped in the release package, is compiled from glitchwall's own
Rust code and these libraries (used under their MIT terms; the MIT license text is above, with each project's
own copyright line, found in its repository):

| Library | Version | License |
| --- | --- | --- |
| [fancy-regex](https://github.com/fancy-regex/fancy-regex) | 0.14.0 | MIT |
| [regex-automata, regex-syntax](https://github.com/rust-lang/regex) | 0.4.19, 0.8.12 | MIT OR Apache-2.0 |
| [aho-corasick](https://github.com/BurntSushi/aho-corasick) | 1.1.5 | Unlicense OR MIT |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [bit-set, bit-vec](https://github.com/contain-rs/bit-set) | 0.8.0 | Apache-2.0 OR MIT |
| [windows, windows-core, windows-result, windows-strings, windows-targets, windows_x86_64_gnu](https://github.com/microsoft/windows-rs) | 0.58.0, 0.52.6 | MIT OR Apache-2.0 |

(proc-macro2, quote, syn, unicode-ident, windows-implement and windows-interface only run while it's compiled;
none of their code is in the exe.)

On Windows, glitchwall's desktop and settings windows use these Python libraries. They aren't bundled in
glitchwall's package: installing it fetches them from PyPI, each with its own license text:

| Library | License |
| --- | --- |
| [pywebview](https://github.com/r0x0r/pywebview) | BSD-3-Clause |
| [pythonnet](https://github.com/pythonnet/pythonnet) | MIT |
| [clr-loader](https://github.com/pythonnet/clr-loader) | MIT |
| [cffi](https://github.com/python-cffi/cffi) | MIT-0 |
| [pycparser](https://github.com/eliben/pycparser) | BSD-3-Clause |
| [bottle](https://github.com/bottlepy/bottle) | MIT |
| [proxy_tools](https://github.com/jtushman/proxy_tools) | MIT |
| [typing_extensions](https://github.com/python/typing_extensions) | PSF-2.0 |
| Microsoft WebView2 SDK (`Microsoft.Web.WebView2.*.dll`, shipped inside pywebview) | [Microsoft WebView2 SDK license](https://www.nuget.org/packages/Microsoft.Web.WebView2/License) |
