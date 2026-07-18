CREATE TABLE repository_snapshots (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    checkpoint TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (task_id, checkpoint)
) STRICT;
