// SPDX-License-Identifier: MIT
// Type-checked against the built package through its own name: every promised
// export resolves, and the shapes below are pinned exactly.

import type * as T from "@brefwiz/api-bones-types";

export type AllExports = [
  T.ApiError,
  T.ValidationError,
  T.ErrorCode,
  T.PaginationParams,
  T.PaginatedResponse,
  T.CursorPagination,
  T.CursorPaginationParams,
  T.CursorPaginatedResponse,
  T.KeysetPaginationParams,
  T.KeysetPaginatedResponse,
  T.HealthStatus,
  T.HealthCheck,
  T.LivenessResponse,
  T.ReadinessResponse,
  T.SortDirection,
  T.SortParams,
  T.FilterOp,
  T.FilterEntry,
  T.FilterParams,
  T.SearchParams,
  T.ApiResponse,
  T.ResponseMeta,
  T.Link,
  T.Links,
  T.RateLimitInfo,
  T.ETag,
  T.IfMatch,
  T.IfNoneMatch,
  T.BulkRequest,
  T.BulkResponse,
  T.BulkItemResult,
  T.AuditInfo,
  T.Slug,
  T.BackoffStrategy,
  T.RetryPolicy,
  T.RequestId,
  T.CorrelationId,
  T.IdempotencyKey,
  T.TraceContext,
  T.TraceId,
  T.SpanId,
  T.SamplingFlags,
  T.Deprecated,
  T.CacheControl,
  T.ApiVersion,
  T.SemverTriple,
  T.StatusCode,
  T.PrincipalId,
  T.Principal,
  T.PrincipalKind,
  T.OrgId,
  T.Duration,
];

type Equal<A, B> = (<X>() => X extends A ? 1 : 2) extends <X>() => X extends B ? 1 : 2
  ? true
  : false;
type Expect<X extends true> = X;

export type SortDirectionShape = Expect<Equal<T.SortDirection, "asc" | "desc">>;
export type HealthStatusShape = Expect<Equal<T.HealthStatus, "pass" | "fail" | "warn">>;

// Generic types carry their payload as `unknown`.
export type PaginatedItems = Expect<Equal<T.PaginatedResponse["items"], unknown[]>>;
export type PaginatedTotal = Expect<Equal<T.PaginatedResponse["total_count"], number>>;
export type ApiResponsePayload = Expect<Equal<T.ApiResponse["data"], unknown>>;

// A newtype id is its own string definition, referenced by the owner.
export type PrincipalIdIsString = Expect<Equal<T.PrincipalId, string>>;
export type PrincipalIdIsReferenced = Expect<Equal<T.Principal["id"], T.PrincipalId>>;

// Objects are closed: no index signature, so the keys are exactly the fields.
export type SortParamsKeys = Expect<Equal<keyof T.SortParams, "sort_by" | "direction">>;
export type SortParamsField = Expect<Equal<T.SortParams["sort_by"], string>>;

// Every payload position is `unknown`; a flattened map keeps its object shape.
export type CursorPageData = Expect<Equal<T.CursorPaginatedResponse["data"], unknown[]>>;
export type KeysetPageItems = Expect<Equal<T.KeysetPaginatedResponse["items"], unknown[]>>;
export type KeysetAfter = Expect<Equal<T.KeysetPaginationParams["after"], unknown>>;
export type KeysetBefore = Expect<Equal<T.KeysetPaginationParams["before"], unknown>>;
export type BulkRequestItems = Expect<Equal<T.BulkRequest["items"], unknown[]>>;
export type BulkResponseResults = Expect<Equal<T.BulkResponse["results"], T.BulkItemResult[]>>;
export type BulkSuccessData = Expect<
  Equal<Extract<T.BulkItemResult, { status: "success" }>["data"], unknown>
>;

// The std duration referenced by `BackoffStrategy`.
export type DurationShape = Expect<Equal<T.Duration, { secs: number; nanos: number }>>;
