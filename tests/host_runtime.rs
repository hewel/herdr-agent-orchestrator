use std::{path::PathBuf, str::FromStr, sync::Arc};

use herdr_harness_coordinator::{
    adapter::WorkerCompletionTools,
    broker::BrokerServer,
    contract::{
        DependencyCondition, DependencyFailurePolicy, HarnessDefinitionV1, HarnessId, HarnessKind,
        HarnessSessionId, HarnessTier, NativeSessionHealth, SCHEMA_VERSION, SessionReusePolicy,
        TaskId, TaskRole, WorktreeHoldId,
    },
    core::{
        ActorContext, CommandOutcome, Coordinator, CoordinatorCommand, DASHBOARD_SCHEMA_VERSION,
        DashboardSessionView, DashboardSupervisorView, DashboardTaskView, DashboardView,
        DashboardWorkerView, HarnessActivity, HarnessPresence, HarnessStatusView, HoldView,
        TaskDependencyView, TaskGraphView, TaskSchedulingState, TaskState, TaskView,
    },
    host::{
        PopupDashboard, PopupSelection, draw_popup_dashboard, render_popup, worker_task_prompt,
    },
};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn worker_task_prompt_requires_a_structured_result_at_the_coordinator_boundary() {
    let task_id =
        TaskId(uuid::Uuid::parse_str("019f7606-a26b-7a41-87dd-95f3a072a226").expect("Task ID"));
    let completion_tools = WorkerCompletionTools {
        attachment_create: "fixture_attachment_create",
        complete: "fixture_complete",
    };

    let prompt = worker_task_prompt(
        task_id,
        "Inspect Cargo.toml without editing files.",
        completion_tools,
    );

    assert_eq!(
        prompt,
        "Inspect Cargo.toml without editing files.\n\nCoordinator completion contract:\n- This is Task 019f7606-a26b-7a41-87dd-95f3a072a226.\n- Normal assistant text is not a Result and does not complete the Task.\n- Execute the requested verification command(s).\n- Call `fixture_attachment_create` with the exact verification output to create immutable evidence.\n- Then call `fixture_complete` exactly once with a `manifest` containing schema_version 1, this task_id, summary, changed_files, at least one verification entry referencing the returned Attachment ID, deviations, risks, and attachments.\n- Do not invent or search for a native turn ID; omit native_turn_id unless the provider explicitly exposes it. The Worker Host binds the Result to terminal provider evidence.\n- Do not finish the native turn until `fixture_complete` reports that the Result was recorded."
    );
}

#[tokio::test]
async fn popup_renders_durable_state_through_the_real_broker_boundary() {
    let state = tempfile::tempdir().expect("state directory");
    let coordinator = Arc::new(
        Coordinator::open(state.path())
            .await
            .expect("Core must open"),
    );
    let CommandOutcome::SupervisorRegistered { capability, .. } = coordinator
        .execute(
            ActorContext::Bootstrap,
            CoordinatorCommand::RegisterSupervisor {
                definition: HarnessDefinitionV1 {
                    schema_version: SCHEMA_VERSION,
                    id: "supervisor".parse().expect("valid ID"),
                    kind: HarnessKind::Codex,
                    tier: HarnessTier::Supervisor,
                    cwd: PathBuf::from("/tmp/project"),
                    launch_profile: None,
                    model: None,
                },
            },
        )
        .await
        .expect("Supervisor registration")
    else {
        panic!("registration must return a capability")
    };
    let bearer = serde_json::to_value(capability)
        .expect("serialize capability")
        .as_str()
        .expect("transparent bearer")
        .to_owned();
    let socket = state.path().join("broker.sock");
    let server = BrokerServer::bind(coordinator, &socket)
        .await
        .expect("broker bind");
    let task = tokio::spawn(server.serve());

    let rendered = render_popup(&socket, bearer)
        .await
        .expect("popup projection");
    assert!(rendered.starts_with("Harness Network\n\n"));
    assert!(rendered.contains("supervisor"));
    assert!(rendered.contains("Tasks"));
    assert!(rendered.contains("Scheduling"));

    task.abort();
}

