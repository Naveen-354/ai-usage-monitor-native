# Privacy

AI Usage Monitor is **local-first**. It reads token counters that your AI coding agents already wrote
to your own disk and keeps aggregated usage in a SQLite file on your own machine.

* **Nothing is uploaded.** The application has no HTTP client, opens no sockets other than one loopback
  connection test for Ollama (below), and has no updater, no analytics and no telemetry. It is a native
  program with no WebView, so there is no browser engine that could load remote content.
  *How this is checked:* by reading `Cargo.lock` and `cargo tree` for the Windows build (no network-client
  crates such as `reqwest`, `hyper`, `ureq` or a TLS stack), and by grepping the sources for sockets. It is
  **not yet an automated test.** The GUI toolkit pulls in `webbrowser`, a helper that can ask the OS to open a
  link in your default browser; the application never calls it and its screens contain no links.
* **No prompts, code or conversations are collected.** Collectors deserialise typed structures that name
  only counter and metadata fields; every other field (message text, tool output, file contents) is skipped
  without being copied, stored or logged.
* **No credential file is ever opened.** Files such as `auth.json`, `.credentials.json`, `oauth_creds.json` and
  SSH keys are never read; at most the app checks whether one exists. This holds for the usage collectors and for
  the optional multi-account feature below.
* **Logs contain paths, counts and states only.**

## Multi-account management (optional)

By default only each agent's own, existing sign-in is read, exactly as above. If you add a second account (the Accounts page, or the
`agm` command-line manager), then:

* **The agent signs you in, not this app.** It starts the agent's own sign-in in a console window of its own. Your password and tokens
  go to the agent; this app never sees them.
* **Each extra account gets its own profile folder** under `%APPDATA%\dev.aiusage.monitor.native\accounts\<agent>\`, written by the
  agent. "Switching" only changes which folder the *next* launch of the agent is pointed at. The app never rewrites an agent's own
  configuration or sign-in.
* **What the app records:** the account's name, whether it is signed in, and which usage belongs to which account. No secret is ever
  put in the database, a log line or an error message.
* **API keys:** the desktop app never asks for one. The command-line manager accepts a key only on standard input (never as an
  argument). It goes either to the agent's own store (Codex keeps its own) or to Windows Credential Manager, and nowhere else.
* **Network:** still none from this app. Signing in is the agent talking to its own service.

## Exactly what is read, per agent

| Agent | File(s) read | Fields used | Deliberately never read |
|---|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | `type`, `message.id`, `message.model`, `message.usage.*` counters, `timestamp`, `cwd`, `sessionId` | `message.content`, tool results, `.credentials.json`, `history.jsonl` |
| Codex | `~/.codex/sessions/**/rollout-*.jsonl` | `token_count` event counters, `turn_context.model`, `session_meta.cwd`/`id`, `timestamp` | message items, `auth.json`, `state_5.sqlite` columns `first_user_message`/`title`/`preview`, `history.jsonl` |
| Gemini CLI | `~/.gemini/tmp/*/chats/*.{jsonl,json}`, `.project_root` | `tokens.*`, `model`, `id`, `timestamp`, `sessionId` | `content`, `thoughts`, `toolCalls`, `oauth_creds.json`, `google_accounts.json` |
| Antigravity | `~/.gemini/antigravity{,-cli}/conversations/*.db` (`gen_metadata`, `steps.metadata`), `conversation_summaries` | usage counters, response model, response id, timestamps, `conversation_id`, `workspace_uris` | step payloads, `title`, `preview`, `history.jsonl`, credentials |
| OpenCode | `~/.local/share/opencode/opencode.db` (`message`, `session`) | `tokens.*`, `modelID`, `time.created`, `path.cwd` | message parts/text, `account`/`credential`/`control_account` tables |
| Ollama / Aider | none | none (reported unavailable) | everything |

Project names come from the folder name of the working directory the agent reported; they are stored
locally so usage can be grouped by project.

## Your data, your control

* The database lives in `%APPDATA%\dev.aiusage.monitor.native` (Settings → Diagnostics shows the exact path).
* Quitting from the tray stops all reading immediately.
* Not implemented yet: JSON/CSV export and a confirmed "clear history". The retention setting is stored but
  pruning is not implemented.

> Status note: the collectors for Claude Code, Codex, Gemini CLI, Antigravity and OpenCode are implemented and
> read exactly the fields listed above (each collector's tests assert that prompt text never reaches an event).
> The Ollama and Aider collectors read nothing at all. The one network-adjacent operation is Ollama detection: a
> single TCP connect to `127.0.0.1:11434` that sends and reads no bytes and is refused for any non-loopback address
> (`crates/core/src/collectors/ollama/mod.rs`).
