# Herdr Harness Coordinator

Herdr Harness Coordinator is the local coordination network through which one Supervisor directs and reviews bounded work performed by autonomous Worker Harnesses.

## Network

**Coordinator**:
The local authority for harness presence, task delivery, message routing, and durable coordination state.
_Avoid_: Orchestrator, workflow engine

**Harness**:
A durable, addressable autonomous coding environment such as OMP or Codex.
_Avoid_: Provider, agent type

**Harness Session**:
One live activation of a Harness, bound to a native session and a Herdr terminal.
_Avoid_: Harness, Agent Run

**Harness Kind**:
The native harness implementation used by a Harness, initially OMP or Codex.
_Avoid_: Provider, role

**Harness Tier**:
The coordination authority assigned to a Harness: Supervisor or Worker.
_Avoid_: Model Tier, Role

**Supervisor Harness**:
The sole Harness that owns user intent, technical direction, task decomposition, corrections, approval, and the final response.
_Avoid_: Parent Agent, root worker

**Worker Harness**:
An autonomous Harness that executes one bounded Task at a time and returns a consolidated result.
_Avoid_: Child Agent, provider run

## Work

**Task**:
A bounded assignment from the Supervisor Harness to one Worker Harness, including repository authority and acceptance context.
_Avoid_: Workflow node, prompt, Agent Run

**Task Conversation**:
The ordered Task, Question, Reply, Correction, Result, and Notification messages associated with one Task.
_Avoid_: Shared chat history, workflow

**Result**:
A Worker Harness's consolidated completion report and verification evidence for Supervisor review.
_Avoid_: Final completion, universal artifact

**Approval**:
The Supervisor Harness's acceptance of a Result and the associated repository state.
_Avoid_: Worker completion, delivery acknowledgement

**Task Role**:
The declared semantic purpose of a Task (implementation, investigation, review, verification, other) used by conservative automatic Session selection.
_Avoid_: Workflow type, model tier

**Session Reuse Policy**:
The declared relationship between a Task and a native Worker Session: required, prefer, fresh, or auto.
_Avoid_: Auto mode, adapter decision

**Task Session Binding**:
The durable, auditable record of which Harness Session a Task was bound to, with the requested and effective reuse policy and decision evidence.
_Avoid_: Routing decision, queue assignment

**Native Session Health**:
Provider-neutral evidence about a native conversation (healthy, context pressure, compacted, ambiguous, failed) consulted only after identity and policy compatibility.
_Avoid_: Harness presence, Task state

**Supervisor Event**:
A durable, deduplicated attention record that may wake the visible Supervisor Harness.
_Avoid_: Notification queue entry, Bus Message

**Supervisor Host**:
The pane-resident process that binds a managed Supervisor's visible native conversation and injects Supervisor Events through its Supervisor Adapter.
_Avoid_: Orchestrator, popup

## Communication

**Bus Message**:
A short, durable communication between the Supervisor Harness and one Worker Harness.
_Avoid_: Prompt, artifact

**Message Kind**:
The purpose of a Bus Message: Task, Result, Question, Reply, Correction, or Notification.
_Avoid_: Event type, role

**Delivery Intent**:
The sender's explicit choice to process a Bus Message after current work or steer the active work.
_Avoid_: Auto mode, adapter decision

**Delivery Receipt**:
Durable evidence about native acceptance of one Bus Message, distinct from Task processing or completion.
_Avoid_: Result, Approval

**Attachment**:
An immutable file stored by the Coordinator and referenced from a Bus Message by identity and digest.
_Avoid_: Raw file path, universal artifact

**Durable Mailbox**:
The persisted ordered messages awaiting or recording delivery for one Harness.
_Avoid_: Provider queue, conversation history

## Repository coordination

**Repository Observation**:
A digest-addressed record of the Git worktree state at a Task checkpoint.
_Avoid_: Repository Snapshot, Publish Delta

**Advisory Worktree Lease**:
The Coordinator's exclusive scheduling claim for one mutating Task against a canonical worktree.
_Avoid_: Sandbox, repository lock

**Worktree Hold**:
A durable block on new mutating Tasks when the current worktree state requires Supervisor reconciliation.
_Avoid_: Automatic rollback, Repository Quarantine
