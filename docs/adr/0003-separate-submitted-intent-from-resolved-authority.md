# Separate submitted intent from resolved authority

Callers submit only a strict, versioned `Run Submission` containing bounded intent and explicit selections; the orchestrator alone generates identities and compiles immutable profile, role, policy, repository, deadline, and task authority into a `Resolved Run Spec`. A single mixed `AgentRunSpec` would let public input appear to control enforcement state and would make configuration reload, idempotency, provenance, and schema evolution ambiguous, so providers accept only sealed resolved specs.
