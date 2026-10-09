# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-10-09

### Added

- *(bones)* [**breaking**] resource and call_shape options; call shape leaves McpProjection

### Added

- *(protos)* `resource` message option and `call_shape` method option with the `CallShape` enum
- *(protos)* vendor `google/api/field_behavior.proto` alongside the bones protos

### Changed

- **BREAKING** *(protos)* `McpProjection.shape` and the `McpShape` enum are removed (field 1 and name `shape` reserved); the call shape moves to the `bones.v1.call_shape` method option, which every projection reads. `McpProjection` keeps only `title`.

### Breaking changes merged after the version was prepared

- feat(bones)!: resource and call_shape options; call shape leaves McpProjection
- feat(bones)!: resource and call_shape options; call shape leaves McpProjection

## [0.4.9] - 2026-10-09

### Added

- *(bones)* attribute dimensions, attr field option and mcp method option
- *(connect)* carry the emitted error code through ErrorInfo and the SDK error text

### Fixed

- *(connect)* drop the version from ErrorInfo, reject malformed details, run every internal lane

### Added

- *(protos)* `attributes` service option, `attr` field option and `mcp` method option

## [0.4.8] - 2026-10-04

### Added

- *(protos)* capability_scopes service option for labelled permission groups

## [0.4.7](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.6...api-bones-protos-v0.4.7) - 2026-10-02

### Added

- *(protos)* add revision to OffsetPageResponse

## [0.4.6](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.5...api-bones-protos-v0.4.6) - 2026-10-02

### Added

- *(protos)* add bones.v1.capability_audience service option

## [0.4.5](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.4...api-bones-protos-v0.4.5) - 2026-09-29

### Added

- *(bones)* a provider declares the broker-enforced scopes of a capability it provides

### Fixed

- *(bones)* state broker-scope admission once, group refusals, neutral names
- *(bones)* refuse an event provider whose canonical stream name collides
- *(bones)* broker scopes are service-bound and enforced at registration
- *(bones)* broker scopes are namespaced, subject-free, on an event capability

### Other

- *(proto)* label broker-scope refusal groups A1-A3 and S1-S5

## [0.4.4](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.3...api-bones-protos-v0.4.4) - 2026-09-29

### Added

- *(bones)* a provider labels the capabilities and permissions it defines

## [0.4.3](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.2...api-bones-protos-v0.4.3) - 2026-09-14

### Added

- *(bones)* a service declares the capabilities it serves and requires

## [0.4.2](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.1...api-bones-protos-v0.4.2) - 2026-08-16

### Added

- *(protos)* declare provider capabilities on services

### Fixed

- *(protos)* keep capability examples generic

### Fixed

- Capability annotation examples now use generic service vocabulary only.

## [0.4.1](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.0...api-bones-protos-v0.4.1) - 2026-08-15

### Fixed

- *(protos)* reserve recycled AuthzKind numbers, renumber PUBLIC/AUTHENTICATED

## [0.4.0](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.3.0...api-bones-protos-v0.4.0) - 2026-08-15

### Added

- *(protos)* [**breaking**] gate RPCs on capability, remove principal-class authz

### Fixed

- *(release)* publish the npm surface to the registry its consumers resolve
