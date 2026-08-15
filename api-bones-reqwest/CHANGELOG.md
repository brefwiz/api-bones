# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [5.0.0](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v4.4.0...api-bones-reqwest-v5.0.0) - 2026-08-15

### Added

- *(connect)* add invalid_field, check_if_match, etag, parse_rfc3339, encode_json ([#86](https://git.brefwiz.com/brefwiz/api-bones/pulls/86))
- [**breaking**] canonical proto shapes — bones.v1 + FilterOp lock + api-bones-protos crate ([#61](https://git.brefwiz.com/brefwiz/api-bones/pulls/61))

### Fixed

- *(ci)* correct two MIT files claiming proprietary, and close three gates

### Other

- *(auth)* [**breaking**] remove auth/org_context/axum_extractors modules (fixes #49) ([#52](https://git.brefwiz.com/brefwiz/api-bones/pulls/52))
