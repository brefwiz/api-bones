# api-bones-reqwest

Reqwest client extensions for [api-bones](https://github.com/brefwiz/api-bones) types.

Provides `ErrorResponseExt` and retry logic for reqwest-based API clients.

## Compatibility

`ErrorResponseExt::rate_limit_info` returns `RateLimitInfo` from api-bones 9.
Consumers that exchange these public types must use the same api-bones 9
compatibility line: Cargo treats types from different major versions as distinct.

## Usage

```toml
[dependencies]
api-bones-reqwest = "2"
```

## License

MIT — see [LICENSE](LICENSE).
