# SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
#
# Coverage as three kinds of job: an archive build, partitioned runs against
# the archive, and one report that joins them. Includes
# build/brefwiz-coverage-common.mk (beside this file), which also carries the
# coverage knobs and private variables (_COVERAGE_IGNORE, _COVERAGE_LINE_GATE,
# _COVERAGE_REPORT_MEMBERS) of build/brefwiz-service.mk, so grading flags and
# floors are those of `ci-coverage`. It needs nothing else: a repo may include
# it alone, or with build/brefwiz-service.mk in either order.
#
# Job contract
#   ci-coverage-archive  build job, no services. Compiles the instrumented
#                        workspace and writes one nextest archive to
#                        COVERAGE_ARCHIVE_FILE. Runs no test and no report.
#   ci-coverage-run      run job, with services. Needs the archive. Runs the
#                        archived tests (all of them, or one slice: with
#                        COVERAGE_PARTITION_INDEX (0-based) and
#                        COVERAGE_PARTITION_COUNT both set it runs nextest
#                        partition hash:<INDEX+1>/<COUNT>; setting only one of
#                        them is an error), then packs the raw profiles into
#                        COVERAGE_PROFILE_FILE, each renamed with a prefix
#                        derived from the partition so bundles from different
#                        jobs never collide. It grades nothing.
#   ci-coverage-report   join job, no services, no test run. Needs the archive
#                        and every bundle named by COVERAGE_PROFILE_FILES. It
#                        extracts the archive and the bundles into the target
#                        directory, grades and writes lcov exactly as
#                        `ci-coverage` does.
# The archive and the bundles are the only state that crosses between jobs.
# A repository without partitions runs `make ci-coverage-run ci-coverage-report`
# in one job.
#
# Stale-objects guard. Every job runs its target through the coverage
# composite, whose stale-objects guard renders the job's own profiles and
# checks the report against the checkout. A recipe that starts with
# COVERAGE_SPLIT_ENTER (the run, the report, any extra gate) records the
# absolute archive path in target/.coverage-split-archive under the archived
# root. The composite deletes that file before make and, when make leaves it,
# renders with `--nextest-archive-file` from the archive's workspace root and
# with the archive's target directory, the layout the recipe ran in; a record
# whose archive is unreadable or belongs to another workspace root fails the
# guard. The archive job writes no record and is rendered in the composite's
# own layout. The workflow needs no step or input for this, and the consumer
# Makefile may set SHELL.
#
# Partition shape. The partitions are equal jobs named <stem>-<n> that each run
# one slice from the one archive; one job joins them. A job's own index <n> is
# its COVERAGE_PARTITION_INDEX, so the specs of the jobs differ only by that
# index. Each run prefixes its profiles with part<INDEX>of<COUNT>-, or whole-
# without a partition. The report reads the member names of every bundle before
# extracting and requires exactly one prefix per bundle, the indices 0..M-1 of
# one M each present exactly once (a whole- bundle only on its own). A missing,
# duplicate, out-of-range or mixed slice, no bundle, a missing bundle file, or a
# profile name already present in the target directory fails the report;
# completeness comes from the bundles themselves.
#
# Checkout location. Each job checks the repository out at its own path, while
# the archive embeds the build job's workspace root and target directory, and
# cargo-llvm-cov derives its default ignore rules from both. The run and report
# targets read them from the archive; when their checkout is elsewhere they
# move the checkout to the archived root (leaving a symlink at the old path,
# refusing if the root exists) and export CARGO_LLVM_COV_TARGET_DIR as the
# archived target directory. A symlink alone would leave the report seeing
# physical paths that differ from the archived ones, which changes what is
# graded.
#
# Compile-time target paths. Paths the build job baked into the test binaries
# from its target directory T (CARGO_BIN_EXE_<name> through env!, build-script
# OUT_DIR) are T/<base-output-directory>/<file>, while the run extracts the
# archive under T/target. Before the run, ci-coverage-run reads
# rust-build-meta.base-output-directories from the archive and links each
# T/<dir> to target/<dir> (relative), so those paths resolve. A name already
# present as anything but that link, or one that is absolute, climbs out of T or
# starts with target, fails the run.
#
# Extra gates. A recipe of its own that reads coverage from the archive (a
# path-floor script calling `cargo llvm-cov report --json`, for example) starts
# with COVERAGE_SPLIT_ENTER so it sees the relocated checkout, the target
# directory and the archive path; see the definition below.
#
# Extra archives. COVERAGE_EXTRA_ARCHIVES lists ids (each [A-Za-z0-9_]+) of
# further nextest archives for executables the main archive cannot hold (a
# test needing different features of a package than the workspace build).
# COVERAGE_ARCHIVE_<id>_ARGS is required and non-empty: the cargo selection of
# that archive, split on whitespace (for example
# `-p my-service --test my_suite`). Its file is the directory of
# COVERAGE_ARCHIVE_FILE plus coverage-nextest-archive-<id>.tar.zst. The archive
# job builds each one after the main archive, in the same instrumented target
# directory; an invalid id or empty ARGS fails before any build. Every
# declared extra must be present in the run and the report job, so all job
# specs stay identical; a missing one fails before any test. The run executes
# the extras first, then the main archive. An extra runs every test it holds
# (--ignore-default-filter): its selection is its COVERAGE_ARCHIVE_<id>_ARGS,
# and the default-filter of COVERAGE_NEXTEST_PROFILE does not apply to it; the
# profile's other settings do. Extra number k (counting from 0 in
# list order) runs whole without a partition, and with one only in the job
# whose COVERAGE_PARTITION_INDEX equals k mod COVERAGE_PARTITION_COUNT. Every
# pass runs even after a failure; any failure exits non-zero and writes no
# bundle. Profiles of all passes share the bundle and its prefix. The report
# extracts each extra into a scratch directory after the main archive and moves
# every regular file outside target/nextest/ into the target directory when it
# is absent there. A byte-identical file already there is skipped. A differing
# file (an executable an extra built with other features) is kept under
# <its directory>/coverage-extra-<id>/ beside the archived one, so the report
# reads both builds; if that path holds a differing file too, the report fails
# with "would replace archived <path>". Nothing is overwritten and
# the extras' nextest metadata is not used. The stale-objects record keeps
# naming the main archive. With COVERAGE_EXTRA_ARCHIVES empty the recipes
# behave as without this paragraph.
#
# Function floor. COVERAGE_FAIL_UNDER_FUNCTIONS, when set, is passed to
# `cargo llvm-cov report` of ci-coverage-report as --fail-under-functions. The
# line floor is unchanged. Empty (the default) passes no flag.
#
# Limit. The archive job has no database, so a crate whose sqlx macros need a
# live DATABASE_URL at compile time cannot be built by it; such a repo keeps
# `ci-coverage`. Offline sqlx (a committed .sqlx) and runtime queries are fine.
# The run step is not passed --locked; the archive fixes the dependency set.

