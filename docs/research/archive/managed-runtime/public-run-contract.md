# Archived public run contract and resolved snapshots

Status: superseded by `docs/research/mvp/coordination-contract.md` and retained only as historical Managed Runtime research.

Former status: resolved for the Managed Runtime MVP.

This contract separates the intent a caller may submit from the immutable
authority the orchestrator compiles for execution. It is authoritative for
individual Managed `AgentRun`s and the fixed Managed implementer → reviewer →
verifier workflow. The machine-readable public shapes are
[`run-submission-v1.schema.json`](../../../../schemas/archive/managed-runtime/run-submission-v1.schema.json)
and
[`dirty-worktree-confirmation-v1.schema.json`](../../../../schemas/archive/managed-runtime/dirty-worktree-confirmation-v1.schema.json).
The persisted snapshot envelopes are documented by
[`resolved-run-spec-v1.schema.json`](../../../../schemas/archive/managed-runtime/resolved-run-spec-v1.schema.json)
and
[`resolved-agent-run-spec-v1.schema.json`](../../../../schemas/archive/managed-runtime/resolved-agent-run-spec-v1.schema.json).

## Contract boundary

A caller submits a `RunSubmissionV1`. It contains only bounded task intent,
explicit provider/profile/role assignments, repository authority, and a root
timeout. It never contains an orchestrator identifier, a provider session,
resolved prompt, tool list, model, environment, execution policy, output
schema, delegation mode, or workflow graph.

After validation, the orchestrator creates immutable, independently versioned
snapshots of the selected profiles, role definitions, effective policies, and
resolved run. Providers receive only a `ResolvedAgentRunSpecV1`; they never
consume the caller's raw submission.

The MVP accepts exactly two submission kinds:

- `agent_run`: one explicit OMP or Codex provider/profile and one built-in
  `implementer`, `reviewer`, or `verifier` role;
- `workflow_run`: the fixed `implement_review_verify_v1` workflow, with an
  explicit OMP or Codex provider/profile for each built-in role.

Managed delegation is injected by the resolver. Pi, OpenCode, Native or Hybrid
delegation, caller-defined roles, generic workflows, automatic routing, and
caller-supplied policies require a later public schema version.

## Public submission

`RunSubmissionV1` is a closed, internally tagged JSON union. Every object
rejects unknown fields and every enum rejects unknown values. `schema_version`
is the integer `1`; there are no implicit compatibility fallbacks.

Common fields are:

| Field | Contract |
| --- | --- |
| `schema_version` | Required integer `1`. |
| `kind` | Required `agent_run` or `workflow_run`. |
| `request_key` | Optional caller idempotency key. |
| `worktree_path` | Required absolute UTF-8 path to the exact Git worktree root. |
| `timeout_seconds` | Required root budget from `1` through `86400`. |
| `task` | Required bounded task intent. |

An `agent_run` has one `assignment` containing required `provider`,
`profile_id`, and `role`. A `workflow_run` has exactly three named
`assignments`: `implementer`, `reviewer`, and `verifier`; each contains a
required `provider` and `profile_id`, while the map key fixes the role.

OMP and Codex are the only v1 provider values. Every assignment requires an
explicit `profile_id`; configured defaults are not part of the public
contract.

### Task intent

Every task requires:

- `title`, at most 200 UTF-8 bytes;
- `objective`, at most 16,384 UTF-8 bytes;
- one or more `acceptance_criteria`, each at most 4,096 UTF-8 bytes;
- `repository`, containing the requested repository authority; and
- `verification`, an ordered list of declared verification intentions.

`context_paths` and `requirements` default to empty arrays. Context paths grant
no write or publication authority. Requirements and acceptance criteria are
resolved constraints, not open architecture or product choices.

An editing role requires at least one publish scope and at least one
verification intent. A reviewer or verifier must have no publish scope,
ignored-publication authorization, or destructive authorization. A read-only
role may declare Scratch Scopes because scratch output is run-private and never
publishable.

Each verification intent contains a unique bounded `id`, the exact command
string requested by the parent, and a bounded human-readable `expected`
result. The separate verification contract defines command execution and
evidence semantics; this contract only freezes caller intent and order.

### Repository authority

`write_scopes` are typed objects, never path strings with trailing-slash
semantics:

```json
{ "kind": "exact_file", "path": "src/download/queue.rs" }
{ "kind": "subtree", "path": "tests/download" }
```

Repository intent distinguishes:

- ordinary exact-file and subtree publish scopes;
- Scratch Scopes, which are writable but never publishable;
- explicit ignored-path publication authorization, which must also be within
  ordinary publish scope; and
- exact destructive authorizations: `delete` with one target, or `rename`
  with independent `from` and `to` endpoints.

Delete targets and both rename endpoints must be in ordinary publish scope.
Context paths, Scratch Scopes, and parent directories do not imply destructive
or ignored-path authority. Duplicate or overlapping publish scopes are
rejected, as are overlaps between publish and scratch scopes. A scope entering
a submodule, nested repository, or repository metadata is unsupported.

