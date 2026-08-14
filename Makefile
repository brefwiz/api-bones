# Makefile for api-bones

.PHONY: help fmt ci-format ci-lint ci-no-std ci-test ci-coverage ci-audit ci-deny build clean \
	proto-lint proto-breaking ci-release-readiness spec-check \
	ci-build-check sdk-e2e-check sdk-e2e-prebuild sc-001-check \
	lockfile ci-lockfile-diff

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
	# --profile ci selects the JUnit-emitting profile the test composite consumes.
	# Without it the suite passes and the job still fails, on a missing artifact
	# rather than a failing test.
	cargo nextest run --workspace --all-features --profile ci

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
# INTO verification for the root crate.
#
# Satellite crates (those with a path dep on api-bones) cannot be verify-compiled
# until the parent version is live on crates.io — the verify step substitutes the
# path dep with the registry version. They are packaged (checking include-list
# completeness, the main failure mode) but compiled via ci-test / ci-lint instead.
PUBLISHABLE_ROOT   := api-bones
PUBLISHABLE_SATS   := api-bones-connect api-bones-tower api-bones-reqwest api-bones-progenitor api-bones-sdk-gen api-bones-test api-bones-protos
PUBLISHABLE_CRATES := $(PUBLISHABLE_ROOT) $(PUBLISHABLE_SATS)

ci-release-readiness: ## CI: package-verify root, package-only satellites (catches broken include paths)
	@set -eu; \
	echo "==> packaging + verifying $(PUBLISHABLE_ROOT)..."; \
	cargo package -p "$(PUBLISHABLE_ROOT)" --allow-dirty; \
	for crate in $(PUBLISHABLE_SATS); do \
		echo "==> packaging (no-verify) $$crate..."; \
		cargo package -p "$$crate" --allow-dirty --no-verify; \
	done; \
	echo "==> all publishable crates packaged."

.PHONY: lockfile
lockfile: ## Regenerate Cargo.lock
	cargo generate-lockfile

.PHONY: ci-lockfile-diff
ci-lockfile-diff: ## Assert Cargo.lock satisfies Cargo.toml
	# `cargo generate-lockfile` re-resolves against the live registry, which makes
	# this a moving target: the repo goes red the moment any unrelated transitive
	# dependency publishes a patch, and the only way out is an unrelated re-lock.
	# `--locked` fails only when Cargo.lock genuinely no longer satisfies
	# Cargo.toml, which is the thing worth gating. (ADR-0021; see
	# ci-workflows/build/brefwiz-service.mk.)
	@cargo metadata --locked --format-version 1 >/dev/null

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
	: "Mirrors policies/spec.schema.json's wire_surface enum, which owns this"; \
	: "list; 'library' was added there and this copy had drifted behind it."; \
	VALID="proto-source utoipa-legacy mixed-transition library"; \
	[ -f "$$SPEC" ] || { echo "ERROR: $$SPEC missing (ADR-0086 L1)"; exit 1; }; \
	WS=$$(awk 'BEGIN{f=0}/^---/{f=!f;next}f&&/^wire_surface:/{print $$2;exit}' "$$SPEC"); \
	[ -n "$$WS" ] || { echo "ERROR: wire_surface field missing (ADR-0086 L1)"; exit 1; }; \
	echo "$$VALID" | tr ' ' '\n' | grep -qx "$$WS" \
		|| { echo "ERROR: wire_surface='$$WS' invalid. Must be one of: $$VALID"; exit 1; }; \
	echo "spec-check OK: wire_surface=$$WS"

# ─── TypeScript packages ────────────────────────────────────────────────────

.PHONY: ts-build ts-test ts-lint
# Every TypeScript package in the repo. Enumerated once and looped over rather
# than named per target: these were hardcoded to api-bones-otel alone, so
# api-bones-axios shipped without its build or tests ever running in CI despite
# having both scripts. A list makes adding a package a one-line change and makes
# an omission visible.
TS_PACKAGES := api-bones-otel api-bones-axios api-bones-connect-ts

.PHONY: canonical-check
canonical-check: ## Run the brefwiz canonical structural gates locally (ADR-0086)
	@if [ -f .ci-workflows/ci-scripts/canonical-check.sh ]; then \
		bash .ci-workflows/ci-scripts/canonical-check.sh; \
	elif [ -f /tmp/ci-workflows/ci-scripts/canonical-check.sh ]; then \
		bash /tmp/ci-workflows/ci-scripts/canonical-check.sh; \
	elif [ -d "$$HOME/git/ci-workflows/ci-scripts" ]; then \
		bash "$$HOME/git/ci-workflows/ci-scripts/canonical-check.sh"; \
	else \
		echo "canonical-check: helper not found in .ci-workflows, /tmp/ci-workflows or ~/git/ci-workflows — fetch ci-workflows first"; \
		exit 1; \
	fi

.PHONY: cds-lint
cds-lint: ## Validate .cds/workflows/ YAML via cdsctl — catches schema breakage that would otherwise make CDS silently drop the branch
	@if [ -f /tmp/ci-workflows/ci-scripts/cds-lint.sh ]; then \
		bash /tmp/ci-workflows/ci-scripts/cds-lint.sh; \
	elif [ -d "$$HOME/git/ci-workflows/ci-scripts" ]; then \
		bash "$$HOME/git/ci-workflows/ci-scripts/cds-lint.sh"; \
	else \
		echo "cds-lint: helper not found in /tmp/ci-workflows or ~/git/ci-workflows — fetch ci-workflows first"; \
		exit 1; \
	fi

.PHONY: ci-ts
ci-ts: ts-lint ts-build ts-test ## CI: the whole TypeScript lane in one target

.PHONY: ci-npm-publish
ci-npm-publish: ## Publish every npm package to the brefwiz registry
	# The scope is routed explicitly rather than left to the environment: without
	# it npm sends @brefwiz/* to GitHub Packages, which is where these used to
	# live and is exactly the split that made them unresolvable to consumers
	# scoped to this registry. publishConfig in each manifest agrees with it.
	@set -eu; \
	REGISTRY="https://git.brefwiz.com/api/packages/brefwiz/npm/"; \
	for pkg in $(TS_PACKAGES); do \
		echo "==> publish $$pkg"; \
		( cd $$pkg \
		  && npm config set @brefwiz:registry "$$REGISTRY" --location project \
		  && npm ci --no-audit --no-fund \
		  && npm run build \
		  && npm publish --access public ); \
	done

ts-build: ## Build TypeScript packages
	@set -e; for pkg in $(TS_PACKAGES); do \
		echo "==> build $$pkg"; \
		( cd $$pkg && npm install --no-audit --no-fund && npm run build ); \
	done

ts-test: ## Test TypeScript packages
	@set -e; for pkg in $(TS_PACKAGES); do \
		echo "==> test $$pkg"; \
		( cd $$pkg && npm install --no-audit --no-fund && npm run test ); \
	done

ts-lint: ## Lint TypeScript packages (format check + biome)
	@set -e; for pkg in $(TS_PACKAGES); do \
		( cd $$pkg && npm install --no-audit --no-fund ); \
	done

.PHONY: pre-commit
pre-commit: ci-format ci-lint ci-test ci-changelog canonical-check cds-lint ## Run all pre-commit checks (ADR-0021)

.PHONY: ci-changelog
ci-changelog: ## CI: verify CHANGELOG.md has entry for current package version (ADR-0021)
	@curl -fsSL https://raw.githubusercontent.com/brefwiz/shared-ci-workflows/main/scripts/check-release-changelog.sh | bash
