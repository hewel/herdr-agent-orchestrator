//! Pane-resident Worker Host and terminal popup entrypoint behavior.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use sha2::{Digest, Sha256};

use crate::{
    adapter::{
        AdapterEvent, HarnessAdapter, HarnessStartSpec, NativeDeliveryKind, NativeTurnStatus,
        ResolvedDelivery, WorkerCompletionTools,
    },
    attachment::AttachmentStore,
    broker::{BROKER_SCHEMA_V2, BrokerOperation, BrokerRequest, call_with_connect_retry},
    contract::{
        DeliveryIntent, HarnessKind, HarnessTier, MessageSubmissionV1, SCHEMA_VERSION,
        TaskSubmissionV1,
    },
    core::{
        ActorContext, CommandOutcome, CoordinatorCommand, CoordinatorQuery, DashboardView,
        HarnessCompatibilityEvidenceV1, HarnessStatusView, HoldView, HostConnectionCapability,
        InboxMessageView, QueryResult, SessionCapability, TaskGraphView, TaskState, TaskView,
    },
    herdr::HerdrSocketClient,
    host_presence::HostHeartbeat,
    process_adapter::{CodexProcessAdapter, OmpProcessAdapter},
    profile::{parse_launch_profile_snapshot, resolve_executable},
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const PRE_WRITE_RETRIES: usize = 3;

/// Runs one Worker pane's provider process until it exits or the Host is stopped.
///
/// # Errors
///
/// Returns an error when Session bootstrap, profile validation, broker delivery, or the
/// provider lifecycle fails.
pub async fn run_worker_host(socket: &Path, state_dir: &Path, bearer: String) -> Result<()> {
    let mut capability = SessionCapability::from_bearer(bearer)?;
    loop {
        match run_worker_host_inner(socket, state_dir, capability.clone()).await {
            Ok(Some(rotated)) => capability = rotated,
            Ok(None) => return Ok(()),
            Err(error) => {
                let _ = broker_execute(
                    socket,
                    capability,
                    CoordinatorCommand::RecordHostFailed {
                        diagnostic: format!("{error:#}"),
                    },
                )
                .await;
                return Err(error);
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the Host loop owns one provider lifecycle and event stream"
)]
async fn run_worker_host_inner(
    socket: &Path,
    state_dir: &Path,
    capability: SessionCapability,
) -> Result<Option<SessionCapability>> {
    let session = broker_query(socket, capability.clone(), CoordinatorQuery::SessionSelf).await?;
    let QueryResult::Session(session) = session else {
        bail!("broker returned the wrong Session bootstrap projection");
    };
    let snapshot = session
        .profile_snapshot
        .context("Worker Session has no launch profile snapshot")?;
    let expected_digest = session
        .profile_digest
        .context("Worker Session has no launch profile digest")?;
    let actual_digest = hex::encode(Sha256::digest(snapshot.as_bytes()));
    if actual_digest != expected_digest {
        bail!("Worker launch profile snapshot digest does not match durable Session state");
    }
    let profile = parse_launch_profile_snapshot(&snapshot)
        .map_err(anyhow::Error::msg)
        .context("decoding durable Worker launch profile snapshot")?;
    if profile.kind != session.definition.kind {
        bail!("Worker Harness Kind differs from its durable launch profile");
    }
    let process_environment = std::env::vars().collect::<BTreeMap<_, _>>();
    let executable = resolve_executable(&profile, &process_environment)
        .context("resolving durable Worker executable")?;
    let mut environment = profile
        .inherit_env
        .iter()
        .filter_map(|name| {
            process_environment
                .get(name)
                .map(|value| (name.clone(), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    crate::mcp::verify_required_worker_tools(socket, capability.clone())
        .await
        .context("verifying required Coordinator tools")?;
    let CommandOutcome::HostConnectionBound {
        capability: host_capability,
        ..
    } = broker_execute(
        socket,
        capability,
        CoordinatorCommand::BindHostConnection {
            instance_id: format!("worker-host:{}", std::process::id()),
            lease_seconds: 15,
        },
    )
    .await?
    else {
        bail!("Coordinator returned the wrong Host connection outcome")
    };
    let capability = host_capability;
    let mut heartbeat = HostHeartbeat::spawn(
        socket.to_path_buf(),
        capability.clone(),
        Duration::from_secs(2),
    );
    environment.insert(
        "HERDR_HARNESS_CAPABILITY".to_owned(),
        serde_json::to_value(&capability)?
            .as_str()
            .context("Host connection capability did not serialize as a bearer")?
            .to_owned(),
    );
    environment.insert("HERDR_HARNESS_ACTOR".to_owned(), "host".to_owned());
    environment.insert(
        "HERDR_COORDINATOR_SOCKET".to_owned(),
        socket.to_string_lossy().into_owned(),
    );
    environment.insert(
        "HERDR_PLUGIN_STATE_DIR".to_owned(),
        state_dir.to_string_lossy().into_owned(),
    );
    let spec = HarnessStartSpec {
        session_id: session.session_id,
        tier: HarnessTier::Worker,
        executable,
        cwd: session.definition.cwd,
        provider_state_dir: state_dir
            .join("sessions")
            .join(session.session_id.to_string()),
        provider_profile: profile.provider_profile,
        model: profile.model,
        config_overlays: profile.config_overlays,
        codex_approval_policy: profile.codex_approval_policy,
        codex_sandbox_mode: profile.codex_sandbox_mode,
        environment,
    };
    tokio::fs::create_dir_all(&spec.provider_state_dir)
        .await
        .context("creating provider Session state directory")?;
    let mut adapter: Box<dyn HarnessAdapter> = match profile.kind {
        HarnessKind::Omp => Box::new(OmpProcessAdapter::new()),
        HarnessKind::Codex => Box::new(CodexProcessAdapter::new()),
    };
    let capabilities = adapter.capabilities();
    let native = adapter
        .start(&spec)
        .await
        .context("starting native Harness")?;
    broker_execute(
        socket,
        capability.clone(),
        CoordinatorCommand::RecordHostCompatibility {
            resolved_executable: spec.executable.clone(),
            observed_version: native.observed_version,
            native_session_id: native.session_id,
            native_thread_id: native.thread_id,
            effective_model: native.model,
            safe_compaction: capabilities.safe_compaction,
            evidence: HarnessCompatibilityEvidenceV1 {
                schema_version: SCHEMA_VERSION,
                kind: profile.kind,
                capabilities,
                successful_checks: match profile.kind {
                    HarnessKind::Omp => vec![
                        "version".to_owned(),
                        "ready".to_owned(),
                        "set_host_tools".to_owned(),
                        "get_state".to_owned(),
                    ],
                    HarnessKind::Codex => vec![
                        "version".to_owned(),
                        "initialize".to_owned(),
                        "initialized".to_owned(),
                        "thread_start".to_owned(),
                    ],
                },
            },
        },
    )
    .await?;
    broker_execute(
        socket,
        capability.clone(),
        CoordinatorCommand::RecordHostReady,
    )
    .await?;
    let snapshot = adapter
        .snapshot()
        .await
        .context("snapshotting native Harness")?;
    broker_execute(
        socket,
        capability.clone(),
        CoordinatorCommand::RecordAdapterSnapshot { snapshot },
    )
    .await?;
    let mut events = adapter.events();
    let mut current_task = None;
    let mut cancellation_requested = None;
    let mut cancellation_started = None;
    let mut event_sequence = session.event_sequence;
    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    loop {
        tokio::select! {
            error = heartbeat.failed() => return Err(error),
            _ = ticker.tick() => {
                if current_task.is_none() {
                    let snapshot = adapter.snapshot().await.context("refreshing native Harness snapshot")?;
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::RecordAdapterSnapshot { snapshot },
                    ).await?;
                }
                let claim = broker_execute(
                    socket,
                    capability.clone(),
                    CoordinatorCommand::ClaimNextTask,
                ).await?;
                if let CommandOutcome::SessionCompactionRequired { .. } = claim {
                    adapter.compact().await.context("compacting required OMP Session")?;
                    let snapshot = adapter.snapshot().await.context("snapshotting compacted OMP Session")?;
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::RecordAdapterSnapshot { snapshot },
                    ).await?;
                    continue;
                }
                if let CommandOutcome::SessionRotationRequired { .. } = claim {
                    adapter.stop().await.context("stopping Session before same-pane rotation")?;
                    let rotated = broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::RotateWorkerSession,
                    ).await?;
                    let CommandOutcome::WorkerSessionRotated { capability, .. } = rotated else {
                        bail!("Coordinator returned the wrong Session rotation outcome")
                    };
                    return Ok(Some(capability));
                }
                let inbox = broker_query(socket, capability.clone(), CoordinatorQuery::Inbox).await?;
                let QueryResult::Inbox(messages) = inbox else { bail!("broker returned the wrong inbox projection") };
                if let Some(message) = messages.first() {
                    let Some(delivery) = resolve_delivery(
                        adapter.as_mut(),
                        socket,
                        state_dir,
                        capability.clone(),
                        message,
                        message.task_id,
                    ).await? else {
                        continue;
                    };
                    match dispatch_with_safe_retries(adapter.as_mut(), delivery).await {
                        Ok(acceptance) => {
                            broker_execute(socket, capability.clone(), CoordinatorCommand::AcceptDelivery {
                                message_id: message.id,
                                native_correlation: acceptance.correlation,
                            }).await?;
                            broker_execute(socket, capability.clone(), CoordinatorCommand::MarkInboxRead {
                                message_ids: vec![message.id],
                            }).await?;
                            if let Some(task_id) = message.task_id {
                                current_task = Some(task_id);
                            }
                        }
                        Err(error) if error.provider_bytes_may_have_been_written() => {
                            broker_execute(socket, capability.clone(), CoordinatorCommand::MarkDeliveryUnknown {
                                message_id: message.id,
                                diagnostic: error.to_string(),
                            }).await?;
                            adapter.stop().await.ok();
                            return Err(error).context("native delivery acceptance became ambiguous");
                        }
                        Err(error) => return Err(error).context("native delivery failed before acceptance"),
                    }
                }
                let tasks = broker_query(socket, capability.clone(), CoordinatorQuery::ListTasks).await?;
                if let QueryResult::Tasks(tasks) = tasks
                    && let Some(task) = tasks.iter().find(|task| task.worker_id == session.definition.id && task.state == TaskState::Cancelling)
                    && cancellation_requested != Some(task.id)
                {
                    if current_task == Some(task.id) {
                        adapter.cancel_active().await.context("cooperatively cancelling native turn")?;
                        cancellation_requested = Some(task.id);
                        cancellation_started = Some(tokio::time::Instant::now());
                    } else {
                        broker_execute(socket, capability.clone(), CoordinatorCommand::RecordCancellationCompleted {
                            task_id: task.id,
                            succeeded: true,
                        }).await?;
                    }
                }
                if let (Some(task_id), Some(started)) = (cancellation_requested, cancellation_started)
                    && started.elapsed() >= Duration::from_secs(15)
                {
                    broker_execute(socket, capability.clone(), CoordinatorCommand::RecordCancellationCompleted {
                        task_id,
                        succeeded: false,
                    }).await?;
                    adapter.stop().await.ok();
                    bail!("cooperative cancellation timed out");
                }
                let session_state = broker_query(socket, capability.clone(), CoordinatorQuery::SessionSelf).await?;
                let QueryResult::Session(session_state) = session_state else {
                    bail!("broker returned the wrong Session projection");
                };
                if session_state.activity == "stopping" {
                    let tasks = broker_query(socket, capability.clone(), CoordinatorQuery::ListTasks).await?;
                    let QueryResult::Tasks(tasks) = tasks else {
                        bail!("broker returned the wrong Task projection");
                    };
                    let active = tasks.iter().any(|task| {
                        task.worker_id == session.definition.id
                            && matches!(task.state, TaskState::Dispatching | TaskState::Working | TaskState::Waiting | TaskState::Reviewing | TaskState::Cancelling | TaskState::DeliveryUnknown)
                    });
                    if !active {
                        adapter.stop().await.context("stopping native Harness")?;
                        broker_execute(socket, capability.clone(), CoordinatorCommand::RecordHostStopped { clean: true }).await?;
                        return Ok(None);
                    }
                }
            }
            event = events.next() => {
                match event {
                    Some(Ok(event)) => {
                        event_sequence = event_sequence.saturating_add(1);
                        broker_execute(socket, capability.clone(), CoordinatorCommand::RecordHostEvent {
                            sequence: event_sequence,
                            event: serde_json::to_value(&event).context("serializing normalized Host event")?,
                        }).await?;
                        match event {
                            AdapterEvent::TurnCompleted { turn_id, status } => {
                                if let Some(task_id) = current_task.take() {
                                    let task = broker_query(socket, capability.clone(), CoordinatorQuery::GetTask { task_id }).await?;
                                    let QueryResult::Task(task) = task else {
                                        bail!("broker returned the wrong Task projection")
                                    };
                                    if task.state == TaskState::Cancelling {
                                        broker_execute(socket, capability.clone(), CoordinatorCommand::RecordCancellationCompleted {
                                            task_id,
                                            succeeded: matches!(status, NativeTurnStatus::Interrupted | NativeTurnStatus::Completed),
                                        }).await?;
                                        cancellation_requested = None;
                                        cancellation_started = None;
                                    } else {
                                        broker_execute(socket, capability.clone(), CoordinatorCommand::RecordTurnCompleted {
                                            task_id,
                                            native_turn_id: turn_id.unwrap_or_else(|| "provider-turn".to_owned()),
                                            succeeded: status == NativeTurnStatus::Completed,
                                        }).await?;
                                    }
                                }
                            }
                            AdapterEvent::Failed { message } => return Err(anyhow!(message)),
                            AdapterEvent::Exited { exit_code } => {
                                bail!("native Harness exited with status {exit_code:?}");
                            }
                            _ => {}
                        }
                    }
                    Some(Err(error)) => return Err(error).context("reading native Harness event"),
                    None => bail!("native Harness event stream closed"),
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("waiting for Worker Host shutdown signal")?;
                adapter.stop().await.context("stopping native Harness")?;
                return Ok(None);
            }
        }
    }
}

/// One top-level Worker row in the Supervisor popup.
#[derive(Debug, Clone)]
pub struct PopupWorkerView {
    id: crate::contract::HarnessId,
    kind: String,
    model: Option<String>,
    launch_profile: Option<String>,
    presence: String,
    activity: String,
    native_health: Option<String>,
    context_percent: Option<String>,
    context_observed_at: Option<String>,
    last_activity: Option<(String, String)>,
    session_id: Option<crate::contract::HarnessSessionId>,
    terminal_id: Option<String>,
    unread_messages: u32,
    active_task: Option<PopupTaskView>,
    queued_tasks: Vec<PopupTaskView>,
    blockers: Vec<String>,
    holds: Vec<HoldView>,
    attention_events: usize,
}

#[derive(Debug, Clone)]
struct PopupTaskView {
    title: String,
    graph: TaskGraphView,
}

impl std::ops::Deref for PopupTaskView {
    type Target = TaskGraphView;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

impl PopupWorkerView {
    fn attention_required(&self) -> bool {
        self.unread_messages > 0
            || !self.holds.is_empty()
            || self.attention_events > 0
            || matches!(
                self.activity.as_str(),
                "waiting" | "reviewing" | "cancelling" | "delivery_unknown"
            )
            || self.active_task.as_ref().is_some_and(|task| {
                matches!(
                    task.task.state,
                    TaskState::Waiting
                        | TaskState::Reviewing
                        | TaskState::Cancelling
                        | TaskState::DeliveryUnknown
                )
            })
    }

    fn priority(&self) -> u8 {
        if self.attention_required() {
            0
        } else if self.active_task.as_ref().is_some_and(|task| {
            matches!(
                task.task.state,
                TaskState::Dispatching | TaskState::Working | TaskState::Cancelling
            )
        }) {
            1
        } else if !self.queued_tasks.is_empty() {
            2
        } else if self.presence == "online" {
            3
        } else {
            4
        }
    }

    fn status_label(&self) -> String {
        self.active_task.as_ref().map_or_else(
            || self.activity.clone(),
            |task| task.task.state.as_str().to_owned(),
        )
    }

    fn status_style(&self) -> Style {
        if self.attention_required() {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if self.presence != "online" {
            Style::default().fg(Color::DarkGray)
        } else if self.active_task.as_ref().is_some_and(|task| {
            matches!(task.task.state, TaskState::Working | TaskState::Dispatching)
        }) {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Cyan)
        }
    }
}

/// Aggregate projection consumed by the terminal-only Harness Network dashboard.
#[derive(Debug, Clone, Default)]
pub struct PopupDashboard {
    workers: Vec<PopupWorkerView>,
    generated_at: Option<String>,
    supervisor_attention: u32,
    supervisor_id: Option<String>,
    supervisor_presence: Option<String>,
    supervisor_activity: Option<String>,
}

impl PopupDashboard {
    /// Combines the currently available durable projections without inspecting terminal output.
    #[must_use]
    pub fn from_projections(
        status: &[HarnessStatusView],
        tasks: &[TaskView],
        graph: &[TaskGraphView],
        holds: &[HoldView],
    ) -> Self {
        let mut workers = status
            .iter()
            .filter(|harness| harness.tier == HarnessTier::Worker)
            .map(|harness| {
                let active_task = harness
                    .active_task_id
                    .and_then(|task_id| tasks.iter().find(|task| task.id == task_id).cloned());
                let queued_tasks = graph
                    .iter()
                    .filter(|entry| {
                        entry.task.worker_id == harness.id && entry.task.state == TaskState::Queued
                    })
                    .cloned()
                    .map(|graph| PopupTaskView {
                        title: graph.task.id.to_string(),
                        graph,
                    })
                    .collect::<Vec<_>>();
                let blockers = active_task.as_ref().map_or_else(Vec::new, |task| {
                    graph
                        .iter()
                        .find(|entry| entry.task.id == task.id)
                        .map_or_else(Vec::new, |entry| {
                            let mut blockers = Vec::new();
                            if entry.waiting_for_worker {
                                blockers.push("waiting Worker".to_owned());
                            }
                            if entry.waiting_for_session {
                                blockers.push("waiting Session".to_owned());
                            }
                            if entry.waiting_for_repository {
                                blockers.push("waiting repository".to_owned());
                            }
                            blockers
                        })
                });
                let worker_task_ids = graph
                    .iter()
                    .filter(|entry| entry.task.worker_id == harness.id)
                    .map(|entry| entry.task.id)
                    .collect::<Vec<_>>();
                let worker_holds = holds
                    .iter()
                    .filter(|hold| worker_task_ids.contains(&hold.task_id))
                    .cloned()
                    .collect();
                PopupWorkerView {
                    id: harness.id.clone(),
                    kind: "unknown".to_owned(),
                    model: None,
                    launch_profile: None,
                    presence: harness.presence.clone(),
                    activity: harness.activity.clone(),
                    native_health: None,
                    context_percent: active_task
                        .as_ref()
                        .and_then(|task| task.context_percent.clone()),
                    context_observed_at: None,
                    last_activity: None,
                    session_id: None,
                    terminal_id: None,
                    unread_messages: harness.unread_messages,
                    active_task: active_task.map(|task| PopupTaskView {
                        title: task.id.to_string(),
                        graph: TaskGraphView {
                            task,
                            scheduling_state: crate::core::TaskSchedulingState::Ready,
                            dependencies: Vec::new(),
                            dependents: Vec::new(),
                            worker_queue_position: None,
                            waiting_for_worker: false,
                            waiting_for_session: false,
                            waiting_for_repository: false,
                        },
                    }),
                    queued_tasks,
                    blockers,
                    holds: worker_holds,
                    attention_events: 0,
                }
            })
            .collect::<Vec<_>>();
        workers.sort_by(|left, right| {
            left.priority()
                .cmp(&right.priority())
                .then_with(|| left.id.cmp(&right.id))
        });
        Self {
            workers,
            generated_at: None,
            supervisor_attention: 0,
            supervisor_id: None,
            supervisor_presence: None,
            supervisor_activity: None,
        }
    }

    /// Adapts the single coherent Coordinator dashboard query for terminal rendering.
    #[must_use]
    pub fn from_dashboard(view: DashboardView) -> Self {
        let mut workers = view
            .workers
            .into_iter()
            .map(|worker| {
                let session = worker.session;
                let blockers = worker.active_task.as_ref().map_or_else(Vec::new, |task| {
                    let mut blockers = Vec::new();
                    if task.waiting_for_worker {
                        blockers.push("waiting Worker".to_owned());
                    }
                    if task.waiting_for_session {
                        blockers.push("waiting Session".to_owned());
                    }
                    if task.waiting_for_repository {
                        blockers.push("waiting repository".to_owned());
                    }
                    blockers
                });
                PopupWorkerView {
                    id: worker.id,
                    kind: format!("{:?}", worker.kind),
                    model: worker.model,
                    launch_profile: worker.launch_profile,
                    presence: session.as_ref().map_or_else(
                        || "offline".to_owned(),
                        |session| session.presence.to_string(),
                    ),
                    activity: session.as_ref().map_or_else(
                        || "offline".to_owned(),
                        |session| session.activity.to_string(),
                    ),
                    native_health: session
                        .as_ref()
                        .map(|session| format!("{:?}", session.native_health)),
                    context_percent: session
                        .as_ref()
                        .and_then(|session| session.context_percent.clone()),
                    context_observed_at: session
                        .as_ref()
                        .and_then(|session| session.context_observed_at.clone()),
                    last_activity: session.as_ref().and_then(|session| {
                        session.last_activity.as_ref().map(|activity| {
                            (activity.summary.clone(), activity.observed_at.clone())
                        })
                    }),
                    session_id: session.as_ref().map(|session| session.id),
                    terminal_id: session.and_then(|session| session.terminal_id),
                    unread_messages: worker.unread_messages,
                    active_task: worker.active_task.map(|task| PopupTaskView {
                        title: task.title,
                        graph: TaskGraphView {
                            task: task.task,
                            scheduling_state: task.scheduling_state,
                            dependencies: task.dependencies,
                            dependents: task.dependents,
                            worker_queue_position: task.worker_queue_position,
                            waiting_for_worker: task.waiting_for_worker,
                            waiting_for_session: task.waiting_for_session,
                            waiting_for_repository: task.waiting_for_repository,
                        },
                    }),
                    queued_tasks: worker
                        .queued_tasks
                        .into_iter()
                        .map(|task| PopupTaskView {
                            title: task.title,
                            graph: TaskGraphView {
                                task: task.task,
                                scheduling_state: task.scheduling_state,
                                dependencies: task.dependencies,
                                dependents: task.dependents,
                                worker_queue_position: task.worker_queue_position,
                                waiting_for_worker: task.waiting_for_worker,
                                waiting_for_session: task.waiting_for_session,
                                waiting_for_repository: task.waiting_for_repository,
                            },
                        })
                        .collect(),
                    blockers,
                    holds: worker.holds,
                    attention_events: worker.attention_events.len(),
                }
            })
            .collect::<Vec<_>>();
        workers.sort_by(|left, right| {
            left.priority()
                .cmp(&right.priority())
                .then_with(|| left.id.cmp(&right.id))
        });
        Self {
            workers,
            generated_at: Some(view.generated_at),
            supervisor_attention: view.supervisor.attention_count,
            supervisor_id: Some(view.supervisor.id.to_string()),
            supervisor_presence: Some(view.supervisor.presence.to_string()),
            supervisor_activity: Some(view.supervisor.activity.to_string()),
        }
    }

    /// Returns Worker ids in their visible priority order.
    pub fn worker_ids(&self) -> impl Iterator<Item = &str> {
        self.workers.iter().map(|worker| worker.id.as_str())
    }

    fn selected(&self, selection: &PopupSelection) -> Option<&PopupWorkerView> {
        selection
            .selected_worker
            .as_ref()
            .and_then(|id| self.workers.iter().find(|worker| worker.id == *id))
    }
}

/// Detail/list mode for a narrow popup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupNarrowMode {
    #[default]
    List,
    Detail,
}

/// Stable popup selection, keyed by durable Harness identity rather than display position.
#[derive(Debug, Clone, Default)]
pub struct PopupSelection {
    selected_worker: Option<crate::contract::HarnessId>,
    narrow_mode: PopupNarrowMode,
}

impl PopupSelection {
    /// Returns the selected durable Worker identity.
    #[must_use]
    pub fn selected_worker_id(&self) -> Option<&crate::contract::HarnessId> {
        self.selected_worker.as_ref()
    }

    /// Shows the selected Worker's detail on a narrow terminal.
    pub fn show_detail(&mut self) {
        self.narrow_mode = PopupNarrowMode::Detail;
    }

    /// Returns a narrow terminal to its Worker list.
    pub fn show_list(&mut self) {
        self.narrow_mode = PopupNarrowMode::List;
    }

    /// Preserves the selected Worker through a refresh, choosing the first visible Worker only
    /// when the previous Worker is absent.
    pub fn retain_worker(&mut self, dashboard: &PopupDashboard) {
        if self.selected_worker.as_ref().is_some_and(|selected| {
            dashboard
                .workers
                .iter()
                .any(|worker| worker.id == *selected)
        }) {
            return;
        }
        self.selected_worker = dashboard.workers.first().map(|worker| worker.id.clone());
    }

    /// Selects the next Worker in visible priority order.
    pub fn select_next(&mut self, dashboard: &PopupDashboard) {
        if self.selected_worker.is_none() {
            self.retain_worker(dashboard);
            return;
        }
        self.retain_worker(dashboard);
        let Some(selected) = self.selected_worker.as_ref() else {
            return;
        };
        let Some(index) = dashboard
            .workers
            .iter()
            .position(|worker| worker.id == *selected)
        else {
            return;
        };
        self.selected_worker = dashboard
            .workers
            .get((index + 1).min(dashboard.workers.len().saturating_sub(1)))
            .map(|worker| worker.id.clone());
    }

    /// Selects the previous Worker in visible priority order.
    pub fn select_previous(&mut self, dashboard: &PopupDashboard) {
        if self.selected_worker.is_none() {
            self.retain_worker(dashboard);
            return;
        }
        self.retain_worker(dashboard);
        let Some(selected) = self.selected_worker.as_ref() else {
            return;
        };
        let Some(index) = dashboard
            .workers
            .iter()
            .position(|worker| worker.id == *selected)
        else {
            return;
        };
        self.selected_worker = dashboard
            .workers
            .get(index.saturating_sub(1))
            .map(|worker| worker.id.clone());
    }

    fn selected_task<'a>(&self, dashboard: &'a PopupDashboard) -> Option<&'a TaskGraphView> {
        dashboard
            .selected(self)
            .and_then(|worker| worker.active_task.as_ref())
            .map(|task| &task.graph)
    }

    /// Returns the selected Worker's active Task target for approve and cancel actions.
    #[must_use]
    pub fn selected_task_id(&self, dashboard: &PopupDashboard) -> Option<crate::contract::TaskId> {
        self.selected_task(dashboard).map(|task| task.task.id)
    }

    /// Returns the selected Worker's first unresolved Hold target, including terminal Tasks.
    #[must_use]
    pub fn selected_hold_task_id(
        &self,
        dashboard: &PopupDashboard,
    ) -> Option<crate::contract::TaskId> {
        dashboard
            .selected(self)
            .and_then(|worker| worker.holds.first())
            .map(|hold| hold.task_id)
    }

    /// Returns the selected Worker only when it currently accepts a stop request.
    #[must_use]
    pub fn selected_stoppable_worker_id<'a>(
        &self,
        dashboard: &'a PopupDashboard,
    ) -> Option<&'a crate::contract::HarnessId> {
        dashboard
            .selected(self)
            .filter(|worker| worker.presence == "online")
            .map(|worker| &worker.id)
    }
}

/// Draws the live Supervisor popup. Wide terminals show the Worker list and detail together;
/// narrow terminals switch between them using Enter/Right and Left.
pub fn draw_popup_dashboard(
    frame: &mut Frame,
    dashboard: &PopupDashboard,
    selection: &PopupSelection,
    stale_error: Option<&str>,
    warning: Option<&str>,
) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).areas(frame.area());
    let supervisor = dashboard
        .supervisor_id
        .as_ref()
        .map_or_else(String::new, |id| {
            let presence = dashboard
                .supervisor_presence
                .as_deref()
                .unwrap_or("unknown");
            let activity = dashboard
                .supervisor_activity
                .as_deref()
                .unwrap_or("unknown");
            format!(" · Supervisor {id} {presence}/{activity}")
        });
    let title = if stale_error.is_some() {
        " Harness Network · STALE ".to_owned()
    } else {
        dashboard.generated_at.as_ref().map_or_else(
            || format!(" Harness Network{supervisor} "),
            |generated_at| format!(" Harness Network{supervisor} · updated {generated_at} "),
        )
    };
    let root = Block::default().borders(Borders::ALL).title(title);
    let inner = root.inner(body);
    frame.render_widget(root, body);
    if inner.width >= 88 {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(inner);
        render_worker_list(frame, list_area, dashboard, selection);
        render_worker_detail(
            frame,
            detail_area,
            dashboard.selected(selection),
            stale_error,
            warning,
        );
    } else if selection.narrow_mode == PopupNarrowMode::Detail {
        render_worker_detail(
            frame,
            inner,
            dashboard.selected(selection),
            stale_error,
            warning,
        );
    } else {
        render_worker_list(frame, inner, dashboard, selection);
    }
    frame.render_widget(
        Paragraph::new("[↑/↓] Select  [Enter/→] Detail  [←] List  [a] Approve  [c] Cancel  [h] Clear Hold\n[s] Stop Worker  [r] Refresh  [o] Open Worker  [Esc/q] Close")
            .style(Style::default().fg(Color::Gray)),
        footer,
    );
}

