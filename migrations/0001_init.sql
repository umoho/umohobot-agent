CREATE TABLE threads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_key TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('active', 'draining', 'closed')),
    opened_at INTEGER NOT NULL,
    last_activity_at INTEGER NOT NULL,
    lease_until INTEGER,
    closed_at INTEGER,
    parent_thread_id INTEGER REFERENCES threads(id) ON DELETE SET NULL,
    summary_cursor INTEGER NOT NULL DEFAULT 0,
    turn_count INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_threads_thread_key_state_opened_at
    ON threads (thread_key, state, opened_at DESC, id DESC);

CREATE INDEX idx_threads_parent_thread_id
    ON threads (parent_thread_id);

CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE RESTRICT,
    seq INTEGER NOT NULL,
    turn_id INTEGER REFERENCES turns(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (
        kind IN (
            'inbound_message',
            'assistant_message',
            'tool_call',
            'tool_result',
            'summary',
            'system_note'
        )
    ),
    sender_id TEXT,
    sender_name TEXT,
    platform_message_id TEXT,
    reply_to_platform_message_id TEXT,
    content_json TEXT NOT NULL,
    visible_to_model INTEGER NOT NULL DEFAULT 1 CHECK (visible_to_model IN (0, 1)),
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_events_thread_seq
    ON events (thread_id, seq);

CREATE INDEX idx_events_thread_created_at
    ON events (thread_id, created_at DESC, id DESC);

CREATE INDEX idx_events_turn_id
    ON events (turn_id);

CREATE TABLE turns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE RESTRICT,
    trigger_event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_version INTEGER NOT NULL,
    context_hash TEXT,
    final_message_id TEXT,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    estimated_usage INTEGER NOT NULL DEFAULT 0 CHECK (estimated_usage IN (0, 1)),
    error_code TEXT,
    lease_until INTEGER
);

CREATE INDEX idx_turns_thread_id_started_at
    ON turns (thread_id, started_at DESC, id DESC);

CREATE INDEX idx_turns_trigger_event_id
    ON turns (trigger_event_id);

CREATE TABLE summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE RESTRICT,
    upto_seq INTEGER NOT NULL,
    summary_text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    model TEXT NOT NULL,
    prompt_version INTEGER NOT NULL
);

CREATE INDEX idx_summaries_thread_id_upto_seq
    ON summaries (thread_id, upto_seq DESC, id DESC);

CREATE TABLE usage_ledger (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('user', 'group', 'session', 'thread', 'provider')),
    scope_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    turn_id INTEGER REFERENCES turns(id) ON DELETE SET NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    estimated INTEGER NOT NULL DEFAULT 0 CHECK (estimated IN (0, 1)),
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_usage_ledger_scope_created_at
    ON usage_ledger (scope_kind, scope_id, provider, created_at DESC, id DESC);

CREATE INDEX idx_usage_ledger_turn_id
    ON usage_ledger (turn_id);

CREATE TABLE usage_totals (
    scope_kind TEXT NOT NULL CHECK (scope_kind IN ('user', 'group', 'session', 'thread', 'provider')),
    scope_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    estimated_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_completion_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_tool_calls INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (scope_kind, scope_id, provider)
);

CREATE INDEX idx_usage_totals_provider
    ON usage_totals (provider);
