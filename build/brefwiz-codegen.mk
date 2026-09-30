# build/brefwiz-codegen.mk — platform primitive.
#
# Content-hash codegen invalidation. cargo's `rerun-if-changed` is mtime-based,
# and macOS Docker bind-mount mtimes are unreliable, so a proto/OpenAPI/sqlx edit
# can silently reuse STALE generated code — a new field dropped on the wire with
# no error. This fragment exports BREFWIZ_CODEGEN_INPUTS_SHA as a content hash of
# the codegen inputs; each codegen build.rs emits
#     println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
# so cargo reruns the generator whenever the inputs' BYTES change, mtime-free.
#
# Consumer contract:
#   1. set CODEGEN_INPUTS to the source-of-truth paths (proto dirs, OpenAPI
#      schema, sqlx query dir) BEFORE including this fragment;
#   2. `include build/brefwiz-codegen.mk`;
#   3. add the rerun-if-env-changed line to each codegen build.rs.
#
# Vendored per-repo alongside api-bones-sdk.mk. Do not fork the hash algorithm —
# the platform-wide var name and hashing live here so every repo agrees.
#
# The hash is recomputed by whichever `make` runs the build — including the inner
# `make` inside `docker run` against the bind-mounted source. Only mtimes are
# unreliable under the bind mount; file CONTENT is faithful, so the shasum is
# correct there and cargo sees the right key without any `-e` forwarding.

CODEGEN_INPUTS ?=
export BREFWIZ_CODEGEN_INPUTS_SHA := $(shell find $(CODEGEN_INPUTS) -type f 2>/dev/null | sort | xargs shasum 2>/dev/null | shasum | cut -d' ' -f1)
