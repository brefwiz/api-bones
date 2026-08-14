---
service: api-bones
wire_surface: library
surface_kind: library
sdk_languages: [rust, typescript]
capability_exposes: []
capability_consumes: []
ci_snowflakes: []
proto_packages: []
openapi_path: ~
publishes: [rust-crates, npm]
version_ecosystem: rust
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
