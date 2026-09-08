# Changelog

All notable changes to `@brefwiz/api-bones-connect` are documented here.

This file is maintained by ready-release-go from the commits that land on
main; entries below the first release heading are written by automation, not
by hand.

## Unreleased

### Added

- `isUnpromptedRetryable`, `isReplayableTransportFailure` and
  `connectionFailureAsUnavailable`, the retry-eligibility vocabulary paired
  with the Rust implementation and answered by a shared parity corpus.

### Fixed

- A request that never reached the server is reported as `Unavailable` rather
  than `Internal`, so a caller can tell a failed hop from a server fault — and
  from a refusal the server actually sent.