include build/brefwiz-coverage-common.mk

COVERAGE_ARCHIVE_FILE  ?= target/coverage-nextest-archive.tar.zst
COVERAGE_PARTITION_INDEX ?=
COVERAGE_PARTITION_COUNT ?=
COVERAGE_PROFILE_FILE  ?= target/coverage-profiles.tar.zst
COVERAGE_PROFILE_FILES ?=
COVERAGE_EXTRA_ARCHIVES ?=
COVERAGE_FAIL_UNDER_FUNCTIONS ?=
_COVERAGE_FUNCTION_GATE = $(if $(strip $(COVERAGE_FAIL_UNDER_FUNCTIONS)),--fail-under-functions $(COVERAGE_FAIL_UNDER_FUNCTIONS),)

# The extra archives reach the recipes through the environment: the ids, and one
# <id>=<args> line per id.
define _COVERAGE_NL


endef
_COVERAGE_EXTRA_IDS = $(COVERAGE_EXTRA_ARCHIVES)
_COVERAGE_EXTRA_TABLE = $(foreach x,$(COVERAGE_EXTRA_ARCHIVES),$(x)=$(COVERAGE_ARCHIVE_$(x)_ARGS)$(_COVERAGE_NL))
export _COVERAGE_EXTRA_IDS _COVERAGE_EXTRA_TABLE