fn render_worker_list(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    dashboard: &PopupDashboard,
    selection: &PopupSelection,
) {
    let attention = dashboard
        .workers
        .iter()
        .filter(|worker| worker.attention_required())
        .count();
    let items = dashboard
        .workers
        .iter()
        .map(|worker| {
            ListItem::new(Line::from(vec![
                Span::styled("● ", worker.status_style()),
                Span::raw(format!("{:<20}", worker.id)),
                Span::styled(worker.status_label(), worker.status_style()),
                Span::raw(format!("  inbox {}", worker.unread_messages)),
            ]))
        })
        .collect::<Vec<_>>();
    let selected = selection.selected_worker.as_ref().and_then(|selected| {
        dashboard
            .workers
            .iter()
            .position(|worker| worker.id == *selected)
    });
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            " Workers · {} attention ",
            attention + usize::try_from(dashboard.supervisor_attention).unwrap_or(usize::MAX)
        )))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = ratatui::widgets::ListState::default();
    state.select(selected);
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_worker_detail(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    worker: Option<&PopupWorkerView>,
    stale_error: Option<&str>,
    warning: Option<&str>,
) {
    let mut lines = detail_notice_lines(stale_error, warning);
    match worker {
        Some(worker) => lines.extend(worker_detail_lines(worker)),
        None => lines.push(Line::from("No top-level Worker is available.")),
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Worker detail "),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn detail_notice_lines<'a>(stale_error: Option<&str>, warning: Option<&str>) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    if let Some(error) = stale_error {
        lines.push(Line::styled(
            format!("Refresh error: {error}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(warning) = warning {
        lines.push(Line::styled(
            format!("Action warning: {warning}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines
}

fn worker_detail_lines(worker: &PopupWorkerView) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::raw("Worker  "),
            Span::styled(
                worker.id.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!(
            "Harness  {}{}{}",
            worker.kind,
            worker
                .model
                .as_deref()
                .map_or_else(String::new, |model| format!(" · {model}")),
            worker
                .launch_profile
                .as_deref()
                .map_or_else(String::new, |profile| format!(" · {profile}")),
        )),
        Line::from(vec![
            Span::raw("Status  "),
            Span::styled(worker.status_label(), worker.status_style()),
            Span::raw(format!(" · {}", worker.presence)),
        ]),
        Line::from(format!("Unread  {}", worker.unread_messages)),
        Line::from(format!("Queued  {}", worker.queued_tasks.len())),
    ];
    if let Some(native_health) = &worker.native_health {
        lines.push(Line::from(format!("Health  {native_health}")));
    }
    if let Some((summary, observed_at)) = &worker.last_activity {
        lines.push(Line::from(format!(
            "Activity  {summary} · observed {observed_at}"
        )));
    }
    if let Some(context_percent) = &worker.context_percent {
        let observed_at = worker
            .context_observed_at
            .as_deref()
            .unwrap_or("observation time unavailable");
        lines.push(Line::from(format!(
            "Context  {context_percent}% · observed {observed_at}"
        )));
    }
    lines.extend(worker_task_lines(worker));
    lines
}

fn worker_task_lines(worker: &PopupWorkerView) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(task) = &worker.active_task {
        lines.extend([
            Line::from(""),
            Line::styled("Active Task", Style::default().add_modifier(Modifier::BOLD)),
            Line::from(format!(
                "{} · {} · revision {}",
                task.title,
                task.task.state.as_str(),
                task.task.result_revision
            )),
        ]);
    }
    if !worker.blockers.is_empty() {
        lines.extend([
            Line::from(""),
            Line::from(format!("Blockers  {}", worker.blockers.join(" · "))),
        ]);
    }
    if !worker.queued_tasks.is_empty() {
        lines.extend([
            Line::from(""),
            Line::styled(
                "Queued Tasks",
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]);
        for task in &worker.queued_tasks {
            let queue = task
                .worker_queue_position
                .map_or_else(|| "-".to_owned(), |position| position.to_string());
            lines.push(Line::from(format!(
                "{} · queue {queue} · {:?}",
                task.title, task.scheduling_state
            )));
            let blockers = task_blockers(task);
            if !blockers.is_empty() {
                lines.push(Line::from(format!("  {}", blockers.join(" · "))));
            }
        }
    }
    for hold in &worker.holds {
        lines.push(Line::styled(
            format!("Worktree Hold  {} · {}", hold.task_id, hold.reason),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines
}

fn task_blockers(task: &TaskGraphView) -> Vec<String> {
    let mut blockers = Vec::new();
    if task.waiting_for_worker {
        blockers.push("waiting Worker".to_owned());
    }
    if task.waiting_for_session {
        blockers.push("waiting Session".to_owned());
    }
    if task.waiting_for_repository {
        blockers.push("waiting repository".to_owned());
    }
    blockers.extend(
        task.dependencies
            .iter()
            .filter(|dependency| dependency.satisfied_by_result_revision.is_none())
            .map(|dependency| {
                format!(
                    "dependency {} {:?}",
                    dependency.task_id, dependency.condition
                )
            }),
    );
    blockers
}

/// Renders one durable text snapshot for non-interactive callers.
///
/// # Errors
///
/// Returns an error when authentication, broker queries, or response decoding fails.
pub async fn render_popup(socket: &Path, bearer: String) -> Result<String> {
    let capability = SessionCapability::from_bearer(bearer)?;
    let dashboard = query_popup_dashboard(socket, &capability).await?;
    let mut output = String::from("Harness Network\n\nWorkers\n");
    if let Some(supervisor_id) = &dashboard.supervisor_id {
        let _ = writeln!(output, "Supervisor {supervisor_id}");
    }
    for worker in dashboard.workers {
        let _ = writeln!(
            output,
            "{} · {} · {} · inbox {}",
            worker.id,
            worker.presence,
            worker.status_label(),
            worker.unread_messages
        );
    }
    output.push_str("\nTasks\nScheduling\n");
    Ok(output)
}

async fn query_popup_dashboard(
    socket: &Path,
    capability: &SessionCapability,
) -> Result<PopupDashboard> {
    let dashboard = broker_query(socket, capability.clone(), CoordinatorQuery::Dashboard).await?;
    let QueryResult::Dashboard(dashboard) = dashboard else {
        bail!("invalid dashboard response")
    };
    Ok(PopupDashboard::from_dashboard(dashboard))
}

async fn focus_popup_worker(
    socket: &Path,
    capability: &SessionCapability,
    worker: &PopupWorkerView,
) -> Result<()> {
    let session_id = worker
        .session_id
        .context("selected Worker has no live Coordinator Session")?;
    let terminal_id = worker
        .terminal_id
        .as_deref()
        .context("selected Worker has no durable terminal identity")?;
    let socket_path = std::env::var_os("HERDR_SOCKET_PATH")
        .map(PathBuf::from)
        .context("HERDR_SOCKET_PATH is unavailable for Worker focus")?;
    let client = HerdrSocketClient::new(socket_path);
    let snapshot = client
        .snapshot()
        .await
        .context("reading Herdr pane snapshot")?;
    let location = snapshot
        .resolve_terminal(terminal_id)
        .context("resolving selected Worker terminal")?;
    broker_execute(
        socket,
        capability.clone(),
        CoordinatorCommand::RecordPaneLocation {
            session_id,
            terminal_id: location.terminal_id.clone(),
            pane_id: location.pane_id.clone(),
        },
    )
    .await
    .context("refreshing selected Worker pane location")?;
    client
        .focus(&location.pane_id)
        .await
        .context("focusing selected Worker pane")?;
    client
        .close_popup()
        .await
        .context("closing Harness Network popup")?;
    Ok(())
}

async fn close_popup() -> Result<()> {
    let socket_path = std::env::var_os("HERDR_SOCKET_PATH")
        .map(PathBuf::from)
        .context("HERDR_SOCKET_PATH is unavailable for popup close")?;
    HerdrSocketClient::new(socket_path)
        .close_popup()
        .await
        .context("closing Harness Network popup")
}

/// Runs the interactive Supervisor popup until Escape or `q` is pressed.
///
/// The selection follows a durable Worker identity across refreshes. Arrow keys select Workers;
/// `a` and `c` apply to the selected active Task, `h` applies to its first unresolved Hold,
/// and `s` stops that selected Worker.
///
/// # Errors
///
/// Returns an error when terminal setup, broker access, or an authorized action fails.
pub async fn run_popup(socket: &Path, bearer: String) -> Result<()> {
    let capability = SessionCapability::from_bearer(bearer.clone())?;
    let mut terminal = ratatui::init();
    let result = run_popup_loop(&mut terminal, socket, &capability).await;
    ratatui::restore();
    result
}

#[expect(
    clippy::too_many_lines,
    reason = "the popup loop keeps refresh, stable Worker selection, and authorized controls together"
)]
async fn run_popup_loop(
    terminal: &mut ratatui::DefaultTerminal,
    socket: &Path,
    capability: &SessionCapability,
) -> Result<()> {
    let mut selection = PopupSelection::default();
    let mut dashboard = None;
    let mut stale_error;
    let mut popup_warning = None;
    loop {
        match query_popup_dashboard(socket, capability).await {
            Ok(next) => {
                selection.retain_worker(&next);
                dashboard = Some(next);
                stale_error = None;
            }
            Err(error) => stale_error = Some(format!("{error:#}")),
        }
        terminal.draw(|frame| {
            let fallback = PopupDashboard::default();
            draw_popup_dashboard(
                frame,
                dashboard.as_ref().unwrap_or(&fallback),
                &selection,
                stale_error.as_deref(),
                popup_warning.as_deref(),
            );
        })?;
        if !event::poll(Duration::from_secs(1))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                close_popup().await?;
                return Ok(());
            }
            KeyCode::Up => {
                if let Some(dashboard) = &dashboard {
                    selection.select_previous(dashboard);
                }
            }
            KeyCode::Down => {
                if let Some(dashboard) = &dashboard {
                    selection.select_next(dashboard);
                }
            }
            KeyCode::Enter | KeyCode::Right => selection.narrow_mode = PopupNarrowMode::Detail,
            KeyCode::Left => selection.narrow_mode = PopupNarrowMode::List,
            KeyCode::Char('r') => popup_warning = None,
            KeyCode::Char('o') => {
                let Some(worker) = dashboard
                    .as_ref()
                    .and_then(|dashboard| dashboard.selected(&selection))
                else {
                    continue;
                };
                match focus_popup_worker(socket, capability, worker).await {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        popup_warning = Some(format!("Unable to focus Worker: {error:#}"));
                    }
                }
            }
            KeyCode::Char('c') => {
                if let Some(task) = dashboard
                    .as_ref()
                    .and_then(|dashboard| selection.selected_task(dashboard))
                {
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::CancelTask {
                            task_id: task.task.id,
                        },
                    )
                    .await?;
                }
            }
            KeyCode::Char('a') => {
                if let Some(task) = dashboard
                    .as_ref()
                    .and_then(|dashboard| selection.selected_task(dashboard))
                {
                    let captured = broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::CaptureRepositoryObservation {
                            task_id: task.task.id,
                            checkpoint: crate::contract::ObservationCheckpoint::Approval,
                        },
                    )
                    .await?;
                    let CommandOutcome::ObservationRecorded { digest, .. } = captured else {
                        bail!("repository capture returned the wrong outcome")
                    };
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::ApproveTask {
                            task_id: task.task.id,
                            result_revision: task.task.result_revision,
                            observation_digest: digest,
                        },
                    )
                    .await?;
                }
            }
            KeyCode::Char('h') => {
                if let Some(task_id) = dashboard
                    .as_ref()
                    .and_then(|dashboard| selection.selected_hold_task_id(dashboard))
                {
                    let captured = broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::CaptureRepositoryObservation {
                            task_id,
                            checkpoint: crate::contract::ObservationCheckpoint::HoldClear,
                        },
                    )
                    .await?;
                    let CommandOutcome::ObservationRecorded { digest, .. } = captured else {
                        bail!("repository capture returned the wrong outcome")
                    };
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::ClearWorktreeHold {
                            task_id,
                            observation_digest: digest,
                            audit_note: "Supervisor reconciled the repository from the popup."
                                .to_owned(),
                        },
                    )
                    .await?;
                }
            }
            KeyCode::Char('s') => {
                if let Some(worker_id) = dashboard.as_ref().and_then(|dashboard| {
                    selection.selected_stoppable_worker_id(dashboard).cloned()
                }) {
                    broker_execute(
                        socket,
                        capability.clone(),
                        CoordinatorCommand::StopWorker { worker_id },
                    )
                    .await?;
                }
            }
            _ => {}
        }
    }
}

