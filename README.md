# api-bones

Opinionated REST API types: errors (RFC 9457), pagination, health checks, and more. No HTTP client, no business logic — just types.

## Usage

```toml
[dependencies]
api-bones = "2.0"
```

## Satellite Crates

| Crate | Description |
|---|---|
| [`api-bones-tower`](api-bones-tower/) | Tower `RequestIdLayer` and `ProblemJsonLayer` middleware |
| [`api-bones-reqwest`](api-bones-reqwest/) | Reqwest client extensions (`RequestBuilderExt`, `ResponseExt`) |

## Types

### Errors (`error`)

| Type / Item | Description |
|---|---|
| `ApiError` | RFC 9457 Problem Details error response |
| `ErrorCode` | Machine-readable error code enum; serializes as a type URI |
| `ErrorTypeMode` | Controls whether error `type` renders as a URL or URN (`std` only) |
| `ValidationError` | Single field-level validation error, carried in `ApiError::errors` |
| `error_type_mode()` | Returns the active `ErrorTypeMode` — requires `std` feature |
| `set_error_type_mode()` | Override the error type mode at startup — requires `std` feature |
| `urn_namespace()` | Returns the active URN namespace |

`ErrorCode` variants and their HTTP status codes:

| Variant | Status |
|---|---|
| `BadRequest` | 400 |
| `ValidationFailed` | 400 |
| `Unauthorized` | 401 |
| `InvalidCredentials` | 401 |
| `TokenExpired` | 401 |
| `TokenInvalid` | 401 |
| `Forbidden` | 403 |
| `InsufficientPermissions` | 403 |
| `ResourceNotFound` | 404 |
| `MethodNotAllowed` | 405 |
| `NotAcceptable` | 406 |
| `RequestTimeout` | 408 |
| `Conflict` | 409 |
| `ResourceAlreadyExists` | 409 |
| `Gone` | 410 |
| `PreconditionFailed` | 412 |
| `PayloadTooLarge` | 413 |
| `UnsupportedMediaType` | 415 |
| `UnprocessableEntity` | 422 |
| `RateLimited` | 429 |
| `PreconditionRequired` | 428 |
| `RequestHeaderFieldsTooLarge` | 431 |
| `InternalServerError` | 500 |
| `NotImplemented` | 501 |
| `BadGateway` | 502 |
| `ServiceUnavailable` | 503 |
| `GatewayTimeout` | 504 |

### Health (`health`)

Implements the IETF Health Check Response Format. Content-Type: `application/health+json`.

| Type | Description |
|---|---|
| `HealthStatus` | `pass` / `warn` / `fail` |
| `HealthCheck` | Individual component check result |
| `LivenessResponse` | `GET /health` — process-alive probe, always 200 |
| `ReadinessResponse` | `GET /health/ready` — dependency-aware probe, 503 on `fail` |

### Pagination (`pagination`)

| Type | Description |
|---|---|
| `PaginatedResponse<T>` | Offset-based response envelope with `data` + `pagination` |
| `PaginationParams` | Query params for offset pagination; fallible constructor enforces `limit` 1–100 |
| `CursorPaginatedResponse<T>` | Cursor-based response envelope |
| `CursorPagination` | Cursor metadata (`has_more`, `next_cursor`) |
| `CursorPaginationParams` | Query params for cursor endpoints; fallible constructor enforces `limit` 1–100 |
| `KeysetPaginatedResponse<T>` | Keyset-based response envelope |
| `KeysetPaginationParams` | Query params for keyset endpoints |

### Query (`query`)

| Type | Description |
|---|---|
| `SortParams` | `sort_by` + `direction` query params |
| `SortDirection` | `asc` / `desc` |
| `FilterEntry` | Single `field` + `operator` + `value` filter |
| `FilterParams` | Collection of `FilterEntry` values |
| `SearchParams` | Full-text search query; fallible constructor enforces 1–500 char limit |

### URL Building (`url`)

| Type | Description |
|---|---|
| `QueryBuilder` | Typed query parameter builder: `.set()`, `.set_opt()`, `.extend_from_struct()`, `.merge_into_url()` |
| `UrlBuilder` | Base URL builder |

### Links (`links`)

| Type | Description |
|---|---|
| `Link` | HATEOAS link with `rel`, `href`, optional `method` and `title` |
| `Links` | `Vec<Link>` with factory helpers (`self_link`, `next`, `prev`) |

### Response Envelopes (`response`)

