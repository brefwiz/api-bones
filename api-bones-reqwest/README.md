# api-bones-reqwest

Reqwest client extensions for [api-bones](https://github.com/brefwiz/api-bones) types.

Provides `ErrorResponseExt` and retry logic for reqwest-based API clients.

## Compatibility

The upcoming release-plz-derived 6.0.0 release uses `api-bones` 9 types in
its public API, including `RateLimitInfo` returned by `rate_limit_info`.
These types are incompatible with the `api-bones` 6 types exposed by
api-bones-reqwest 5. Upgrade consumers to the `api-bones` 9 compatibility
line when adopting this release.

## Usage

```toml
[dependencies]
api-bones-reqwest = "2"
```

## License

MIT — see [LICENSE](LICENSE).
