# Makefile for api-bones

.PHONY: help fmt ci-format ci-lint ci-no-std ci-test ci-coverage ci-audit ci-deny ci-auto-tag build clean

.DEFAULT_GOAL := help

# Optional: path to a local advisory-db clone (used by ci-audit).
# Shared CI workflow clones the internal mirror and passes ADVISORY_DB=/tmp/advisory-db.
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

ci-auto-tag: ## CI: tag root Cargo.toml version if untagged, then dispatch release.yml. Expects GITEA_TOKEN, GITHUB_SERVER_URL, GITHUB_REPOSITORY.
	@VERSION=$$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2); \
	if [ -z "$$VERSION" ]; then \
	  echo "::error::Could not read version from root Cargo.toml"; \
	  exit 1; \
	fi; \
	git config user.name  "ci-bot"; \
	git config user.email "ci@brefwiz.com"; \
	if git ls-remote --tags origin | grep -q "refs/tags/$$VERSION$$"; then \
	  echo "Tag $$VERSION already exists — skipping."; \
	else \
	  echo "New version detected — tagging $$VERSION"; \
	  git tag -a "$$VERSION" -m "Release $$VERSION"; \
	  git push origin "$$VERSION"; \
	  echo "Tag $$VERSION pushed — dispatching release workflow."; \
	  curl -sf -X POST \
	    -H "Authorization: token $$GITEA_TOKEN" \
	    -H "Content-Type: application/json" \
	    "$$GITHUB_SERVER_URL/api/v1/repos/$$GITHUB_REPOSITORY/actions/workflows/release.yml/dispatches" \
	    -d "{\"ref\":\"$$VERSION\"}"; \
	fi

clean: ## Clean build artifacts
	cargo clean
