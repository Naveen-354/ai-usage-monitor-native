-- AI Usage Monitor - accounts.
-- Forward-only, like 0001. Every usage row now belongs to an *account* of its agent. Everything recorded before this
-- migration belongs to the agent's 'default' account (the login the agent itself uses, outside any managed profile).

-- Usage events: which account produced the event.
ALTER TABLE usage_events ADD COLUMN account_id TEXT NOT NULL DEFAULT 'default';
CREATE INDEX idx_events_account_ts ON usage_events (agent_id, account_id, ts_utc_ms);

-- The 15-minute rollup gets the account in its primary key, so it is rebuilt (SQLite cannot change a primary key in place).
CREATE TABLE usage_buckets_v2 (
    bucket_utc_s        INTEGER NOT NULL,   -- start of the bucket, unix seconds, multiple of 900
    agent_id            TEXT    NOT NULL,
    account_id          TEXT    NOT NULL DEFAULT 'default',
    model_id            INTEGER NOT NULL,
    project_id          INTEGER NOT NULL DEFAULT 0,
    input_tokens        INTEGER NOT NULL DEFAULT 0,
    output_tokens       INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens  INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens    INTEGER NOT NULL DEFAULT 0,
    estimated_tokens    INTEGER NOT NULL DEFAULT 0,
    events              INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_utc_s, agent_id, account_id, model_id, project_id)
) WITHOUT ROWID;

INSERT INTO usage_buckets_v2 (bucket_utc_s, agent_id, account_id, model_id, project_id, input_tokens, output_tokens,
                              cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_tokens, events)
SELECT bucket_utc_s, agent_id, 'default', model_id, project_id, input_tokens, output_tokens,
       cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_tokens, events
FROM usage_buckets;

DROP TABLE usage_buckets;
ALTER TABLE usage_buckets_v2 RENAME TO usage_buckets;
CREATE INDEX idx_buckets_agent   ON usage_buckets (agent_id, bucket_utc_s);
CREATE INDEX idx_buckets_account ON usage_buckets (agent_id, account_id, bucket_utc_s);
CREATE INDEX idx_buckets_project ON usage_buckets (project_id, bucket_utc_s);

-- Accounts. Metadata only: this table (like every other table, log and setting file) never holds a password, token,
-- cookie or API key. A managed account's sign-in lives where the agent itself keeps it (inside the account's own profile
-- folder, or in the OS credential store); an API key the user gave us lives only in the OS credential store.
CREATE TABLE accounts (
    agent_id             TEXT    NOT NULL REFERENCES agents(id),
    account_id           TEXT    NOT NULL,           -- short slug, unique within the agent, never reused
    label                TEXT    NOT NULL,           -- what the user calls it
    kind                 TEXT    NOT NULL CHECK (kind IN ('default', 'managed')),
    profile_dir          TEXT,                       -- managed accounts: the folder the agent is pointed at
    identity             TEXT,                       -- e-mail / user name when the agent's own status command says it
    auth_state           TEXT    NOT NULL DEFAULT 'unknown'
                         CHECK (auth_state IN ('valid', 'expired', 'not_logged_in', 'pending', 'unknown')),
    auth_detail          TEXT,                       -- short plain-language reason, never raw agent output
    auth_checked_utc_ms  INTEGER,
    created_utc_ms       INTEGER NOT NULL,
    last_used_utc_ms     INTEGER,
    removed_utc_ms       INTEGER,                    -- soft delete: the history stays, the sign-in is gone
    PRIMARY KEY (agent_id, account_id)
) WITHOUT ROWID;
CREATE UNIQUE INDEX idx_accounts_label ON accounts (agent_id, lower(label)) WHERE removed_utc_ms IS NULL;

-- The account each agent is currently using. One row per agent, so switching one agent never touches another.
CREATE TABLE active_accounts (
    agent_id         TEXT    PRIMARY KEY REFERENCES agents(id),
    account_id       TEXT    NOT NULL,
    switched_utc_ms  INTEGER NOT NULL,
    FOREIGN KEY (agent_id, account_id) REFERENCES accounts (agent_id, account_id)
) WITHOUT ROWID;
