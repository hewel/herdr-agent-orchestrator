# Coordinator public contract

Status: resolved for the Harness Coordinator MVP.

This contract defines the versioned caller boundary for Harness definitions, Tasks, messages, Results, receipts, identity, authorization, and lifecycle commands. It is normative for the CLI, MCP tools, provider host-tool bridges, popup commands, and persisted public values.

## Contract principles

- Public inputs contain caller intent; the Coordinator generates message, Task, Session, attempt, and Attachment identities.
- Every public object has `schema_version: 1`, is closed to unknown fields, and uses explicit enum values.
- The broker derives sender identity from a live Session capability. No public message input has an authoritative `from` field.
- Native acceptance, Task processing, Result submission, and Supervisor Approval are separate facts.
- Request retries use an optional idempotency key and never create a second accepted command for the same actor, operation kind, and canonical payload.
- Timestamps are UTC RFC 3339 with microsecond precision. Durable ordering uses database sequence numbers, not wall-clock order.

## Identity

### Harness IDs

A Harness ID is a user-selected immutable lowercase ASCII slug:

- 1 to 64 bytes;
- begins with `a-z`;
- continues with `a-z`, `0-9`, or single `-` separators;
- has no leading, trailing, or repeated separator; and
- is unique for the lifetime of one Coordinator state directory.

The reserved default Supervisor ID is `supervisor`. An archived or stopped Harness ID is not reusable for a different kind, tier, repository, or profile.

### Generated IDs

The Coordinator generates UUIDv7 values for Harness Sessions, Tasks, messages, delivery attempts, Attachments, Repository Observations, and Worktree Holds.

Native session IDs, Herdr terminal IDs, and pane IDs are opaque strings in separate fields and never substitute for Coordinator identity.

### Session capabilities

Every registered Session receives a random high-entropy capability scoped to its Harness ID, Session ID, tier, and connection generation. The broker derives the actor from that capability. Expired, stopped, replaced, or mismatched capabilities are rejected.

The MCP proxy retains its capability in process memory. A CLI fallback may use a mode-`0600` capability file created for the current Herdr pane. This prevents ordinary cross-session mistakes but is not a security boundary against a malicious same-user process.

## Harness admission

`HarnessDefinitionV1` contains:

| Field | Rule |
| --- | --- |
| `schema_version` | exactly `1` |
| `id` | valid immutable Harness ID |
| `kind` | `omp` or `codex` |
| `tier` | `supervisor` or `worker` |
| `cwd` | absolute canonical path; for Workers it must be the canonical Git worktree root and becomes the registered repository root |
| `launch_profile` | required nonempty profile identifier for Workers; optional for Supervisor |
| `model` | optional nonempty display or launch value |

The first Supervisor registration succeeds only when no active Supervisor exists. Re-registration of the same durable Harness creates a new Session only after the previous Session is stopped or proven disconnected.

Worker `start` creates the durable Harness when absent or requires every immutable field to match when present. Starting an already-online Worker is idempotent and returns its active Session.

The Coordinator launches Workers. `register` cannot adopt an arbitrary Worker process in v1.

## Task submission

`TaskSubmissionV1` contains:

| Field | Rule |
| --- | --- |
| `schema_version` | exactly `1` |
| `request_key` | optional 1-128 Unicode scalar idempotency key, additionally limited to 512 UTF-8 bytes by typed validation |
| `worker_id` | existing Worker Harness |
| `related_task_id` | optional existing Task used only for UI grouping, audit context, related-review repository rules, and Session reuse preference; it does not control scheduling |
| `depends_on` | at most 32 immutable scheduling dependencies; omitted by legacy v1 payloads as an empty array |
| `task_role` | required `implementation`, `investigation`, `review`, `verification`, or `other`; semantic purpose used by conservative automatic Session selection |
| `session_reuse` | required `required`, `prefer`, `fresh`, or `auto`; requested relationship between the Task and a native Worker Session |
| `preferred_session_id` | optional existing Coordinator Harness Session; mandatory when `session_reuse` is `required` |
| `title` | 1-160 Unicode scalar values |
| `instructions` | 1-16,384 Unicode scalar values, additionally limited to 65,536 UTF-8 bytes by typed validation |
| `attachments` | at most 32 admitted Attachment IDs |
| `repository` | required repository authority |

