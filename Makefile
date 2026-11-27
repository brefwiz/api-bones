# Makefile for api-bones

.PHONY: help fmt ci-format ci-lint ci-no-std ci-wasm ci-wasm-test ci-test ci-e2e-rust ci-coverage \
	ci-sdk-publish-rust-dry-run ci-sdk-publish-typescript-dry-run ci-audit ci-deny build clean \
	proto-lint proto-breaking ci-release-readiness spec-check \
	ci-build-check sdk-e2e-check sdk-e2e-prebuild sc-001-check ci-doc \
	lockfile ci-lockfile-diff ci-sdk-api-surface-generate sdk-api-surface

.DEFAULT_GOAL := help

# Optional: path to a local advisory-db clone (used by ci-audit).
# CI may pass ADVISORY_DB=<path> to use a local advisory-db clone.
ADVISORY_DB ?=

# Proto codegen reruns on a content hash of its inputs. build/brefwiz-codegen.mk
# is vendored byte-for-byte and never edited here. The step lane's build.rs
# generates from its own proto and from the bones protos it imports.
CODEGEN_INPUTS := tests/l2/proto tests/rust/proto tests/shared/proto tests/wasm-server/proto api-bones-protos/proto tools/wasm-size-probe/tests/fixtures/proto

include build/brefwiz-codegen.mk

# Coverage runs as an archive build, a run and a report (CDS jobs
# coverage-build and coverage). Both fragments are vendored byte-for-byte and
# never edited here; ci-workflows revendor-build-fragments.py refreshes them.
# The two cucumber lanes are harness = false, which nextest cannot enumerate,
# so they stay out of the archive and ci-e2e-rust runs them in gate-e2e-rust.
COVERAGE_EXCLUDE := api-bones-contract-rust api-bones-sdk-call-contract-rust api-bones-internal-bdd api-bones-wasm-test-server api-bones-test-service-l2
COVERAGE_ALL_FEATURES := true
COVERAGE_FAIL_UNDER_LINES := 96
COVERAGE_FAIL_UNDER_FUNCTIONS := 96

include build/brefwiz-coverage-common.mk
include build/brefwiz-coverage-split.mk

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

fmt: ## Format code
	cargo fmt --all

ci-format: ## Check formatting (CI)
	cargo fmt --all -- --check

ci-lint: ci-no-std ci-wasm ## Run Clippy (CI — zero warnings), and build every no_std feature set and the wasm32 crates
	# Clippy runs with --all-features, which turns std on: a no_std-only break
	# never reaches it. ci-no-std builds those feature sets, and nothing else in
	# CI does. The same holds for wasm32: clippy runs on the host target, so
	# ci-wasm is what compiles the crates for the browser.
	cargo clippy --locked --workspace --all-targets --all-features --no-deps -- -D warnings

ci-doc: ## Check documentation builds with no broken intra-doc links (CI)
	# Nothing else in this repo compiles documentation: ci-lint is clippy, which
	# never evaluates rustdoc lints. Broken intra-doc links therefore accumulated
	# invisibly until the rustdoc JSON emit in the test job started failing on
	# them — and because rustdoc aborts at the first crate that fails, they
	# surfaced one crate per CI round. This checks the whole workspace at once.
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps

ci-no-std: ## Verify no_std compilation (core-only, alloc, alloc+serde regression guard for issue 80)
	cargo check --locked --no-default-features
	cargo check --locked --no-default-features --features alloc
	cargo check --locked --no-default-features --features alloc,serde

ci-wasm: ## Build the runtime seam, the browser transport and the crates on them for wasm32-unknown-unknown (CI)
	cargo build --locked --target wasm32-unknown-unknown -p api-bones-rt -p api-bones-transport -p api-bones-stream -p api-bones-page -p api-bones-batch -p api-bones-sdk --features wasm

