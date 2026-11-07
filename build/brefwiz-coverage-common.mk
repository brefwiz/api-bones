# SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
#
# What the coverage recipes share: the coverage knobs with their defaults, the
# private grading variables, and the tool installers. Included by
# build/brefwiz-service.mk and by build/brefwiz-coverage-split.mk, so a repo
# can use either, or both in any order. The include guard makes a second
# include a no-op.

ifndef _BREFWIZ_COVERAGE_COMMON_MK
_BREFWIZ_COVERAGE_COMMON_MK := 1

.PHONY: install-nextest install-llvm-cov

install-nextest:
	@command -v cargo-nextest >/dev/null 2>&1 || cargo install cargo-nextest --locked

install-llvm-cov:
	@command -v cargo-llvm-cov >/dev/null 2>&1 || cargo install cargo-llvm-cov --locked

COVERAGE_NEEDS_DB         ?= false
COVERAGE_DB_UP_TARGET     ?= pg-up
COVERAGE_DB_ROLES_TARGET  ?=
PG_URL                    ?=
COVERAGE_IGNORE_REGEX     ?=
COVERAGE_FAIL_UNDER_LINES ?= 80
COVERAGE_EXTRA_FEATURES   ?=
COVERAGE_EXCLUDE          ?=
COVERAGE_ALL_FEATURES     ?= false
# Defaults to the same "ci" profile `ci-test` always runs under (every repo
# vendoring this file already declares it), so the coverage lane's nextest
# run emits JUnit at the same canonical path (target/nextest/ci/junit.xml)
# the verdict composite's coverage.yml stages by default — with no new
# per-repo nextest.toml or CI input required.
COVERAGE_NEXTEST_PROFILE  ?= ci
COVERAGE_LCOV_OUTPUT      ?= true
# llvm-cov refuses an empty regex outright, so a repo that ignores nothing
# passes no flag at all rather than an empty one.
_COVERAGE_IGNORE = $(if $(strip $(COVERAGE_IGNORE_REGEX)),--ignore-filename-regex '$(COVERAGE_IGNORE_REGEX)',)
_COVERAGE_LINE_GATE = $(if $(filter 100,$(COVERAGE_FAIL_UNDER_LINES)),--fail-uncovered-lines 0,--fail-under-lines $(COVERAGE_FAIL_UNDER_LINES))
# `report` at a package root keeps only the root package. The glob selects
# every workspace member; `report --workspace` is refused before
# cargo-llvm-cov 0.9.0, which the runner image predates.
_COVERAGE_REPORT_MEMBERS = --package '*'

endif
