# `api-bones/proto/` — canonical proto shapes

Companion to the `api-bones` Rust crate. The Rust crate (`./src/`) ships
canonical types for the utoipa-derived HTTP+JSON wire surface; this
directory ships canonical `.proto` shapes for the proto-source wire
surface (gRPC, gRPC-Web, and Connect-protocol HTTP+JSON).

## Layout

```
proto/
├── buf.yaml               # lint + breaking-change rules
├── README.md
└── bones/v1/
    ├── pagination.proto   # PageRequest, PageResponse (cursor-based)
    │                      # OffsetPageRequest, OffsetPageResponse (offset/limit)
    ├── queries.proto      # SortDirection, SortField, SortParams
    │                      # FilterOp, FilterEntry, FilterParams
    │                      # SearchParams
    ├── errors.proto       # ValidationFailure
    └── ratelimit.proto    # RateLimitInfo
```

Versioning lives in the proto package name (`bones.v1`, `bones.v2`, ...).
Field-number additions are backward-compatible; renames / deletions are not
— bump the package major and run a parallel-window migration.

## Shape inventory + Rust counterparts

| Proto type                    | Rust counterpart                              | Notes |
|-------------------------------|-----------------------------------------------|-------|
| `bones.v1.PageRequest`        | `api_bones::CursorPaginationParams`           | Cursor-based (AIP-158, default for new APIs). |
| `bones.v1.PageResponse`       | `api_bones::CursorPagination` + revision      | `revision` doubles as list-level ETag. |
| `bones.v1.OffsetPageRequest`  | `api_bones::PaginationParams`                 | Offset/limit (admin UIs, small datasets). |
| `bones.v1.OffsetPageResponse` | `api_bones::PaginatedResponse<T>` envelope    | Total count + has_more. |
| `bones.v1.SortDirection`      | `api_bones::query::SortDirection`             | Asc/Desc enum. |
| `bones.v1.SortField`          | `api_bones::query::SortParams` (single field) | Per-field sort directive. |
| `bones.v1.SortParams`         | `api_bones::query::SortParams` (multi-field)  | Composes `SortField`s for multi-key sort. |
| `bones.v1.FilterOp`           | (proto-side discipline)                       | Closed enum — services MUST NOT accept off-list operators. The Rust counterpart keeps `operator` as a free string; proto locks it down. |
| `bones.v1.FilterEntry`        | `api_bones::query::FilterEntry`               | Field/op/value triple. |
| `bones.v1.FilterParams`       | `api_bones::query::FilterParams`              | AND-combined filter set. |
| `bones.v1.SearchParams`       | `api_bones::query::SearchParams`              | Free-text search with optional field scoping. |
| `bones.v1.ValidationFailure`  | `api_bones::error::ValidationError`           | Connect error detail; one per failing field. |
| `bones.v1.RateLimitInfo`      | `api_bones::ratelimit::RateLimitInfo`         | Typed response metadata (gRPC trailer / Connect response header). |

## How services consume these

Services import the shapes by package name:

```proto
syntax = "proto3";
package myservice.v1;

import "bones/v1/pagination.proto";
import "bones/v1/errors.proto";

message ListFoosRequest {
  bones.v1.PageRequest page = 1;
  // ...service-specific filters
}
message ListFoosResponse {
  repeated Foo items = 1;
  bones.v1.PageResponse page = 2;
}
```

Distribution to the consuming repo is the consumer's choice (git
submodule of this repo, buf module dependency once published to a
registry, vendored copies). Generated Rust / TS / Go bindings live
inside each consuming codebase — these `.proto` files are the source,
the bindings are downstream artifacts.

## Shapes deliberately NOT in `bones.v1`

The audit of the api-bones Rust surface against the bar "canonical AND a
wire shape" produced this skip list. Documented so future contributors
don't redo the audit:

| Rust type | Why no proto equivalent |
|---|---|
| `ApiResponse<T>`, `ResponseMeta`, `ApiError`, `ProblemJson` | Connect protocol ships its own response/error envelope; the proto wire shape doesn't redefine it. |
| `Link`, `Links` (HATEOAS) | Proto-source services navigate via RPC, not URLs. |
| `ETag`, `IfMatch`, `IfNoneMatch` | Just `string` on the proto wire; envelope semantics are usually annotation-driven on the response field rather than typed wrappers. |
| `IdempotencyKey`, `Cursor`, `Slug`, `OrgId`, `CorrelationId`, `RequestId` | Strong-typed Rust wrappers around `string`/`uuid`; proto uses the primitive directly. |
| `CacheControl`, `Vary`, `ContentType`, `HttpMethod`, `StatusCode`, `RangeHeader`, `ContentRange`, `ByteRange`, `HeaderName`, `HeaderValue`, `CorsOrigin`, `CorsHeaders` | HTTP-layer concerns. Proto/gRPC uses metadata, different model. |
| `HealthStatus`, `HealthCheck`, `LivenessResponse`, `ReadinessResponse` | Standardized by `grpc.health.v1` — services use `tonic-health` directly. |
| `BulkRequest<T>`, `BulkResponse<T>`, `BulkItemResult<T>` | Generics don't translate cleanly to proto. Bulk operations in proto-land model as streaming RPCs (per-item progress) rather than request/response with item arrays. Add per-service if needed. |
| `Deprecated`, `Example<T>`, `DeprecatedField` | utoipa/OpenAPI metadata helpers. Proto has its own field/method deprecation mechanism. |
| `Principal`, `PrincipalId`, `AuditInfo`, `ResolvedPrincipal`, `DeviceLease` | Server-side audit context types, not wire shapes. |
| `ApiVersion`, `SemverTriple` | Version strings — proto package version naming (`bones.v1`) is the canonical mechanism. |
| `TraceContext`, `TraceId`, `SpanId`, `SamplingFlags` | W3C Trace Context is standardized elsewhere; OpenTelemetry's proto definitions cover this. |
| `UrlBuilder`, `QueryBuilder`, `RetryPolicy`, `BackoffStrategy`, `RetryAfter` | Client-side helpers, never on the wire. |
| `ApiErrorBuilder`, `ApiResponseBuilder`, `HealthCheckBuilder`, `ReadinessResponseBuilder` | Rust builder types. |

If a service genuinely needs a canonical type from the skip list, add it
with a brief rationale here and a PR to bump the version.

## When to add a new shape

A new `bones.v1.*` message gets added when it would otherwise be
**duplicated verbatim** across two or more services. The bar:

1. Identical field shape + semantics across services (not just similar).
2. Cross-service SDK consumers benefit from one type instead of N.
3. Versioning discipline is maintainable (proto3 backward-compat rules apply).

If the shape varies per service, it doesn't belong here.

## CI

`make proto-lint` runs `buf lint`; `make proto-breaking` runs `buf
breaking` against `origin/main`. Both are gated by the canonical checks in
`.cds/workflows/main.yml`.