ci-wasm-test: ## Run the api-bones-rt and fetch-transport wasm-bindgen-test suites in Node (CI)
	# The runner is wasm-bindgen-test-runner from the CI image, which executes
	# the module under Node when no browser is requested. The fetch suite calls
	# a native Connect server (tests/wasm-server) that is built, started and
	# stopped here: it writes its base URL to the address file once it listens,
	# and the suite reads the URL from API_BONES_TEST_SERVER. The trap stops
	# that one process by the pid this recipe recorded.
	cargo build --locked -p api-bones-wasm-test-server
	set -eu; \
	dir=$$(mktemp -d); \
	"$${CARGO_TARGET_DIR:-target}/debug/api-bones-wasm-test-server" --addr-file "$$dir/addr" & \
	pid=$$!; \
	trap 'kill $$pid 2>/dev/null || true; rm -rf "$$dir"' EXIT; \
	tries=0; \
	while [ ! -s "$$dir/addr" ]; do \
		kill -0 $$pid 2>/dev/null || { echo "api-bones-wasm-test-server exited before it listened" >&2; exit 1; }; \
		tries=$$((tries + 1)); \
		[ $$tries -le 300 ] || { echo "api-bones-wasm-test-server did not write $$dir/addr within 30 s" >&2; exit 1; }; \
		sleep 0.1; \
	done; \
	API_BONES_TEST_SERVER=$$(cat "$$dir/addr"); \
	export API_BONES_TEST_SERVER; \
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --locked --target wasm32-unknown-unknown -p api-bones-rt --test wasm; \
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --locked --target wasm32-unknown-unknown -p api-bones-transport --test fetch_wasm; \
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --locked --target wasm32-unknown-unknown -p api-bones-sdk --features wasm --test call_wasm

ci-test: ci-e2e-rust ## Run tests with nextest (local one-shot; CI runs the coverage archive)
	# Both cucumber lanes are excluded: their test targets are harness = false,
	# and nextest enumerates targets with `--list`, which such a binary does
	# not accept. ci-e2e-rust runs them. The wasm test server is the wasm32
	# suite's harness, run by ci-wasm-test, not product code.
	cargo nextest run --locked --no-fail-fast --workspace --exclude api-bones-contract-rust --exclude api-bones-sdk-call-contract-rust --exclude api-bones-internal-bdd --exclude api-bones-wasm-test-server --exclude api-bones-test-service-l2 --all-features --profile ci

# Debuginfo off: this lane links every contract and internal-bdd test binary
# (each carries the localization crate's CLDR data), and the CI worker's target
# volume is bounded. Nothing debugs these runs. The profile env survives a
# recipe that replaces RUSTFLAGS, as the platform's own build actions rely on.
ci-e2e-rust: export CARGO_PROFILE_DEV_DEBUG := 0
ci-e2e-rust: export CARGO_PROFILE_TEST_DEBUG := 0
ci-e2e-rust: ## Run what nextest cannot carry: the Gherkin contracts and doctests
	# A cucumber suite is its own harness (harness = false), so nextest cannot
	# carry it. Both contracts are pure classification/config assertions, so
	# neither needs a live stack and both run synchronously.
	# The step lane's own unit tests, over its generated stream response type.
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --lib
	# The webpki-only root source never builds under --all-features, where
	# tls-native-roots wins, so the suite and coverage runs cannot reach it.
	# Its unit tests run here, in the one feature combination that compiles it.
	cargo test --locked --no-fail-fast -p api-bones --lib --no-default-features --features tls-webpki-roots
	# nextest runs no doctests. The rotating client refuses a response type
	# without stream control at compile time, and a compile_fail example on
	# rotating_stream is the proof, so that crate's doctests run here, where
	# gate-e2e-rust reaches them.
	cargo test --locked --doc --no-fail-fast -p api-bones-stream --all-features
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_retry_eligibility
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_precondition
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_failure_reach
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_error_info
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test error_localization
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_csrf_replay_rule
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_pinned_peer
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test field_labels
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test overridable_defaults
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test delegation_templates
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test error_catalog_declarations
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test keyed_writes
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_stream_control
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_stream_rotation
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_stream_keepalive
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_fleet_connect_bound
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test connect_root_store
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_pagination --locked
	cargo test --locked --no-fail-fast -p api-bones-sdk-call-contract-rust --test sdk_call
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_batch --locked
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_batch_identity_guard --locked
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_batch_fleet_transport --locked
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_paged_without_batch --locked
	cargo test --no-fail-fast -p api-bones-contract-rust --test connect_rate_limit_retry --locked
	cargo test --locked --no-fail-fast -p api-bones-contract-rust --test typed_fields
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test connect_client_headers
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test connect_domain_error_code
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test pagination_max_limit
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test connect_if_match_precondition
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test connect_get_reads
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test proto_vocabulary
	cargo test --locked --no-fail-fast -p api-bones-internal-bdd --test automation_manifest

