# api-bones

Opinionated REST API types: errors (RFC 9457), pagination, health checks, and more. No HTTP client, no business logic — just types.

## Usage

Add the dependency:

```toml
[dependencies]
api-bones = "1.10"
```

## Types

### Errors (`error`)

| Type / Item | Description |
|---|---|
| `ApiError` | RFC 9457 Problem Details error response |
| `ErrorCode` | Machine-readable error code enum; serializes as a type URI |
| `ErrorTypeMode` | Controls whether error `type` renders as a URL or URN |
| `ValidationError` | Single field-level validation error, carried in `ApiError::errors` |
| `error_type_mode()` | Returns the active `ErrorTypeMode` (env-resolved or programmatic) |
| `set_error_type_mode()` | Override the error type mode at startup |
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
| `Conflict` | 409 |
| `ResourceAlreadyExists` | 409 |
| `UnprocessableEntity` | 422 |
| `RateLimited` | 429 |
| `InternalServerError` | 500 |
| `ServiceUnavailable` | 503 |

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
| `Pagination` | Offset-based metadata (`total`, `page`, `per_page`, `total_pages`) |
| `PaginationParams` | Query params for offset pagination (defaults: `page=1`, `per_page=20`) |
| `CursorPaginatedResponse<T>` | Cursor-based response envelope |
| `CursorPagination` | Cursor metadata (`has_more`, `next_cursor`) |
| `PaginationQuery` | Query params for limit/offset endpoints (`limit`, `offset`) |

### Common primitives (`common`)

| Type / Item | Description |
|---|---|
| `Timestamp` | RFC 3339 timestamp — `DateTime<Utc>` with `chrono`, `String` without |
| `ResourceId` | RFC 4122 UUID v4 — `uuid::Uuid` with `uuid` feature |
| `new_resource_id()` | Generate a new `ResourceId` (requires `uuid` feature) |
| `parse_timestamp()` | Parse an RFC 3339 string into `Timestamp` (requires `chrono` feature) |

### Models (`models`)

| Type | Description |
|---|---|
| `ErrorResponse` | Simple `{"error": "..."}` error body |

## Features

| Feature  | Default | Description |
|---|---|---|
| `serde`  | ✅ | `Serialize`/`Deserialize` on all types |
| `uuid`   | ✅ | `ResourceId = uuid::Uuid`, UUID fields on `ApiError` |
| `chrono` | ✅ | `Timestamp = DateTime<Utc>` |
| `axum`   | ❌ | `IntoResponse` impl for `ApiError` (sets `application/problem+json`) |
| `utoipa` | ❌ | `ToSchema`/`IntoParams` derives for OpenAPI generation |

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