Repository authority contains an absolute canonical `root`, `access`, and `write_scopes`.

- `root` must equal the canonical Git worktree root registered for the Worker.
- `read_only` requires an empty `write_scopes` array.
- `mutating` requires at least one scope.
- A scope has explicit `exact_file` or `subtree` kind and one normalized repository-relative path.
- Paths reject empty components, `.`, `..`, absolute forms, backslashes, NUL, repeated separators, trailing separators, repository metadata, nested repositories, submodules, and symlink escape.
- Duplicate scopes and an exact file contained by a declared subtree are rejected. Nested subtrees are normalized to the broader scope.

Each `depends_on` entry contains an existing upstream `task_id`, a `condition`, and an optional `failure_policy` that defaults to `cancel`.

- `result_ready` requires a valid Result, settled native top-level turn, `reviewing` state, and no Worktree Hold. The exact satisfying Result revision is recorded.
- `approved` requires explicit Supervisor Approval and records the approved Result revision.
- `cancel` cancels an undispatched dependent if the upstream Task fails or is cancelled.
- `keep_blocked` preserves the dependent for explicit Supervisor reconciliation.

The Coordinator rejects a missing upstream Task, self-dependency, repeated upstream Task, unsupported condition, cycle, cross-state-directory dependency, repository-authority mismatch, or dependency-edge mutation after dispatch. Validation, including defensive topological cycle validation, completes before any Task or edge is committed. Dependencies do not select Workers, generate Tasks, or authorize direct Worker-to-Worker messages.

The Coordinator generates the Task and root Task-message IDs in one transaction. A Task and all dependency edges persist only after its Worker, Attachments, repository identity, graph, and public shape validate.

## Task scheduling contract

Scheduling readiness is distinct from the Task execution lifecycle:

```text
blocked
ready
```

`blocked` means at least one declared condition is unsatisfied. `ready` means all conditions are satisfied, but it does not promise immediate dispatch. A Task with no dependencies is immediately `ready`. Worker capacity, per-Worker FIFO position, repository leases, and Worktree Holds remain independent admission gates.

The Coordinator reevaluates affected Tasks when Tasks and Results change state, a Hold is created or cleared, a Worker becomes available or reconnects, or repository authority becomes available. Readiness and downstream failure transitions are transactional and idempotent. Several Tasks may become Ready in one reevaluation; no explicit branch queues are created.

Ready Tasks for one Worker retain Task creation order. A later Task cannot bypass the earliest Ready Task if that earlier Task is waiting for Worker or repository admission. Ready Tasks assigned to separate idle Workers may dispatch concurrently. A Blocked Task never acquires a repository lease.

For every satisfied dependency, the Coordinator creates or reuses an immutable Result snapshot Attachment and records the satisfying Result revision. When the dependent dispatches, its dependency inputs are frozen to those revisions. A pre-dispatch Correction or new Worktree Hold revokes provisional `result_ready` satisfaction and reblocks the dependent; a later revision may satisfy it again. Once dispatched, a newer upstream revision never mutates or replays downstream work automatically.

## Task Session reuse and binding

Every Task declares `task_role` and `session_reuse`. Session selection runs after dependency readiness and per-Worker FIFO admission and before dispatch. Dependency edges never encode reuse, and reuse never reorders the graph or bypasses repository admission.

`auto` resolves deterministically from `task_role` and declared relationships before any candidate is inspected:

- `review` and `verification` resolve to `fresh`;
- `implementation` with a `related_task_id` or at least one `depends_on` edge resolves to `prefer`;
- every other combination resolves to `fresh`.

