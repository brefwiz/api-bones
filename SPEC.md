---
service: api-bones
wire_surface: library
surface_kind: library
# The union of every sdk_surfaces target below, which the schema requires it
# to equal.
sdk_languages: [rust, typescript]
# The publishable workspace members, kept in step with cargo metadata: an
# undeclared publishable crate is surface drift, and this repo previously had
# all eight publishable with no mechanism publishing any of them.
# The surfaces this library ships in more than one language.
#
# Distinct from `sdk_languages` above, which describes a generated SDK backed
# by a service. These are hand-written library surfaces that exist twice, and
# declaring them makes feature-closure require a Gherkin contract with direct
# backing in every target language.
#
# No `owns:` — this surface names no RPC endpoint, and needs no canary: there
# is no image here to run one against.
sdk_surfaces:
  connect-retry-eligibility:
    contract: tests/features/connect_retry_eligibility.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-connect]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # Which entry of the TypeScript package carries workload identity. Rust has
  # no entry split: the crate is server-only, so this surface is TS alone.
  connect-workload-identity:
    contract: tests/features/connect_workload_identity.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The browser transport's route for contract-declared public reads: only the
  # web entry has a public lane to reach, so this surface is TS alone.
  connect-public-read:
    contract: tests/features/connect_public_read.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The default If-Match precondition, attached from the transport itself.
  # Rust carries the same decision, but from outside the package this surface
  # names (see internal_behavior_owners, below) -- so this surface is TS
  # alone, the same way connect-workload-identity is.
  connect-precondition:
    contract: tests/features/connect_precondition.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
internal_behavior_owners:
  # with_bearer sets transport-layer default headers no declared sdk_surface
  # exposes -- callers reach it directly as library code, not through a
  # generated contract.
  - paths: [src/connect/transport.rs]
    feature: tests/internal-bdd/tests/features/connect_client_headers.feature
  # MAX_LIMIT/DEFAULT_LIMIT and the validation/clamping logic that reads them
  # are library code no declared sdk_surface exposes -- callers reach them
  # directly as PaginationParams/CursorPaginationParams/KeysetPaginationParams
  # constructors and the Connect page builder, not through a generated
  # contract.
  - paths: [src/pagination.rs, src/connect/page.rs, src/fake_impls.rs]
    feature: tests/internal-bdd/tests/features/pagination_max_limit.feature
  # The Rust half of the default If-Match precondition lives in the root
  # crate, outside the api-bones-connect package root the TypeScript half
  # answers to as its SDK contract (connect-precondition, below) -- so this
  # half is library code no declared sdk_surface exposes.
  - paths: [src/connect/mod.rs, src/connect/precondition_client.rs]
    feature: tests/features/connect_precondition.feature
library_crates:
  - api-bones
  - api-bones-progenitor
  - api-bones-protos
  - api-bones-reqwest
  - api-bones-sdk-gen
  - api-bones-test
  - api-bones-tower
rpc_protocols:
  supported: [proto, json]
  service:
    default: proto
  webapp:
    default: proto
capability_exposes: []
capability_consumes: []
ci_snowflakes: []
proto_packages: []
openapi_path: ~
publishes: [rust-crates, npm]
version_ecosystem: rust
# Ceilings, not targets: they only ratchet down (`--lower` rewrites them as the
# duplication goes), and a new copy-paste still fails against them. The existing
# volume is tracked in the issue below rather than accepted silently.
duplication_baseline:
  issue: 2
  adapter_construction: 0
  noop_in_production: 0
  test_double_in_production: 0
  duplicate_dispatch: 0
  duplicate_literals: 0
  clone_tokens: 2467
  test_clone_tokens: 564
ci_checks:
  license_headers:
    # Cargo metadata is authoritative here: these crates are MIT, and the
    # check's default expects the proprietary identifier every other repo uses.
    mode: cargo-per-crate
migration_baseline:
  utoipa_handler_count: 0
  baseline_commit: ~
migration_priority: ~
migration_eta: ~
---

# api-bones — Development Spec

api-bones is the shared error/response and Connect-transport layer every Brefwiz
service builds on. It owns no wire surface of its own: it supplies the types and
transport composition that consumer services use to define theirs, which is what
`wire_surface: library` records.

It publishes on two ecosystems, and they release on separate lineages:

- Rust crates to crates.io on `v*` tags.
- npm packages (`api-bones-connect`, `api-bones-axios`, `api-bones-otel`) on
  their own prefixed tags. `api-bones-connect` publishes to the brefwiz
  registry, because packages published there depend on it and npm resolves a
  registry per scope rather than per package.

## Canonical home

This repository lives in Gitea; the GitHub repository is a downstream push
mirror kept for public visibility. CI runs on CDS from `.cds/workflows/`.
