# api-bones-reqwest

Reqwest client extensions for [api-bones](https://github.com/brefwiz/api-bones) types.

Provides `ErrorResponseExt` and retry logic for reqwest-based API clients.

## Compatibility

Public response types use api-bones 9. This dependency change requires the
api-bones-reqwest 6 breaking release; consumers must use api-bones 9 when
passing these types between crates.

## Usage

```toml
[dependencies]
api-bones-reqwest = "6"
```

## License

MIT — see [LICENSE](LICENSE).
