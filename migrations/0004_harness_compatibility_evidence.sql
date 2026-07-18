ALTER TABLE harness_sessions ADD COLUMN resolved_executable TEXT;
ALTER TABLE harness_sessions ADD COLUMN observed_version TEXT;
ALTER TABLE harness_sessions ADD COLUMN native_thread_id TEXT;
ALTER TABLE harness_sessions ADD COLUMN effective_model TEXT;
ALTER TABLE harness_sessions ADD COLUMN compatibility_evidence_json TEXT;
