# Collector findings — verified against this machine

Every format below was **inspected on the real local installation** on 2026-10-07 (Windows 11,
user `NaveenDhanasekaran`). Nothing here is taken from memory or documentation alone. Where a
format could not be verified against local data, that is stated explicitly.

Inspection rules followed: only key names, value types, counts and numbers were read. Message
text, prompts, file contents and credential files (`auth.json`, `.credentials.json`,
`oauth_creds.json`, `id_ed25519`) were **never opened**, and the collectors must not open them either.

## Implementation status (Rust collectors, verified against the real data above)

| Agent | Result of `cargo test … -- --ignored` on this machine |
|---|---|
| Claude Code | 23,022 raw records → 10,190 unique responses; fresh input 177,732 · output 12,515,736 · cache-read 3,838,391,534 · cache-write 64,765,547 — identical to an independent untyped parser |
| Codex | Σ deltas 793,880,810 = independent reference; 65/65 comparable sessions match individually |
| Gemini CLI | 4,263 records → 1,812 unique; fresh 33,771,083 · cached 127,178,603 · output 857,662 · total 161,807,348 — identical to the reference |
| Antigravity | 5,458 rows → 5,434 generations; 0 duplicate response ids; output = thinking + response on all; input 57,325,239 · output 2,445,071 · cache-read 401,113,018 |
| OpenCode | synthetic fixtures only (no local data) |

The opt-in real-data tests live next to each collector (`real_data_*`, `#[ignore]`) and compare against a
*differently written* parser rather than copying numbers, so they keep working as the data grows.

## Summary

| Agent | Installed here | Data source | Verified | Local data | Status |
|---|---|---|---|---|---|
| Claude Code | yes (`claude.exe`, npm 2.1.263) | `~/.claude/projects/**/*.jsonl` | yes | 48 files, 10,061 unique responses | **Supported** |
| Codex CLI / Desktop | yes (npm 0.147.0) | `~/.codex/sessions/**/rollout-*.jsonl` | yes | 68 rollouts, 7,115 `token_count` events | **Supported** |
| Gemini CLI | data only — npm install is broken, no `gemini` on PATH | `~/.gemini/tmp/*/chats/*.jsonl` + `*.json` | yes | 1,126 files, 1,812 unique messages, last used 2026-07 | **Supported (historical)** |
| Antigravity CLI (`agy`) | yes (`agy.exe` 1.3.1) | `~/.gemini/antigravity-cli/conversations/*.db` (+ IDE: `~/.gemini/antigravity/conversations/*.db`) | yes, via schema embedded in `agy.exe` | 106 DBs, 5,434 generations, active today | **Supported** |
| OpenCode | yes (npm 1.18.4) | `~/.local/share/opencode/opencode.db` | schema yes; **values no** | 0 sessions, 0 messages (confirmed by `opencode stats`) | **Supported, unverified values** |
| Ollama | **no** (only leftover `~/.ollama/{config,server}.json`) | none persisted | n/a | none | **Unavailable** |
| Aider | **no** | `.aider.chat.history.md` (per project) | no — cannot inspect | none | **Unavailable** |

## Normalised event model

Agents disagree on whether "input" includes cached tokens, so the database stores **disjoint**
counters and never mixes semantics:

| Field | Meaning |
|---|---|
| `input_tokens` | fresh (non-cached) input tokens |
| `cache_read_tokens` | input tokens served from cache |
| `cache_write_tokens` | input tokens written to cache |
| `output_tokens` | output tokens, **including** reasoning/thinking |
| `reasoning_tokens` | informational subset of `output_tokens` (NULL when unknown) |
| `total_tokens` | `input + cache_read + cache_write + output` |

The headline total includes cached tokens by default (matches the spec example where cached is
part of input). A display setting can exclude them. Aggregates store the columns separately so the
toggle needs no re-ingestion.

## Claude Code

* **Files**: `~/.claude/projects/<url-encoded-cwd>/<sessionId>.jsonl` (also recurse for `subagents/`).
* **Record**: `type == "assistant"` with `message.usage`.
* **Fields**: `message.id`, `message.model`, `requestId`, `timestamp` (ISO-8601 UTC), `cwd`,
  `sessionId`, `isSidechain`, and `message.usage.{input_tokens, output_tokens,
  cache_creation_input_tokens, cache_read_input_tokens, output_tokens_details.thinking_tokens}`.
