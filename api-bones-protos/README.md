# api-bones-protos

[![crates.io](https://img.shields.io/crates/v/api-bones-protos.svg)](https://crates.io/crates/api-bones-protos)
[![docs.rs](https://img.shields.io/docsrs/api-bones-protos)](https://docs.rs/api-bones-protos)

Embedded bytes of the canonical proto shapes shipped by the
[`api-bones`](https://crates.io/crates/api-bones) crate ecosystem
(`bones/v1/*.proto`).

**Zero runtime dependencies.** This crate exists purely as a
distribution mechanism for the proto bytes. Pair it with a staging
library (e.g.
[`proto-build-kit`](https://crates.io/crates/proto-build-kit)) to
write the bytes to a tempdir at protoc-relative paths at consumer
build time:

```rust,no_run
use proto_build_kit::Stager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let staged = Stager::new()
        .with(api_bones_protos::files())
        .stage()?;

    // Add staged.path() to your codegen include path:
    connectrpc_build::Config::new()
        .files(&["proto/my/v1/svc.proto"])
        .includes(&["proto/", staged.path()])
        .include_file("_connectrpc.rs")
        .compile()?;
    Ok(())
}
```

## What's included

| File                              | Messages |
|-----------------------------------|----------|
| `bones/v1/pagination.proto`       | `PageRequest`, `PageResponse`, `OffsetPageRequest`, `OffsetPageResponse` |
| `bones/v1/queries.proto`          | `SortDirection`, `SortField`, `SortParams`, `FilterOp`, `FilterEntry`, `FilterParams`, `SearchParams` |
| `bones/v1/errors.proto`           | `ValidationFailure` |
| `bones/v1/ratelimit.proto`        | `RateLimitInfo` |

See `proto/README.md` in the api-bones repo for the canonical
schema inventory and the per-message Rust counterparts in the
`api-bones` crate.

## License

MIT.
