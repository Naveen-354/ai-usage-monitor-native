-- AI Usage Monitor — initial schema.
-- Forward-only: never edit this file once released; add 0002_*.sql instead.
-- All timestamps are UTC (unix milliseconds unless the column says _s). Local time is applied only for display.

CREATE TABLE agents (
    id                 TEXT PRIMARY KEY,
    display_name       TEXT NOT NULL,
    first_seen_utc_ms  INTEGER
) WITHOUT ROWID;

CREATE TABLE models (
    id        INTEGER PRIMARY KEY,
    agent_id  TEXT NOT NULL REFERENCES agents(id),
    name      TEXT NOT NULL,
    UNIQUE (agent_id, name)
);

-- id 0 is the reserved "UNKNOWN PROJECT" row so project_id can be NOT NULL (and part of a PK).
CREATE TABLE projects (
    id         INTEGER PRIMARY KEY,
    key        TEXT NOT NULL UNIQUE,   -- canonical root path (case-folded on case-insensitive filesystems)
    name       TEXT NOT NULL,          -- display name (folder name)
    root_path  TEXT NOT NULL
);
INSERT INTO projects (id, key, name, root_path) VALUES (0, '', 'UNKNOWN PROJECT', '');

CREATE TABLE sessions (
    id                  INTEGER PRIMARY KEY,
    agent_id            TEXT NOT NULL REFERENCES agents(id),
    external_id         TEXT NOT NULL,
    project_id          INTEGER NOT NULL DEFAULT 0 REFERENCES projects(id),
    first_event_utc_ms  INTEGER NOT NULL,
    last_event_utc_ms   INTEGER NOT NULL,
    UNIQUE (agent_id, external_id)
);
CREATE INDEX idx_sessions_last    ON sessions (last_event_utc_ms);
CREATE INDEX idx_sessions_project ON sessions (project_id);

-- Append-oriented fact table. dedupe_key makes ingestion idempotent; a re-seen key may only RAISE
-- counters (streaming partial -> final), never lower them.
CREATE TABLE usage_events (
    id                  INTEGER PRIMARY KEY,
    dedupe_key          TEXT NOT NULL UNIQUE,
    agent_id            TEXT NOT NULL REFERENCES agents(id),
    model_id            INTEGER NOT NULL REFERENCES models(id),
    project_id          INTEGER NOT NULL DEFAULT 0 REFERENCES projects(id),
    session_id          INTEGER REFERENCES sessions(id),
    ts_utc_ms           INTEGER NOT NULL,
    input_tokens        INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens       INTEGER NOT NULL CHECK (output_tokens >= 0),
    cache_read_tokens   INTEGER NOT NULL CHECK (cache_read_tokens >= 0),
    cache_write_tokens  INTEGER NOT NULL CHECK (cache_write_tokens >= 0),
    reasoning_tokens    INTEGER CHECK (reasoning_tokens IS NULL OR reasoning_tokens >= 0), -- NULL = unknown, not 0
    accuracy            TEXT NOT NULL CHECK (accuracy IN ('actual', 'estimated')),         -- 'unavailable' is never stored
    source              TEXT NOT NULL
);
CREATE INDEX idx_events_ts          ON usage_events (ts_utc_ms);
CREATE INDEX idx_events_agent_ts    ON usage_events (agent_id, ts_utc_ms);
CREATE INDEX idx_events_model_ts    ON usage_events (model_id, ts_utc_ms);
CREATE INDEX idx_events_project_ts  ON usage_events (project_id, ts_utc_ms);
CREATE INDEX idx_events_session     ON usage_events (session_id);

-- Incrementally maintained rollup in 15-minute UTC buckets. 15 minutes divides every real-world UTC
-- offset (including +5:30 and +5:45), so local day/week/month/year boundaries always fall on a bucket edge.
-- Month/year views scan this table, not usage_events.
CREATE TABLE usage_buckets (
    bucket_utc_s        INTEGER NOT NULL,   -- start of the bucket, unix seconds, multiple of 900
    agent_id            TEXT    NOT NULL,
    model_id            INTEGER NOT NULL,
    project_id          INTEGER NOT NULL DEFAULT 0,
    input_tokens        INTEGER NOT NULL DEFAULT 0,
    output_tokens       INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens  INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens    INTEGER NOT NULL DEFAULT 0,
    estimated_tokens    INTEGER NOT NULL DEFAULT 0,  -- part of the total that came from 'estimated' events
    events              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_utc_s, agent_id, model_id, project_id)
) WITHOUT ROWID;
CREATE INDEX idx_buckets_agent   ON usage_buckets (agent_id, bucket_utc_s);
CREATE INDEX idx_buckets_project ON usage_buckets (project_id, bucket_utc_s);

-- Where each collector stopped reading, so restarts never double count or skip.
CREATE TABLE file_cursors (
    agent_id         TEXT    NOT NULL,
    path             TEXT    NOT NULL,
    size             INTEGER NOT NULL,
    mtime_ms         INTEGER NOT NULL,
    byte_offset      INTEGER NOT NULL,
    state            TEXT,                -- collector-private JSON (e.g. Codex running totals)
    updated_utc_ms   INTEGER NOT NULL,
    PRIMARY KEY (agent_id, path)
) WITHOUT ROWID;

CREATE TABLE collector_health (
    agent_id              TEXT PRIMARY KEY,
    availability          TEXT NOT NULL,
    detail                TEXT,
    note                  TEXT,
    last_run_utc_ms       INTEGER,
    last_success_utc_ms   INTEGER,
    last_event_utc_ms     INTEGER,
    events_total          INTEGER NOT NULL DEFAULT 0,
    skipped_records       INTEGER NOT NULL DEFAULT 0,
    source_paths          TEXT NOT NULL DEFAULT '[]'
) WITHOUT ROWID;

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL            -- JSON
) WITHOUT ROWID;
