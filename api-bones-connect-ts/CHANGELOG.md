# Changelog

## [Unreleased]

### Fixed

- A call refused for rate limiting (`resource_exhausted` with `Retry-After`) no longer gives up on a burst or holds for a whole window. Any call, whatever its idempotency, waits the server's pushback plus up to 25% jitter and is replayed without drawing on the retry budget, since a throttle is refused before the handler runs; a refusal without `Retry-After` is never replayed. Calls to the same origin queue behind one refusal instead of each being refused, and calls through a relative or absent base URL share one gate per transport. A pushback longer than `MAX_THROTTLE_WAIT_MS` (5 s) fails fast. Once attempts are spent the call rejects with the exported `RateLimitedError` (a `resource_exhausted` error carrying `retryAfterMs`), never `unauthenticated`. No configuration is required.
- The web transport keeps the last CSRF synchronizer the server named in `sessionStorage` per origin, so a cold page load's first unsafe call carries it instead of being refused and replayed. `forgetCsrfSynchronizer(baseUrl)` (from `/web`) drops it on logout; an Unauthenticated answer drops it too.
- Share CSRF synchronizers and not-delivered evidence across bundled package copies on one page, preserving BFF origin isolation.

### Changed

- Refuse generated List and declared finite-list stream methods without a paging policy entry, naming the method in `PagingPolicyError`.
- **BREAKING** Change `bones.v1.Total.Kind` proto3 JSON names from `ESTIMATE` and `EXACT` to `KIND_ESTIMATE` and `KIND_EXACT`, and map page total kinds through the generated enum. Wire numbers 1 and 2 remain unchanged.

### Added

- Add `mapListCall(call, f)`: a lazy `ListCall<T>` to `ListCall<U>` adapter so SDK wrappers return domain types from `pagedClient` lists. Page metadata, cache flags and `span` pass through; nothing is buffered.
- Add `createPlatformClient(service, transport, policy)` and `PlatformClient` for generated SDK composition: batching, paging and one identity-bound revision cache per client. Preserve the two-argument raw constructor.
- Export the generated `bones.v1.Total` schema as `TotalSchema`, `Total_Kind` and `TotalMessage` from the package root.

## [0.3.2](https://git.brefwiz.com/brefwiz/api-bones/releases/tag/sdk/v0.3.2) - 2026-09-14

### ❤️ Thanks to all contributors! ❤️

@gsalingu, @release-bot

### Misc

- chore: release [[#33](https://git.brefwiz.com/brefwiz/api-bones/pulls/33)]
- fix/connect-node-records-tls-state [[#34](https://git.brefwiz.com/brefwiz/api-bones/pulls/34)]
- feat(bones): a service declares the capabilities it serves and requires [[#32](https://git.brefwiz.com/brefwiz/api-bones/pulls/32)]
- fix/publish-to-brefwiz-registry [[#30](https://git.brefwiz.com/brefwiz/api-bones/pulls/30)]

## [0.3.1](https://git.brefwiz.com/brefwiz/api-bones/releases/tag/sdk/v0.3.1) - 2026-09-08

### ❤️ Thanks to all contributors! ❤️

@gsalingu, @release-bot

### Misc

- chore: release [[#29](https://git.brefwiz.com/brefwiz/api-bones/pulls/29)]
- gherkin-library-surface [[#27](https://git.brefwiz.com/brefwiz/api-bones/pulls/27)]
- npm-axis-ready-release-go [[#26](https://git.brefwiz.com/brefwiz/api-bones/pulls/26)]
- library-parity-enrolment [[#25](https://git.brefwiz.com/brefwiz/api-bones/pulls/25)]
- fix-connection-failure-unavailable [[#24](https://git.brefwiz.com/brefwiz/api-bones/pulls/24)]
- chore/dependency-consolidation [[#23](https://git.brefwiz.com/brefwiz/api-bones/pulls/23)]
- feat(connect): acquire client TLS from the Workload API by default [[#22](https://git.brefwiz.com/brefwiz/api-bones/pulls/22)]
- chore: release [[#15](https://git.brefwiz.com/brefwiz/api-bones/pulls/15)]
- fix(protos): keep capability examples generic [[#18](https://git.brefwiz.com/brefwiz/api-bones/pulls/18)]
- feat(protos): declare provider capabilities on services [[#17](https://git.brefwiz.com/brefwiz/api-bones/pulls/17)]
