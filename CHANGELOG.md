# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.7.0] - 2026-04-08

### Fixed

- `ErrorCode` utoipa schema now emits the actual wire format (`urn:api-bones:error:*` strings) instead of Rust variant names, fixing SDK client deserialization of error responses

## [1.6.0] - 2026-04-06

### Added

- `schemars` support: `JsonSchema` derive on all public types
- Runnable usage examples for axum, pagination, and health checks
- Comprehensive per-type example files
- Doc-tests on all public items

### Changed

- Renamed crate from `shared-types` to `api-bones`

### Fixed

- `ErrorTypeMode` now uses `RwLock` instead of `OnceLock` for test safety
- CI: correct crate name in publish summary
- CI: add Forgejo cargo registry configuration

## [1.5.0] - 2026-04-06

### Added

- `Slug` validated newtype for URL-friendly identifiers
- `source()` chaining for `ApiError` and `ValidationError` implementations
- `no_std` support behind feature flags

## [1.4.0] - 2026-04-04

### Added

- `SortParams`, `FilterParams`, and `SearchParams` query types
- Generic `ApiResponse<T>` envelope with metadata
- HATEOAS `Link` and `Links` types
- `RateLimitInfo` type for rate limit metadata
- `ETag` and conditional request types (RFC 7232)
- `arbitrary` and `proptest` feature flags
- `BulkRequest`, `BulkResponse`, and `BulkItemResult` types
- `AuditInfo` struct for resource audit metadata with arbitrary/proptest support
- `fake` crate integration for test fixture generation
- Typestate builder patterns for `ApiError`, `HealthCheck`, `ReadinessResponse`
- `CursorPaginationParams` and usage guidance

### Fixed

- Restored `lib.rs` modules and synced `Cargo.toml` after bulk types addition

## [1.3.1] - 2026-04-04

### Fixed

- Removed duplicate page-based pagination types

### Changed

- Pinned Rust toolchain to 1.94
- Removed Gherkin feature file from shared-types

## [1.3.0] - 2026-04-04

### Added

- Flat `PaginatedResponse` and validated `PaginationParams`

## [1.2.0] - 2026-03-25

### Changed

- Migrated cargo registry to git.brefwiz.com

## [1.1.0] - 2026-03-25

### Added

- iCalendar RFC 5545 support as optional `calendar` feature

## [1.0.0] - 2026-03-14

### Added

- Initial release with core API types: `ApiError`, `ValidationError`, `HealthCheck`, `ReadinessResponse`, `PaginationParams`

[Unreleased]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.6.0...HEAD
[1.6.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.5.0...v1.6.0
[1.5.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.4.0...v1.5.0
[1.4.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.3.1...v1.4.0
[1.3.1]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.3.0...v1.3.1
[1.3.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.2.0...v1.3.0
[1.2.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.1.0...v1.2.0
[1.1.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.0.0...v1.1.0
[1.0.0]: https://git.brefwiz.com/brefwiz/api-bones/releases/tag/v1.0.0
