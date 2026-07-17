# Domain Docs

This is a single-context repository.

## Before exploring

Read these sources when they exist:

- `CONTEXT.md` at the repository root for domain language.
- Relevant ADRs under `docs/adr/`.

If they do not exist, proceed silently. Domain-modeling workflows create them when terminology or architectural decisions are resolved.

## Expected layout

```
/
├── CONTEXT.md
├── docs/
│   └── adr/
└── src/
```

## Domain vocabulary

Use terminology defined in `CONTEXT.md` consistently in issue titles, implementation plans, hypotheses, tests, and documentation.

If a necessary concept is absent, reconsider whether it belongs to the existing vocabulary or note it as a potential domain-modeling gap.

## Architectural decisions

If proposed work conflicts with an existing ADR, surface the conflict explicitly rather than silently overriding the decision.
