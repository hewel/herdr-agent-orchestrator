# Herdr Harness pane and plugin contract

Status: resolved for the Harness Coordinator MVP.

This contract defines Worker pane launch, Supervisor registration, live terminal identity, focus, metadata, cancellation, popup behavior, reconnect, and cold-restart handling. It targets the locally verified Herdr `0.7.4` socket protocol `16`.

## Confirmed Herdr facts

- A plugin is an out-of-process package with manifest-declared actions and pane entrypoints.
- `plugin.pane.open` launches a declared entrypoint. Normal tab panes receive Herdr workspace, tab, pane, socket, and plugin environment.
- A popup is session-modal, has no pane ID, is not present in pane APIs, and cannot own a Harness lifecycle.
- `session.snapshot` bootstraps workspace, tab, pane, terminal, focus, and agent state. Event subscriptions are required afterward, and reconnect requires a fresh snapshot.
- A pane has a public mutable `pane_id` and a live `terminal_id`. Moving a live terminal changes pane location while retaining terminal identity.
- `pane.report_agent` has one semantic status authority. `pane.report_metadata` changes presentation without owning lifecycle state.
- Detach and reattach preserve live processes. A cold Herdr server restart restores layout but loses the original processes.

The implementation targets the [Herdr socket API](https://herdr.dev/docs/socket-api/).

## Plugin entrypoints

The manifest uses Herdr `0.7.4` as a minimum feature floor, requires socket protocol `16`, and declares:

```text
worker          placement = "tab", focus = false
harness-network placement = "popup"
workspace       context = "workspace"
```

Herdr product releases newer than `0.7.4` are accepted while protocol `16` remains compatible. The workspace action targets the invoking workspace and calls idempotent desired-state operations; the Coordinator never maps workspace state to Herdr's global plugin enable/disable switch.

The Worker entrypoint receives a Coordinator Harness Session ID, connects to the local broker, validates the Session, and becomes the Harness Host that starts and owns OMP or Codex.

The popup receives no Harness identity from a pane. It connects as a presentation client and receives the selected durable Harness or Task ID through plugin-owned context.

## Supervisor registration

The Supervisor exists in two shapes. A managed Supervisor runs in a Coordinator-launched pane whose Supervisor Host binds the visible native conversation through a Supervisor Adapter and injects durable Supervisor Events as native follow-up or steer turns. A self-registered Supervisor registers through the Coordinator MCP proxy or CLI fallback as before.

Registration records its durable Harness ID, current Session ID, Harness Kind, model when known, cwd, terminal ID, current pane location, and last-seen sequence.

For a self-registered Supervisor the Coordinator does not adopt or become parent of the Supervisor process and cannot inject a native turn. For both shapes, Supervisor attention is durable inbox and Supervisor Event state first; native injection by a managed Host is an at-least-once optimization, never the system of record. Undelivered events are surfaced through metadata and popup state.

If the Supervisor pane moves, the Coordinator updates public pane, tab, and workspace IDs while retaining the Session and terminal binding. If the Supervisor disconnects, Workers continue and Results wait for review.

## Worker ownership

The plugin opens one normal pane per Worker Session. The Harness Host is the pane's real process owner and launches one native harness child. The native harness manages its own tools and child agents.

The official OMP or Codex integration may remain semantic status authority. The Coordinator does not call `pane.report_agent` for competing state; it records durable Task state in SQLite and publishes only presentation metadata.

Identity lifetimes are distinct:

| Identity | Meaning |
| --- | --- |
| Harness ID | durable mailbox and launch definition |
| Harness Session ID | one live Coordinator activation |
| native session/thread ID | provider conversation identity |
| `terminal_id` | stable binding to one live Herdr terminal |
| `pane_id` | current public pane location |
| workspace/tab IDs | mutable UI location |

## Status and metadata

The Harness Host emits monotonic Session events to the broker. SQLite is authoritative for Task and mailbox state. Herdr metadata is a projection rebuilt after reconnect.

Suggested metadata:

```text
title: OMP Worker
state: working
detail: download queue fix
inbox: 0
```

The popup derives its list and detail views from Coordinator queries, not screen scraping or metadata tokens.

## Focus

Focus resolves the stored `terminal_id` through a fresh or current snapshot, updates stale pane location, and calls `plugin.pane.focus` for the current plugin-owned Worker pane. A missing terminal marks the Session disconnected or failed before returning an error.

Supervisor focus uses its registered current pane but is not required to be plugin-owned.

## Cancellation and stop

Task cancellation and Harness stop are distinct:

- **Cancel Task** persists cancellation intent and asks the Harness Adapter to abort the active native turn. The durable Harness remains available.
- **Stop Harness** cancels active work when needed, stops the adapter and Harness Host, and closes the Worker pane. The durable Harness and mailbox remain.

Cancellation waits for the provider-specific cooperative sequence. If it exceeds the configured grace period, the Coordinator calls `plugin.pane.close`. Forced closure is recorded separately and creates Worktree Hold for a possibly mutating Task.

Closing the popup calls only `popup.close` and never cancels work.

## Broker reconnect

After broker restart, a live Harness Host reconnects with Session capability, terminal identity, and its latest monotonic event sequence. The broker:

1. loads durable Session and Task state;
2. takes a fresh `session.snapshot`;
3. proves the `terminal_id` still exists;
4. refreshes pane, tab, and workspace locations;
5. replays any missing sequenced host events;
6. resubscribes to Herdr events; and
7. republishes metadata.

The broker never infers message acceptance from pane presence. Adapter correlation and durable attempts remain authoritative.

## Pane loss and cold restart

Unexpected Worker pane loss fails the live Harness Session. A queued Task remains queued for a future Session. A dispatched Task becomes failed; a mutating Task enters Worktree Hold.

After a cold Herdr restart, original Supervisor and Worker processes are gone even if layout is restored. The Coordinator marks their Sessions disconnected or failed, preserves mail and evidence, and requires explicit Supervisor and Worker reactivation. It never adopts, resumes, or replays uncertain native work automatically.

## Popup

The popup is a viewer/controller over durable state. It supports:

- Harness and presence list;
- current and queued Tasks;
- inbox and message details;
- Result revisions and Attachments;
- Repository Observations and Worktree Holds;
- focus;
- send Reply, Correction, or Notification;
- approve or cancel Task;
- clear Hold with digest and note; and
- stop Worker Harness.

Controls are authorized through the active Supervisor capability. Worker or unauthenticated popup commands are rejected.

## Acceptance scenarios

- A Worker opens unfocused in a normal tab and publishes native status plus Coordinator metadata.
- Moving the pane changes public location without changing Harness Session identity.
- Focus works after resolving a stale pane ID through terminal identity.
- Broker restart reconnects a live Worker without replaying accepted messages.
- Cooperative cancellation uses the adapter; timeout escalation closes the pane and records a Hold.
- Closing the popup leaves every Harness running.
- Supervisor disconnection leaves Workers running and Results durable.
- Cold restart fails active Sessions, preserves mail, and performs no automatic native resume.
