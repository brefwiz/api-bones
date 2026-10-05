# api-bones-test

Test helpers for [api-bones](https://github.com/brefwiz/api-bones) consumers:
builders for api-bones types and assertion helpers for axum and reqwest
responses, so services share one test vocabulary.

## Compatibility

The next release requires `api-bones` 9, including the `PaginatedResponse`
returned by response assertions. Consumers using the `api-bones` 6 types
exposed by api-bones-test 6 must upgrade to the `api-bones` 9 line.
Release-plz derives the required api-bones-test 7.0.0 breaking release.

| Feature | Adds |
|---------|------|
| `builders` (default) | pure-Rust builders, no IO |
| `axum` | `axum-test` assertion helpers and `TestServer`; enables `builders` |
| `reqwest` | reqwest assertion helpers; enables `builders` |
| `nats` | `JetStream` `AuditCapture` fixture; enables `builders` |

## Usage

```toml
[dev-dependencies]
api-bones-test = { version = "6", features = ["axum"] }
```

## License

MIT — see [LICENSE](https://github.com/brefwiz/api-bones/blob/main/LICENSE).
