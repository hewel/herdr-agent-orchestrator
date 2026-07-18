---
status: accepted
---

# Coordinate autonomous harnesses under one Supervisor

Herdr Harness Coordinator is a durable communication and presence layer between one authoritative Supervisor Harness and autonomous Worker Harnesses, rather than a workflow engine that decomposes provider runs itself. This preserves each harness's native multi-agent strengths and keeps technical direction and final approval with the Supervisor, while the Coordinator owns only top-level identity, task delivery, messaging, lifecycle control, and coordination state; it supersedes ADR-0001 and ADR-0003.
