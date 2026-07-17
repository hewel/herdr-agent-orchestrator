# Herdr Agent Orchestrator

## Project direction

- Build a Rust Herdr plugin that runs, supervises, and coordinates coding agents in normal Herdr-managed terminal panes.
- The parent agent retains ownership of intent, architecture, decomposition, acceptance criteria, verification design, final diff review, and the user-facing response.
- Child agents receive bounded tasks with resolved requirements, declared write scopes, and objectively verifiable completion criteria.
- Treat the architecture supplied during repository initialization as the product source of truth. Keep this guide concise; do not duplicate the full design here.

## Architectural boundaries

- Keep provider, role, task, and policy independent. OMP and Codex are providers, not hard-coded responsibilities.
- Hide provider-specific protocols behind a shared Rust adapter interface. Shared runtime code consumes normalized events and structured artifacts, not native protocol messages.
- Prompts describe role behavior; execution policies enforce permissions. Read-only and write-scope rules must be runtime constraints, not prompt-only requests.
- Exchange structured run specifications, reports, and handoff artifacts instead of relying on cross-provider conversation history.
- The child-agent pane owns the real process. Popups display and control runs but must not own agent lifecycles.
- Official Herdr integrations remain authoritative for native agent identity and lifecycle; this plugin adds task, role, policy, and verification metadata.

## Repository safety

- Inspect and preserve the user's existing repository state before every editing run.
- Capture a Git baseline, validate declared write scopes, and acquire a repository-specific editing lock before execution.
- Permit only one editing agent per worktree. Read-only verification may run concurrently when it cannot interfere with edits.
- Compare final state to the baseline and invalidate out-of-scope changes. Never automatically revert, merge, or discard unexpected modifications.
- Reject unresolved architecture choices in child tasks; return them to the parent agent for a decision.

## MVP boundaries

- Require explicit provider and role selection. Initial providers are OMP and Codex; built-in roles are implementer, reviewer, and verifier.
- Include structured run specs and artifacts, repository guards, verification commands, normalized lifecycle events, persistent JSON state, Herdr metadata, a Ratatui popup, and cancel/focus/inspect controls.
- Keep role and policy concepts in the domain model from the start, even when initial role definitions are built in.
- Defer automatic routing, user-defined roles, inheritance, workflow DAGs, provider switching, multiple editing agents, automatic worktrees/merge/rollback, additional providers, graphical or web UIs, distributed workers, and deep recursive delegation.

## Implementation order

1. Define `AgentRunSpec`, task packets, assignments, policies, events, and structured artifacts.
2. Implement role/policy resolution, persistent state, Git baselines, write-scope validation, repository locking, and verification.
3. Add the Herdr command/socket boundary and complete one end-to-end OMP run.
4. Add Codex App Server support only after the shared runtime and OMP path are stable.
5. Add normalized presentation, popup controls, inspection, cancellation, and the reviewer/verifier flows.

The first milestone must prove the full OMP path from parent submission through a bounded edit, verification, scope validation, structured artifact, popup result, and parent review.

## Rust and persistence conventions

- Use async Rust with Tokio and provider traits with `async-trait`; use Serde-backed versioned JSON/TOML types at process and disk boundaries.
- Use the Git CLI initially rather than `git2`.
- Store run data beneath `HERDR_PLUGIN_STATE_DIR`; use atomic replacement for frequently updated JSON and append-only JSONL for events.
- Keep provider protocol types inside provider modules and keep Herdr transport details behind the Herdr integration boundary.
- Prefer focused tests around domain validation, state transitions, repository guards, protocol translation, cancellation, and artifact construction.

## Change discipline

- Implement vertical slices in the recommended order; do not scaffold deferred subsystems speculatively.
- Preserve public and persisted schemas deliberately with explicit `schema_version` fields.
- Keep verification commands declared in the task packet and report their command, exit status, pass/fail result, and concise evidence.
- When runtime behavior matters, validate through the real process or Herdr boundary rather than treating compilation alone as completion.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues, and external pull requests are also a triage request surface. See `docs/agents/issue-tracker.md`.

### Triage labels

The repository uses the five default triage labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository. See `docs/agents/domain.md`.
