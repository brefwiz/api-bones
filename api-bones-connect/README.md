# api-bones-connect

Connect RPC adapter primitives for [api-bones](https://crates.io/crates/api-bones) services (ADR-0096).

Re-exports the full `api_bones::connect` surface as a standalone crate for consumers who prefer a dedicated dependency over enabling the `connect` feature flag on `api-bones` directly.

## Usage

```toml
[dependencies]
api-bones-connect = "0.1"
```

```rust
use api_bones_connect::{
    DomainErrorKind, IntoDomainErrorKind, domain_to_connect,
    chrono_to_timestamp, parse_uuid, build_page, ConnectOptionExt as _,
};
```

See [api-bones docs](https://docs.rs/api-bones) for the full Connect module reference.

## License

MIT