Candidate preference order is the `preferred_session_id` Session, then the Session last bound to `related_task_id`, then the most recent live Session of the assigned Worker. A candidate is reusable only when every identity and safety check passes: same Worker definition, Harness Kind, launch-profile snapshot digest, canonical repository, effective tool policy, and compatible effective model; Session online and idle; and no active Task, unresolved Question, ambiguous delivery, unresolved cancellation, or unresolved Worktree Hold attributable to that Session.

Native health is consulted only after identity and policy compatibility. `failed`, `ambiguous`, and `context_pressure` reject the candidate. `compacted` rejects automatic reuse (`auto` starts fresh) but satisfies an explicit `required` or `prefer` request. `healthy` accepts.

`fresh` accepts a live Session that has never been bound to another Task; a previously bound Session is never reused under `fresh`. A rejected candidate blocks dispatch: `invalid_state` for `required`, otherwise `target_offline`, and the Task remains queued until a compatible Session exists. The Coordinator never silently binds an incompatible Session.

The decision persists as a durable Task Session Binding recording the Task, Harness Session, requested and effective policy, whether native context was reused, a stable machine-readable reason code, a human-readable reason, the Adapter snapshot and context evidence at decision time, and the binding time. At most one current binding exists per Task. Bindings survive restart and are revalidated at dispatch against live Session state.

## Task state contract

Allowed states are:

```text
queued
dispatching
working
waiting
reviewing
cancelling
delivery_unknown
approved
cancelled
failed
```

Only the Coordinator writes state transitions. Commands with an invalid source state fail without side effects.

Key rules:

- `queued → dispatching` requires an online idle Worker and eligible repository state.
- `dispatching → working` requires adapter acceptance.
- `dispatching → delivery_unknown` occurs when provider acceptance cannot be proved or disproved.
- `working → waiting` requires an accepted blocking Question from the assigned Worker.
- `waiting → working` requires the correlated Supervisor Reply to be natively accepted.
- `working → reviewing` requires one valid Result for the current revision and terminal native-turn evidence.
- `reviewing → working` requires a Supervisor Correction accepted by the same Worker.
- `reviewing → approved` requires Supervisor Approval and a matching current Repository Observation.
- Terminal failure or cancellation after mutating dispatch creates or preserves a Worktree Hold.

One Worker may have one Task in `dispatching`, `working`, `waiting`, `reviewing`, or `cancelling`. Ready Tasks wait in FIFO order until the active slot and repository admission are available. `delivery_unknown` never satisfies an edge and never triggers automatic replay.

## Message admission

`MessageSubmissionV1` contains:

| Field | Rule |
| --- | --- |
| `schema_version` | exactly `1` |
| `request_key` | optional idempotency key |
| `to` | existing permitted Harness ID |
| `task_id` | required except for network-level Notification |
| `kind` | `question`, `reply`, `correction`, or `notification` |
| `text` | 1-16,384 Unicode scalar values, additionally limited to 65,536 UTF-8 bytes by typed validation |
| `attachments` | at most 32 admitted Attachment IDs |
| `reply_to` | required only for Reply and references an unanswered Question in the same Task |
| `delivery` | optional `follow_up` or `steer`; omission resolves to `follow_up` before persistence |

Task and Result are reserved broker-created message kinds. A generic send cannot create them.

The route matrix is enforced before persistence:

- Supervisor to assigned Worker: Reply, Correction, Notification;
- Worker to Supervisor: Question, Notification;
- Worker to Worker: always rejected;
- a Result is created only by `CompleteTask`; and
- a Task message is created only by `CreateTask`.

`steer` is accepted for a Supervisor Correction or Notification addressed to the Worker actively executing that Task while the adapter reports a steerable turn. A Worker blocking Question may request Supervisor steering only when it supplies a bounded `steer_reason` explaining why the answer invalidates active Supervisor work; ordinary Questions and all Results use `follow_up`.

A Question is always blocking in v1 and moves the Task to `waiting` after message persistence. A nonblocking inquiry must use Notification and does not reserve Reply correlation.

Exactly one Reply may answer a blocking Question. A repeated idempotent Reply returns the original outcome; another payload is rejected.

## Result admission

