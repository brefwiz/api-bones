# api-bones-reqwest

Reqwest client extensions for [api-bones](https://github.com/brefwiz/api-bones) types.

Provides `ErrorResponseExt` and retry logic for reqwest-based API clients.

## Compatibility

The next release requires `api-bones` 9, including the `RateLimitInfo`
returned by `rate_limit_info`. Consumers using the `api-bones` 6 types
exposed by api-bones-reqwest 5 must upgrade to the `api-bones` 9 line.
Release-plz derives the required api-bones-reqwest 6.0.0 breaking release.

## Usage

```toml
[dependencies]
api-bones-reqwest = "6"
```

## License

MIT — see [LICENSE](LICENSE).