#[test]
fn dashboard_sorts_attention_workers_first_and_keeps_selection_by_harness_id() {
    let workers = vec![
        HarnessStatusView {
            id: HarnessId::from_str("idle-worker").expect("valid Worker ID"),
            tier: HarnessTier::Worker,
            presence: "online".to_owned(),
            activity: "idle".to_owned(),
            unread_messages: 0,
            active_task_id: None,
        },
        HarnessStatusView {
            id: HarnessId::from_str("attention-worker").expect("valid Worker ID"),
            tier: HarnessTier::Worker,
            presence: "online".to_owned(),
            activity: "waiting".to_owned(),
            unread_messages: 1,
            active_task_id: None,
        },
    ];
    let dashboard = PopupDashboard::from_projections(&workers, &[], &[], &[]);
    let mut selection = PopupSelection::default();
    selection.select_next(&dashboard);
    selection.retain_worker(&dashboard);

    assert_eq!(
        dashboard.worker_ids().collect::<Vec<_>>(),
        vec!["attention-worker", "idle-worker"]
    );
    assert_eq!(
        selection.selected_worker_id().map(HarnessId::as_str),
        Some("attention-worker")
    );
}

#[test]
fn dashboard_renders_wide_list_detail_layout_with_fixed_controls() {
    let workers = vec![HarnessStatusView {
        id: HarnessId::from_str("waiting-worker").expect("valid Worker ID"),
        tier: HarnessTier::Worker,
        presence: "online".to_owned(),
        activity: "waiting".to_owned(),
        unread_messages: 1,
        active_task_id: None,
    }];
    let dashboard = PopupDashboard::from_projections(&workers, &[], &[], &[]);
    let mut selection = PopupSelection::default();
    selection.select_next(&dashboard);
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).expect("Test backend terminal");

    terminal
        .draw(|frame| draw_popup_dashboard(frame, &dashboard, &selection, None, None))
        .expect("dashboard must render");

    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Workers"));
    assert!(rendered.contains("Worker detail"));
    assert!(rendered.contains("[a] Approve"));
}

#[test]
fn dashboard_renders_selected_worker_detail_on_a_narrow_terminal() {
    let workers = vec![HarnessStatusView {
        id: HarnessId::from_str("offline-worker").expect("valid Worker ID"),
        tier: HarnessTier::Worker,
        presence: "offline".to_owned(),
        activity: "idle".to_owned(),
        unread_messages: 0,
        active_task_id: None,
    }];
    let dashboard = PopupDashboard::from_projections(&workers, &[], &[], &[]);
    let mut selection = PopupSelection::default();
    selection.select_next(&dashboard);
    selection.show_detail();
    let backend = TestBackend::new(70, 16);
    let mut terminal = Terminal::new(backend).expect("Test backend terminal");

    terminal
        .draw(|frame| {
            draw_popup_dashboard(
                frame,
                &dashboard,
                &selection,
                Some("broker unavailable"),
                None,
            );
        })
        .expect("dashboard must render");

    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Worker detail"));
    assert!(rendered.contains("STALE"));
    assert!(rendered.contains("broker unavailable"));
}

#[test]
fn dashboard_preserves_selected_worker_when_attention_reorders_rows() {
    let initial = PopupDashboard::from_projections(
        &[
            harness_status("alpha-worker", "idle", 0, None),
            harness_status("beta-worker", "idle", 0, None),
        ],
        &[],
        &[],
        &[],
    );
    let mut selection = PopupSelection::default();
    selection.select_next(&initial);
    selection.select_next(&initial);
    let reordered = PopupDashboard::from_projections(
        &[
            harness_status("alpha-worker", "waiting", 1, None),
            harness_status("beta-worker", "idle", 0, None),
        ],
        &[],
        &[],
        &[],
    );

    selection.retain_worker(&reordered);

    assert_eq!(
        selection.selected_worker_id().map(HarnessId::as_str),
        Some("beta-worker")
    );
}

