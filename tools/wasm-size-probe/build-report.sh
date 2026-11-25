#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Builds every wasm size probe variant, runs wasm-bindgen and wasm-opt over it,
# and writes target/wasm-size/report.json (api-bones.wasm-size/v1) plus its
# markdown summary. Outputs always land in <repo>/target/wasm-size, regardless
# of CARGO_TARGET_DIR (which only locates cargo's build output). Fails on a
# wasm-bindgen that is not the one Cargo.lock pins, a missing tool, a failed
# build, or an empty variant set. Installs nothing: the tools are the ci-rust
# image's.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

target="${CARGO_TARGET_DIR:-$root/target}"
work="$root/target/wasm-size"
variants="$work/variants"
package=api-bones-wasm-size-probe
module=api_bones_wasm_size_probe

for tool in cargo rustc wasm-bindgen wasm-opt; do
  command -v "$tool" >/dev/null || {
    echo "wasm-size: required tool '$tool' is not on PATH" >&2
    exit 1
  }
done

locked="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, "", $0); print; exit }' Cargo.lock)"
installed="$(wasm-bindgen --version | awk '{ print $2 }')"
if [ -z "$locked" ] || [ "$locked" != "$installed" ]; then
  echo "wasm-size: wasm-bindgen $installed does not match Cargo.lock's $locked" >&2
  exit 1
fi
binaryen="$(wasm-opt --version)"

rm -rf "$work"
mkdir -p "$variants"

# The feature set rustc's wasm32-unknown-unknown output uses; wasm-opt rejects
# the module at validation without it. The validator stays on.
opt_features=(
  --enable-bulk-memory
  --enable-bulk-memory-opt
  --enable-sign-ext
  --enable-nontrapping-float-to-int
  --enable-mutable-globals
  --enable-reference-types
  --enable-multivalue
)

# name|profile|features|wasm-opt
spec=(
  "floor|wasm-size|floor|true"
  "floor-no-opt|wasm-size|floor|false"
  "floor-s|wasm-size-s|floor|true"
  "page|wasm-size|page|true"
  "batch|wasm-size|batch|true"
  "stream|wasm-size|stream|true"
  "retry|wasm-size|retry|true"
  "gzip|wasm-size|gzip|true"
  "methods-10|wasm-size|methods-10|true"
  "methods-50|wasm-size|methods-50|true"
  "second-service|wasm-size|second-service|true"
  "all|wasm-size|all|true"
  "all-s|wasm-size-s|all|true"
)

for entry in "${spec[@]}"; do
  IFS='|' read -r name profile features opt <<<"$entry"
  out="$variants/$name"
  mkdir -p "$out/bindgen"
  cargo build --locked -p "$package" --lib --target wasm32-unknown-unknown \
    --profile "$profile" --no-default-features --features "$features"
  wasm-bindgen --target web --out-dir "$out/bindgen" \
    "$target/wasm32-unknown-unknown/$profile/$module.wasm"
  if [ "$opt" = true ]; then
    wasm-opt -Oz "${opt_features[@]}" "$out/bindgen/${module}_bg.wasm" -o "$out/module.wasm"
  else
    cp "$out/bindgen/${module}_bg.wasm" "$out/module.wasm"
  fi
  cp "$out/bindgen/$module.js" "$out/glue.js"
  rm -rf "$out/bindgen"
  features_json="$(printf '"%s"' "${features//,/\",\"}")"
  printf '{"features":[%s],"profile":"%s","wasm_opt":%s}\n' \
    "$features_json" "$profile" "$opt" >"$out/meta.json"
done

cargo run --locked -q -p "$package" --bin wasm-size-report -- \
  --dir "$variants" \
  --commit "$(git rev-parse HEAD)" \
  --rustc "$(rustc --version)" \
  --wasm-bindgen "$installed" \
  --binaryen "$binaryen" \
  --out "$work/report.json" \
  --summary "$work/summary.md"

# The artifact is named by its file's basename.
cp "$work/report.json" "$work/wasm-size-report"
