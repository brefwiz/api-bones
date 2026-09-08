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
