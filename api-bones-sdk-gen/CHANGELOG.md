# Changelog

All notable changes to `api-bones-sdk-gen` are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.0] — 2026-04-24

### Added
- Post-processing step `rewrite_envelope_types`: rewrites `AxiosPromise<XxxResponse>` →
  `AxiosPromise<XxxResponseData>` in the generated `api.ts` so that `resp.data` is
  statically typed as the unwrapped payload, matching what the axios interceptor delivers
  at runtime. No `as any` casts needed in SDK consumers.
- Wrapper envelope interfaces are preserved but annotated `@deprecated` with a pointer to
  the inner data type.
- Three unit tests covering detection, rewriting, and the no-op path for non-envelope types.

## [0.1.0] — 2026-04-10

### Added
- Initial release: `schema`, `rust`, `ts`, `all`, and `makefile` subcommands.
- TypeScript SDK generation via `openapi-generator-cli 7.12.0` with automatic
  `@brefwiz/api-bones-axios` interceptor splicing into `index.ts`.
- Rust SDK generation via `api-bones-progenitor` with envelope-stripping `ClientHooks`.
- Shared `api-bones-sdk.mk` Makefile fragment emitted by `makefile` subcommand.
