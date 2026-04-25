# Changelog

All notable changes to `@brefwiz/api-bones-axios` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this package adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [4.4.0] — 2026-04-25

### Changed

- Version aligned with the `api-bones` workspace (`4.4.0`). No functional changes.

## [0.1.1] — 2026-04-24

### Fixed
- `addEnvelopeUnwrapInterceptor` now accepts the real axios default export (`AxiosStatic`) and any `AxiosInstance` without a type error. Previously, the structural `AxiosLikeInstance` type used `AxiosInterceptorManager<EnvelopeAxiosResponse>` which was incompatible with axios's native `AxiosInterceptorManager<AxiosResponse>` instantiation.
- Removed the unused `[key: string]: unknown;` index signature from `EnvelopeAxiosRequestConfig`; it blocked assignment from axios's `InternalAxiosRequestConfig`.

## [0.1.0] — 2026-04-23

### Added
- Initial release: `addEnvelopeUnwrapInterceptor`, `getEnvelopeMeta`, `getEnvelopeLinks`, and structural types for the `api_bones::response::ApiResponse` envelope.
