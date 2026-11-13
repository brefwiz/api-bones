# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.1] - 2026-11-13

### Added

- *(batch,nested-write)* an existing record is an item's own refusal; a nested write may name a sibling service's method

## [0.6.0] - 2026-11-13

### Added

- *(automation)* nested_write_of children are the named write's child, never on a read

## [0.5.0] - 2026-11-12

### Added

- *(batch_of)* a batch keeps its single's idempotency level; write batches and the aborted item outcome

## [0.4.1] - 2026-11-12

### Other

- updated the following local packages: api-bones-protos

## [0.4.0] - 2026-11-11

### Added

- [**breaking**] api-bones-transport, the one fleet Connect client transport

### Breaking changes merged after the version was prepared

- feat!: api-bones-transport, the one fleet Connect client transport
- feat!: api-bones-transport, the one fleet Connect client transport

## [0.3.2] - 2026-11-11

### Other

- updated the following local packages: api-bones-protos

## [0.3.1] - 2026-11-08

### Fixed

- *(automation)* read actions and triggers without the manifest proto

## [0.3.0] - 2026-11-08

### Added

- *(automation)* nested_write_of names a unary write

## [0.2.15] - 2026-11-08

### Added

- *(protos)* add bones.v1.nested_write_of for atomic nested child writes

## [0.2.14] - 2026-11-06

### Added

- *(sdk)* paged client fails closed, platform composition, Rust paging emitter

### Added

- Emit Rust page traits and platform-client list extension methods from descriptor paging policy.

## [0.2.13] - 2026-11-05

### Other

- updated the following local packages: api-bones-protos

## [0.2.12] - 2026-11-05

### Other

- updated the following local packages: api-bones-protos

## [0.2.11] - 2026-11-05

### Added

- platform pagination, safe batch providers and typed fields

## [0.2.10] - 2026-11-05

### Added

- *(automation)* check a product's declarations without its identity

### Added

- `check_surface`: a product's build applies the compiler's rules to its
  declarations without its identity and gets the `declaration_digest` its
  release manifest carries.

### Changed

- `compile` refuses a template that names the declaring product (M1); its own
  triggers and actions are referenced with `product` empty.

## [0.2.9] - 2026-11-04

### Added

- *(automation)* validate documents against admitted manifests

### Added

- Shared document admission and publish check against admitted product manifests,
  including cross-product binding types and required parameter values at publish.

## [0.2.8] - 2026-11-02

### Other

- updated the following local packages: api-bones-protos

## [0.2.7] - 2026-11-01

### Other

- updated the following local packages: api-bones-protos

## [0.2.6] - 2026-10-31

### Other

- updated the following local packages: api-bones-protos

## [0.2.5] - 2026-10-28

### Other

- *(license)* clear license-boundary errors

## [0.2.4] - 2026-10-26

### Other

- updated the following local packages: api-bones-protos

## [0.2.3] - 2026-10-24

### Added

- *(automation)* refuse a batch_of that names no sibling or widens its authorization

## [0.2.2] - 2026-10-23

### Other

- updated the following local packages: api-bones-protos

## [0.2.1] - 2026-10-18

### Other

- updated the following local packages: api-bones-protos

## [0.2.0] - 2026-10-18

### Added

- *(automation)* compiler refuses A4, A5, A6 and C1

### Other

- *(release)* metadata for #166

## [0.1.0] - 2026-10-17

### Added

- *(automation)* [**breaking**] manifest compiler, DSSE signature and policy schema refusal
