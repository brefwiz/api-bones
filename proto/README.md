# `api-bones/proto/` — canonical proto shapes

Companion to the `api-bones` Rust crate. The Rust crate (`./src/`)
ships canonical types for the utoipa-legacy wire surface; this
directory ships canonical `.proto` shapes for the proto-source wire
surface (ADR-0085).

## Layout

```
proto/bones/v1/
├── pagination.proto    # PageRequest { cursor, page_size }
│                       # PageResponse { next_cursor, revision }
└── (more as needed)
```

Versioning lives in the proto package name (`bones.v1`, `bones.v2`,
...). Field-number additions are backward-compatible; renames /
deletions are not — bump the package major.

## How services consume these

Services do NOT vendor these files into their own repos.
`service-kit-envelope-build` stages them onto the protoc include path
at build time (alongside `envelope/v1/conventions.proto`). Service
authors write:

```proto
syntax = "proto3";
package myservice.v1;

import "bones/v1/pagination.proto";

message ListFoosRequest {
  bones.v1.PageRequest page = 1;
  // ...service-specific filters
}

message ListFoosResponse {
  repeated Foo items = 1;
  bones.v1.PageResponse page = 2;
}
```

`connectrpc-build` (driven by `service-kit-envelope-build`) generates
the Rust bindings for `bones.v1.PageRequest` / `PageResponse` inside
each consuming service's `*-api-types` crate. There is no published
`api-bones-proto` Rust crate — generated types are scoped to the
consuming crate, the standard buf/connectrpc/protobuf pattern.

## When to add a new shape here

Add a `bones.v1` message when it would otherwise be **duplicated
verbatim** across two or more brefwiz services. The bar is:

1. Identical field shape + semantics across services (not just similar).
2. Cross-service SDK consumers benefit from one type instead of N.
3. Versioning discipline is maintainable (proto3 backward-compat
   rules apply).

If the shape varies per service, it doesn't belong here.

## Versioning

This directory is **not** under api-bones' Rust crate semver. The
proto files are repo-level resources. Changes go through the same
PR/review flow as any platform contract change. Breaking changes
bump the package version (`bones.v1` → `bones.v2`) and ship both
during the migration window.

Ref: ADR-0085 (proto-source canonical surface).