#[test]
fn dashboard_actions_target_the_selected_active_task_and_terminal_hold() {
    let active_task = task_graph(
        "019f8d00-0000-7000-8000-000000000001",
        "active-worker",
        "Active implementation",
        TaskState::Working,
    );
    let failed_task = task_graph(
        "019f8d00-0000-7000-8000-000000000002",
        "held-worker",
        "Failed mutation",
        TaskState::Failed,
    );
    let hold = HoldView {
        id: WorktreeHoldId(uuid("019f8d00-0000-7000-8000-000000000003")),
        repository_key: "/repo".to_owned(),
        task_id: failed_task.task.id,
        reason: "worker host lost".to_owned(),
    };
    let dashboard = PopupDashboard::from_projections(
        &[
            harness_status("active-worker", "working", 0, Some(active_task.task.id)),
            harness_status("held-worker", "failed", 0, None),
        ],
        &[active_task.task.clone(), failed_task.task.clone()],
        &[active_task.clone(), failed_task],
        &[hold],
    );
    let mut selection = PopupSelection::default();
    selection.retain_worker(&dashboard);

    assert_eq!(
        selection.selected_hold_task_id(&dashboard),
        Some(TaskId(uuid("019f8d00-0000-7000-8000-000000000002")))
    );
    selection.select_next(&dashboard);
    assert_eq!(
        selection.selected_task_id(&dashboard),
        Some(active_task.task.id)
    );
    assert_eq!(
        selection
            .selected_stoppable_worker_id(&dashboard)
            .map(HarnessId::as_str),
        Some("active-worker")
    );
}

#[test]
fn dashboard_renders_queued_titles_and_dependency_blockers() {
    let mut queued = task_graph(
        "019f8d00-0000-7000-8000-000000000004",
        "queued-worker",
        "Verify the dashboard",
        TaskState::Queued,
    );
    queued.worker_queue_position = Some(2);
    queued.waiting_for_repository = true;
    queued.dependencies.push(TaskDependencyView {
        task_id: TaskId(uuid("019f8d00-0000-7000-8000-000000000005")),
        condition: DependencyCondition::Approved,
        failure_policy: DependencyFailurePolicy::KeepBlocked,
        satisfied_by_result_revision: None,
    });
    let dashboard = PopupDashboard::from_dashboard(DashboardView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        generated_at: "2026-07-23T12:00:00Z".to_owned(),
        supervisor: dashboard_supervisor(),
        workers: vec![DashboardWorkerView {
            schema_version: DASHBOARD_SCHEMA_VERSION,
            id: "queued-worker".parse().expect("Worker ID"),
            kind: HarnessKind::Codex,
            launch_profile: None,
            model: None,
            session: None,
            unread_messages: 0,
            active_task: None,
            queued_tasks: vec![dashboard_task("Verify the dashboard", queued)],
            holds: Vec::new(),
            attention_events: Vec::new(),
        }],
        attention_events: Vec::new(),
    });
    let mut selection = PopupSelection::default();
    selection.retain_worker(&dashboard);

    let rendered = render_dashboard(&dashboard, &selection, None, None);

    assert!(rendered.contains("Verify the dashboard"));
    assert!(rendered.contains("waiting repository"));
    assert!(rendered.contains("dependency"));
}

