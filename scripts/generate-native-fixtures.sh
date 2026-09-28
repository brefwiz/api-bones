#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output="$repo_root/tests/typescript/generated"
first=$(mktemp -d)
second=$(mktemp -d)
trap 'rm -rf "$first" "$second"' EXIT

cargo build --quiet --manifest-path "$repo_root/Cargo.toml" \
  -p api-bones-sdk-gen --bin protoc-gen-bones-native
plugin_path="$repo_root/target/debug"
template="$repo_root/api-bones-sdk-gen/tests/fixtures/buf.gen.yaml"

generate() {
  destination=$1
  (cd "$destination" && PATH="$plugin_path:$PATH" buf generate \
    "$repo_root/api-bones-protos/proto" --template "$template" \
    --path fixtures/native.proto --path fixtures/other/native-second.proto)
  mv "$destination/bones_native.rs" "$destination/plugin_native.rs"
  mv "$destination/bones_native.ts" "$destination/plugin_native.ts"
  buf build "$repo_root/api-bones-protos/proto" --path fixtures/native.proto \
    --as-file-descriptor-set -o "$destination/native.pb"
  cargo run --quiet --manifest-path "$repo_root/Cargo.toml" \
    -p api-bones-sdk-gen --bin api-bones-sdk-gen -- native \
    --descriptor-set "$destination/native.pb" \
    --rust-out "$destination/bones_native.rs" \
    --typescript-out "$destination/bones_native.ts" \
    --proof-out "$destination/proof.json"
}

generate "$first"
generate "$second"
cmp "$first/bones_native.rs" "$second/bones_native.rs"
cmp "$first/bones_native.ts" "$second/bones_native.ts"
cmp "$first/proof.json" "$second/proof.json"
grep -q '"rpcCount":0' "$first/proof.json"
# Imported descriptors provide metadata resolution only. buf's requested file
# owns emitted clients.
if grep -F 'ImportedOnlyClient' "$first/plugin_native.rs" "$first/plugin_native.ts"; then
  echo "plugin emitted native service from an imported file" >&2
  exit 1
fi
cmp "$first/plugin_native.rs" "$second/plugin_native.rs"
cmp "$first/plugin_native.ts" "$second/plugin_native.ts"
grep -F 'pub struct FixtureNativeV1NativeQueueClient' "$first/plugin_native.rs"
grep -F 'export class FixtureNativeV1NativeQueueClient' "$first/plugin_native.ts"
grep -F 'pub struct FixtureSecondV1SecondQueueClient' "$first/plugin_native.rs"
grep -F 'export class FixtureSecondV1SecondQueueClient' "$first/plugin_native.ts"
cp "$first/plugin_native.rs" "$first/plugin_check.rs"
printf '\nfn selected_client<T: NativeTransport>(transport: T) { let _ = FixtureNativeV1NativeQueueClient::new(transport); }\n' \
  >> "$first/plugin_check.rs"
rustc --edition 2024 --crate-type lib "$first/plugin_check.rs" \
  -o "$first/plugin_check.rlib"
mkdir "$first/rpc-only"
(cd "$first/rpc-only" && PATH="$plugin_path:$PATH" buf generate \
  "$repo_root/api-bones-protos/proto" --template "$template" \
  --path fixtures/rpc-only.proto)
if find "$first/rpc-only" -type f | grep -q .; then
  echo "plugin emitted output for ordinary RPC-only input" >&2
  exit 1
fi

buf build "$repo_root/api-bones-protos/proto" \
  --path fixtures/native-ambiguous.proto --as-file-descriptor-set \
  -o "$first/ambiguous.pb"
if cargo run --quiet --manifest-path "$repo_root/Cargo.toml" \
  -p api-bones-sdk-gen --bin api-bones-sdk-gen -- native \
  --descriptor-set "$first/ambiguous.pb" \
  --rust-out "$first/ambiguous.rs" \
  --typescript-out "$first/ambiguous.ts" 2>"$first/ambiguous.txt"; then
  echo "ambiguous native descriptor was accepted" >&2
  exit 1
fi
grep -F 'duplicate native operation "publish" in Ambiguous' "$first/ambiguous.txt"

printf 'stale\n' > "$first/stale.rs"
cp "$first/bones_native.ts" "$first/stale.ts"
if cargo run --quiet --manifest-path "$repo_root/Cargo.toml" \
  -p api-bones-sdk-gen --bin api-bones-sdk-gen -- native \
  --descriptor-set "$first/native.pb" --rust-out "$first/stale.rs" \
  --typescript-out "$first/stale.ts" --check 2>"$first/stale-rust.txt"; then
  echo "stale Rust output was accepted" >&2; exit 1