`ResultManifestV1` is accepted only from the assigned Worker Session for its current Task and native turn revision. It contains:

- matching `task_id`;
- nonempty summary up to 16,384 Unicode scalar values and 65,536 UTF-8 bytes;
- unique normalized repository-relative `changed_files`;
- one or more verification entries with command, exit code, pass/fail, and evidence Attachment;
- deviations and risks as bounded strings; and
- at most 32 Attachments.

The broker accepts at most one Result per native turn. It validates structure immediately but does not transition to `reviewing` until the adapter reports terminal turn completion. A terminal turn without a valid Result fails the Task. A Result followed by native failure remains evidence but does not become reviewable.

`changed_files` is Worker-reported evidence. The Repository Observation comparison is independently authoritative for scope reporting.

## Approval and Worktree Hold commands

Approval is Supervisor-only and requires Task state `reviewing`, the current Result revision, no active related read-only Task, no unresolved delivery uncertainty, no Worktree Hold, and the expected current Repository Observation digest.

Approval records the accepted Result revision and observation atomically, completes the Task, and releases its Advisory Worktree Lease.

Worktree Hold clearance is Supervisor-only and requires the current observation digest plus a nonempty audit note. A stale digest fails. Clearance never edits files. For a reviewable Task it restores review eligibility; for a terminal failed or cancelled Task it releases the retained lease.

## Delivery attempts and receipts

Every message has one current `DeliveryReceiptV1` and one or more immutable attempts.

Receipt states are:

```text
pending
dispatching
accepted
retryable_failed
permanent_failed
unknown
cancelled
```

An attempt records generated attempt ID and number, target Harness and Session, delivery intent, timestamps, whether provider bytes may have been written, optional native correlation, adapter acceptance evidence, and typed diagnostics.

Offline delivery stays `pending`. A definitive failure before provider bytes were written may become `retryable_failed` and retry with bounded exponential backoff. Any loss after bytes may have been accepted becomes `unknown`. `unknown` never retries automatically.

Reading an inbox records `read_at` independently of native delivery state.

## Supervisor events and the managed Supervisor Host

The exact durable-Inbox rule: every Supervisor-attention fact is persisted as a durable Supervisor Event in the same transaction as the fact that raised it, before any native injection is attempted. The durable event store, not the native Supervisor conversation, is the system of record. An event raised by an inbox Message records that `source_message_id`; acknowledging the event marks the source Message read in the same transaction.

Event kinds are `result_ready`, `blocking_question`, `task_failed`, `delivery_unknown`, `worktree_hold_created`, `task_graph_completed`, and `notification`. Each event carries a unique `source_key`; re-emission of the same fact inserts nothing, so retry and reevaluation never duplicate attention.

Event delivery states are:

```text
pending
dispatching
accepted
processed
unknown
cancelled
```

The Supervisor Host claims the oldest `pending` event first and never holds more than one event in `dispatching` or `accepted`. Each claim appends an immutable attempt recording the target Session, whether provider bytes may have been written, native correlation, and acceptance evidence. `accepted` means the provider accepted the injection; it is not model processing. `processed` means the Supervisor explicitly acknowledged the event (one to thirty-two IDs per acknowledgement) or a superseding durable fact settled it. A lost acknowledgement after bytes may have been written becomes `unknown`; `unknown` is never retried automatically and settles only through explicit Supervisor reconciliation (`retry`, `processed`, or `cancel`) with a nonempty audit note.

When the Supervisor runs as a managed Harness in a Coordinator-launched pane, a Supervisor Host owns one Supervisor Adapter that binds the visible native conversation, records its native session and thread identity, injects events as follow-up or steer according to their delivery intent, and snapshots lifecycle and native health. A self-registered unmanaged Supervisor keeps the pull model: events remain durable inbox state surfaced through Herdr metadata and the popup, and the Coordinator injects nothing.

Offline recovery is durable by construction. Pending events survive broker, Host, and Supervisor restarts and are claimed in creation order once a Supervisor Host binds again. A cold Herdr restart never replays uncertain injections: events stay `pending` or `unknown` until the Supervisor reconciles them.