# Shell for a recipe: fails when an extra id is not [A-Za-z0-9_]+. Argument 1 is
# the target name.
define _COVERAGE_EXTRA_IDS_OK
set -f; for _x in $$_COVERAGE_EXTRA_IDS; do case "$$_x" in ''|*[!A-Za-z0-9_]*) echo "$(1): COVERAGE_EXTRA_ARCHIVES id '$$_x' must match [A-Za-z0-9_]+" >&2; exit 1 ;; esac; done; set +f
endef

# Shell for a recipe: defines _args_of <id>, which prints COVERAGE_ARCHIVE_<id>_ARGS.
define _COVERAGE_EXTRA_ARGS_FN
_args_of() { printf '%s\n' "$$_COVERAGE_EXTRA_TABLE" | while read -r _l; do case "$$_l" in "$$1="*) printf '%s' "$${_l#*=}"; break ;; esac; done; }
endef

# COVERAGE_SPLIT_ENTER is the entry point for any recipe that reads coverage
# from the archive: `$(call COVERAGE_SPLIT_ENTER,<target name>)` inside a recipe
# shell line, followed by `; \`. It reads the archive metadata, moves the
# checkout to the archived workspace root when it is elsewhere (idempotent: a
# checkout already there is not moved, and a second recipe after a move runs
# from the same root), changes into the root and exports
#   CARGO_LLVM_COV_TARGET_DIR      the archived target directory
#   COVERAGE_NEXTEST_ARCHIVE_FILE  the absolute archive path in the root
# and writes the absolute archive path to target/.coverage-split-archive in the
# root, which the coverage composite's stale-objects guard reads. It refuses
# when the archive is missing. It sets _M, a scratch directory that is removed
# when the shell exits.
define COVERAGE_SPLIT_ENTER
_A='$(COVERAGE_ARCHIVE_FILE)'; \
if [ ! -f "$$_A" ]; then echo "$(1): archive $$_A is missing; run ci-coverage-archive in the build job and restore the file here" >&2; exit 1; fi; \
_HERE=$$(pwd -P); \
_M=$$(mktemp -d) || exit 1; \
trap 'rm -rf "$$_M"' EXIT; \
{ zstd -dc "$$_A"; echo $$? > "$$_M/zstd-rc"; } | tar -x -C "$$_M" target/nextest/cargo-metadata.json target/nextest/binaries-metadata.json && [ "$$(cat "$$_M/zstd-rc")" = 0 ] || { echo "$(1): $$_A is not a nextest archive" >&2; exit 1; }; \
_ROOT=$$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["workspace_root"])' "$$_M/target/nextest/cargo-metadata.json") || exit 1; \
_TD=$$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["rust-build-meta"]["target-directory"])' "$$_M/target/nextest/binaries-metadata.json") || exit 1; \
if [ "$$_HERE" != "$$_ROOT" ]; then \
	if [ -e "$$_ROOT" ] || [ -L "$$_ROOT" ]; then echo "$(1): cannot move the checkout to the archived workspace root $$_ROOT: it exists" >&2; exit 1; fi; \
	mkdir -p "$$(dirname "$$_ROOT")" && mv "$$_HERE" "$$_ROOT" && ln -s "$$_ROOT" "$$_HERE" || exit 1; \
