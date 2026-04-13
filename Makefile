# Makefile for my-service-types

.PHONY: help fmt ci-format ci-lint ci-test ci-audit build clean

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

fmt: ## Format code
	cargo fmt --all

ci-format: ## Check formatting (CI)
	cargo fmt --all -- --check

ci-lint: ## Run Clippy (CI — zero warnings)
	cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings

ci-test: ## Run tests (CI)
	cargo test --workspace --all-features

build: ## Build the crate
	cargo build --release

ci-audit: ## Run cargo audit (PLATFORM-008 — strict, no default ignores)
	cargo audit

clean: ## Clean build artifacts
	cargo clean
