use std::path::PathBuf;

use herdr_harness_coordinator::{
    adapter::{AdapterLifecycle, AdapterSnapshot},
    contract::{
        HarnessSessionId, NativeSessionHealth, RepositoryAccess, SessionReusePolicy, TaskId,
        TaskRepositoryAuthorityV1, TaskRole, TaskSubmissionV1,
    },
    session_reuse::{SessionReuseCandidate, effective_policy, evaluate_session_reuse},
};

fn task(role: TaskRole, policy: SessionReusePolicy) -> TaskSubmissionV1 {
    TaskSubmissionV1 {
        schema_version: 1,
        request_key: None,
        worker_id: "worker".parse().expect("valid Worker ID"),
        related_task_id: None,
        depends_on: Vec::new(),
        task_role: role,
        session_reuse: policy,
        preferred_session_id: None,
        title: "Evaluate reuse".to_owned(),
        instructions: "Evaluate this candidate Session without native delivery.".to_owned(),
        attachments: Vec::new(),
        repository: TaskRepositoryAuthorityV1 {
            root: PathBuf::from("/tmp/repository"),
            access: RepositoryAccess::ReadOnly,
            write_scopes: Vec::new(),
        },
    }
}

fn healthy_candidate() -> SessionReuseCandidate {
    SessionReuseCandidate {
        session_id: HarnessSessionId::new(),
        same_worker: true,
        same_harness_kind: true,
        same_launch_profile: true,
        same_repository: true,
        same_tool_policy: true,
        compatible_model: true,
        has_active_task: false,
        has_unresolved_question: false,
        has_delivery_unknown: false,
        has_unresolved_cancellation: false,
        has_session_worktree_hold: false,
        native_protocol_unambiguous: true,
        previously_bound: true,
        adapter: AdapterSnapshot {
            lifecycle: AdapterLifecycle::Idle,
            session_id: Some("native-1".to_owned()),
            thread_id: Some("thread-1".to_owned()),
            active_turn_id: None,
            steerable: false,
            queued_input_count: Some(0),
            model: Some("model".to_owned()),
            native_health: NativeSessionHealth::Healthy,
            context_tokens: Some(43_000),
            context_window: Some(100_000),
            context_percent: Some(43.0),
            compaction_count: Some(0),
        },
    }
}

#[test]
fn auto_review_requires_fresh_session() {
    let task = task(TaskRole::Review, SessionReusePolicy::Auto);

    assert_eq!(effective_policy(&task), SessionReusePolicy::Fresh);
}

#[test]
fn related_auto_implementation_prefers_reuse() {
    let mut task = task(TaskRole::Implementation, SessionReusePolicy::Auto);
    task.related_task_id = Some(TaskId::default());

    assert_eq!(effective_policy(&task), SessionReusePolicy::Prefer);
}

#[test]
fn prefer_reuses_healthy_idle_compatible_session() {
    let task = task(TaskRole::Implementation, SessionReusePolicy::Prefer);

    assert!(evaluate_session_reuse(&task, &healthy_candidate()).reusable);
}

#[test]
fn context_pressure_is_checked_after_identity_compatibility() {
    let task = task(TaskRole::Implementation, SessionReusePolicy::Prefer);
    let mut candidate = healthy_candidate();
    candidate.adapter.native_health = NativeSessionHealth::ContextPressure;

    assert_eq!(
        evaluate_session_reuse(&task, &candidate).reason_code,
        "context_pressure"
    );
}

#[test]
fn required_session_never_accepts_a_different_candidate() {
    let mut task = task(TaskRole::Implementation, SessionReusePolicy::Required);
    task.preferred_session_id = Some(HarnessSessionId::new());

    assert_eq!(
        evaluate_session_reuse(&task, &healthy_candidate()).reason_code,
        "preferred_session_mismatch"
    );
}
