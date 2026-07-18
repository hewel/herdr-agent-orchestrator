# Herdr Harness Coordinator

## Project direction

- Build a lightweight Rust Herdr plugin that coordinates autonomous top-level coding Harnesses in normal Herdr-managed panes.
- One Supervisor Harness owns intent, architecture, Task decomposition, Worker selection, corrections, approval, final verification, and the user-facing response.
- Worker Harnesses execute bounded Tasks and may use their native multi-agent systems; the Coordinator sees only their top-level identity, messages, lifecycle, and consolidated Result.
- Treat `docs/ARCHITECTURE.md` as the product source of truth and `CONTEXT.md` as the canonical vocabulary. Keep this guide concise.

## Architectural boundaries

- The Coordinator owns durable Harness identity, Task queues, star-topology messaging, delivery evidence, pane/process control, and advisory repository coordination. It is not a workflow engine.
- OMP and Codex are the MVP Harness Kinds. Pi follows the MVP; OpenCode is later.
- Hide native protocols behind the shared Rust Harness Adapter interface. Provider-specific frames and child-session details stay inside adapters and native logs.
- Require one active Supervisor per Coordinator state directory and one active Task per Worker. Reject Worker-to-Worker messaging.
- Use `docs/research/mvp/coordination-contract.md` as the normative public Task/message/Result boundary.
- The Worker pane owns the Harness Host and native process. The popup observes and controls durable state but never owns a Harness lifecycle.
- Worker events always enter the durable Supervisor inbox. A Coordinator-managed Supervisor Host may additionally inject safe native follow-up turns into the visible bound Session; an unmanaged, already-running Supervisor remains pull-notified through inbox, metadata, and popup only.

## Repository coordination

- Use `docs/research/mvp/advisory-worktree-contract.md` as the normative MVP repository boundary.
- Inspect and preserve existing staged, unstaged, and untracked state before every mutating Task.
- Permit only one mutating Task per canonical worktree and hold its Advisory Worktree Lease through Result review and Corrections until Supervisor Approval.
- Compare Git-visible changes with declared exact-file and subtree scopes, but describe this as advisory detection rather than sandbox enforcement.
- Create a Worktree Hold after uncertain, failed, cancelled, or out-of-scope mutation. Only digest-confirmed Supervisor reconciliation may clear it.
- Never automatically revert, reset, clean, merge, publish, or discard repository changes.

## MVP boundaries

- Require explicit Worker Harness and launch profile selection. Do not route Harnesses or models automatically.
- Include OMP and Codex adapters, persistent top-level sessions, durable SQLite mailboxes, strict versioned contracts, immutable Attachments, delivery receipts, Questions, Replies, Corrections, Results, Approval, Herdr metadata, popup controls, focus, cancellation, and Worker stop.
- Permit native multi-agent behavior but do not visualize, address, budget, or route native children.
- Defer peer messaging, workflow DAGs, automatic decomposition, multiple mutating Tasks per worktree, automatic worktrees, merge/rollback/publication, hostile-process isolation, universal artifacts, distributed brokers, and graphical or web UIs.

## Implementation order

1. Build the Coordinator Core, contracts, SQLite state, mailbox, Attachments, Task lifecycle, and advisory worktree coordination.
2. Prove the full Supervisor → OMP Worker → native multi-agent → Result → Correction or Approval path in real Herdr panes.
3. Prove the equivalent persistent-thread path for Codex.
4. Add MCP/host-tool bridges, metadata, popup inbox and Task views, focus, cancellation, stop, and Hold controls.
5. Add Pi only after both MVP paths are stable.

## Rust and persistence conventions

- Use async Rust with Tokio and `async-trait`; use Serde-backed versioned JSON/TOML types at process and disk boundaries.
- Keep the Coordinator Core as a deep command/query module. OMP and Codex justify the real Harness Adapter seam; avoid pass-through module layers.
- Use the Git CLI initially rather than `git2`.
- Store indexed runtime state in SQLite beneath `HERDR_PLUGIN_STATE_DIR`; store Attachments, transcripts, logs, diffs, and verification evidence as files.
- Keep native protocol types inside adapter modules and Herdr socket types inside the Herdr integration module.
- Prefer focused tests around schema and typed validation, Task transitions, route authorization, delivery ambiguity, adapter translation, cancellation, Repository Observations, leases, and Holds.

## Change discipline

- Implement vertical slices in the documented order and do not scaffold deferred subsystems.
- Preserve public and persisted contracts deliberately with explicit `schema_version` fields; never loosen v1 meaning in place.
- Preserve unrelated local edits and never normalize, revert, or discard user changes.
- Report verification command, exit status, pass/fail result, evidence, deviations, and risks in every Worker Result.
- Validate runtime behavior through the real provider and Herdr boundaries; compilation alone is not completion.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues, and external pull requests are also a triage request surface. See `docs/agents/issue-tracker.md`.

### Triage labels

The repository uses `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository. See `docs/agents/domain.md`.
