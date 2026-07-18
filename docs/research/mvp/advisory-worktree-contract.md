# Advisory worktree contract

Status: resolved for the Harness Coordinator MVP.

This contract defines the Coordinator's deliberately limited repository promise. Worker Harnesses operate directly in their configured live Git worktree. The Coordinator serializes its own mutating Tasks, records evidence, detects declared-scope drift, and stops scheduling when repository state is uncertain. It is not a sandbox, transaction, publisher, or rollback system.

## Guarantees and non-guarantees

The Coordinator guarantees that:

- it dispatches at most one mutating Task for one canonical worktree;
- it records the Git-visible baseline needed to compare later checkpoints;
- it preserves pre-existing staged, unstaged, and untracked user state as baseline;
- it compares baseline-to-checkpoint changed paths with declared write scopes;
- it retains the Advisory Worktree Lease through Result review and Corrections;
- abnormal or out-of-scope state creates a durable Worktree Hold; and
- it never automatically reverts, merges, resets, cleans, publishes, or discards files.

The Coordinator does not guarantee that:

- a Worker or native child cannot write outside declared scope;
- another same-user process cannot edit the worktree;
- a same-scope edit can be attributed to the Worker rather than the user;
- ignored-file content is completely observed;
- commands, network, credentials, processes, or tools are isolated; or
- a failed Task leaves a reversible candidate.

The UI and Result must call this coordination **advisory** and must not imply sandbox enforcement.

## Repository identity

Before accepting a Task, the Coordinator uses sanitized Git CLI commands to resolve canonical worktree root, Git common-directory identity, main or linked-worktree identity, current `HEAD` or unborn state, and submodule or nested-repository boundaries.

The MVP rejects bare repositories, non-Git directories, write scopes entering `.git`, submodules, nested repositories, and worktree roots that differ from the selected Worker's registered root.

Repository identity is a tuple of canonical worktree path and canonical Git common directory. The tuple, not a user-spelled path, keys leases and holds.

## Repository Observation

A Repository Observation is immutable and digest-addressed. It records:

- schema version and generated observation ID;
- repository identity and Task ID;
- checkpoint kind: `before_dispatch`, `result`, `cancel`, `failure`, `approval`, or `hold_clear`;
- `HEAD`, branch, and index metadata;
- staged and unstaged binary diffs or immutable Attachment references;
- untracked paths with file type, size, and SHA-256 for regular files;
- ignored path names needed to report newly introduced ignored entries;
- normalized Git status entries;
- changed paths relative to the Task baseline;
- scope classification for every changed path;
- command versions, exit status, and concise diagnostics; and
- canonical observation digest.

The baseline Observation captures existing user changes rather than rejecting or normalizing them. Later changed paths mean changes relative to that baseline, not differences from `HEAD` alone.

Observation failure before Task dispatch rejects or defers the Task without starting the Worker. Observation failure after native dispatch creates a Worktree Hold because repository state cannot be proved.

## Scope matching

`exact_file` authorizes one normalized path. `subtree` authorizes the named directory and descendants on path-component boundaries. Scope comparison is lexical after validated repository-relative normalization and symlink checks.

Changed paths include additions, modifications, deletions, renames, type changes, staged changes, unstaged changes, and newly created untracked files. A rename requires both source and destination to be authorized.

Changes outside declared scope do not trigger automatic cleanup. The Task becomes non-approvable and the worktree enters Hold with the Observation and path classifications attached.

The Coordinator reports ignored-file limitations. A newly visible ignored path outside scope creates a Hold, but a content-only change to a pre-existing ignored file may be undetectable in the advisory MVP.

## Advisory Worktree Lease

One mutating Task acquires an exclusive lease immediately before native dispatch. The lease is stored durably and backed by an operating-system file lock under the plugin state directory so another Coordinator process using the same state directory cannot dispatch conflicting work.

The lease persists through Worker execution, blocking Questions, Result review, related read-only reviews, Corrections, and temporary Supervisor disconnection.

Normal Approval releases the lease after the approval Observation matches the expected digest.

Queued cancellation before dispatch needs no lease. After dispatch, cancellation or failure retains the lease behind a Worktree Hold until reconciliation.

Durable lease rows are diagnostic after a crash; the operating-system lock is authoritative for live ownership. A free lock does not authorize new work while a Hold or unfinished dispatched Task remains durable.

## Read-only related Tasks

An unrelated Task targeting a leased worktree remains queued. A read-only Task may run concurrently only when it explicitly references the mutating Task, the parent is in stable `reviewing` state, no Correction is active, and the reviewer captures its own starting Observation.

Multiple related read-only Tasks may share the stable review checkpoint. A Correction waits until they finish. Any drift while a related reviewer is active fails the review and creates a Hold on the parent worktree.

## Worktree Hold

A Hold is created after possible mutation when cancellation, process loss, delivery uncertainty, observation failure, out-of-scope drift, repository-identity change, or a stale or partial cancellation makes the worktree uncertain.

A Hold records Task, lease, repository identity, reason, latest Observation, creation time, and resolution audit fields. It blocks every new mutating Task for that repository. Related read-only inspection may be allowed explicitly by the Supervisor.

Hold clearance requires the sole Supervisor Session, a fresh successful Observation, the exact expected digest, and a nonempty reconciliation note.

The clear operation records evidence and removes only the scheduling block. It performs no repository mutation. A reviewable Task returns to review eligibility; a terminal failed or cancelled Task releases its retained lease.

## Cancellation and process loss

Cancellation persists intent before contacting the adapter. The Harness Host asks the adapter to abort the active native turn, waits for terminal evidence within the configured grace period, captures a cancellation Observation, and then stops the native process. If cooperative cancellation does not settle, Herdr closes the Worker pane and the Task enters Hold.

Repeated cancellation is idempotent. A completion/cancellation race retains both pieces of evidence; if a valid Result and clean Observation completed before cancellation intent, the Task may enter `reviewing`, otherwise it becomes cancelled or failed under Hold.

A broker restart may reconnect a still-live Worker Host using Session identity and monotonic event sequence. A Worker Host or cold Herdr restart loses the native process and never triggers automatic resume, adoption, or replay. A dispatched mutating Task then enters Hold.

## Approval checkpoint

Approval requires a fresh Observation with the same repository identity and baseline, no unclassified drift, all changed paths inside declared scope, no active related review, the selected Result revision, and a digest matching the Supervisor command.

Approval records acceptance and releases the lease. It does not commit or publish the Worker changes. The live worktree remains exactly as inspected for the user or Supervisor to review and commit separately.

## Acceptance scenarios

- A dirty worktree is observed, a scoped Worker edit preserves the original dirty paths, and Approval releases the lease without altering either set of changes.
- Two mutating Tasks for the same linked worktree serialize even when spelled through different paths.
- Different worktrees sharing one Git common directory may run concurrently when their canonical worktree identities differ.
- A Worker or user edits outside scope; the advisory system creates the same Hold because attribution is intentionally unavailable.
- A related Codex review runs against a stable Result checkpoint; an OMP Correction waits until the review finishes.
- A Worker crashes after an edit; no rollback occurs, and only digest-confirmed Supervisor reconciliation unblocks the worktree.
- A queued Task is cancelled before dispatch; it terminates without a lease or Hold.