fi
grep -F "stale generated output: $first/stale.rs" "$first/stale-rust.txt"
cp "$first/bones_native.rs" "$first/stale.rs"
printf 'stale\n' > "$first/stale.ts"
if cargo run --quiet --manifest-path "$repo_root/Cargo.toml" \
  -p api-bones-sdk-gen --bin api-bones-sdk-gen -- native \
  --descriptor-set "$first/native.pb" --rust-out "$first/stale.rs" \
  --typescript-out "$first/stale.ts" --check 2>"$first/stale-typescript.txt"; then
  echo "stale TypeScript output was accepted" >&2; exit 1
fi
grep -F "stale generated output: $first/stale.ts" "$first/stale-typescript.txt"

for fixture in native-leading-digit native-keyword native-constructor native-transport native-collision native-rust2024-keyword; do
  buf build "$repo_root/api-bones-protos/proto" --path "fixtures/$fixture.proto" \
    --as-file-descriptor-set -o "$first/$fixture.pb"
  if cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p api-bones-sdk-gen \
    --bin api-bones-sdk-gen -- native --descriptor-set "$first/$fixture.pb" \
    --rust-out "$first/$fixture.rs" --typescript-out "$first/$fixture.ts" \
    2>"$first/$fixture.txt"; then
    echo "unsafe identifier fixture $fixture was accepted" >&2; exit 1
  fi
  case "$fixture" in
    native-leading-digit) expected='native operation name "1publish" must be lower_snake_case' ;;
    native-keyword) expected='native operation name "new" conflicts with generated client API' ;;
    native-constructor) expected='native operation name "constructor" conflicts with generated client API' ;;
    native-transport) expected='native operation name "transport" conflicts with generated client API' ;;
    native-collision) expected='native operation name "foo__bar" must be lower_snake_case' ;;
    native-rust2024-keyword) expected='native operation name "gen" conflicts with generated client API' ;;
  esac
  grep -F "$expected" "$first/$fixture.txt"
done
buf build "$repo_root/api-bones-protos/proto" \
  --path fixtures/native-symbol-collision-a.proto \
  --path fixtures/native-symbol-collision-b.proto \
  --as-file-descriptor-set -o "$first/symbol-collision.pb"
if cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p api-bones-sdk-gen \
  --bin api-bones-sdk-gen -- native --descriptor-set "$first/symbol-collision.pb" \
  --rust-out "$first/symbol-collision.rs" --typescript-out "$first/symbol-collision.ts" \
  2>"$first/symbol-collision.txt"; then
  echo "generated client symbol collision was accepted" >&2; exit 1
fi
grep -F 'native services collide on generated client name "FooBarQueueClient"' \
  "$first/symbol-collision.txt"
buf build "$repo_root/api-bones-protos/proto" \
  --path fixtures/native-same-name-a.proto --path fixtures/native-same-name-b.proto \
  --as-file-descriptor-set -o "$first/same-name.pb"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p api-bones-sdk-gen \
  --bin api-bones-sdk-gen -- native --descriptor-set "$first/same-name.pb" \
  --rust-out "$first/same-name.rs" --typescript-out "$first/same-name.ts"
grep -F 'FixtureAlphaV1SameNameClient' "$first/same-name.rs"
grep -F 'FixtureBetaV1SameNameClient' "$first/same-name.ts"

mkdir -p "$output"
cp "$first/bones_native.ts" "$output/native.ts"
cp "$first/plugin_native.ts" "$output/plugin-native.ts"
rust_hash=$(sha256sum "$first/bones_native.rs" | cut -d' ' -f1)
rust_hash_second=$(sha256sum "$second/bones_native.rs" | cut -d' ' -f1)
ts_hash=$(sha256sum "$first/bones_native.ts" | cut -d' ' -f1)
ts_hash_second=$(sha256sum "$second/bones_native.ts" | cut -d' ' -f1)
printf '{"rustHashes":["%s","%s"],"typescriptHashes":["%s","%s"],"rpcCount":0,"rpcOnlyFiles":0,"duplicateDiagnostic":"duplicate native operation \\"publish\\" in Ambiguous","staleRustDiagnostic":"stale generated output","staleTypescriptDiagnostic":"stale generated output","identifierDiagnostics":["leading digit","reserved new","reserved constructor","reserved transport","case collision","rust 2024 keyword","generated symbol collision"],"disambiguatedClients":["FixtureAlphaV1SameNameClient","FixtureBetaV1SameNameClient"],"multiDirectoryClients":["FixtureNativeV1NativeQueueClient","FixtureSecondV1SecondQueueClient"]}\n' \
  "$rust_hash" "$rust_hash_second" "$ts_hash" "$ts_hash_second" > "$output/proof.json"
