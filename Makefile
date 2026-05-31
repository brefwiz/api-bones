# Makefile for api-bones

.PHONY: help fmt ci-format ci-lint ci-no-std ci-test ci-coverage ci-audit ci-deny build clean \
	proto-lint proto-breaking ci-release-readiness spec-check \
	ci-build-check sdk-e2e-check sdk-e2e-prebuild sc-001-check

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

# ─── Canonical proto shapes ──────────────────────────────────────────────────

proto-lint: ## Lint proto/bones/v1/*.proto with buf
	buf lint api-bones-protos/proto

proto-breaking: ## Check api-bones-protos/proto/ for breaking changes vs origin/main
	@if git cat-file -e origin/main:api-bones-protos/proto/buf.yaml 2>/dev/null; then \
		buf breaking api-bones-protos/proto --against ".git#branch=origin/main,subdir=api-bones-protos/proto"; \
	elif git cat-file -e origin/main:proto/buf.yaml 2>/dev/null; then \
		echo "proto/ on origin/main — comparing against the pre-move location"; \
		buf breaking api-bones-protos/proto --against ".git#branch=origin/main,subdir=proto"; \
	else \
		echo "proto/ not present on origin/main yet — skipping breaking-change check"; \
	fi

# Release-readiness: run `cargo package` (with verify-compile, not just
# --list) for every publishable workspace member. Catches the failure mode
# where `include = [...]` doesn't actually ship the referenced files inside
# the .crate tarball — e.g. paths using `..` traversal that `cargo package`
# silently drops. This is the EXACT check `cargo publish` does, run at PR
# time so broken include lists fail in CI, not after a tag push.
#
# `--no-verify` would skip the unpack + compile step (the part that caught
# the missing proto bytes for api-bones-protos 0.1.0); we explicitly opt
# INTO verification.
PUBLISHABLE_CRATES := api-bones api-bones-tower api-bones-reqwest api-bones-progenitor api-bones-sdk-gen api-bones-test api-bones-protos

ci-release-readiness: ## CI: `cargo package` every publishable crate with full verify (catches broken include paths)
	@set -eu; \
	for crate in $(PUBLISHABLE_CRATES); do \
		echo "==> packaging + verifying $$crate..."; \
		cargo package -p "$$crate" --allow-dirty; \
	done; \
	echo "==> all publishable crates package-verified."

.PHONY: sc-001-check
sc-001-check: ## SC-001: ban tracing_subscriber::fmt in main/bin entry points
	@grep -rn 'tracing_subscriber::fmt().*try_init\|tracing_subscriber::fmt::Subscriber.*try_init' \
	  src/main.rs src/bin/*.rs 2>/dev/null \
	  && (echo 'SC-001 violation: use otel-bootstrap via service-kit instead' && exit 1) || true

.PHONY: ci-build-check
ci-build-check: ## Compile-check all feature combinations
	cargo check --all-features
	cargo check --no-default-features

.PHONY: sdk-e2e-check
sdk-e2e-check: ## No SDK E2E targets for this library crate
	@echo "sdk-e2e-check: no-op (library crate)"

.PHONY: sdk-e2e-prebuild
sdk-e2e-prebuild: ## No SDK E2E prebuild for this library crate
	@echo "sdk-e2e-prebuild: no-op (library crate)"

.PHONY: spec-check
spec-check: ## L1 ADR-0086: SPEC.md exists and wire_surface is valid
	@SPEC=SPEC.md; \
	VALID="proto-source utoipa-legacy mixed-transition"; \
	[ -f "$$SPEC" ] || { echo "ERROR: $$SPEC missing (ADR-0086 L1)"; exit 1; }; \
	WS=$$(awk 'BEGIN{f=0}/^---/{f=!f;next}f&&/^wire_surface:/{print $$2;exit}' "$$SPEC"); \
	[ -n "$$WS" ] || { echo "ERROR: wire_surface field missing (ADR-0086 L1)"; exit 1; }; \
	echo "$$VALID" | tr ' ' '\n' | grep -qx "$$WS" \
		|| { echo "ERROR: wire_surface='$$WS' invalid. Must be one of: $$VALID"; exit 1; }; \
	echo "spec-check OK: wire_surface=$$WS"

.PHONY: pre-commit
pre-commit: ci-format ci-lint ci-test ci-changelog ## Run all pre-commit checks (ADR-0021)

.PHONY: ci-changelog
ci-changelog: ## CI: verify CHANGELOG.md has entry for current package version (ADR-0021)
	@bash <(curl -fsSL https://raw.githubusercontent.com/brefwiz/shared-ci-workflows/main/scripts/check-release-changelog.sh)
