---
status: superseded by ADR-0004
---

# Use one top-level orchestrator

Herdr Harness Coordinator is the single authority for top-level workflows, scheduling, policy, repository safety, and artifact routing. OMP, Codex, Pi, and OpenCode remain execution providers because allowing Herdr, provider-native multi-agent systems, and a custom workflow runtime to coordinate peers independently would create competing ownership, inconsistent enforcement, and ambiguous cancellation and status.

Provider-native children may be introduced later only as observable, policy-bounded descendants of a managed `AgentRun`; they never become top-level workflow nodes independently.