async fn resolve_delivery(
    adapter: &mut dyn HarnessAdapter,
    socket: &Path,
    state_dir: &Path,
    capability: HostConnectionCapability,
    message: &InboxMessageView,
    task_id: Option<crate::contract::TaskId>,
) -> Result<Option<ResolvedDelivery>> {
    let (text, intent, attachment_ids) = if message.kind == "task" {
        let task: TaskSubmissionV1 =
            serde_json::from_value(message.body.clone()).context("decoding root Task Message")?;
        let task_id = task_id.context("root Task delivery must carry a Task identity")?;
        let resolved = broker_query(
            socket,
            capability.clone(),
            CoordinatorQuery::ResolvedTaskInput { task_id },
        )
        .await?;
        let QueryResult::ResolvedTaskInput(resolved) = resolved else {
            bail!("broker returned the wrong resolved Task input projection");
        };
        let mut attachments = resolved.explicit_attachments;
        attachments.extend(
            resolved
                .dependency_results
                .into_iter()
                .map(|dependency| dependency.attachment_id),
        );
        (
            worker_task_prompt(task_id, &task.instructions, adapter.completion_tools()),
            DeliveryIntent::FollowUp,
            attachments,
        )
    } else {
        let message: MessageSubmissionV1 =
            serde_json::from_value(message.body.clone()).context("decoding Bus Message")?;
        (message.text, message.delivery, message.attachments)
    };
    let snapshot = adapter
        .snapshot()
        .await
        .context("capturing Adapter state before delivery")?;
    if intent == DeliveryIntent::FollowUp
        && snapshot.lifecycle == crate::adapter::AdapterLifecycle::Working
        && !adapter.capabilities().active_turn_follow_up
    {
        return Ok(None);
    }
    let kind = match intent {
        DeliveryIntent::Steer => NativeDeliveryKind::Steer,
        DeliveryIntent::FollowUp
            if snapshot.lifecycle == crate::adapter::AdapterLifecycle::Working =>
        {
            NativeDeliveryKind::FollowUp
        }
        DeliveryIntent::FollowUp => NativeDeliveryKind::StartTurn,
    };
    let mut attachments = Vec::with_capacity(attachment_ids.len());
    let store = AttachmentStore::new(state_dir);
    for attachment_id in attachment_ids {
        let result = broker_query(
            socket,
            capability.clone(),
            CoordinatorQuery::GetAttachment { attachment_id },
        )
        .await?;
        let QueryResult::Attachment(metadata) = result else {
            bail!("broker returned the wrong Attachment projection");
        };
        store
            .verify(&metadata)
            .await
            .context("verifying immutable Attachment before provider delivery")?;
        attachments.push(crate::adapter::ResolvedAttachment {
            id: metadata.id,
            path: state_dir.join(metadata.storage_path),
            media_type: metadata.media_type,
        });
    }
    Ok(Some(ResolvedDelivery {
        correlation: message.id.to_string(),
        task_id,
        kind,
        text,
        attachments,
    }))
}