#[test]
fn dashboard_renders_idle_worker_context_and_distinguishes_action_warnings() {
    let dashboard = PopupDashboard::from_dashboard(DashboardView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        generated_at: "2026-07-23T12:00:00Z".to_owned(),
        supervisor: DashboardSupervisorView {
            schema_version: DASHBOARD_SCHEMA_VERSION,
            id: "supervisor".parse().expect("Supervisor ID"),
            session_id: HarnessSessionId(uuid("019f8d00-0000-7000-8000-000000000006")),
            presence: HarnessPresence::Online,
            activity: HarnessActivity::Idle,
            unread_messages: 0,
            attention_count: 0,
            terminal_id: None,
            pane_id: None,
        },
        workers: vec![DashboardWorkerView {
            schema_version: DASHBOARD_SCHEMA_VERSION,
            id: "context-worker".parse().expect("Worker ID"),
            kind: HarnessKind::Codex,
            launch_profile: Some("codex-review".to_owned()),
            model: Some("gpt-5.6-sol".to_owned()),
            session: Some(DashboardSessionView {
                schema_version: DASHBOARD_SCHEMA_VERSION,
                id: HarnessSessionId(uuid("019f8d00-0000-7000-8000-000000000007")),
                presence: HarnessPresence::Online,
                activity: HarnessActivity::Idle,
                native_health: NativeSessionHealth::Healthy,
                context_tokens: Some(42_000),
                context_window: Some(100_000),
                context_percent: Some("42".to_owned()),
                compaction_count: Some(0),
                context_observed_at: Some("2026-07-23T11:59:59Z".to_owned()),
                last_seen_at: "2026-07-23T12:00:00Z".to_owned(),
                terminal_id: Some("terminal-context".to_owned()),
                pane_id: Some("w1:p2".to_owned()),
                last_activity: None,
            }),
            unread_messages: 0,
            active_task: None,
            queued_tasks: Vec::new(),
            holds: Vec::new(),
            attention_events: Vec::new(),
        }],
        attention_events: Vec::new(),
    });
    let mut selection = PopupSelection::default();
    selection.retain_worker(&dashboard);

    let rendered = render_dashboard(
        &dashboard,
        &selection,
        None,
        Some("focus restoration failed"),
    );

    assert!(rendered.contains("Context  42%"));
    assert!(rendered.contains("Action warning: focus restoration failed"));
    assert!(!rendered.contains("STALE"));
}

#[test]
fn dashboard_orders_a_mixed_worker_network_without_native_child_rows() {
    let held_task = task_graph(
        "019f8d00-0000-7000-8000-000000000010",
        "held-worker",
        "Held mutation",
        TaskState::Failed,
    );
    let hold = HoldView {
        id: WorktreeHoldId(uuid("019f8d00-0000-7000-8000-000000000011")),
        repository_key: "/repo".to_owned(),
        task_id: held_task.task.id,
        reason: "worker host lost".to_owned(),
    };
    let dashboard = PopupDashboard::from_dashboard(DashboardView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        generated_at: "2026-07-23T12:00:00Z".to_owned(),
        supervisor: dashboard_supervisor(),
        workers: vec![
            dashboard_worker(
                "active-worker",
                "working",
                Some(TaskState::Working),
                Vec::new(),
            ),
            dashboard_worker(
                "queued-worker",
                "idle",
                None,
                vec![dashboard_task(
                    "Queued verification",
                    task_graph(
                        "019f8d00-0000-7000-8000-000000000012",
                        "queued-worker",
                        "Queued verification",
                        TaskState::Queued,
                    ),
                )],
            ),
            dashboard_worker(
                "waiting-worker",
                "waiting",
                Some(TaskState::Waiting),
                Vec::new(),
            ),
            dashboard_worker(
                "review-worker",
                "reviewing",
                Some(TaskState::Reviewing),
                Vec::new(),
            ),
            dashboard_worker("offline-worker", "offline", None, Vec::new()),
            DashboardWorkerView {
                holds: vec![hold],
                ..dashboard_worker("held-worker", "failed", None, Vec::new())
            },
        ],
        attention_events: Vec::new(),
    });

    assert_eq!(
        dashboard.worker_ids().collect::<Vec<_>>(),
        vec![
            "held-worker",
            "review-worker",
            "waiting-worker",
            "active-worker",
            "queued-worker",
            "offline-worker",
        ]
    );
    assert!(!dashboard.worker_ids().any(|id| id.contains("child")));
}

fn harness_status(
    id: &str,
    activity: &str,
    unread_messages: u32,
    active_task_id: Option<TaskId>,
) -> HarnessStatusView {
    HarnessStatusView {
        id: id.parse().expect("valid Worker ID"),
        tier: HarnessTier::Worker,
        presence: "online".to_owned(),
        activity: activity.to_owned(),
        unread_messages,
        active_task_id,
    }
}

