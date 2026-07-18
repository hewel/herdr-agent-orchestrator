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
| `related_task_id` | optional existing Task visible to the Supervisor |
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

The Coordinator generates the Task and root Task-message IDs in one transaction. A Task is queued only after its Worker, Attachments, repository identity, and public shape validate.

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

One Worker may have one Task in `dispatching`, `working`, `waiting`, `reviewing`, or `cancelling`. Later Tasks stay FIFO `queued`.

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

`steer` is accepted only for a Supervisor Correction or Notification addressed to the Worker actively executing that Task while the adapter reports a steerable turn. All other messages use `follow_up`.

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

## Attachments

Attachment admission:

1. opens the supplied path without following a final symlink;
2. requires a regular readable file within configured size limits;
3. streams it into a run-owned temporary file while computing SHA-256;
4. fsyncs and atomically renames it into the Attachment store;
5. persists identity, digest, byte size, media type, original name, and storage-relative path; and
6. returns the generated Attachment ID.

After admission, messages reference only Attachment IDs. Missing or digest-mismatched stored files are durable corruption and block delivery. Automatic garbage collection is deferred.

## Command idempotency

Every mutating command may carry `request_key`. The Coordinator persists:

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