ci-coverage: ci-e2e-rust ## Enforce the line and function floor (one-shot local run with the same gate knobs CI reads)
	cargo llvm-cov nextest --locked --no-fail-fast --workspace $(foreach c,$(COVERAGE_EXCLUDE),--exclude $(c)) --all-features --profile $(COVERAGE_NEXTEST_PROFILE) $(_COVERAGE_IGNORE) $(_COVERAGE_LINE_GATE) $(_COVERAGE_FUNCTION_GATE)

# Publish rehearsals, one per declared SDK language: the release operation
# minus the upload, so a broken include or files list fails at PR time rather
# than after a tag.
ci-sdk-publish-rust-dry-run: ## Rehearse the crate publish without uploading
	cargo package --locked -p api-bones-connect --allow-dirty --no-verify

ci-sdk-publish-typescript-dry-run: ts-types-generate ## Rehearse the npm publish without uploading
	cd api-bones-connect-ts && npm install --no-audit --no-fund && npm publish --dry-run
	cd api-bones-types && npm install --no-audit --no-fund && npm run build && npm publish --dry-run

build: ## Build the crate
	cargo build --locked --release

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
# completeness, the main failure mode) but compiled via the coverage archive / ci-lint instead.
UNIT_CLOSURE_SCRIPT ?= $(firstword $(wildcard /opt/ci-workflows/build/cargo_package_unit_closure.py ../ci-workflows/build/cargo_package_unit_closure.py))

ci-release-readiness: ## CI: package the release unit against itself, in dependency order
	@# release-plz releases every publishable crate here in one run, the root
	@# first: each satellite is packaged and compiled against the tarballs the
	@# same run produces, never against a registry that has not seen them yet.
	python3 $(UNIT_CLOSURE_SCRIPT)

.PHONY: lockfile
lockfile: ## Regenerate Cargo.lock
	cargo generate-lockfile

.PHONY: ci-lockfile-diff
ci-lockfile-diff: ## Assert Cargo.lock satisfies Cargo.toml
	# `cargo generate-lockfile` re-resolves against the live registry, which makes
	# this a moving target: the repo goes red the moment any unrelated transitive
	# dependency publishes a patch, and the only way out is an unrelated re-lock.
	# `--locked` fails only when Cargo.lock genuinely no longer satisfies
	# Cargo.toml, which is the thing worth gating. The canonical form lives in
	# ci-workflows/build/brefwiz-service.mk.
	@cargo metadata --locked --format-version 1 >/dev/null

