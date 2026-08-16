# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Capability annotation examples now use generic service vocabulary only.

## [0.4.1](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.4.0...api-bones-protos-v0.4.1) - 2026-08-15

### Fixed

- *(protos)* reserve recycled AuthzKind numbers, renumber PUBLIC/AUTHENTICATED

## [0.4.0](https://git.brefwiz.com/brefwiz/api-bones/compare/api-bones-protos-v0.3.0...api-bones-protos-v0.4.0) - 2026-08-15

### Added

- *(protos)* [**breaking**] gate RPCs on capability, remove principal-class authz

### Fixed

- *(release)* publish the npm surface to the registry its consumers resolve
