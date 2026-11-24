# Other agents: where their logs are and what glitchwall would need

Research notes (2026-11-24), read from each project's own source code. Not a promise of support yet.
glitchwall needs, per tool call: the tool's name, what it acted on (command, path, query), its output, a
timestamp, and that the log is written while the agent works (so the wall can follow it live).

## Gemini CLI (google-gemini/gemini-cli, read at v0.71.1)

Source: `packages/core/src/services/chatRecordingService.ts`, `chatRecordingTypes.ts`,
`packages/core/src/core/geminiChat.ts`, `packages/core/src/config/storage.ts`.

- **Where:** `~/.gemini/tmp/<project id>/chats/session-<timestamp>-<shortId>.jsonl` (base dir is `~/.gemini`,
  or the OS temp dir if there is no home). Subagents: `chats/<parent session id>/<session id>.jsonl`.
  Older versions wrote one `.json` per session; current code renames those to `.jsonl` on resume.
- **How:** append-only JSONL (`fs.appendFileSync`, one JSON record per line). Sometimes the whole file is
  rewritten atomically (temp file + rename), e.g. on resume of an unreadable file: a follower must handle the
  file shrinking or being replaced.
- **Record kinds per line:**
  - a message: `{id, timestamp, type: user|gemini|info|error|warning, content, ...}`. Gemini messages carry
    `toolCalls: [{id, name, args, result, status, timestamp, agentId?, displayName?, description?, resultDisplay?}]`.
    **The same message id is appended again in full each time it changes** (last one wins).
  - `{"$set": {...}}`: metadata (sessionId, projectHash, startTime, kind main|subagent, summary...)
  - `{"$patch": {...}}`: partial updates (content, toolCalls results by id, removeIds, orderIds)
  - `{"$rewindTo": "<message id>"}`: the user rewound; later messages are dropped
- **Tool call lifecycle:** first written with `status: "scheduled"`, args, no result, as soon as the model asks
  for it (so it survives a crash mid-tool). Written again on completion with `status` success/error/cancelled,
  `result` (function response parts) and `resultDisplay` (what the UI shows; for shell, the output text).
  That maps onto glitchwall's two events: the call (cmd) on first sight, the output (out) on completion.
- **Tool names to map:** `run_shell_command` (args.command), `read_file` / `read_many_files` (paths),
  `write_file`, `replace` (edit), `glob`, `search_file_content` / grep, `google_web_search`, `web_fetch`,
  plus MCP tools. (Names to confirm against `packages/core/src/tools/` before mapping.)
- **Cost to users:** free tier with a Google account. That answers "does it work without Claude Pro".

## Codex CLI (openai/codex, read at rust-v0.172.0)

Source: `codex-rs/rollout/src/recorder.rs`, `compression.rs`, `policy.rs`, `codex-rs/protocol/src/models.rs`.

- **Where:** `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<timestamp>-<conversation id>.jsonl`; `CODEX_HOME`
  defaults to `~/.codex`. A reverted thread gets `..._<rollout id>.jsonl`. Archived sessions:
  `$CODEX_HOME/archived_sessions/` (flat).
- **Compression:** a background job compresses *cold* rollouts to `.jsonl.zst`. The live session stays plain
  `.jsonl`, so glitchwall can ignore `.zst` files entirely (they are finished sessions).
- **How:** append-only JSONL, written as the session runs (the file is created on the first item). Each line is
  `{"timestamp": ..., "type": <item kind>, "payload": {...}}`; kinds include `session_meta`, `response_item`,
  `event_msg`, `turn_context`, `compacted`.
- **Tool calls** are `response_item` payloads (`type` field inside payload):
  - `function_call`: `{name, arguments, call_id, namespace?}`. `arguments` is a JSON *string* to parse.
    Shell runs here (tool names `shell` with `command` as an argv list, or `exec_command` with `cmd`; confirm
    which the current version uses), plus `update_plan`, MCP tools (`namespace` set), etc.
  - `function_call_output`: `{call_id, output}`, the result, matched by `call_id`.
  - `custom_tool_call` / `custom_tool_call_output`: `{name, input, call_id}` / `{call_id, output}`. `apply_patch`
    (file edits) arrives this way, `input` being the patch text (file paths are in its `*** Update File:` lines).
  - `local_shell_call`: `{call_id, status, action: {type: exec, command: [...]}}` (older shell path).
  - `web_search_call`: `{status, action}` (the query is in `action`).
- **Lifecycle:** call and output are two separate lines, matched by `call_id`. Same shape as Claude Code's
  tool_use / tool_result, so it maps onto glitchwall's cmd/out events directly.
- **Watch out:** the format changes between releases (fields added, kinds renamed). The reader must skip
  anything it doesn't know rather than fail, and the fixtures should come from a real recent session.
- **Cost to users:** ChatGPT plan (Plus and up) or an OpenAI API key.

## Plan for adding an agent

1. A reader per agent next to `claude_logs.py` (Python) and `logs.rs` (Rust), turning its lines into the same
   events glitchwall already draws (`cmd`, `out`, `edit`, `read`, `search`, `web`...). Redaction stays where it
   is, so every agent gets the same masking.
2. Fixtures recorded the same way as now: made-up lines run through the Python reader, Rust must match exactly.
3. Real-log check before release: install the agent, run a short session, compare.
4. PRIVACY.md and the README say exactly which folders are read for each agent, and each agent can be turned
   off. Only folders of agents the user turned on are read.
5. Order: Gemini CLI (free tier), Codex CLI, then OpenCode (local models).

## Not planned for now

- **Aider:** only a Markdown transcript (`.aider.chat.history.md`), no structured tool-call records.
