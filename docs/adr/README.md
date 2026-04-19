# ADRs — moved

Canonical Architecture Decision Records for the brefwiz ecosystem live in the
unified architecture repository:

  <https://git.brefwiz.com/brefwiz/brefwiz-architecture>

See [`docs/adr-manifest.toml`](../adr-manifest.toml) at the repo root for the
pinned upstream commit and the list of platform ADRs that govern this codebase.

## Why

Cross-cutting decisions (nested-org identity, capability model, token format,
event broker, trust model, etc.) used to be duplicated across service repos.
They now live in a single home so that producers and consumers agree on the
contract without drift.

## Consulting ADRs

1. Start with `docs/adr-manifest.toml` — it lists the platform and service
   ADRs that govern this codebase.
2. Resolve every ADR against `origin/HEAD` of the brefwiz-architecture `main`
   branch. Merged = prod; there is no per-repo pin to bump.
3. Read the ADR's TL;DR in brefwiz-architecture. Load the Summary only if the
   TL;DR is ambiguous; load the Full only if the Summary references a rationale
   you must reproduce.

In Claude Code, the `/adr` skill automates this consultation pattern.

## Historical

Files previously stored under `docs/adr/` in this repo have been preserved
verbatim in brefwiz-architecture under `historical/<service>/` for provenance.
They are no longer the source of truth and MUST NOT be edited here.

## Proposing a new ADR

1. Clone brefwiz-architecture.
2. Copy the appropriate template from `templates/` (`platform-adr.md` for
   cross-cutting decisions; `service-adr.md` for service-scope decisions).
3. Open a PR in brefwiz-architecture. On merge, the ADR is production.
4. If the new ADR affects this codebase, open a follow-up PR here adding
   its entry to `docs/adr-manifest.toml`.
