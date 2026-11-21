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
   limited to absent, `null` or `*.localhost`. CORS headers are only sent to
   allowed origins.

## Known limits

- Redaction is pattern- and entropy-based. A secret with no known prefix, no
  telling variable name and low randomness (say, a dictionary-word password
  typed as a bare argument) can get through. Use `agent-wallpaper pause` when
  streaming or screen sharing, or `--commands-only`.
- Any program already running as you can read the key file and the logs
  themselves; this tool doesn't protect against local malware.
- Other Lively web wallpapers also run on `*.localhost` origins. They still
  need the key, which only this wallpaper's `config.js` has.
- `config.js` contains the key. Don't publish an exported wallpaper folder.
  If you did, delete the `key` file in the state dir and run
  `agent-wallpaper install` again to make a new one.

## Reporting

Please report vulnerabilities privately (repository security advisory, or
email the maintainer) rather than in a public issue. Include what leaked or
which check was bypassed, and a fake example that reproduces it. Never send a
real secret.