`WatchTaskGraph` registers a durable, idempotent completion watch on an explicit root Task set. When every watched root reaches `reviewing` or a terminal state, the Coordinator completes the watch once and emits one `task_graph_completed` event.

## Attachments

Attachment admission:

1. opens the supplied path without following a final symlink;
2. requires a regular readable file within configured size limits;
3. streams it into a run-owned temporary file while computing SHA-256;
4. fsyncs and atomically renames it into the Attachment store;
5. persists identity, digest, byte size, media type, original name, and storage-relative path; and
6. returns the generated Attachment ID.

After admission, messages reference only Attachment IDs. Missing or digest-mismatched stored files are durable corruption and block delivery. Automatic garbage collection is deferred.

Explicit Task Attachments and dependency Result snapshots are resolved separately. The dependent Worker receives immutable Attachment references and upstream Task and Result revision metadata; the Coordinator never inlines a large upstream Result into Task text.

## Task graph query

`harness_task_graph` is a Supervisor-only read query. For each Task it reports scheduling and execution state, dependency conditions and failure policies, satisfied Result revisions, direct dependents, Worker queue position, and whether Worker capacity or repository admission is the current wait. It does not expose database mutation or permit dependency editing.

## Dashboard and pane locations

`Dashboard` is a Supervisor-only transactional read query. One result contains the live Supervisor summary and every top-level Worker with its Harness Kind and model, latest live Session health and context observation, latest normalized Host activity, active and queued Task graph rows, unread count, unresolved Holds, relevant Supervisor Events, and durable terminal and pane identities. It never exposes provider-native child agents or derives state from terminal text.

The local broker envelope preserves operation set v1 unchanged. `Dashboard` and `RecordPaneLocation` require broker `schema_version = 2`; a v1 request for either returns `unsupported_version`, while all original v1 operations and response shapes remain available. Dashboard read-model objects carry their own closed `schema_version = 1`, independent from the broker envelope and the public Task/message/Result schemas. Task titles live only in the Dashboard Task projection, so the existing v1 `TaskView` is unchanged.

`RecordPaneLocation` binds a live Coordinator Session to the terminal and pane identities returned or resolved by Herdr. A Worker Host may update only its own Session. The Supervisor may update any Coordinator-managed live Session. Empty identities, duplicate live terminal bindings, ended Sessions, and Worker attempts to update another Session are rejected. Repeating an identical assignment is a convergent no-op, including its `last_seen_at` evidence. Pane-location recording is presentation evidence and does not change Task delivery, repository authority, or native acceptance state.

## Command idempotency

Every caller-intent mutation that can create a second durable effect may carry `request_key`. Host evidence commands instead use generation fencing, monotonic sequences, or convergent assignments. For request-keyed commands, the Coordinator persists:

```text
actor Harness ID + command kind + request key
→ canonical input digest + original outcome
```

The same key and digest returns the original outcome. The same key with another digest is rejected. Keys are scoped to the durable Harness, not one Session, so reconnect retries remain safe.

## Error classes

Public errors use stable categories:

```text
invalid_input
unauthenticated
forbidden
not_found
conflict
invalid_state
target_offline
unsupported_version
repository_blocked
delivery_unknown
storage_failure
adapter_failure
herdr_failure
```

Errors contain a concise message and optional durable evidence reference. Native protocol text is diagnostic and never becomes the public error category.

## Compatibility

Version 1 schemas never loosen or change meaning in place. Adding fields, enum values, defaults, identity rules, or state semantics creates a new public schema version. Persisted internal tables may migrate independently when public v1 behavior is preserved.

The accepted Session Reuse and Managed Supervisor Host plan made one deliberate exception: `TaskSubmissionV1` was replaced in place, adding required `task_role` and `session_reuse` plus optional `preferred_session_id`. Persisted pre-plan Tasks were migrated with the conservative defaults `other` and `auto`. This exception is settled history and sets no precedent for further in-place v1 changes.