/// Formats the native Worker prompt for one Task and preserves structured Result authority.
#[must_use]
pub fn worker_task_prompt(
    task_id: crate::contract::TaskId,
    instructions: &str,
    completion_tools: WorkerCompletionTools,
) -> String {
    let WorkerCompletionTools {
        attachment_create,
        complete,
    } = completion_tools;
    format!(
        "{instructions}\n\nCoordinator completion contract:\n- This is Task {task_id}.\n- Normal assistant text is not a Result and does not complete the Task.\n- Execute the requested verification command(s).\n- Call `{attachment_create}` with the exact verification output to create immutable evidence.\n- Then call `{complete}` exactly once with a `manifest` containing schema_version 1, this task_id, summary, changed_files, at least one verification entry referencing the returned Attachment ID, deviations, risks, and attachments.\n- Do not invent or search for a native turn ID; omit native_turn_id unless the provider explicitly exposes it. The Worker Host binds the Result to terminal provider evidence.\n- Do not finish the native turn until `{complete}` reports that the Result was recorded."
    )
}

async fn dispatch_with_safe_retries(
    adapter: &mut dyn HarnessAdapter,
    delivery: ResolvedDelivery,
) -> crate::adapter::AdapterResult<crate::adapter::NativeAcceptance> {
    let mut last = None;
    for _ in 0..PRE_WRITE_RETRIES {
        match adapter.dispatch(delivery.clone()).await {
            Ok(acceptance) => return Ok(acceptance),
            Err(error) if error.provider_bytes_may_have_been_written() => {
                return Err(error);
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.expect("at least one retry attempt"))
}

async fn broker_query<C>(
    socket: &Path,
    capability: C,
    query: CoordinatorQuery,
) -> Result<QueryResult>
where
    C: Into<ActorContext>,
{
    let response = call_with_connect_retry(
        socket,
        &BrokerRequest {
            schema_version: BROKER_SCHEMA_V2,
            request_id: uuid::Uuid::now_v7().to_string(),
            operation: BrokerOperation::Query {
                actor: capability.into(),
                query,
            },
        },
        Duration::from_secs(10),
    )
    .await?;
    decode_result(response)
}

async fn broker_execute<C>(
    socket: &Path,
    capability: C,
    command: CoordinatorCommand,
) -> Result<CommandOutcome>
where
    C: Into<ActorContext>,
{
    let response = call_with_connect_retry(
        socket,
        &BrokerRequest {
            schema_version: BROKER_SCHEMA_V2,
            request_id: uuid::Uuid::now_v7().to_string(),
            operation: BrokerOperation::Execute {
                actor: capability.into(),
                command,
            },
        },
        Duration::from_secs(10),
    )
    .await?;
    decode_result(response)
}

fn decode_result<T: serde::de::DeserializeOwned>(
    response: crate::broker::BrokerResponse,
) -> Result<T> {
    if let Some(error) = response.error {
        bail!("broker {:?}: {}", error.category, error.message);
    }
    serde_json::from_value(response.result.context("broker response omitted result")?)
        .context("decoding typed broker result")
}