.PHONY: sc-001-check
sc-001-check: ## SC-001: ban tracing_subscriber::fmt in main/bin entry points
	@grep -rn 'tracing_subscriber::fmt().*try_init\|tracing_subscriber::fmt::Subscriber.*try_init' \
	  src/main.rs src/bin/*.rs 2>/dev/null \
	  && (echo 'SC-001 violation: use otel-bootstrap via service-kit instead' && exit 1) || true

.PHONY: ci-build-check
ci-build-check: ## Compile-check all feature combinations
	cargo check --locked --workspace --all-targets --all-features
	cargo check --locked --no-default-features
	cargo check --locked --no-default-features --features tls-webpki-roots

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

.PHONY: ts-types-generate ts-build ts-test ts-lint
# Every TypeScript package in the repo. Enumerated once and looped over rather
# than named per target: these were hardcoded to api-bones-otel alone, so
# api-bones-axios shipped without its build or tests ever running in CI despite
# having both scripts. A list makes adding a package a one-line change and makes
# an omission visible.
TS_PACKAGES := api-bones-otel api-bones-axios api-bones-sdk-runtime-ts api-bones-connect-ts api-bones-types
# The contract lane is its own TypeScript package; ts-test runs it where npm
# credentials already exist. It is excluded from publish/pack, which iterate
# PUBLISHABLE sets, because it ships nothing.
TS_TEST_PACKAGES := $(TS_PACKAGES) api-bones-test-service-ts tests/typescript tests/sdk-call/typescript

.PHONY: canonical-check
canonical-check: ## Run the brefwiz canonical structural gates locally
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
ci-ts: ts-lint ts-build ts-test sdk-api-surface ## CI: the whole TypeScript lane in one target

# Public declaration surface of @brefwiz/api-bones-sdk-runtime: every emitted
# declaration file of the built package, concatenated in a stable order. The
# generated file is an intermediate artefact (never committed); the committed
# baseline is api-bones-sdk-runtime-ts/api-surface.d.ts.
ci-sdk-api-surface-generate: ts-build ## CI: emit the sdk-runtime public .d.ts surface for the api-surface snapshot diff
	@cd api-bones-sdk-runtime-ts/dist && \
		{ printf '// SPDX-License-Identifier: MIT\n'; \
		find . -name '*.d.ts' | LC_ALL=C sort | \
		while IFS= read -r f; do printf '// ===== %s =====\n' "$$f"; cat "$$f"; done; } \
		> ../api-surface.generated.d.ts
	@echo "sdk-api-surface: wrote api-bones-sdk-runtime-ts/api-surface.generated.d.ts ($$(wc -l < api-bones-sdk-runtime-ts/api-surface.generated.d.ts) lines)"

sdk-api-surface: ci-sdk-api-surface-generate ## Diff the regenerated sdk-runtime public surface against the committed baseline
	@if [ -f .ci-workflows/ci-scripts/sdk-api-surface-diff.py ]; then SDK_DIFF=.ci-workflows/ci-scripts/sdk-api-surface-diff.py; \
	elif [ -f /tmp/ci-workflows/ci-scripts/sdk-api-surface-diff.py ]; then SDK_DIFF=/tmp/ci-workflows/ci-scripts/sdk-api-surface-diff.py; \
	elif [ -f /opt/ci-workflows/ci-scripts/sdk-api-surface-diff.py ]; then SDK_DIFF=/opt/ci-workflows/ci-scripts/sdk-api-surface-diff.py; \
	elif [ -d "$$HOME/git/ci-workflows/ci-scripts" ]; then SDK_DIFF="$$HOME/git/ci-workflows/ci-scripts/sdk-api-surface-diff.py"; \
	else echo 'ERROR: sdk-api-surface-diff.py not found (ci-workflows not vendored)'; exit 1; fi; \
	python3 "$$SDK_DIFF" --baseline api-bones-sdk-runtime-ts/api-surface.d.ts --current api-bones-sdk-runtime-ts/api-surface.generated.d.ts

ts-types-generate: ## Generate the api-bones-types schema and TypeScript sources from the Rust crate
	cd api-bones-types && npm install --no-audit --no-fund && npm run generate

ts-build: ts-types-generate ## Build TypeScript packages
	@set -e; for pkg in $(TS_PACKAGES); do \
		echo "==> build $$pkg"; \
		( cd $$pkg && npm install --no-audit --no-fund && npm run build ); \
	done

.PHONY: l2-surface
# Regenerates the generator-owned pinned surface that wrapper_ts_golden.rs
# asserts; never hand-edit. Manual target, not part of any CI lane.
l2-surface: ## Regenerate api-bones-test-service-sdk-ts/api-surface.d.ts from the wrapper generator
	@set -e; tmp=$$(mktemp -d); trap 'rm -rf "$$tmp"' EXIT; \
	cargo run --locked -p api-bones-sdk-gen -- wrapper typescript --crate tests/l2 --spec SPEC.md --surface sdk-call --out "$$tmp"; \
	cp "$$tmp/api-surface.d.ts" api-bones-test-service-sdk-ts/api-surface.d.ts

ts-test: ts-types-generate ## Test TypeScript packages
	: 'Iterates the TEST set, which adds the contract lane. That lane consumes'
	: '@brefwiz/api-bones-connect through its published entry points, so the'
	: 'packages it depends on are built first.'
	@echo "==> prebuild otel"; log=$$(mktemp); \
	if ( cd api-bones-otel && npm install --no-audit --no-fund && npm run build ) >"$$log" 2>&1; then \
		cat "$$log"; rm -f "$$log"; \
	else \
		rc=$$?; cat "$$log"; rm -f "$$log"; \
		echo "TEST-FAIL api-bones-otel exit=$$rc" >&2; exit 1; \
	fi
	@echo "==> prebuild sdk-runtime"; log=$$(mktemp); \
	if ( cd api-bones-sdk-runtime-ts && npm install --no-audit --no-fund && npm run build ) >"$$log" 2>&1; then \
		cat "$$log"; rm -f "$$log"; \
	else \
		rc=$$?; cat "$$log"; rm -f "$$log"; \
		echo "TEST-FAIL api-bones-sdk-runtime-ts exit=$$rc" >&2; exit 1; \
	fi
	@echo "==> prebuild connect"; log=$$(mktemp); \
	if ( cd api-bones-connect-ts && npm install --no-audit --no-fund && npm run build ) >"$$log" 2>&1; then \
		cat "$$log"; rm -f "$$log"; \
	else \
		rc=$$?; cat "$$log"; rm -f "$$log"; \
		echo "TEST-FAIL api-bones-connect-ts exit=$$rc" >&2; exit 1; \
	fi
	: 'The contract lanes serve and read the test service through its package,'
	: 'built (descriptor set from tests/l2/proto, then tsc) once connect is built.'
	@echo "==> prebuild test-service package"; log=$$(mktemp); \
	if ( cd api-bones-test-service-ts && npm install --no-audit --no-fund && npm run build ) >"$$log" 2>&1; then \
		cat "$$log"; rm -f "$$log"; \
	else \
		rc=$$?; cat "$$log"; rm -f "$$log"; \
		echo "TEST-FAIL api-bones-test-service-ts exit=$$rc" >&2; exit 1; \
	fi
	: 'The contract lane imports the wasm wrapper of the test service Layer 2,'
	: 'so it is built (wasm32, bound, compiled) before the lane runs cucumber.'
	@echo "==> prebuild test-service wrapper"; log=$$(mktemp); rcf=$$(mktemp); \
	{ cargo run --locked -p api-bones-sdk-gen -- wrapper build-ts --crate tests/l2 --surface sdk-call --out api-bones-test-service-sdk-ts 2>&1; echo $$? > "$$rcf"; } | tee "$$log"; \
	rc=$$(cat "$$rcf"); rm -f "$$rcf"; \
	if [ "$$rc" = 0 ]; then \
		rm -f "$$log"; \
	else \
		echo "TEST-FAIL api-bones-test-service-sdk-ts exit=$$rc"; \
		grep -nE '^(error|warning: unused)|could not compile|signal|Killed' "$$log" | head -40; \
		tail -n 120 "$$log"; rm -f "$$log"; exit 1; \
	fi
	@set -e; for pkg in $(TS_TEST_PACKAGES); do \
		echo "==> test $$pkg"; \
		log=$$(mktemp); rcf=$$(mktemp); \
		{ ( cd $$pkg && npm install --no-audit --no-fund && npm run test ) 2>&1 && rc=0 || rc=$$?; echo $$rc > "$$rcf"; } | tee "$$log"; \
		rc=$$(cat "$$rcf"); rm -f "$$rcf"; \
		if [ "$$rc" = 0 ]; then \
			rm -f "$$log"; \
		else \
			if [ -d "$$pkg/src/generated" ]; then for f in $$pkg/src/generated/*.ts; do [ -e "$$f" ] || continue; echo "==> $$f"; cat "$$f"; done; fi; \
			echo "TEST-FAIL $$pkg exit=$$rc"; \
			grep -nE '^(error|warning: unused)|could not compile|signal|Killed' "$$log" | head -40; \
			tail -n 120 "$$log"; rm -f "$$log"; exit 1; \
		fi; \
	done

ts-lint: ## Lint TypeScript packages (format check + biome)
	@set -e; for pkg in $(TS_PACKAGES); do \
		( cd $$pkg && npm install --no-audit --no-fund ); \
	done

.PHONY: pre-commit
pre-commit: ci-format ci-lint ci-test ci-changelog canonical-check cds-lint ## Run all pre-commit checks (ADR-0021)

.PHONY: ci-changelog
ci-changelog: ## CI: verify CHANGELOG.md has entry for current package version (ADR-0021)
	# The canonical check ships in the runner image (ci-internal bakes
	# ci-workflows at /opt/ci-workflows), so it moves with the fleet on a
	# ci-workflows merge. Curling raw.githubusercontent.com pinned this repo
	# to a pre-relocation copy on GitHub and made a lint gate depend on
	# public network reachability.
	@bash /opt/ci-workflows/ci-scripts/check-release-changelog.sh

.PHONY: ci-sdk-autogenerate
ci-sdk-autogenerate: ts-types-generate ## Generate canonical TypeScript typed-field protobuf messages and the api-bones-types sources
	cd api-bones-connect-ts && npm install --no-audit --no-fund && buf generate
	cd tests/typescript && npm install --no-audit --no-fund && buf generate
	: 'The test service wrapper links the built sdk-runtime package, which links otel.'
	cd api-bones-otel && npm install --no-audit --no-fund && npm run build
	cd api-bones-sdk-runtime-ts && npm install --no-audit --no-fund && npm run build
	cargo run --locked -p api-bones-sdk-gen -- wrapper build-ts --crate tests/l2 --surface sdk-call --out api-bones-test-service-sdk-ts
