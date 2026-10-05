# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [6.0.0] - 2026-10-05

### Fixed

- **Breaking:** release on a new major version because public response types now come from `api-bones` 9 rather than 6. Consumers must use compatible `api-bones` 9 types.

## [5.0.20] - 2026-10-04

### Fixed

- *(release)* [**breaking**] declare public dependency compatibility releases

## [5.0.19] - 2026-10-03

### Other

- updated the following local packages: api-bones

## [5.0.18](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.17...api-bones-reqwest-v5.0.18) - 2026-10-02

### Other

- updated the following local packages: api-bones

## [5.0.17](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.16...api-bones-reqwest-v5.0.17) - 2026-10-02

### Other

- updated the following local packages: api-bones

## [5.0.16](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.15...api-bones-reqwest-v5.0.16) - 2026-10-01

### Other

- updated the following local packages: api-bones

## [5.0.15](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.14...api-bones-reqwest-v5.0.15) - 2026-09-29

### Other

- updated the following local packages: api-bones

## [5.0.14](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.13...api-bones-reqwest-v5.0.14) - 2026-09-29

### Other

- updated the following local packages: api-bones

## [5.0.13](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.12...api-bones-reqwest-v5.0.13) - 2026-09-29

### Other

- updated the following local packages: api-bones

## [5.0.12](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.11...api-bones-reqwest-v5.0.12) - 2026-09-29

### Other

- updated the following local packages: api-bones

## [5.0.11](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.10...api-bones-reqwest-v5.0.11) - 2026-09-29

### Other

- updated the following local packages: api-bones

## [5.0.10](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.9...api-bones-reqwest-v5.0.10) - 2026-09-28

### Other

- updated the following local packages: api-bones

## [5.0.9](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.8...api-bones-reqwest-v5.0.9) - 2026-09-28

### Other

- updated the following local packages: api-bones

## [5.0.8](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.7...api-bones-reqwest-v5.0.8) - 2026-09-28

### Other

- updated the following local packages: api-bones

## [5.0.7](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.6...api-bones-reqwest-v5.0.7) - 2026-09-28

### Other

- updated the following local packages: api-bones

## [5.0.6](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.5...api-bones-reqwest-v5.0.6) - 2026-09-28

### Other

- updated the following local packages: api-bones

## [5.0.5](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.4...api-bones-reqwest-v5.0.5) - 2026-09-26

### Other

- updated the following local packages: api-bones

## [5.0.4](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.3...api-bones-reqwest-v5.0.4) - 2026-09-26

### Other

- updated the following local packages: api-bones

## [5.0.3](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.2...api-bones-reqwest-v5.0.3) - 2026-09-26

### Fixed

- *(duplication)* parameterize the invalid-json-body test pair
- *(upstream-source-custody)* drop the mockito server after mock.assert_async
- *(upstream-source-custody)* remove five clean lint suppressions

## [5.0.2](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.1...api-bones-reqwest-v5.0.2) - 2026-09-25

### Other

- updated the following local packages: api-bones

## [5.0.1](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v5.0.0...api-bones-reqwest-v5.0.1) - 2026-09-24

### Fixed

- *(deps)* each published crate requires the api-bones it is built against

## [5.0.0](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-reqwest-v4.4.0...api-bones-reqwest-v5.0.0) - 2026-08-15

### Added

- *(connect)* add invalid_field, check_if_match, etag, parse_rfc3339, encode_json ([#86](https://git.brefwiz.com/brefwiz/api-bones/pulls/86))
- [**breaking**] canonical proto shapes — bones.v1 + FilterOp lock + api-bones-protos crate ([#61](https://git.brefwiz.com/brefwiz/api-bones/pulls/61))

### Fixed

- *(ci)* correct two MIT files claiming proprietary, and close three gates

### Other

- *(auth)* [**breaking**] remove auth/org_context/axum_extractors modules (fixes #49) ([#52](https://git.brefwiz.com/brefwiz/api-bones/pulls/52))
