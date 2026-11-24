<sub>[← back to the README](../README.md)</sub>

# Privacy and security

> **TL;DR:** it reads your agent's logs, masks the secrets, and shows them to one page on your own machine. That's it.

### ✅ It reads

In `ambient` and `art` mode it reads no logs at all, only its own settings. In `live` mode:

- **Claude Code session logs:** `~/.claude/projects/*/*.jsonl` (or `$CLAUDE_CONFIG_DIR/projects`),
  opened **read-only**, only logs touched in the last 30 minutes, only the newly appended part.
  From each line it takes tool calls and their results.
- **Its own files:** a random key, `settings.json`, a pid file.

Nothing else: not your terminal, not your shell history, not other windows, not the screen.

### 🚫 It never

- **Talks to the internet.** The feed listens on `127.0.0.1` only and makes no outgoing connections.
  No telemetry, no analytics, no update checks. The page loads nothing remote (the fonts are bundled).
  The one part glitchwall doesn't control: WebView2 is Microsoft's Edge engine, and it may talk to
  Microsoft for its own updates and safety checks the way Edge does. The page itself only ever asks `127.0.0.1`.
- **Writes to your logs**, or anywhere outside the files listed below. It never edits your shell profiles,
  `~/.bashrc` or any other program's settings.
- **Records what you type** or reads other windows. See [What it deliberately doesn't do](#what-it-deliberately-doesnt-do-and-why).
- **Downloads or installs anything.** If something it needs is missing, it tells you and can open the
  download page; the rest is up to you.
- **Shows unmasked secrets.** Text is redacted *before* it's stored in memory, so there's nothing unredacted to leak. See [Redaction](#redaction).
- **Serves anyone but the wallpaper.** Every request needs:
  - the per-install random key, which only the wallpaper has (in its `config.js`, or after the `#` in the
    address of glitchwall's own windows, which is never sent to a server), compared in constant time;
  - a `Host` of `127.0.0.1`/`localhost` on the right port, which blocks DNS rebinding;
  - no `Origin`, `null` (a local file page), or the feed's own address (glitchwall's own windows load the
    page from the feed). Pages from other local servers are refused too.

  Changing a setting from the settings window needs all of that too, plus a JSON request sent from the
  feed's own address, so neither a website, a file page nor a plain form can change one. Port and screens
  can't be changed from it at all.

  Websites you visit can't read it. Refused requests are noted in `last-refused.txt`, without the key.

  Its pages can be shown inside a frame only by another of its own pages (`frame-ancestors 'self'`): the
  settings window frames the wallpaper page, with no key, to draw a backdrop's thumbnail. A website can't
  frame them, so it can't trick you into clicking the settings window through a page of its own.

### 📁 Files it adds (all removed by `uninstall`)

| Where | What |
| --- | --- |
| `%USERPROFILE%\.glitchwall\` | key, settings, pid file, refusal note, `webview\` and `webview-settings\` (WebView2's caches for glitchwall's own windows) |
| `HKCU\...\CurrentVersion\Run\glitchwall` | start at logon |
| `%APPDATA%\Microsoft\Windows\Start Menu\Programs\glitchwall settings.lnk` | Windows: the settings window in the Start menu |
| `HKCU\Software\Classes\DesktopBackground\Shell\glitchwall` | Windows: *glitchwall settings* in the desktop's right-click menu |

**Why the home folder on Windows, not `%LOCALAPPDATA%`?** Packaged (MSIX) apps such as Claude Desktop
or Store terminals get new AppData folders redirected into a private copy. `install` run from one of
them and the feed started at logon would then read different key files, and every request would be
refused. Set `GLITCHWALL_HOME` to put the state somewhere else.

### What it deliberately doesn't do, and why

glitchwall is a wallpaper: something nice to look at that happens to show what your agent is doing. Anything that
would make it look like it's collecting your information has no place in it, even when it works and is masked.
So these were built, tried and taken out again, on purpose:

- **Your own terminal commands.** For a while `glitchwall terminal on` put a small hook in your PowerShell
  profile, WSL's `~/.bashrc` and (through Clink) cmd, so the commands you typed showed as cards too. To make the
  output show for WSL, it went as far as reading other console windows' screens. It all stayed on your PC and was
  masked, but a program that logs every command you type and reads other windows is exactly what spyware does, and
  no amount of explaining makes that code look harmless to someone reading it. It was removed, hooks and all.
  Your agent's calls (which include the commands it runs) are the show; your own typing isn't.
- **Fetching what's missing.** `install` used to download Microsoft's WebView2 installer and run it, after
  checking Microsoft's signature. A wallpaper that downloads and runs installers is a second "wait, what?", and
  Windows 11 already has WebView2. Now it only tells you and offers the download page.

What's left reads one thing, your agent's own logs, read-only, and draws them. If a future idea needs more than
that, it has to be worth explaining here first.

## Redaction

Applied to every command, file path and output before it's kept:

| Layer | What it catches |
| --- | --- |
| **Known formats** | AWS, Google, Azure (storage keys, SAS signatures), GitHub, GitLab, Slack (tokens and webhooks), Discord, Stripe, Anthropic, OpenAI, Hugging Face, npm, PyPI, NuGet, Docker Hub, DigitalOcean, HashiCorp Vault, Tailscale, SendGrid, Mailgun, Twilio, Doppler, Grafana, Netlify, Vercel, Telegram and more; JWTs; PEM / OpenSSH / PGP / PuTTY / age private keys |
| **By name** | `password=`, `API_KEY: …`, `"client_secret": "…"`, `export X_TOKEN=`, `$env:X_KEY =`, `setx`, `--password x`, `--api-key=x`, `mysql -pX`, `curl -u user:pass`, `Authorization:` / `Cookie:` / `X-Api-Key:` headers, `.netrc`, credentials in URLs and connection strings (`postgres://user:pass@`, `Password=…;`) |
| **High entropy** | long random-looking tokens (mixed case + digits, high Shannon entropy) and long hex strings, even with no known prefix |
| **Personal** | home folder → `~` (all Windows, Git Bash and WSL spellings), user name → `user`, email local parts → `••••` |
| **Optional** | `--commands-only` drops all tool output |

It errs toward masking. The tests in [`tests/test_redact.py`](../tests/test_redact.py) feed 50+ realistic fake
secrets through it, plus ordinary text that must survive untouched.

## ⚠️ Disclaimers (please read)

- **Redaction is a safety net, not a guarantee.** It's pattern- and entropy-based. A secret with no
  known format, no telling variable name and low randomness (say, a dictionary-word password typed as a
  bare argument) can get through. **You are responsible for what's on your screen.** When streaming,
  recording or screen sharing, use `glitchwall pause`.
- **It can't protect you from local malware.** Any program already running as you can read your
  agent's logs and this tool's key file directly.
- **Your logs contain whatever your agent did.** glitchwall shows a masked slice of that; it doesn't
  decide what your agent runs.
- **`config.js` holds your feed key.** Don't publish an exported wallpaper folder. If you did, delete
  the `key` file in the state folder and run `glitchwall install` again.
- **Provided as is, with no warranty**, under the MIT license. See [LICENSE](../LICENSE) and [SECURITY.md](../SECURITY.md).
- **Not affiliated with Anthropic.** "Claude" and "Claude Code" are trademarks of
  Anthropic; they're named here only to say which logs glitchwall reads.
- **Free and open source, forever.** No accounts, no paid tier, no ads. Donations are appreciated but never
  required, and they don't unlock anything. See [Support](../README.md#support).
