# Contract schemas

These Draft 2020-12 schemas are the checked-in wire and persistence boundary
for the Managed Runtime MVP:

- `run-submission-v1.schema.json` validates the closed public submission union;
- `dirty-worktree-confirmation-v1.schema.json` validates exact confirmation
  messages for prepared dirty runs; and
- `resolved-run-spec-v1.schema.json` validates the immutable top-level snapshot
  envelope shape; and
- `resolved-agent-run-spec-v1.schema.json` validates the sealed child-level
  authority passed to a provider adapter.

Files beneath `fixtures/` are golden examples. Schema validation is only the
first admission layer. Rust typed validation must additionally enforce UTF-8
byte limits, lexical and descriptor-relative path rules, scope overlap,
role-policy compatibility, unique verification IDs, worktree identity,
idempotency, snapshot digests, dirty-path equality, and configuration
provenance described in the
[public run contract](../docs/research/mvp/public-run-contract.md).

The `digest` fields in resolved fixtures are real SHA-256 values over RFC 8785
canonical bytes of `value`. Their `submission_digest` fields likewise match the
corresponding public fixture after `request_key` removal. The fixtures use only
integers and ordinary UTF-8 strings so generic key-sorted JSON is sufficient
for a local smoke check; production code must use a conforming RFC 8785
implementation.

Do not loosen a v1 schema in place. Any public field, default, constraint,
enum, or semantic change creates a new public schema version. Persisted
snapshot envelope versions evolve independently when public v1 meaning is
unchanged.
