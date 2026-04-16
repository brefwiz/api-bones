# Makefile for my-service-types

.PHONY: help fmt ci-format ci-lint ci-no-std ci-test ci-coverage ci-audit build clean

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

fmt: ## Format code
	cargo fmt --all

ci-format: ## Check formatting (CI)
	cargo fmt --all -- --check

ci-lint: ## Run Clippy (CI — zero warnings)
	cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings

ci-no-std: ## Verify no_std compilation (core-only and alloc)
	cargo check --no-default-features
	cargo check --no-default-features --features alloc

ci-test: ## Run tests with nextest (CI)
	cargo nextest run --workspace --all-features

ci-coverage: ## Enforce 100% function coverage with llvm-cov + nextest (CI)
	cargo llvm-cov nextest --workspace --all-features --fail-under-functions 100

build: ## Build the crate
	cargo build --release

ci-audit: ## Run cargo audit (PLATFORM-008 — strict, no default ignores)
	cargo audit

clean: ## Clean build artifacts
	cargo clean
