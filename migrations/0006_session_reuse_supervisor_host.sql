ALTER TABLE tasks ADD COLUMN task_role TEXT NOT NULL DEFAULT 'other'
    CHECK (task_role IN ('implementation', 'investigation', 'review', 'verification', 'other'));
ALTER TABLE tasks ADD COLUMN session_reuse_policy TEXT NOT NULL DEFAULT 'auto'
    CHECK (session_reuse_policy IN ('required', 'prefer', 'fresh', 'auto'));
ALTER TABLE tasks ADD COLUMN preferred_session_id TEXT REFERENCES harness_sessions(id);
ALTER TABLE tasks ADD COLUMN expected_profile_digest TEXT;
ALTER TABLE tasks ADD COLUMN expected_model TEXT;
ALTER TABLE tasks ADD COLUMN expected_tool_policy_digest TEXT;

UPDATE tasks
SET submission_json = json_set(
    submission_json,
    '$.task_role', COALESCE(json_extract(submission_json, '$.task_role'), 'other'),
    '$.session_reuse', COALESCE(json_extract(submission_json, '$.session_reuse'), 'auto'),
    '$.preferred_session_id', json_extract(submission_json, '$.preferred_session_id')
);

ALTER TABLE harness_sessions ADD COLUMN native_health TEXT NOT NULL DEFAULT 'ambiguous'
    CHECK (native_health IN ('healthy', 'context_pressure', 'compacted', 'ambiguous', 'failed'));
ALTER TABLE harness_sessions ADD COLUMN context_tokens INTEGER;
ALTER TABLE harness_sessions ADD COLUMN context_window INTEGER;
ALTER TABLE harness_sessions ADD COLUMN context_percent REAL;
ALTER TABLE harness_sessions ADD COLUMN compaction_count INTEGER;
ALTER TABLE harness_sessions ADD COLUMN tool_policy_digest TEXT;
ALTER TABLE harness_sessions ADD COLUMN adapter_snapshot_json TEXT;
ALTER TABLE harness_sessions ADD COLUMN adapter_snapshot_at TEXT;
ALTER TABLE harness_sessions ADD COLUMN safe_compaction INTEGER NOT NULL DEFAULT 0
    CHECK (safe_compaction IN (0, 1));

CREATE TABLE task_session_bindings (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    harness_session_id TEXT NOT NULL REFERENCES harness_sessions(id),
    requested_policy TEXT NOT NULL,
    effective_policy TEXT NOT NULL,
    reused INTEGER NOT NULL CHECK (reused IN (0, 1)),
    reason_code TEXT NOT NULL,
    decision_reason TEXT NOT NULL,
    adapter_snapshot_json TEXT NOT NULL,
    context_tokens INTEGER,
    context_window INTEGER,
    context_percent REAL,
    bound_at TEXT NOT NULL,
    superseded_at TEXT
) STRICT;

CREATE UNIQUE INDEX one_current_task_session_binding
ON task_session_bindings(task_id)
WHERE superseded_at IS NULL;

INSERT INTO task_session_bindings (
    task_id, harness_session_id, requested_policy, effective_policy, reused,
    reason_code, decision_reason, adapter_snapshot_json, bound_at
)
SELECT id, active_session_id, 'auto', 'auto', 0, 'migrated_legacy_binding',
       'Binding recovered from the pre-session-reuse task row', '{}', updated_at
FROM tasks
WHERE active_session_id IS NOT NULL;

CREATE TABLE supervisor_events (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN (
        'result_ready', 'blocking_question', 'task_failed', 'delivery_unknown',
        'worktree_hold_created', 'task_graph_completed', 'notification'
    )),
    task_id TEXT REFERENCES tasks(id),
    result_revision INTEGER,
    source_message_id TEXT REFERENCES messages(id),
    source_key TEXT NOT NULL UNIQUE,
    summary TEXT NOT NULL,
    attachments_json TEXT NOT NULL,
    delivery_intent TEXT NOT NULL CHECK (delivery_intent IN ('follow_up', 'steer')),
    state TEXT NOT NULL CHECK (state IN (
        'pending', 'dispatching', 'accepted', 'processed', 'unknown', 'cancelled'
    )),
    created_sequence INTEGER NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    processed_at TEXT
) STRICT;

CREATE INDEX supervisor_events_attention_fifo
ON supervisor_events(state, created_sequence);

CREATE TABLE supervisor_event_attempts (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL REFERENCES supervisor_events(id),
    attempt_number INTEGER NOT NULL,
    target_session_id TEXT REFERENCES harness_sessions(id),
    state TEXT NOT NULL CHECK (state IN ('dispatching', 'accepted', 'unknown', 'cancelled')),
    provider_bytes_may_have_been_written INTEGER NOT NULL CHECK (provider_bytes_may_have_been_written IN (0, 1)),
    native_correlation TEXT,
    acceptance_evidence_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(event_id, attempt_number)
) STRICT;

CREATE TABLE task_graph_watches (
    id TEXT PRIMARY KEY,
    supervisor_id TEXT NOT NULL REFERENCES harnesses(id),
    request_key TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(supervisor_id, request_key)
) STRICT;

CREATE TABLE task_graph_watch_roots (
    watch_id TEXT NOT NULL REFERENCES task_graph_watches(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    PRIMARY KEY (watch_id, task_id)
) STRICT;
