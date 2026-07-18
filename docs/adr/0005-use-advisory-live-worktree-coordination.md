---
status: accepted
---

# Use advisory live-worktree coordination for the MVP

Worker Harnesses edit their configured live Git worktree using their native safeguards, while the Coordinator serializes mutating Tasks, records before-and-after repository evidence, and blocks uncertain worktrees for Supervisor reconciliation. This deliberately trades the stronger isolation and publication guarantees of private overlays for a lightweight autonomous-harness MVP; the Coordinator never automatically reverts, merges, publishes, or discards files, and this decision supersedes ADR-0002.