* **Mapping**: `input=input_tokens`, `cache_read=cache_read_input_tokens`,
  `cache_write=cache_creation_input_tokens`, `output=output_tokens`, `reasoning=thinking_tokens`.
  Claude's `input_tokens` **excludes** cache reads and writes (already disjoint).
* **DUPLICATES — critical**: one API response is written as several lines (one per content block).
  Measured: **22,504 assistant records → 10,061 unique `message.id`s** (2.24× over-count if summed
  naively). 234 ids also appear in more than one file (resumed sessions). One id had differing usage
  (streaming partial vs final).
  * Dedupe globally on `message.id` (key `claude:<message.id>`), keep the **per-field maximum**.
* **Skip**: `model == "<synthetic>"` and all-zero usage.
* **Project**: `cwd` → git root basename.

## Codex

* **Files**: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
* **Record**: `type == "event_msg"` and `payload.type == "token_count"`; `payload.info` may be `null` (skip).
* **Fields**: `payload.info.total_token_usage` is **cumulative per session**; `last_token_usage` is the
  last API call. Both have `{input_tokens, cached_input_tokens, cache_write_input_tokens,
  output_tokens, reasoning_output_tokens, total_tokens}`.
* **Model**: from the most recent `turn_context.payload.model`. **Project**: `session_meta.payload.cwd`.
* **Mapping**: Codex `input_tokens` **includes** cached → `input = input_tokens − cached_input_tokens`,
  `cache_read = cached_input_tokens`, `output = output_tokens` (already includes reasoning),
  `reasoning = reasoning_output_tokens`. `cache_write_input_tokens` was 0 in all 7,091 local events.
* **Algorithm**: emit the **delta of `total_token_usage`** between consecutive events. Skip repeated
  totals (175 seen). If the total **decreases** (7 seen: compaction/restart) treat the new total as a
  fresh baseline. Event key `codex:<session_id>:<cumulative total after event>`.
* **Verification**: for 55 of 56 rollouts with a thread row, the final cumulative total equals Codex's
  own `threads.tokens_used` in `state_5.sqlite` **exactly**. The one outlier has a stale DB counter.
  Sum of deltas = 793,880,810. No event signature appears in more than one file (forks do not copy history).
* **Do not open** `state_5.sqlite` columns `first_user_message`, `title`, `preview` (conversation text).
* **Windows**: active rollouts are held open by Codex; open with shared read access and treat a
  failed open as transient.

## Gemini CLI

* **Files**: `~/.gemini/tmp/<project>/chats/session-*.jsonl` (new) and `session-*.json` (legacy, 19 files).
  `~/.gemini/tmp/<project>/.project_root` holds the project path.
* **Record**: message with `type == "gemini"` and a `tokens` object:
  `{input, output, cached, thoughts, tool, total}`; plus `model`, `id`, `timestamp`.
  The JSONL also contains `{"$set":{"messages":[…]}}` snapshot records that **re-emit whole message
  arrays**.
* **Verified identity** (1,812/1,812 messages): `total = input + output + thoughts + tool`, and
  `cached ≤ input` always, so `cached` is a **subset of `input`**; `thoughts` is **additional** to `output`.
* **Mapping**: `input = input − cached + tool`, `cache_read = cached`, `output = output + thoughts`,
  `reasoning = thoughts`. (Sum equals `tokens.total`.)
* **DUPLICATES — critical**: 4,263 token-bearing records → **1,812 unique** messages (2.35× over-count).
  Dedupe on `(sessionId, message.id)`, key `gemini:<sessionId>:<id>`. No differing values, none across files.
* **State**: `~/.gemini/tmp` last written 2026-07; the npm package dir has no `package.json` (failed
  upgrade). Report as "data found, CLI not on PATH, last activity <date>".

## Antigravity CLI (`agy`) and Antigravity IDE

Conversations are SQLite databases containing **protobuf blobs**. The schema was **not guessed**: it was
read from the protobuf descriptors embedded in `agy.exe` (a Go binary), then validated against data.

