# Contributing

Thank you for your interest in contributing to api-bones!

## Before You Start

- Check [existing issues](https://github.com/brefwiz/api-bones/issues) to avoid duplicates.
- For significant changes, open an issue first to discuss the approach.

## Development Setup

```sh
git clone https://github.com/brefwiz/api-bones.git
cd api-bones
cargo build
cargo test --all-features
```

Requires Rust 1.85+. Install via [rustup](https://rustup.rs).

## Running Tests

```sh
# Full test suite
cargo test --workspace --all-features

# Specific crate
cargo test -p api-bones-tower

# no_std check
cargo check --no-default-features
cargo check --no-default-features --features alloc
```

## Code Style

This project enforces strict linting — `cargo clippy` runs at `deny` level for `all` and `pedantic`. Your PR must pass:

```sh
cargo fmt --check
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features
```

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`

Breaking changes must include `BREAKING CHANGE:` in the commit body or use `feat!:`/`fix!:`.

## Pull Requests

- One concern per PR.
- Include tests for new behaviour.
- Update `CHANGELOG.md` under `[Unreleased]`.

## License

By contributing, you agree your contributions are licensed under the [MIT License](LICENSE).
