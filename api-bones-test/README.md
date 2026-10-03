# api-bones-test

Test helpers for [api-bones](https://github.com/brefwiz/api-bones) consumers:
builders for api-bones types and assertion helpers for axum and reqwest
responses, so services share one test vocabulary.

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
