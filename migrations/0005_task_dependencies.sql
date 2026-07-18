ALTER TABLE tasks ADD COLUMN scheduling_state TEXT NOT NULL DEFAULT 'ready'
    CHECK (scheduling_state IN ('blocked', 'ready'));

CREATE TABLE task_dependencies (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    dependency_task_id TEXT NOT NULL REFERENCES tasks(id),
    condition TEXT NOT NULL CHECK (condition IN ('result_ready', 'approved')),
    failure_policy TEXT NOT NULL DEFAULT 'cancel'
        CHECK (failure_policy IN ('cancel', 'keep_blocked')),
    satisfied_at TEXT,
    satisfied_by_result_revision INTEGER,
    result_snapshot_attachment_id TEXT REFERENCES attachments(id),
    bound_at TEXT,
    PRIMARY KEY (task_id, dependency_task_id),
    CHECK (task_id <> dependency_task_id),
    CHECK ((satisfied_at IS NULL) = (satisfied_by_result_revision IS NULL))
) STRICT;

CREATE INDEX task_dependencies_by_upstream
ON task_dependencies(dependency_task_id);

CREATE INDEX ready_tasks_by_worker_fifo
ON tasks(worker_id, scheduling_state, created_sequence);

CREATE TABLE task_scheduling_transitions (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    from_state TEXT,
    to_state TEXT NOT NULL CHECK (to_state IN ('blocked', 'ready')),
    evidence_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE result_dependency_snapshots (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    result_revision INTEGER NOT NULL,
    attachment_id TEXT NOT NULL UNIQUE REFERENCES attachments(id),
    PRIMARY KEY (task_id, result_revision)
) STRICT;
