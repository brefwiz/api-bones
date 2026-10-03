# api-bones-sdk-gen

CLI that generates a service's Rust and TypeScript SDKs from its OpenAPI
schema, with [api-bones](https://github.com/brefwiz/api-bones) `ApiResponse`
envelope handling on by default.

| Subcommand | Does |
|------------|------|
| `schema` | dumps the OpenAPI schema from a service binary |
| `rust` | generates the Rust progenitor SDK tree |
| `ts` | generates the TypeScript axios SDK tree |
| `all` | runs `schema`, `rust` and `ts` in sequence |

Run `api-bones-sdk-gen <subcommand> --help` for each one's arguments.

## License

MIT — see [LICENSE](https://github.com/brefwiz/api-bones/blob/main/LICENSE).