* **Files**: `~/.gemini/antigravity-cli/conversations/<id>.db` (103) and
  `~/.gemini/antigravity/conversations/<id>.db` (3, IDE). Table `gen_metadata(idx INTEGER, data BLOB, size)`.
* **Decoding** (`gen_metadata.data` = `CortexStepGeneratorMetadata`):

  ```
  CortexStepGeneratorMetadata { 1: chat_model = ChatModelMetadata, 2: step_indices (packed uint32) }
  ChatModelMetadata { 3: model(enum), 4: usage = ModelUsageStats, 9: chat_start_metadata, 19: response_model(string),
                      21: model_display_name(string), 17: retry_infos (DUPLICATES usage — ignore) }
  ChatStartMetadata { 4: created_at = Timestamp{1: seconds, 2: nanos} }
  ModelUsageStats   { 2: input_tokens, 3: output_tokens, 4: cache_write_tokens, 5: cache_read_tokens,
                      9: thinking_output_tokens, 10: response_output_tokens, 11: response_id(string) }
  CortexStepMetadata (steps.metadata) { 1: created_at = Timestamp }
  ```
* **Mapping**: `input=input_tokens`, `cache_read=cache_read_tokens`, `cache_write=cache_write_tokens`,
  `output=output_tokens`, `reasoning=thinking_output_tokens`. Model = `response_model` (always present).
* **Validation on 5,434 generations**: `output = thinking + response` on 100%; `cache_read > input` on 4,876
  (so input excludes cache reads); `response_id` unique (0 duplicates) → key `agy:<response_id>`;
  24 all-zero rows skipped.
* **Timestamp fallback — critical**: 3,836 rows (70%, the newer ones) have **no** `chat_start_metadata`.
  Recover from `steps.metadata` (idx = first of `step_indices`) → `created_at`. Recovered 100%; without it
  every row after 2026-08-11 is lost. Rows with no derivable time are skipped and counted in diagnostics.
* **Project**: `conversation_summaries.workspace_uris` (read only `conversation_id`, `workspace_uris`;
  never `title`/`preview`).
* **Caveat**: internal, undocumented format, validated against `agy` 1.3.1. The collector must degrade to
  "unavailable: unrecognised format" when decoding yields nothing — never to zeros.

## OpenCode

* `~/.local/share/opencode/opencode.db` (SQLite). Verified tables: `session` (with `tokens_input`,
  `tokens_output`, `tokens_reasoning`, `tokens_cache_read`, `tokens_cache_write`, `cost`, `model`,
  `directory`, `time_created`, `time_updated`) and `message(id, session_id, time_created, time_updated, data JSON)`.
* **All tables are empty** on this machine; `opencode stats` independently reports 0 sessions / 0 messages.
* Message `data` shape verified from the **installed binary** (`opencode.exe`, 1.18.4 source):
  assistant `info.tokens = {input, output, reasoning, cache:{read, write}}`, `info.cost`,
  `info.modelID`, `info.providerID`, `info.time.created`. OpenCode's own `stats` adds `reasoning` into
  output, so `output = output + reasoning`, `reasoning = reasoning`.
* Values cannot be validated here. Implemented per the verified shape, tested on synthetic fixtures, and
  labelled **"format verified from binary, no local data to validate"** in diagnostics.
* Dedupe key `opencode:<message.id>`, keep per-field maximum (rows are updated in place while streaming).

## Ollama — unavailable

Not installed (no process, no listener on 11434, no models, no install dir); `~/.ollama` has only
`config.json`, `server.json` and an SSH key. Even when installed, Ollama keeps **no usage history**:
token counts (`prompt_eval_count`, `eval_count`) exist only in live API responses. The collector
reports `Unavailable` with the reason. An opt-in local proxy could capture real counts in future (TASK-031),
but it cannot be verified until Ollama is installed.

## Aider — unavailable

Not installed (no pip/pipx/uv package, nothing on PATH, no `~/.aider*`). Its per-project
`.aider.chat.history.md` prints rounded values ("1.2k sent"), which would at best be `Estimated`, and
locating those files would mean scanning arbitrary project directories. Reported as `Unavailable`
until Aider is installed and the format can be inspected.