fn task_graph(id: &str, worker_id: &str, _title: &str, state: TaskState) -> TaskGraphView {
    TaskGraphView {
        task: TaskView {
            id: TaskId(uuid(id)),
            worker_id: worker_id.parse().expect("valid Worker ID"),
            state,
            result_revision: 0,
            task_role: TaskRole::Other,
            requested_session_policy: SessionReusePolicy::Auto,
            effective_session_policy: None,
            harness_session_id: None,
            session_reused: None,
            session_decision_reason: None,
            context_percent: None,
        },
        scheduling_state: TaskSchedulingState::Ready,
        dependencies: Vec::new(),
        dependents: Vec::new(),
        worker_queue_position: None,
        waiting_for_worker: false,
        waiting_for_session: false,
        waiting_for_repository: false,
    }
}

fn dashboard_task(title: &str, graph: TaskGraphView) -> DashboardTaskView {
    DashboardTaskView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        title: title.to_owned(),
        task: graph.task,
        scheduling_state: graph.scheduling_state,
        dependencies: graph.dependencies,
        dependents: graph.dependents,
        worker_queue_position: graph.worker_queue_position,
        waiting_for_worker: graph.waiting_for_worker,
        waiting_for_session: graph.waiting_for_session,
        waiting_for_repository: graph.waiting_for_repository,
    }
}

fn dashboard_supervisor() -> DashboardSupervisorView {
    DashboardSupervisorView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        id: "supervisor".parse().expect("Supervisor ID"),
        session_id: HarnessSessionId(uuid("019f8d00-0000-7000-8000-000000000009")),
        presence: HarnessPresence::Online,
        activity: HarnessActivity::Idle,
        unread_messages: 0,
        attention_count: 0,
        terminal_id: None,
        pane_id: None,
    }
}

fn dashboard_worker(
    id: &str,
    activity: &str,
    active_state: Option<TaskState>,
    queued_tasks: Vec<DashboardTaskView>,
) -> DashboardWorkerView {
    let session = (activity != "offline").then(|| DashboardSessionView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        id: HarnessSessionId::new(),
        presence: HarnessPresence::Online,
        activity: match activity {
            "working" => HarnessActivity::Working,
            "waiting" => HarnessActivity::Waiting,
            "reviewing" => HarnessActivity::Reviewing,
            "failed" => HarnessActivity::Failed,
            _ => HarnessActivity::Idle,
        },
        native_health: NativeSessionHealth::Healthy,
        context_tokens: None,
        context_window: None,
        context_percent: None,
        compaction_count: None,
        context_observed_at: None,
        last_seen_at: "2026-07-23T12:00:00Z".to_owned(),
        terminal_id: None,
        pane_id: None,
        last_activity: None,
    });
    let active_task = active_state.map(|state| {
        dashboard_task(
            "Active work",
            task_graph(&TaskId::new().to_string(), id, "Active work", state),
        )
    });
    DashboardWorkerView {
        schema_version: DASHBOARD_SCHEMA_VERSION,
        id: id.parse().expect("Worker ID"),
        kind: HarnessKind::Codex,
        launch_profile: None,
        model: None,
        session,
        unread_messages: 0,
        active_task,
        queued_tasks,
        holds: Vec::new(),
        attention_events: Vec::new(),
    }
}

fn uuid(value: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(value).expect("valid UUID")
}

fn render_dashboard(
    dashboard: &PopupDashboard,
    selection: &PopupSelection,
    stale_error: Option<&str>,
    warning: Option<&str>,
) -> String {
    let backend = TestBackend::new(120, 28);
    let mut terminal = Terminal::new(backend).expect("Test backend terminal");
    terminal
        .draw(|frame| {
            draw_popup_dashboard(frame, dashboard, selection, stale_error, warning);
        })
        .expect("dashboard must render");
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}
