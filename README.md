# api-bones

When you're building a platform — not just one service, but a cohesive family of applications — every team (or AI agent) eventually invents their own error format, their own pagination shape, their own health check response. They all look slightly different. Clients have to handle each variation. SDK generation becomes guesswork. And the moment you want to generate polyglot client libraries from your OpenAPI schemas, you discover there's no shared contract to generate *from*.

This problem is amplified in the AI era. An agent asked to scaffold a new service will invent its own conventions from scratch, every single time, unless there is a protocol to follow. The successful platform of this era is the one that gives agents and developers a unified language to build on — so inter-service communication is consistent whether the author is human or AI.

**api-bones is that unified protocol.** RFC-grounded, dependency-light types for the full surface area of a REST API: errors, pagination, health checks, auth headers, identity propagation, response envelopes, and more. No HTTP client, no framework opinions, no business logic — just types that compose cleanly across every service in your stack, regardless of who or what wrote it.

The entire brefwiz platform is built in Rust — not for idiomatic reasons, but because memory safety, zero-cost abstractions, and a type system that eliminates classes of bugs at compile time are non-negotiable properties at the infrastructure layer. Every crate enforces `unsafe_code = deny`, `warnings = deny`, and `clippy::pedantic = deny` at the workspace level. api-bones is no different.

## Who this is for

Any builder assembling a collection of services that need to speak the same language:

- **Platform engineers** standardizing error and response shapes across a microservice estate
- **Founding engineers** who don't want to invent these conventions from scratch on service #3
- **SDK authors** who want a well-typed OpenAPI foundation to generate polyglot clients from
- **WASM / embedded** targets — full `no_std` support, down to `core`-only if needed

## Usage

```toml
[dependencies]
api-bones = "4.0"
```

## Use cases

### Consistent errors across every service

Instead of each service inventing its own 404 body, every handler returns `ApiError::not_found(…)` and clients always get [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) Problem Details:

```rust
use api_bones::{ApiError, ErrorCode};

fn find_booking(id: u64) -> Result<(), ApiError> {
    Err(ApiError::not_found(format!("booking {id} not found")))
}
```

Wire format:

```json
{
  "type": "urn:api-bones:error:resource-not-found",
  "title": "Resource Not Found",
  "status": 404,
  "detail": "booking 42 not found"
}
```

The same shape. Every service. Every client. Always.

### Pagination that works the same everywhere

Offset, cursor, or keyset — all three patterns share a consistent envelope. A consumer that knows how to page through one endpoint knows how to page through all of them.

### Health checks your orchestrator already understands

`LivenessResponse` and `ReadinessResponse` implement the [IETF Health Check Response Format](https://datatracker.ietf.org/doc/html/draft-inadarei-api-health-check). Kubernetes, Nomad, whatever you're running — it reads the content-type and knows what to do.

### Polyglot SDK generation

Every type ships with `utoipa` (OpenAPI schema) and `schemars` (JSON Schema) support. Generate your OpenAPI spec, run your SDK generator, and the TypeScript / Python / Go client gets the same precise shapes your Rust handlers produce.

## Satellite Crates

| Crate | Description |
|---|---|
| [`api-bones-tower`](api-bones-tower/) | Tower `RequestIdLayer` and `ProblemJsonLayer` middleware |
| [`api-bones-reqwest`](api-bones-reqwest/) | Reqwest client extensions (`RequestBuilderExt`, `ResponseExt`) |

## Types

### Errors (`error`)

Implements [RFC 9457 Problem Details](https://www.rfc-editor.org/rfc/rfc9457).

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

Implements the [IETF Health Check Response Format](https://datatracker.ietf.org/doc/html/draft-inadarei-api-health-check). Content-Type: `application/health+json`.

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
api-bones = { version = "4", default-features = false, features = ["alloc"] }

# pure no_std (core types only)
api-bones = { version = "4", default-features = false }
```

## License

MIT — see [LICENSE](LICENSE)
