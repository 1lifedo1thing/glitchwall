# agent-live-wallpaper

Your coding agent's tool calls as a live desktop wallpaper. Every command, edit,
read and search your agent runs pops up somewhere on the screen like a retro
pop-up ad: it glitches in, types itself out with its output underneath, stays a
few seconds, then collapses like an old CRT.

Green phosphor, [Departure Mono](https://departuremono.com), frosted glass,
scanlines, dim pixel rain. Works today with **Claude Code** session logs.

![four tool calls on the wallpaper, one curl command with its Authorization header masked](docs/screenshot-1.png)
![a git commit, a cargo build and an ls on the wallpaper](docs/screenshot-2.png)

*Both screenshots come from demo mode (`agent-wallpaper demo`), not real work.*

## Try it without installing anything

Needs [uv](https://docs.astral.sh/uv/) (or pipx). Nothing is read from your logs in demo mode.

```
uvx agent-live-wallpaper demo --open
```

(Not on PyPI yet. From a checkout of this repo: `uvx --from . agent-live-wallpaper demo --open`.)

That starts a fake feed and opens the wallpaper page in your browser at
`http://127.0.0.1:47834/`. Ctrl+C to stop.

## Install (Windows + Lively Wallpaper)

1. Install [Lively Wallpaper](https://github.com/rocksdanister/lively) (free, open source).
2. Install this tool for good (so it can start at logon), then run its installer:

```
uv tool install agent-live-wallpaper
agent-wallpaper install
```

`install` copies the wallpaper into Lively's library, writes its `config.js`,
starts the feed in the background, adds a start-at-logon entry and switches
Lively to it. Your previous wallpaper is remembered.

| Command | What it does |
| --- | --- |
| `agent-wallpaper install` | set everything up (`--commands-only`, `--no-autostart`, `--no-set`, `--port N`) |
| `agent-wallpaper status` | what is installed and running |
| `agent-wallpaper start` / `stop` | start or stop the background feed |
| `agent-wallpaper pause` / `resume` | **stream-safe**: the feed serves nothing and the wallpaper clears at once; only rain stays |
| `agent-wallpaper config --commands-only on` | show the calls but never their output |
| `agent-wallpaper config --label "TEXT"` | the text in the bottom-right corner |
| `agent-wallpaper demo` | fake events + the page, for screenshots and trying it |
| `agent-wallpaper export DIR` | write the wallpaper and its `config.js` to a folder (other wallpaper apps) |
| `agent-wallpaper uninstall` | remove everything `install` added and put your old wallpaper back |

Lively's wallpaper settings give you sliders for typing speed, how long a block
lingers, blocks on screen, rain and glass opacity.

**Streaming?** Run `agent-wallpaper pause` before you go live (or put it in
your stream start script) and `resume` after. Pausing happens in the feed
itself, so nothing can reach the screen even if the wallpaper is visible.
`--commands-only` is a lighter option that hides all tool output.

## What it reads, and what it never does

**Reads**

- Claude Code session logs: `~/.claude/projects/*/*.jsonl` (or `$CLAUDE_CONFIG_DIR/projects`),
  opened **read-only**, only logs touched in the last 30 minutes, only the
  newly appended part. From each line it takes tool calls and their results.
- Its own files: a random key, `settings.json`, a pid file.
- On Windows, Lively's `Settings.json` and `WallpaperLayout.json` (to find the
  library and to know which wallpaper to put back).

**Never**

- **No network.** The feed listens on `127.0.0.1` only. It makes no outgoing
  connections. No telemetry, no update checks, no analytics. The wallpaper
  page loads nothing from the internet (the font is bundled).
- **Never writes to your logs** or anywhere outside the paths listed below.
- **Never shows unmasked secrets.** Text is redacted *before* it is stored in
  memory, so nothing unredacted is ever served. See [Redaction](#redaction).
- **Never serves anyone but the wallpaper.** Every request needs the per-install
  random key (only the wallpaper's `config.js` has it), a `Host` of
  `127.0.0.1`/`localhost` on the right port (blocks DNS rebinding), and either
  no `Origin`, `null` (a local file page) or `*.localhost` (how Lively serves
  wallpapers). Websites you visit can't read it. Refusals are noted in
  `last-refused.txt` without the key.

**Files it adds** (all removed by `uninstall`)

| Where | What |
| --- | --- |
| `%USERPROFILE%\.agent-live-wallpaper\` (Windows), `~/Library/Application Support/agent-live-wallpaper/` (macOS), `~/.local/state/agent-live-wallpaper/` (Linux) | key, settings, pid file, refusal note, previous-wallpaper note |
| `<Lively library>\wallpapers\agent-live-wallpaper\` | the wallpaper page and its `config.js` |
| `HKCU\...\CurrentVersion\Run\agent-live-wallpaper` / `~/.config/autostart/agent-live-wallpaper.desktop` / `~/Library/LaunchAgents/io.github.agent-live-wallpaper.plist` | start at logon |

Why the home folder on Windows, not `%LOCALAPPDATA%`: packaged (MSIX) apps such
as Claude Desktop or Store terminals get new AppData folders redirected into a
private copy. `install` run from one of them and the feed started at logon
would then read different key files, and every request would be refused.
Version 0.1.0 used `%LOCALAPPDATA%\agent-live-wallpaper`; `install` and
`uninstall` move or remove it. Set `AGENT_WALLPAPER_HOME` to put the state
somewhere else.

## Redaction

Applied to every command, file path and output before it is kept:

- **Known formats**: AWS, Google, Azure (storage keys, SAS signatures), GitHub,
  GitLab, Slack (tokens and webhooks), Discord, Stripe, Anthropic, OpenAI,
  Hugging Face, npm, PyPI, NuGet, Docker Hub, DigitalOcean, Vault, Tailscale,
  SendGrid, Mailgun, Twilio, Doppler, Grafana, Netlify, Vercel, Telegram and
  more; JWTs; PEM / OpenSSH / PGP / PuTTY / age private keys.
- **By name**: `password=`, `API_KEY: ...`, `"client_secret": "..."`,
  `export X_TOKEN=`, `$env:X_KEY = `, `setx`, `--password x`, `--api-key=x`,
  `mysql -pX`, `curl -u user:pass`, `Authorization:` / `Cookie:` / `X-Api-Key:`
  headers, `.netrc`, credentials in URLs and connection strings
  (`postgres://user:pass@`, `Password=...;`).
- **High entropy**: long random-looking tokens (mixed case + digits, high
  Shannon entropy) and long hex strings are masked even with no known prefix.
- **Personal**: your home folder becomes `~` (all Windows, Git Bash and WSL
  spellings), your user name becomes `user`, email local parts are masked.
- **Optional**: `--commands-only` drops all tool output.

Redaction errs toward masking. It is a safety net, not a guarantee: if you
stream or share your screen, also use `pause`. The tests in
[`tests/test_redact.py`](tests/test_redact.py) feed 50+ realistic fake secrets
through it.

## macOS and Linux

The feed and the page are cross-platform (Python stdlib, plain HTML), but only
Windows + Lively is tested so far.

```
uv tool install agent-live-wallpaper
agent-wallpaper install
```

On macOS/Linux, `install` writes the wallpaper to the state dir, adds a login
item (LaunchAgent / XDG autostart), starts the feed and prints the page's
`file://` path. Then either:

- point a web-wallpaper app at that `index.html`, for example
  [Plash](https://github.com/sindresorhus/Plash) (macOS) or
  [Komorebi](https://github.com/Komorebi-Fork/komorebi) / KDE's
  HTML wallpaper plugins (Linux), or
- open it fullscreen in a browser on a spare monitor:
  `chromium --kiosk file:///path/to/index.html` (or `firefox --kiosk`).

A `file://` page sends `Origin: null`, which the feed accepts with the key.

## Development

```
uv run python -m unittest discover -s tests
uv build
```

Stdlib only at runtime, Python 3.9+. Code layout:

| File | Role |
| --- | --- |
| `src/agent_live_wallpaper/claude_logs.py` | follow the logs, pair each call with its output |
| `src/agent_live_wallpaper/redact.py` | all masking |
| `src/agent_live_wallpaper/server.py` | the local feed and its access checks |
| `src/agent_live_wallpaper/platforms.py` | Lively, autostart, background process |
| `src/agent_live_wallpaper/wallpaper/` | the page itself (`index.html?still` renders without animation, for screenshots) |

## License

MIT, see [LICENSE](LICENSE). Departure Mono is © Helena Zhang, SIL Open Font
License 1.1, see [OFL.txt](src/agent_live_wallpaper/wallpaper/OFL.txt).
Not affiliated with Anthropic or Lively.