| Type | Description |
|---|---|
| `ApiResponse<T>` | Generic envelope: `data`, `meta`, optional `links` |
| `ApiResponseBuilder<T>` | Builder for `ApiResponse` |
| `ResponseMeta` | Request ID, pagination info, timestamp |

### Identity Headers

| Type | Module | Header |
|---|---|---|
| `RequestId` | `request_id` | `X-Request-Id` |
| `CorrelationId` | `correlation_id` | `X-Correlation-Id` |
| `IdempotencyKey` | `idempotency` | `Idempotency-Key` |
| `TraceContext` | `traceparent` | `traceparent` (W3C) |

All implement the `HeaderId` trait (`as_str()`, `header_name()`).

### Auth (`auth` feature)

| Type | Description |
|---|---|
| `BearerToken` | `Authorization: Bearer <token>` |
| `BasicCredentials` | `Authorization: Basic <base64>` |
| `ApiKeyCredentials` | API key credential |
| `OAuth2Token` | OAuth2 access token with expiry |
| `AuthScheme` | Enum over all supported schemes |
| `Scope` | OAuth2 scope string |
| `Permission` | Permission string |

### Common Primitives (`common`)

| Type / Item | Description |
|---|---|
| `Timestamp` | RFC 3339 timestamp — `DateTime<Utc>` with `chrono`, `String` without |
| `ResourceId` | RFC 4122 UUID v4 — `uuid::Uuid` with `uuid` feature |
| `new_resource_id()` | Generate a new `ResourceId` (requires `uuid` feature) |
| `parse_timestamp()` | Parse an RFC 3339 string into `Timestamp` (requires `chrono` feature) |

### Other

| Type | Module | Description |
|---|---|---|
| `RateLimitInfo` | `ratelimit` | Rate limit headers metadata |
| `AuditInfo` | `audit` | Created/updated by + timestamps |
| `Slug` | `slug` | Validated URL-friendly identifier (1–128 chars, `[a-z0-9-]`) |
| `ETag` | `etag` | Entity tag for conditional requests (RFC 7232) |
| `IfMatch` / `IfNoneMatch` | `etag` | Conditional request headers |
| `BulkRequest<T>` | `bulk` | Batch write request |
| `BulkResponse<T>` | `bulk` | Batch write results |
| `CacheControl` | `cache` | `Cache-Control` header |
| `Cursor` | `cursor` | Opaque, optionally HMAC-signed pagination cursor |

## Features

| Feature | Default | Description |
|---|---|---|
| `std` | ✅ | Full feature set; enables `HashMap`-backed types and `std::error::Error` |
| `alloc` | ✅ | Heap types (`String`, `Vec`) for `no_std` contexts |
| `serde` | ✅ | `Serialize`/`Deserialize` on all types |
| `uuid` | ✅ | `ResourceId = uuid::Uuid`; UUID fields on `ApiError` |
| `chrono` | ✅ | `Timestamp = DateTime<Utc>` |
| `validator` | ✅ | `#[validate]` derives for framework-level validation |
| `axum` | ❌ | `IntoResponse` + `FromRequestParts` impls |
| `utoipa` | ❌ | `ToSchema`/`IntoParams` derives for OpenAPI generation |
| `schemars` | ❌ | `JsonSchema` derives |
| `auth` | ❌ | Auth scheme types; requires `alloc` + `base64` + `zeroize` |
| `base64` | ❌ | `Vec<u8>` ↔ Base64 serde helpers |
| `hmac` | ❌ | HMAC-SHA256 signing for `Cursor` |
| `http` | ❌ | `http::HeaderName`/`HeaderValue` conversions |
| `fake` | ❌ | `fake` crate `Dummy` impls for all types |
| `arbitrary` | ❌ | `arbitrary::Arbitrary` impls |
| `proptest` | ❌ | `proptest` strategy impls |

### `no_std` Support

```toml
# no_std + alloc (WASM, embedded with allocator)
api-bones = { version = "2", default-features = false, features = ["alloc"] }

# pure no_std (core types only)
api-bones = { version = "2", default-features = false }
```

## Example

```rust
use api_bones::{ApiError, ErrorCode};

fn find_booking(id: u64) -> Result<(), ApiError> {
    Err(ApiError::not_found(format!("booking {id} not found")))
}
```

Wire format (RFC 9457):

```json
{
  "type": "urn:api-bones:error:resource-not-found",
  "title": "Resource Not Found",
  "status": 404,
  "detail": "booking 42 not found"
}
```

## License

Apache 2.0
