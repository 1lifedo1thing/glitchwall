# Security

This tool shows parts of your coding sessions on screen, so leaks matter more
than usual. Its promises:

1. **Local only.** The feed binds `127.0.0.1`, makes no outgoing connections,
   has no telemetry or update checks. The page loads nothing remote.
2. **Read-only on your logs.** Session logs are opened for reading; nothing in
   `~/.claude` is ever written.
3. **Redact before storing.** Text is masked as it enters memory, so the feed
   can't serve unmasked text even by mistake.
4. **Only the wallpaper may read the feed.** Per-install random key (32 bytes,
   `secrets.token_urlsafe`) compared in constant time, `Host` pinned to
   `127.0.0.1:<port>` / `localhost:<port>` against DNS rebinding, `Origin`
   limited to absent, `null` (a local file page) or the feed's own address.
   Other local servers' pages are refused too. CORS headers are only sent to
   allowed origins. Changing a setting also needs a JSON POST from the feed's
   own address, where the settings window lives.
5. **Not framed by others, not sniffed.** Every response says
   `X-Frame-Options: SAMEORIGIN`, `frame-ancestors 'self'`, `nosniff` and
   `Referrer-Policy: no-referrer`. Only glitchwall's own pages may frame one
   (the settings window shows the wallpaper page, without the key, as a
   backdrop thumbnail); a website can't, so it can't trick you into clicking
   the settings through a page of its own.
6. **A pause can't be undone by accident.** A settings file glitchwall can't
   read (a bad hand edit) counts as paused when it starts; a running feed
   keeps its last good settings.
7. **No input watching.** It never reads the keyboard or the mouse buttons
   outside its own windows. It looks only at the mouse position (hot corner)
   and where other windows are (to stop drawing when the wallpaper is
   covered), never their titles or contents. See docs/PRIVACY.md.
8. **Redaction can't be stalled.** Each event's text is cut (at a word) to
   4,000 characters before it's redacted, and the patterns are bounded, so
   long or crafted text can't make the feed hang.

## Known limits

- Redaction is pattern- and entropy-based. A secret with no known prefix, no
  telling variable name and low randomness (say, a dictionary-word password
  typed as a bare argument) can get through. Use `glitchwall pause` when
  streaming or screen sharing, or `--commands-only`.
- Any program already running as you can read the key file and the logs
  themselves; this tool doesn't protect against local malware.
- Any page opened from a file sends `Origin: null`, as does a sandboxed frame on
  a website. Those can read the feed only with the key, which only this
  wallpaper's `config.js` has, and can't change settings at all.
- `config.js` contains the key. Don't publish an exported wallpaper folder.
  If you did, delete the `key` file in the state dir and run
  `glitchwall install` again to make a new one.

## Reporting

Please report vulnerabilities privately (repository security advisory, or
email the maintainer) rather than in a public issue. Include what leaked or
which check was bypassed, and a fake example that reproduces it. Never send a
real secret.