fi; \
cd "$$_ROOT" || exit 1; \
case "$$_A" in /*) ;; *) _A="$$_ROOT/$$_A" ;; esac; \
mkdir -p target && printf '%s\n' "$$_A" > target/.coverage-split-archive || exit 1; \
export CARGO_LLVM_COV_TARGET_DIR="$$_TD" COVERAGE_NEXTEST_ARCHIVE_FILE="$$_A"
endef

# The completeness check of ci-coverage-report: argv is pairs of a bundle path
# and a file listing that bundle's member names. It travels in the environment so
# its indentation survives.
define _COVERAGE_SLICE_CHECK
import collections, re, sys
args = sys.argv[1:]
pat = re.compile(r"^(?:whole-|part([0-9]+)of([0-9]+)-)")
errs, wholes, slices = [], 0, []
for bundle, names in zip(args[0::2], args[1::2]):
    keys = set()
    for line in open(names).read().splitlines():
        m = pat.match(line)
        keys.add(None if m is None else (m.group(1), m.group(2)))
    if len(keys) != 1 or None in keys:
        errs.append("bundle %s must carry profiles of exactly one slice prefix" % bundle)
    elif next(iter(keys))[0] is None:
        wholes += 1
    else:
        i, n = next(iter(keys))
        slices.append((int(i), int(n)))
total = len(args) // 2
if not errs and wholes and total != 1:
    errs.append("a whole- bundle must be the only bundle, got %d bundles" % total)
if not errs and slices:
    counts = sorted(set(x[1] for x in slices))
    if len(counts) > 1:
        errs.append("mixed slice counts: " + ", ".join(str(c) for c in counts))
    else:
        m = counts[0]
        seen = collections.Counter(x[0] for x in slices)
        gone = [i for i in range(m) if i not in seen]
        twice = sorted(i for i, c in seen.items() if c > 1)
        stray = sorted(i for i in seen if i >= m)
        if gone:
            errs.append("missing slice(s) " + ", ".join(str(i) for i in gone) + " of " + str(m))
        if twice:
            errs.append("duplicate slice(s) " + ", ".join(str(i) for i in twice))
        if stray:
            errs.append("slice index(es) " + ", ".join(str(i) for i in stray) + " outside 0.." + str(m - 1))
for e in errs:
    sys.stderr.write("ci-coverage-report: " + e + "\n")
sys.exit(1 if errs else 0)
endef
export _COVERAGE_SLICE_CHECK

# Links every base output directory of the archive into the target directory:
# argv is the binaries metadata file and the target directory.
define _COVERAGE_LINK_BASES
import json, os, sys
meta, td = sys.argv[1:3]
dirs = json.load(open(meta))["rust-build-meta"].get("base-output-directories")
if not isinstance(dirs, list) or not all(isinstance(d, str) for d in dirs):
    sys.exit("ci-coverage-run: the archive records no base-output-directories list of strings")
for d in dirs:
    parts = d.split("/")
    if not d or d.startswith("/") or "" in parts or "." in parts or ".." in parts or parts[0] == "target":
        sys.exit("ci-coverage-run: unusable base output directory %r in the archive" % d)
    link = os.path.join(td, d)
    want = "/".join([".."] * (len(parts) - 1) + ["target", d])
    if os.path.islink(link) and os.readlink(link) == want:
        continue
    if os.path.lexists(link):
        sys.exit("ci-coverage-run: cannot link %s to target/%s: the name exists" % (link, d))
    os.makedirs(os.path.dirname(link), exist_ok=True)
    os.symlink(want, link)
endef
export _COVERAGE_LINK_BASES

.PHONY: ci-coverage-archive ci-coverage-run ci-coverage-report

ci-coverage-archive: install-nextest install-llvm-cov ## CI: build the coverage nextest archive (no tests, no services)
	@set -e; \
	$(call _COVERAGE_EXTRA_IDS_OK,ci-coverage-archive); \
	$(call _COVERAGE_EXTRA_ARGS_FN); \
	for _id in $$_COVERAGE_EXTRA_IDS; do \
		case "$$(_args_of "$$_id")" in *[![:space:]]*) ;; *) echo "ci-coverage-archive: COVERAGE_ARCHIVE_$${_id}_ARGS must be set and non-empty" >&2; exit 1 ;; esac; \
	done; \
	cargo fetch --locked; \
	mkdir -p "$$(dirname '$(COVERAGE_ARCHIVE_FILE)')"; \
	cargo llvm-cov nextest-archive --locked \
		--workspace \
		$(if $(COVERAGE_NEXTEST_PROFILE),--profile $(COVERAGE_NEXTEST_PROFILE),) \
		$(foreach x,$(COVERAGE_EXCLUDE),--exclude $(x)) \
		$(if $(filter true,$(COVERAGE_ALL_FEATURES)),--all-features,) \
		$(foreach f,$(COVERAGE_EXTRA_FEATURES),--features $(f)) \
		--archive-file '$(COVERAGE_ARCHIVE_FILE)'; \
	for _id in $$_COVERAGE_EXTRA_IDS; do \
		cargo llvm-cov nextest-archive --locked \
			$(if $(COVERAGE_NEXTEST_PROFILE),--profile $(COVERAGE_NEXTEST_PROFILE),) \
			$$(_args_of "$$_id") \
			--archive-file "$(dir $(COVERAGE_ARCHIVE_FILE))coverage-nextest-archive-$$_id.tar.zst"; \
	done

ci-coverage-run: install-nextest install-llvm-cov ## CI: run the coverage archive (one slice with COVERAGE_PARTITION_INDEX/COUNT) and pack its profiles
	@if [ "$(COVERAGE_NEEDS_DB)" = "true" ] && [ -z "$${CI}" ]; then $(MAKE) $(COVERAGE_DB_UP_TARGET); fi
	@$(call _COVERAGE_EXTRA_IDS_OK,ci-coverage-run); \
	_DB_URL=$${CI:+$${DATABASE_URL}}; _DB_URL=$${_DB_URL:-$(PG_URL)}; \
	if [ "$(COVERAGE_NEEDS_DB)" = "true" ] && [ -n "$(COVERAGE_DB_ROLES_TARGET)" ]; then \
		DATABASE_URL=$$_DB_URL $(MAKE) $(COVERAGE_DB_ROLES_TARGET) || exit $$?; \
	fi; \
	_PFX=whole-; _PART=""; _I='$(COVERAGE_PARTITION_INDEX)'; _N='$(COVERAGE_PARTITION_COUNT)'; \
	if [ -n "$$_I" ] || [ -n "$$_N" ]; then \
		if [ -z "$$_I" ] || [ -z "$$_N" ]; then echo "ci-coverage-run: COVERAGE_PARTITION_INDEX and COVERAGE_PARTITION_COUNT must be set together" >&2; exit 1; fi; \
		case "$$_I$$_N" in *[!0-9]*) echo "ci-coverage-run: COVERAGE_PARTITION_INDEX and COVERAGE_PARTITION_COUNT must be non-negative integers" >&2; exit 1 ;; esac; \
		_I=$$(expr "$$_I" + 0); _N=$$(expr "$$_N" + 0); \
		if [ "$$_N" -lt 1 ] || [ "$$_I" -ge "$$_N" ]; then echo "ci-coverage-run: COVERAGE_PARTITION_INDEX $$_I must be below COVERAGE_PARTITION_COUNT $$_N (count at least 1)" >&2; exit 1; fi; \
		_PART="--partition hash:$$((_I+1))/$$_N"; _PFX="part$${_I}of$${_N}-"; \
	fi; \
	$(call COVERAGE_SPLIT_ENTER,ci-coverage-run); \
	for _id in $$_COVERAGE_EXTRA_IDS; do \
		_F="$(dir $(COVERAGE_ARCHIVE_FILE))coverage-nextest-archive-$$_id.tar.zst"; \
		if [ ! -f "$$_F" ]; then echo "ci-coverage-run: extra archive $$_F is missing; run ci-coverage-archive in the build job and restore the file here" >&2; exit 1; fi; \
	done; \
	mkdir -p "$$_TD"; \
	python3 -I -c "$$_COVERAGE_LINK_BASES" "$$_M/target/nextest/binaries-metadata.json" "$$_TD" || exit 1; \
	find "$$_TD" -name '*.profraw' -exec rm -f {} +; \
	EXIT=0; _K=0; \
	for _id in $$_COVERAGE_EXTRA_IDS; do \
		if [ -z "$$_I" ] || [ $$((_K % _N)) -eq "$$_I" ]; then \
			_F="$(dir $(COVERAGE_ARCHIVE_FILE))coverage-nextest-archive-$$_id.tar.zst"; \
			case "$$_F" in /*) ;; *) _F="$$_ROOT/$$_F" ;; esac; \
			_RC=0; \
			DATABASE_URL=$$_DB_URL cargo llvm-cov nextest \
				--archive-file "$$_F" \
				$(if $(COVERAGE_NEXTEST_PROFILE),--profile $(COVERAGE_NEXTEST_PROFILE),) \
				--ignore-default-filter \
				--no-report \
				--no-fail-fast \
				$(_COVERAGE_IGNORE) \
				|| _RC=$$?; \
			if [ $$_RC -ne 0 ]; then echo "ci-coverage-run: extra archive $$_id failed (exit $$_RC)"; [ $$EXIT -ne 0 ] || EXIT=$$_RC; fi; \
		fi; \
		_K=$$((_K+1)); \
	done; \
	_RC=0; \
	DATABASE_URL=$$_DB_URL cargo llvm-cov nextest \
		--archive-file "$$COVERAGE_NEXTEST_ARCHIVE_FILE" \
		$(if $(COVERAGE_NEXTEST_PROFILE),--profile $(COVERAGE_NEXTEST_PROFILE),) \
		$$_PART \
		--no-report \
		--no-fail-fast \
		$(_COVERAGE_IGNORE) \
		|| _RC=$$?; \
	if [ $$_RC -ne 0 ]; then [ $$EXIT -ne 0 ] || EXIT=$$_RC; fi; \
	if [ $$EXIT -ne 0 ]; then echo "ci-coverage-run: test phase failed (exit $$EXIT)"; exit $$EXIT; fi; \
	_S="$$_M/profiles"; mkdir -p "$$_S"; _C=0; \
	for _f in $$(find "$$_TD" -name '*.profraw'); do cp "$$_f" "$$_S/$$_PFX$$(basename "$$_f")" && _C=$$((_C+1)); done; \
	if [ $$_C -eq 0 ]; then echo "ci-coverage-run: the run left no .profraw in $$_TD" >&2; exit 1; fi; \
	mkdir -p "$$(dirname '$(COVERAGE_PROFILE_FILE)')"; \
	( cd "$$_S" && tar -cf - * ) | zstd -q -f -o '$(COVERAGE_PROFILE_FILE)' || exit 1; \
	[ -s '$(COVERAGE_PROFILE_FILE)' ] || { echo "ci-coverage-run: could not write $(COVERAGE_PROFILE_FILE)" >&2; exit 1; }; \
	echo "ci-coverage-run: packed $$_C profiles into $(COVERAGE_PROFILE_FILE)"

ci-coverage-report: install-llvm-cov ## CI: join the partition profiles and grade (--show-missing-lines; ≥COVERAGE_FAIL_UNDER_LINES%)
	@_N=0; \
	for _B in $(COVERAGE_PROFILE_FILES); do _N=$$((_N+1)); done; \
	if [ $$_N -eq 0 ]; then echo "ci-coverage-report: COVERAGE_PROFILE_FILES lists no profile bundle" >&2; exit 1; fi; \
	for _B in $(COVERAGE_PROFILE_FILES); do \
		if [ ! -f "$$_B" ]; then echo "ci-coverage-report: profile bundle $$_B is missing" >&2; exit 1; fi; \
	done; \
	_V=$$(mktemp -d) || exit 1; _K=0; _L=""; \
	for _B in $(COVERAGE_PROFILE_FILES); do \
		_K=$$((_K+1)); \
		{ zstd -dc "$$_B"; echo $$? > "$$_V/zstd-rc"; } | tar -t > "$$_V/$$_K" && [ "$$(cat "$$_V/zstd-rc")" = 0 ] || { rm -rf "$$_V"; echo "ci-coverage-report: cannot read the profile bundle $$_B" >&2; exit 1; }; \
		_L="$$_L $$_B $$_V/$$_K"; \
	done; \
	python3 -I -c "$$_COVERAGE_SLICE_CHECK" $$_L || { _RC=$$?; rm -rf "$$_V"; exit $$_RC; }; \
	rm -rf "$$_V"; \
	$(call COVERAGE_SPLIT_ENTER,ci-coverage-report); \
	mkdir -p "$$_TD"; \
	{ zstd -dc "$$COVERAGE_NEXTEST_ARCHIVE_FILE"; echo $$? > "$$_M/zstd-rc"; } | tar -x -C "$$_TD" && [ "$$(cat "$$_M/zstd-rc")" = 0 ] || { echo "ci-coverage-report: cannot extract the archive into $$_TD" >&2; exit 1; }; \
	for _id in $$_COVERAGE_EXTRA_IDS; do \
		_F="$(dir $(COVERAGE_ARCHIVE_FILE))coverage-nextest-archive-$$_id.tar.zst"; \
		if [ ! -f "$$_F" ]; then echo "ci-coverage-report: extra archive $$_F is missing" >&2; exit 1; fi; \
		_S="$$_M/extra-$$_id"; mkdir -p "$$_S"; \
		{ zstd -dc "$$_F"; echo $$? > "$$_M/zstd-rc"; } | tar -x -C "$$_S" && [ "$$(cat "$$_M/zstd-rc")" = 0 ] || { echo "ci-coverage-report: cannot extract the extra archive $$_F" >&2; exit 1; }; \
		( cd "$$_S" && find . -type f ! -path './target/nextest/*' ) > "$$_M/extra-names" || exit 1; \
		while IFS= read -r _n; do \
			_n=$${_n#./}; \
			_T="$$_n"; \
			if [ -e "$$_TD/$$_T" ] || [ -L "$$_TD/$$_T" ]; then \
				if cmp -s "$$_S/$$_n" "$$_TD/$$_T"; then continue; fi; \
				_T="$$(dirname "$$_n")/coverage-extra-$$_id/$$(basename "$$_n")"; \
				if [ -e "$$_TD/$$_T" ] || [ -L "$$_TD/$$_T" ]; then \
					cmp -s "$$_S/$$_n" "$$_TD/$$_T" && continue; \
					echo "ci-coverage-report: extra archive $$_id would replace archived $$_T" >&2; exit 1; \
				fi; \
				echo "ci-coverage-report: extra archive $$_id holds a different $$_n; kept as $$_TD/$$_T"; \
			fi; \
			mkdir -p "$$(dirname "$$_TD/$$_T")" && mv "$$_S/$$_n" "$$_TD/$$_T" || exit 1; \
		done < "$$_M/extra-names"; \
	done; \
	for _B in $(COVERAGE_PROFILE_FILES); do \
		{ zstd -dc "$$_B"; echo $$? > "$$_M/zstd-rc"; } | tar -t > "$$_M/names" && [ "$$(cat "$$_M/zstd-rc")" = 0 ] || { echo "ci-coverage-report: cannot read the profile bundle $$_B" >&2; exit 1; }; \
		while IFS= read -r _n; do \
			if [ -e "$$_TD/$$_n" ]; then echo "ci-coverage-report: $$_B would overwrite the existing profile $$_n" >&2; exit 1; fi; \
		done < "$$_M/names"; \
		{ zstd -dc "$$_B"; echo $$? > "$$_M/zstd-rc"; } | tar -x -C "$$_TD" && [ "$$(cat "$$_M/zstd-rc")" = 0 ] || { echo "ci-coverage-report: cannot extract the profile bundle $$_B into $$_TD" >&2; exit 1; }; \
	done; \
	cargo llvm-cov report --locked --nextest-archive-file "$$COVERAGE_NEXTEST_ARCHIVE_FILE" $(_COVERAGE_REPORT_MEMBERS) \
		$(_COVERAGE_LINE_GATE) \
		$(_COVERAGE_FUNCTION_GATE) \
		--show-missing-lines \
		$(_COVERAGE_IGNORE) || exit $$?; \
	if [ "$(COVERAGE_LCOV_OUTPUT)" = "true" ]; then \
		cargo llvm-cov report --locked --nextest-archive-file "$$COVERAGE_NEXTEST_ARCHIVE_FILE" $(_COVERAGE_REPORT_MEMBERS) $(_COVERAGE_IGNORE) --lcov --output-path lcov.info; \
	fi