### Path rules

`worktree_path` must name an existing absolute UTF-8 directory. The resolver
stores the submitted value and descriptor-resolves the canonical worktree
identity; sanitized Git discovery must report that exact resolved directory as
the worktree root.

Every repository-relative path is a nonempty UTF-8 Linux path no longer than
4,096 bytes. It rejects NUL, an absolute prefix, an empty component, `.`, `..`,
repeated separators, a trailing separator, and repository metadata paths.
There is no Unicode normalization. On Linux a backslash is an ordinary
filename byte. Existing components and missing targets are additionally
subject to the descriptor-relative and symlink rules in the
[repository safety contract](repository-safety-contract.md).

### Fixed limits

The decoder rejects a submission or confirmation body larger than 1 MiB.
Unless a lower field limit is stated above:

| Value | Maximum |
| --- | ---: |
| `request_key`, `profile_id`, verification `id` | 128 bytes |
| repository path | 4,096 bytes |
| requirement, criterion, verification `expected` | 4,096 bytes |
| verification command | 16,384 bytes |
| context paths or write scopes | 256 entries each |
| requirements or acceptance criteria | 128 entries each |
| scratch scopes | 64 entries |
| ignored or destructive authorizations | 256 entries each |
| verification intents | 32 entries |
| dirty-confirmation paths | 4,096 entries |

Lists default to empty only where the schema says so. Array order is
significant in the canonical typed request. Duplicate IDs and duplicate paths
are invalid even where JSON Schema alone cannot express the rule.

## Identity and idempotency

The orchestrator generates lowercase UUIDv7 identifiers. The top-level
`run_id` is the durable Herdr and orchestrator identity. For an individual
submission it is also the sole `AgentRun` ID. For a workflow it identifies the
workflow root, and the orchestrator preallocates three separate child
`AgentRun` IDs at durable acceptance.

