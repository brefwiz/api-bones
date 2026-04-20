# Makefile for api-bones

.PHONY: help fmt ci-format ci-lint ci-no-std ci-test ci-coverage ci-audit ci-deny build clean

.DEFAULT_GOAL := help

# Optional: path to a local advisory-db clone (used by ci-audit).
# CI may pass ADVISORY_DB=<path> to use a local advisory-db clone.
ADVISORY_DB ?=

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

fmt: ## Format code
	cargo fmt --all

ci-format: ## Check formatting (CI)
	cargo fmt --all -- --check

ci-lint: ## Run Clippy (CI — zero warnings)
	cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings

ci-no-std: ## Verify no_std compilation (core-only, alloc, alloc+serde regression guard for issue 80)
	cargo check --no-default-features
	cargo check --no-default-features --features alloc
	cargo check --no-default-features --features alloc,serde

ci-test: ## Run tests with nextest (CI)
	cargo nextest run --workspace --all-features

ci-coverage: ## Enforce 100% function coverage with llvm-cov + nextest (CI)
	cargo llvm-cov nextest --workspace --all-features --fail-under-functions 100

build: ## Build the crate
	cargo build --release

ci-audit: ## Run cargo audit (PLATFORM-008 — strict, no default ignores). Pass ADVISORY_DB=... to use a local clone.
	cargo audit $(if $(ADVISORY_DB),--db $(ADVISORY_DB),)

ci-deny: ## CI: dependency license audit
	cargo deny check licenses

clean: ## Clean build artifacts
	cargo clean
