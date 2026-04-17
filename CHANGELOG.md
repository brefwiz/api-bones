# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.1] - 2026-04-10

### Fixed

- CI: publish pipeline now correctly publishes all workspace crates (`api-bones-tower`, `api-bones-reqwest`)
- `api-bones-tower` version aligned to `2.0.1` (was incorrectly left at `0.1.0`)

## [2.0.0] - 2026-04-09

### Breaking Changes

- **`response::Links` removed** — use `links::Links` everywhere. `links::Links` is `Vec<Link>`-backed and supports arbitrary `rel` types; the old 3-field struct is gone.
- **`models::ErrorResponse` removed** — use `ApiError` directly for all error responses.
- **`reqwest` feature removed from main crate** — `ApiError::from_response` moved to the new `api-bones-reqwest` satellite crate.
- **`RequestIdParseError` renamed to `RequestIdError`** — now an enum with meaningful variants.

### Added

- **`api-bones-tower`** satellite crate — Tower `RequestIdLayer` and `ProblemJsonLayer` extracted from main crate; main crate is now pure types.
- **`api-bones-reqwest`** satellite crate — reqwest client extensions (`RequestBuilderExt`, `ResponseExt`) extracted from main crate.
- **`HeaderId` trait** — shared abstraction over `RequestId`, `CorrelationId`, `IdempotencyKey` HTTP header newtypes (`as_str()`, `header_name()`).
- **`TryFrom<StatusCode> for ErrorCode`** — 4xx/5xx status codes now convert back to their canonical `ErrorCode`.
- **`QueryBuilder`** — typed query parameter builder with `.set()`, `.set_opt()`, `.extend_from_struct<T: Serialize>()`, `.merge_into_url()`.
- **Fallible constructors** for all constrained param types: `PaginationParams::new()`, `CursorPaginationParams::new()`, `KeysetPaginationParams::new()`, `SearchParams::new()` — enforce constraints without the `validator` feature.
- **`FromRequestParts`** implemented directly on core types (`PaginationParams`, `SortParams`, `IfMatch`, `IfNoneMatch`) when `axum` feature is enabled — no wrapper newtypes needed.
- **`ETag::parse_list()`** — parse comma-separated `If-Match`/`If-None-Match` header values.
- **Feature powerset CI** via `cargo-hack` — all feature combinations tested.
- New runnable examples: `auth_flow`, `bulk_envelope`, `cursor_hmac`, `error_construction`, `cache_control`, `etag_conditional`, `range_headers`, `trace_context`, `axum_core_extractors`, `query_builder`.

### Changed

- `api-bones-tower` and `api-bones-reqwest` version aligned with main crate (`2.0.0`).
- `api-bones-tower` adds optional `uuid` and `chrono` feature passthroughs.
- `PaginationParams` fields use `#[serde(default)]` matching the rest of the query param types.
- `CorrelationId` redundant constructors (`new_random`, `new_id`) removed — keep only `new_uuid()`.
- `ErrorTypeMode` std-only limitation (`error_type_mode()`, `set_error_type_mode()`) documented explicitly.

### Removed

- `models::ErrorResponse` (use `ApiError`)
- `response::Links` (use `links::Links`)
- `reqwest` feature from main crate (moved to `api-bones-reqwest`)
- `tower` feature from main crate (moved to `api-bones-tower`)
- `CorrelationId::new_random()`, `CorrelationId::new_id()` (redundant)

## [1.10.0] - 2026-04-09

### Added

- 12 new `ErrorCode` variants rounding out common HTTP error codes (#82):
  - `MethodNotAllowed` (405), `NotAcceptable` (406), `RequestTimeout` (408)
  - `Gone` (410), `PreconditionFailed` (412), `PayloadTooLarge` (413)
  - `UnsupportedMediaType` (415), `PreconditionRequired` (428)
  - `RequestHeaderFieldsTooLarge` (431), `NotImplemented` (501)
  - `BadGateway` (502), `GatewayTimeout` (504)
- Each variant wired through `status_code()`, `title()`, `urn_slug()`, and `from_type_uri()` roundtrip

## [1.9.0] - 2026-04-09

### Added

- **Axum extractors** (`axum` feature) — `FromRequestParts` impls removing the need for downstream newtype wrappers:
  - `PaginationParams` and `CursorPaginationParams` — parse query string and run `validator` range checks (limit 1..=100)
  - `SortParams` — parse `sort_by` / `direction` query params
  - `IfMatch` and `IfNoneMatch` — parse the matching conditional request headers, including the `*` wildcard and comma-separated tag lists
  - All rejections are `ApiError::bad_request`, so consumers get Problem+JSON bodies for free
- **`ETag` wire-format parsing** (`http` feature):
  - `impl FromStr for ETag` — accepts `"v1"` / `W/"v1"`, rejects unquoted, empty, or malformed input
  - `ETag::parse_list(&str)` — handles comma-separated header values (e.g. `If-Match: "a", W/"b"`)
  - `ParseETagError` enum with `Display` + `std::error::Error`
- **Structured rate-limit metadata on `ApiError`**:
  - `ApiError::with_rate_limit(RateLimitInfo)` — attaches quota data to any error
  - `ApiError::rate_limited_with(RateLimitInfo)` — 429 constructor that derives `detail` from `retry_after`
  - New optional `rate_limit` field serializes inline on `ApiError` and propagates to `ProblemJson` as the `rate_limit` extension member
- Runnable example `axum_extractors_and_ratelimit` demonstrating the new extractors and structured 429 bodies

## [1.8.0] - 2026-04-08

### Added

- `ProblemJson` — RFC 7807 / 9457 wire-format response type with flat extension members
  - Fields: `type`, `title`, `status`, `detail`, `instance`, `extensions` (flattened `HashMap`)
  - `ProblemJson::new(type, title, status, detail)` constructor
  - `ProblemJson::with_instance(uri)` builder method
  - `ProblemJson::extend(key, value)` for inserting extension members (e.g. `trace_id`)
  - `From<ApiError> for ProblemJson` — converts `request_id` → `instance`, `errors` → extension
  - `IntoResponse` for `ProblemJson` (requires `axum` feature) with `application/problem+json` Content-Type
  - `utoipa::ToSchema` and `schemars::JsonSchema` derives (requires respective features)
  - `problem_json` runnable example

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

[Unreleased]: https://git.brefwiz.com/brefwiz/api-bones/compare/v2.0.0...HEAD
[2.0.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.10.0...v2.0.0
[1.10.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.9.0...v1.10.0
[1.9.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.8.0...v1.9.0
[1.8.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.7.0...v1.8.0
[1.7.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.6.0...v1.7.0
[1.6.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.5.0...v1.6.0
[1.5.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.4.0...v1.5.0
[1.4.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.3.1...v1.4.0
[1.3.1]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.3.0...v1.3.1
[1.3.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.2.0...v1.3.0
[1.2.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.1.0...v1.2.0
[1.1.0]: https://git.brefwiz.com/brefwiz/api-bones/compare/v1.0.0...v1.1.0
[1.0.0]: https://git.brefwiz.com/brefwiz/api-bones/releases/tag/v1.0.0
