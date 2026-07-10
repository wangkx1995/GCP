# Domain Docs

This repository uses a single-context domain documentation layout.

## Before exploring

- Read `CONTEXT.md` at the repository root when it exists.
- Read relevant ADRs under `docs/adr/` when that directory exists.
- If these files do not exist, proceed silently. Domain-modeling skills create them lazily when terminology or decisions are resolved.

## Expected structure

```text
/
├── CONTEXT.md
├── docs/adr/
└── src/
```

## Consumer rules

- Use terminology defined in `CONTEXT.md` rather than introducing synonyms.
- Treat missing concepts as possible domain-modeling gaps.
- Explicitly flag proposed work that conflicts with an existing ADR.
