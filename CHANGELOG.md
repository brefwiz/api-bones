# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.3.1] - 2026-04-19

### Added

- `Principal::from_owned(String) -> Self` — construct a principal from a runtime-owned
  string for database round-trips. Accepts any string without UUID validation. Use only for
  deserialization; prefer `Principal::human` / `Principal::system` for new actors.

## [2.3.0] - 2026-04-18

### Added

- `OrgId` — UUID v4 tenant identifier newtype, propagated via the `X-Org-Id` HTTP header.
  Implements `HeaderId`, `FromStr`, `Display`, `From<Uuid>`, axum `FromRequestParts`
  (behind the `axum` feature), and serde. Requires the `uuid` feature.
- `OrganizationContext` — cross-cutting platform context bundle carrying `OrgId`,
  `Principal`, `RequestId`, `Vec<Role>`, and `Option<Attestation>` in a single
  cheap-to-clone struct. Constructor: `OrganizationContext::new(org_id, principal,
  request_id)`. Builder methods: `with_roles`, `with_attestation`. Serde derives gated on
  the `serde` feature.
- `Role(Arc<str>)` — token-neutral role label newtype. No permission semantics in
  api-bones; quorumauth owns permission evaluation.
- `Attestation { kind: AttestationKind, raw: Vec<u8> }` — opaque credential payload with
  kind tag. Downstream auth crates decode the raw bytes per kind.
- `AttestationKind` — `#[non_exhaustive]` enum: `Biscuit`, `Jwt`, `ApiKey`, `Mtls`.

## [2.2.1] - 2026-04-18

### Added

- `Principal::from_stored(String) -> Principal` — reconstruct a `Principal` from a
  previously persisted string (database column, serialised message). Accepts both UUID
  and system-name formats without validation, since the value was already validated at
  write time. Use on read paths only; prefer `human` or `system` for new principals.

## [2.2.0] - 2026-04-18

### Added

- `Principal::human(uuid: Uuid) -> Principal` — UUID-typed constructor for human/operator
  actors. Requires the `uuid` feature (default). Prevents PII (emails, display names) from
  entering audit logs and OTEL spans.
- `Principal::try_parse(&str) -> Result<Principal, PrincipalParseError>` — parses any
  valid UUID text form; rejects arbitrary strings, emails, and empty input.
- `PrincipalParseError` — error type returned by `try_parse`; exposes the offending input
  string. Implements `Display` and `std::error::Error`.
- `ResolvedPrincipal { id: Principal, display_name: Option<String> }` — read-path display
  helper that pairs an opaque `Principal` with an optional name resolved at read time by
  the identity service. Never persisted. Implements `Display` fallback to UUID, serde
  (skips `display_name` when `None`), and `From<Principal>`.

### Changed (BREAKING)

- `Principal::new(impl Into<String>)` **removed**. Passing arbitrary strings is a
  GDPR/PII violation (see issue #204). Migrate:
  - Human actors: `Principal::human(uuid)` or `Principal::try_parse(uuid_str)?`
  - System actors: `Principal::system("my-service.worker")` (unchanged)
- `fake` / `arbitrary` / `proptest` impls for `Principal` now generate UUID-backed values
  instead of random strings.

### Changed

- CI: enrolled in canonical `brefwiz/ci-workflows/.gitea/workflows/ci.yml@main`
  (previously called deprecated `rust-ci.yml` alias).

## [2.1.0] - 2026-04-16

### Added

- `audit::Principal` — canonical actor-identity newtype (`Cow<'static, str>`-backed).
  - `Principal::new(impl Into<String>)` for end-user/operator IDs.
  - `Principal::system(&'static str)` — `const`, infallible, zero-alloc for autonomous actors.
  - `as_str`, `Display`, non-redacting `Debug`, `Eq`, `Hash`, `Clone`.
  - Feature-gated integrations: `serde` (transparent string), `utoipa`, `schemars`,
    `arbitrary`, `proptest`.
- Re-export: `api_bones::Principal`.

### Changed (BREAKING)

- `AuditInfo::created_by` and `AuditInfo::updated_by` are now non-optional
  `Principal` fields (previously `Option<String>`). System processes are still
  actors and must declare themselves via `Principal::system`.
- `AuditInfo::new(created_at, updated_at, created_by: Principal, updated_by: Principal)`.
- `AuditInfo::now(created_by: Principal)` — `updated_by` now initialized to a
  clone of `created_by` rather than `None`.
- `AuditInfo::touch(&mut self, updated_by: Principal)` — no longer accepts
  `Option`.

### Migration

Downstream crates that previously passed `Option<String>` must migrate to
`Principal`:

```rust
// Before
AuditInfo::now(Some("alice".to_string()));
audit.touch(None);

// After
use api_bones::Principal;
AuditInfo::now(Principal::new("alice"));
audit.touch(Principal::system("my-service.cleanup"));
```

## [2.0.3] - 2026-04-10

### Added

- `api-bones-tower`: optional `uuid` and `chrono` feature passthroughs for parity with `api-bones-reqwest`
- `api-bones-tower`, `api-bones-reqwest`: rustdoc feature tables documenting default and optional `api-bones` features

### Changed

- `api-bones-tower`, `api-bones-reqwest`: versions aligned to `2.0.3`

## [2.0.2] - 2026-04-10

### Fixed

- `ApiError::causes` annotated with `#[schema(value_type = Vec<Object>)]` to prevent infinite recursion in utoipa schema generation (`causes: Vec<Self>` caused a stack overflow when any crate called `OpenApi::openapi()` with `ApiError` in its components)

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