The optional `request_key` matches `[A-Za-z0-9._:-]{1,128}` and is globally
unique within one plugin state store. The orchestrator materializes every
schema default, preserves strings exactly, retains array order, removes
`request_key`, and serializes the resulting typed value with the
[JSON Canonicalization Scheme in RFC 8785](https://www.rfc-editor.org/rfc/rfc8785).
SHA-256 of those exact UTF-8 bytes is the semantic request digest:

- the same key and same semantic digest return the already accepted run;
- the same key and a different digest return `request_key_conflict`;
- concurrent identical submissions produce one durable run.

Caller-supplied run, task, workflow, child, pane, or provider-session IDs are
never accepted.

## Validation and durable acceptance

Admission is ordered so that public errors, immutable authority, and timeout
accounting are deterministic:

1. Enforce body size and decode JSON.
2. Validate the closed schema, tag, enums, and fixed bounds.
3. Build the typed normalized request and its canonical digest.
4. Resolve `request_key` before consulting mutable configuration.
5. Validate lexical paths and cross-field invariants.
6. Resolve and validate the supported Git worktree identity.
7. Resolve profiles, built-in roles, provider compatibility, binary identity,
   task-policy compatibility, and effective policy inputs.
8. Atomically durably accept the run: allocate IDs, store the canonical
   submission and frozen configuration snapshots, record `accepted_at`, and
   derive the root deadline.
9. Queue for and acquire the Worktree Lease; queue time consumes the budget.
10. Revalidate repository identity, prove safety prerequisites, and capture the
    Repository Snapshot.
11. If a writable path is dirty, enter `awaiting_confirmation`; otherwise seal
    the resolved root spec and start execution.

Only failures before step 8 are synchronous submission errors with no run ID.
Every later result is durable run lifecycle evidence. No provider process or
provider-owned session is created before the resolved spec is sealed. Herdr
may create the worker pane immediately after durable acceptance so the worker
can own queueing, snapshot, confirmation, and startup state.

## Dirty-worktree confirmation

Dirty writable paths are confirmed against a prepared run, not trusted in the
initial submission. The orchestrator returns a challenge containing the run
ID, Repository Snapshot digest, and a deterministically sorted set of paths
with staged, unstaged, untracked, and ignored classifications.

`DirtyWorktreeConfirmationV1` must echo the exact run ID, snapshot digest, and
path evidence. Exact repetition is idempotent. A mismatched digest, missing or
extra path, changed classification, excessive path count, expired deadline, or
any intervening filesystem or Git change invalidates the run. The orchestrator
does not refresh a prepared run in place; the caller must submit a new run.
Each path appears exactly once and has at least one true dirty classification;
the schema expresses the latter and typed validation enforces path uniqueness.

Confirmation is consent to operate on the captured dirty baseline. It does not
widen publish, ignored-path, or destructive authority.

## Immutable resolved snapshots

Every stored authority object uses an independently versioned envelope:

```rust
pub struct SnapshotEnvelope<T> {
    pub schema_version: u32,
    pub digest: Sha256Digest,
    pub value: T,
}
```

The digest is lowercase SHA-256 over RFC 8785 canonical UTF-8 JSON of `value`
after the typed snapshot serializer has materialized its required fields.
Snapshot storage is write-once; an existing digest must byte-match its stored
value.

At durable acceptance the resolver freezes:

- the orchestrator/plugin version and relevant configuration revision;
- each selected profile's safe identifier and constraints, excluding
  credentials and reusable capabilities;
- the canonical provider executable path, reported version, and binary digest;
- each role revision, prompt, provider compatibility, output-schema reference,
  and base policy;
- caller policy inputs, precedence decisions, effective policy, and provenance.

Later configuration changes affect only new submissions. Provider sessions,
pane bindings, progress, lifecycle state, publication state, and credentials
remain mutable run records and are not part of a resolved spec.

`ResolvedTopLevelRunSpecV1` is a closed agent/workflow union containing the
generated identity, submission digest, acceptance and deadline timestamps,
resolved repository identity, normalized repository authority, Repository
Snapshot and safety-backend evidence, Managed mode, and frozen orchestrator,
provider executable, profile, role, policy, and task snapshot references. A
clean baseline stores `dirty_confirmation_digest: null`; the field is never
omitted.

The fixed workflow root additionally stores template ID
`implement_review_verify_v1`, all three preallocated child IDs, frozen node
assignments, dependencies, and deterministic task-source declarations. The
implementer child spec can be sealed immediately. Reviewer and verifier child
specs are new immutable linked records created only when their required
implementation, patch, candidate, and nonblocking review artifacts exist. The
root spec is never mutated to fill late-bound inputs.

Each executable child is sealed as a separate `ResolvedAgentRunSpecV1`. It
records its Agent Run ID, top-level run ID, exact workflow node (or `null` for
an individual submission), fixed assignment snapshots, task snapshot, ordered
input Artifact references, node budget, root deadline, top-level spec
reference, and repository/safety snapshot references. Provider adapters accept
only this child-level shape. Reviewer and verifier golden fixtures prove that
late-bound artifacts create new immutable child specs rather than changing the
workflow root.

## Root deadline

`timeout_seconds` covers the entire individual run or workflow from durable
acceptance. It includes queueing, Worktree Lease acquisition, snapshotting,
dirty confirmation, provider startup and execution, artifact collection,
review, verification, and prepublication work.

Each node budget is:

```text
min(frozen role cap, frozen profile cap, remaining root deadline)
```

The persisted UTC timestamps are audit evidence; runtime enforcement also uses
a monotonic clock. Expiry follows the normalized lifecycle contract. Once the
publication point of no return closes the cancellation gate, publication
continues to a known terminal or quarantined state rather than being
interrupted by the root deadline.

## Public errors and evolution

Pre-acceptance errors use the small closed envelope:

```rust
pub struct SubmissionErrorV1 {
    pub schema_version: u32,
    pub code: SubmissionErrorCodeV1,
    pub message: String,
    pub field: Option<String>,
}
```

The v1 stable code set is:

```text
malformed_json
unsupported_schema_version
unknown_field
unknown_enum_value
value_out_of_bounds
invalid_path
invalid_task
unknown_profile
incompatible_assignment
unsupported_worktree
request_key_conflict
```

Cross-field scope, authority, and verification-intent failures use
`invalid_task`; worktree discovery and unsupported repository shape use
`unsupported_worktree`. Internal error chains are evidence, not stable public
codes. After acceptance, the run lifecycle owns reason codes and terminal
precedence.

Any public field, default, constraint, enum, or semantic change requires
`schema_version: 2`. Adding Pi, roles, delegation modes, workflows, or generic
policy input to v1 is forbidden. Persisted snapshot schemas may evolve
independently when their interpretation of public v1 input is unchanged.
Decoders for stored versions are retained for at least the configured state
retention period.

## Downstream boundaries

This contract intentionally leaves these decisions to their dedicated
contracts:

- exact built-in role-policy payloads and precedence;
- verification command execution, evidence, and gating;
- lifecycle cancellation races and terminal-state precedence;
- artifact payload schemas and parent-review handoff;
- persistence retention and recovery representation; and
- CLI, plugin, and Herdr transport encoding.

Those contracts may refine runtime behavior but must not silently widen or
reinterpret the v1 public submission.

## Required proof scenarios

Implementation acceptance must prove at least:

- golden individual and fixed-workflow submissions match the checked-in
  schemas and round-trip through typed canonicalization;
- unknown fields, enums, schema versions, oversize values, invalid paths,
  overlapping scopes, and incompatible role authority fail before acceptance;
- concurrent equal idempotency requests create one run and unequal requests
  conflict;
- dirty confirmation is exact, snapshot-bound, idempotent, and invalidated by
  any intervening repository change;
- configuration reload cannot change an accepted run;
- queue, confirmation, startup, and workflow child work consume one root
  deadline;
- workflow children are immutable linked records with preallocated IDs; and
- providers can be constructed only from a sealed `ResolvedAgentRunSpecV1`.
