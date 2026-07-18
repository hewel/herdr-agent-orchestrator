# Coordinator contract schemas

These Draft 2020-12 schemas are the checked-in wire and persistence boundary for the Harness Coordinator MVP:

- `harness-definition-v1.schema.json` defines durable Harness registration and launch identity;
- `harness-launch-profile-v1.schema.json` resolves an explicit profile ID into a pinned provider launch;
- `harness-launch-profile-v2.schema.json` resolves a bare or absolute executable, optional native profile, and explicit model without pinning an installed Harness release;
- `task-submission-v1.schema.json` defines bounded Task and repository authority input;
- `message-submission-v1.schema.json` defines Question, Reply, Correction, and Notification input;
- `result-manifest-v1.schema.json` defines the consolidated Worker Result;
- `delivery-receipt-v1.schema.json` defines current native-delivery evidence; and
- `repository-observation-v1.schema.json` defines advisory Git checkpoint evidence.

`common-v1.schema.json` contains shared scalar and value definitions. Files under `fixtures/` are golden valid and invalid examples. The archived Managed Runtime schemas remain under `archive/managed-runtime/` for historical reference and are not active contracts.

Schema validation is the first admission layer. Rust typed validation must additionally enforce canonical Git identity, descriptor-relative path and symlink rules, UTF-8 byte ceilings beyond JSON Schema's scalar-value `maxLength`, scope overlap normalization, session-bound authorization, route permissions, Task state transitions, idempotency digests, delivery ambiguity, and Repository Observation integrity described in the MVP contracts.

Run `node scripts/validate-contracts.mjs` to parse every active schema, resolve local references, compile patterns, validate positive and negative wire fixtures, and check semantic route fixtures.

Do not loosen a v1 schema in place. Adding fields, enum values, defaults, or semantic meaning creates a new public schema version.
