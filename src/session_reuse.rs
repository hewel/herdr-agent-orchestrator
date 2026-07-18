//! Conservative, deterministic Worker Session reuse decisions.

use serde::{Deserialize, Serialize};

use crate::{
    adapter::{AdapterLifecycle, AdapterSnapshot},
    contract::{
        HarnessSessionId, NativeSessionHealth, SessionReusePolicy, TaskRole, TaskSubmissionV1,
    },
};

/// Identity and safety evidence resolved by the Coordinator before native delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent auditable compatibility gates form a deliberate decision matrix"
)]
pub struct SessionReuseCandidate {
    pub session_id: HarnessSessionId,
    pub same_worker: bool,
    pub same_harness_kind: bool,
    pub same_launch_profile: bool,
    pub same_repository: bool,
    pub same_tool_policy: bool,
    pub compatible_model: bool,
    pub has_active_task: bool,
    pub has_unresolved_question: bool,
    pub has_delivery_unknown: bool,
    pub has_unresolved_cancellation: bool,
    pub has_session_worktree_hold: bool,
    pub native_protocol_unambiguous: bool,
    pub previously_bound: bool,
    pub adapter: AdapterSnapshot,
}

/// Auditable output of one Session selection decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReuseDecision {
    pub reusable: bool,
    pub effective_policy: SessionReusePolicy,
    pub reason_code: String,
    pub reason: String,
}

/// Resolves conservative `auto` behavior without encoding reuse in DAG edges.
#[must_use]
pub fn effective_policy(task: &TaskSubmissionV1) -> SessionReusePolicy {
    if task.session_reuse != SessionReusePolicy::Auto {
        return task.session_reuse;
    }
    match task.task_role {
        TaskRole::Review | TaskRole::Verification => SessionReusePolicy::Fresh,
        TaskRole::Implementation
            if task.related_task_id.is_some() || !task.depends_on.is_empty() =>
        {
            SessionReusePolicy::Prefer
        }
        TaskRole::Implementation | TaskRole::Investigation | TaskRole::Other => {
            SessionReusePolicy::Fresh
        }
    }
}

/// Evaluates one candidate after dependency readiness and Worker selection.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "ordered compatibility gates stay together so reason precedence remains auditable"
)]
pub fn evaluate_session_reuse(
    task: &TaskSubmissionV1,
    candidate: &SessionReuseCandidate,
) -> SessionReuseDecision {
    let policy = effective_policy(task);
    if policy == SessionReusePolicy::Fresh {
        return reject(
            policy,
            "fresh_requested",
            "Task requires a fresh native Session",
        );
    }
    if task
        .preferred_session_id
        .is_some_and(|id| id != candidate.session_id)
    {
        return reject(
            policy,
            "preferred_session_mismatch",
            "candidate is not the preferred Session",
        );
    }
    let checks = [
        (
            candidate.same_worker,
            "worker_mismatch",
            "Worker definition differs",
        ),
        (
            candidate.same_harness_kind,
            "kind_mismatch",
            "Harness kind differs",
        ),
        (
            candidate.same_launch_profile,
            "profile_mismatch",
            "launch-profile snapshot differs",
        ),
        (
            candidate.same_repository,
            "repository_mismatch",
            "canonical repository differs",
        ),
        (
            candidate.same_tool_policy,
            "tool_policy_mismatch",
            "effective tool policy differs",
        ),
        (
            candidate.compatible_model,
            "model_mismatch",
            "effective model is incompatible",
        ),
        (
            candidate.adapter.lifecycle == AdapterLifecycle::Idle,
            "session_not_idle",
            "Session is not Online and Idle",
        ),
        (
            !candidate.has_active_task,
            "active_task",
            "Session already has an active Task",
        ),
        (
            !candidate.has_unresolved_question,
            "unresolved_question",
            "Session has an unresolved Question",
        ),
        (
            !candidate.has_delivery_unknown,
            "delivery_unknown",
            "Session has ambiguous delivery",
        ),
        (
            !candidate.has_unresolved_cancellation,
            "unresolved_cancellation",
            "Session has unresolved cancellation",
        ),
        (
            !candidate.has_session_worktree_hold,
            "worktree_hold",
            "Session caused an unresolved Worktree Hold",
        ),
        (
            candidate.native_protocol_unambiguous,
            "protocol_ambiguous",
            "native protocol state is ambiguous",
        ),
    ];
    if let Some((_, code, reason)) = checks.into_iter().find(|(passed, _, _)| !passed) {
        return reject(policy, code, reason);
    }
    match candidate.adapter.native_health {
        NativeSessionHealth::Failed => reject(policy, "native_failed", "native Session failed"),
        NativeSessionHealth::Ambiguous => reject(
            policy,
            "native_ambiguous",
            "native Session health is ambiguous",
        ),
        NativeSessionHealth::ContextPressure => reject(
            policy,
            "context_pressure",
            "native context pressure is unsafe",
        ),
        NativeSessionHealth::Compacted if task.session_reuse == SessionReusePolicy::Auto => reject(
            policy,
            "auto_compacted_fresh",
            "automatic reuse starts fresh after compaction",
        ),
        NativeSessionHealth::Healthy | NativeSessionHealth::Compacted => SessionReuseDecision {
            reusable: true,
            effective_policy: policy,
            reason_code: "compatible_healthy_session".to_owned(),
            reason: "compatible healthy idle Session preserves useful Task context".to_owned(),
        },
    }
}

fn reject(
    effective_policy: SessionReusePolicy,
    reason_code: &str,
    reason: &str,
) -> SessionReuseDecision {
    SessionReuseDecision {
        reusable: false,
        effective_policy,
        reason_code: reason_code.to_owned(),
        reason: reason.to_owned(),
    }
}
