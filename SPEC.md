---
service: api-bones
wire_surface: library
surface_kind: library
# The original field_behavior.proto introduced by 33432d1 byte-matches this
# upstream revision (SHA-256 bb7ed77725c95f751ec2c47cd92359d0137c39383b8a62aef41bab7aa7478da5).
# The only local addition is its Apache-2.0 SPDX header.
third_party_payloads:
  - {id: googleapis-protos, upstream: https://github.com/googleapis/googleapis, version: "94bc991b3f1df58bc841801631715894a7ac5e28", license: Apache-2.0, paths: [api-bones-protos/vendor]}
generated_paths:
  - tests/typescript/generated/bones/v1/annotations_pb.js
  - tests/typescript/generated/bones/v1/annotations_pb.d.ts
  - tests/typescript/generated/bones/v1/errors_pb.js
  - tests/typescript/generated/bones/v1/errors_pb.d.ts
  - tests/typescript/generated/bones/v1/fields_pb.js
  - tests/typescript/generated/bones/v1/fields_pb.d.ts
  - tests/typescript/generated/bones/v1/pagination_pb.js
  - tests/typescript/generated/bones/v1/pagination_pb.d.ts
  - tests/typescript/generated/bones/sdk/v1/boundary_pb.js
  - tests/typescript/generated/bones/sdk/v1/boundary_pb.d.ts
  - tests/typescript/generated/contract/v1/fields_pb.js
  - tests/typescript/generated/contract/v1/fields_pb.d.ts
  - api-bones-sdk-runtime-ts/src/generated/bones/v1/annotations_pb.ts
  - api-bones-sdk-runtime-ts/src/generated/bones/v1/errors_pb.ts
  - api-bones-sdk-runtime-ts/src/generated/bones/v1/pagination_pb.ts
  - api-bones-sdk-runtime-ts/src/generated/bones/sdk/v1/boundary_pb.ts
  - api-bones-connect-ts/src/error-catalogs/*.generated.ts  # embedded from api-bones-i18n/catalogs/errors by `npm run embed-catalogs`, committed so the module resolves before a build
# The union of every sdk_surfaces target below, which the schema requires it
# to equal.
sdk_languages: [rust, typescript]
# The publishable workspace members, kept in step with cargo metadata: an
# undeclared publishable crate is surface drift, and this repo previously had
# all eight publishable with no mechanism publishing any of them.
# The surfaces this library ships in more than one language.
#
# Distinct from `sdk_languages` above, which describes a generated SDK backed
# by a service. These are hand-written library surfaces that exist twice, and
# declaring them makes feature-closure require a Gherkin contract with direct
# backing in every target language.
#
# No `owns:` — this surface names no RPC endpoint, and needs no canary: there
# is no image here to run one against.
sdk_surfaces:
  connect-retry-eligibility:
    contract: tests/features/connect_retry_eligibility.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-connect]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # A caller with no SVID pins one exact SPIFFE ID against a committed trust
  # bundle. Both packages answer the same contract.
  connect-pinned-peer:
    contract: tests/features/connect_pinned_peer.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-connect]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The proto field labels read back from a descriptor. The artefact is the
  # shipped `bones/v1` proto, which the Rust crate embeds and no TypeScript
  # package wraps -- so this surface is Rust alone. The TypeScript lane
  # still answers the same file over a buf-built descriptor, as a
  # cross-language conformance check outside the surface.
  field-labels:
    contract: tests/features/field_labels.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-protos]
  # The origin of an overridable default, read back from a descriptor. Same
  # artefact and same Rust-only surface as the field labels; the TypeScript
  # lane answers the file over a buf-built descriptor as a conformance check.
  overridable-defaults:
    contract: tests/features/overridable_defaults.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-protos]
  # The delegation types a provider ships as templates, read back from a
  # descriptor and checked against what the provider declares. The artefact is
  # the shipped `bones/v1` proto and the check api-bones-protos carries, which
  # no TypeScript package wraps -- so this surface is Rust alone, the same way
  # field-labels is.
  delegation-templates:
    contract: tests/features/delegation_templates.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-protos]
  # The typed arguments an error code declares with `bones.v1.error_args`,
  # read back from a descriptor and checked against the catalog strings of the
  # code by the `error-catalog` check api-bones-i18n ships. The artefacts are
  # the shipped `bones/v1` proto and that check, which no TypeScript package
  # wraps -- so this surface is Rust alone, the same way field-labels is.
  error-catalog-declarations:
    contract: tests/features/error_catalog_declarations.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-protos, api-bones-i18n]
  # The keyed write -- single or batch -- a contract author marks with
  # `bones.v1.idempotency_key`, read back from a descriptor and checked
  # against the rules api-bones-automation ships: a key is scoped to the
  # organization, the workload, the relayed person when there is one and the
  # method, and is looked up only after the call is authenticated and
  # authorized; a keyed request embedded in another (itself included) carries
  # an empty key and the embedding method declares INVALID_ARGUMENT; a request
  # holding nothing but the key (and no extension range) never declares
  # ALREADY_EXISTS and has no keyed batch. The artefacts are the
  # shipped `bones/v1` proto and those checks, which no TypeScript package
  # wraps -- so this surface is Rust alone, the same way delegation-templates
  # is.
  keyed-writes:
    contract: tests/features/keyed_writes.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-protos, api-bones-automation]
  # Which entry of the TypeScript package carries workload identity. Rust has
  # no entry split: the crate is server-only, so this surface is TS alone.
  connect-workload-identity:
    contract: tests/features/connect_workload_identity.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The browser transport's route for contract-declared public reads: only the
  # web entry has a public lane to reach, so this surface is TS alone.
  connect-public-read:
    contract: tests/features/connect_public_read.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # A relative base URL is resolved against the page: only the TypeScript
  # transport has a page, so this surface is TS alone.
  connect-rate-limit-relative-base:
    contract: tests/features/connect_rate_limit_relative_base.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The browser transport echoes the BFF's CSRF synchronizer: only the web
  # entry talks to a session BFF, so this surface is TS alone, the same way
  # connect-public-read is.
  connect-csrf-synchronizer:
    contract: tests/features/connect_csrf_synchronizer.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The default If-Match precondition, attached from the transport itself.
  # Rust carries the same decision, but from outside the package this surface
  # names (see internal_behavior_owners, below) -- so this surface is TS
  # alone, the same way connect-workload-identity is.
  connect-precondition:
    contract: tests/features/connect_precondition.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # `onRetry` is attached to the retry interceptor itself, and Rust has no
  # client interceptor pipeline for one to live in -- so this surface is TS
  # alone, the same way connect-workload-identity is.
  connect-retry-observability:
    contract: tests/features/connect_retry_observability.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # Replay of requests that provably never left the client lives in the retry
  # interceptor, and Rust has no client interceptor pipeline -- so this surface
  # is TS alone, the same way connect-retry-observability is.
  connect-undelivered-retry:
    contract: tests/features/connect_undelivered_retry.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # A refused call's wait, its bound by the call's deadline and abort, and its
  # typed rejection. Rust runs the one retry policy in api-bones under the page
  # and batch call types; TypeScript runs it in the retry interceptor. Both
  # packages answer the one contract.
  connect-rate-limit-retry:
    contract: tests/features/connect_rate_limit_retry.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-page, api-bones-batch]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The emitted error code a refusal carries: read back from the Connect error
  # detail, exposed on the SDK error type, and rendered as a text token. The
  # wire message is the `bones/v1` errors proto, which the Rust crate embeds;
  # the Rust half lives in the root crate and its `api-bones-connect` re-export.
  connect-error-info:
    contract: tests/features/connect_error_info.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-connect, api-bones-protos]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # A refusal that names when to come back: Retry-After, RetryInfo and
  # no-store written by one rounding rule, read back from the detail first and
  # the header second.
  connect-retry-after:
    contract: tests/features/connect_retry_after.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-connect]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The one replay rule for the BFF CSRF synchronizer, exported by both
  # packages so no caller restates it.
  connect-csrf-replay-rule:
    contract: tests/features/connect_csrf_replay_rule.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-connect]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The control frame a long-lived server stream is replaced with: the codec
  # and the field number. The codec lives in api-bones-stream, which depends on
  # no api-bones major, and api-bones re-exports it. TypeScript needs no codec:
  # it reads the control as a field of the consumer's own generated message,
  # which connect-stream-rotation covers -- so this surface is Rust alone.
  connect-stream-control:
    contract: tests/features/connect_stream_control.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-stream]
  # The rotation decision logic that replaces a long-lived stream before the
  # server cuts it: both languages answer the one contract, action for action.
  connect-stream-rotation:
    contract: tests/features/connect_stream_rotation.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-stream]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # What a page that imports one export carries: only the TS package is bundled
  # for a browser, so this surface is TS alone.
  connect-tree-shaking:
    contract: tests/features/connect_tree_shaking.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The keepalive an idle server stream carries so a proxy does not cut it:
  # the reserved-field payload, the opt-in header, and the reader that drops
  # it before the caller sees it. Both languages answer the one contract.
  connect-stream-keepalive:
    contract: tests/features/connect_stream_keepalive.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-stream]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The bound a fleet-internal channel puts on establishing its connection, and
  # the failure that says it happened while connecting. Both live in
  # api-bones-transport, the fleet's one Connect client transport, which
  # api-bones-connect builds its in-fleet client from. TypeScript has no fleet
  # client, so this surface is Rust alone.
  connect-fleet-connect-bound:
    contract: tests/features/connect_fleet_connect_bound.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-transport, api-bones-connect]
  # Page iteration on generated platform clients in Rust and TypeScript. The
  # Rust paging glue of a generated client is emitted by api-bones-automation
  # (emit_rust_paging), so that crate ships this surface's Rust target too.
  connect-pagination:
    contract: tests/features/connect_pagination.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-page, api-bones-batch, api-bones-protos, api-bones-automation]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The adapter that turns a list call of wire messages into a list call of
  # domain values: TypeScript Layer 2 only.
  connect-map-list-call:
    contract: tests/features/connect_map_list_call.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  connect-batch:
    contract: tests/features/connect_batch.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-batch, api-bones-protos]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  connect-batch-client-construction:
    contract: tests/features/connect_batch_client_construction.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  connect-pagination-client-construction:
    contract: tests/features/connect_pagination_client_construction.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  connect-batch-identity-guard:
    contract: tests/features/connect_batch_identity_guard.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-batch]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The root certificates a Connect client trusts, which an operator injects
  # with `SSL_CERT_FILE`. TypeScript leaves root trust to its runtime, so this
  # surface is Rust alone.
  connect-root-store:
    contract: tests/features/connect_root_store.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones]
  # How far a failed call got, and the rule that only a call proven never to
  # have reached the server is replayed. TypeScript has no fleet transport to
  # classify from, so this surface is Rust alone.
  connect-failure-reach:
    contract: tests/features/connect_failure_reach.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones, api-bones-transport]
  # A batch read through a generated client on the fleet transport. TypeScript
  # has no fleet client, so this surface is Rust alone.
  connect-batch-fleet-transport:
    contract: tests/features/connect_batch_fleet_transport.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-batch, api-bones-transport]
  connect-paged-without-batch:
    contract: tests/features/connect_paged_without_batch.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-batch]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  typed-fields:
    contract: tests/features/typed_fields.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-fields, api-bones-protos]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # Layer 2 error rendering (wire-contract-proto-connect decision 9, ARB-keys-10
  # ruled A): a Connect error, a problem+json body or a ValidationFailure
  # renders in the viewer's locale from the owner's catalog. Catalog key
  # resolution, fallback, locale negotiation and argument bounds have one
  # definition tested in both languages; CLDR plural/number/date formatting is
  # native per language (CTO carve-out 2026-11-20). No `owns:` — this surface
  # names no RPC endpoint.
  error-localization:
    contract: tests/features/error_localization.feature
    targets:
      rust:
        delivery: package
        packages: [api-bones-i18n, api-bones]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The TypeScript base module carries English only and every other platform
  # catalog loads on demand through loadErrorCatalog. Rust embeds all 29
  # catalogs at compile time and has nothing to load, so this surface is TS
  # alone.
  error-localization-loading:
    contract: tests/features/error_localization_loading.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-connect"]
  # The Layer 2 call helper every service's Rust Layer 2 runs through: one
  # call budget, trace context and identity re-check decided in Rust. A server
  # stream reads the identity at open and checks it again before each item is
  # delivered; a change ends the stream with IDENTITY_CHANGED. A list page
  # fetched ahead is checked again before it is delivered.
  # TypeScript reaches it only through the generated wrapper package of the
  # test service over the wasm binding. This surface is the only owner of the
  # wrapper's l2_methods; it names no `owns:` because it is a library with no
  # image.
  sdk-call:
    contract: tests/sdk-call/features/sdk_call.feature
    l2_methods:
      contract.v1.NumbersService/ListNoSideEffects:
        rust: [list]
        typescript: [list]
      contract.v1.WidgetsService/ListWidgetParts:
        rust: [list_widget_parts]
        typescript: [listWidgetParts]
      contract.v1.WidgetsService/GetWidget:
        rust: [get_widget]
        typescript: [getWidget]
      contract.v1.WidgetsService/BatchGetWidgets:
        rust: [batch_get_widgets]
        typescript: [batchGetWidgets]
      contract.v1.EventsService/Watch:
        rust: [watch]
        typescript: [watch]
    targets:
      rust:
        delivery: package
        packages: [api-bones-test-service-l2]
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-test-service-sdk"]
  # Rust callers use api-bones-sdk directly; the runtime is the TypeScript face
  # alone, so this surface is TS alone.
  sdk-runtime:
    contract: tests/features/sdk_runtime.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-sdk-runtime"]
  # The wasm load and boundary are TS alone; Rust unit tests private.rs
  # check_boundary + macro golden cover the native side. A native Rust client
  # compiles together with its core, so no wasm module can differ from its
  # build stamp and no wrapper can speak another boundary version. The
  # refusals of an altered artifact are proven under internal_behavior_owners.
  # No `l2_methods:`: the sdk-call surface owns the wrapper's methods.
  sdk-wasm-load:
    contract: tests/sdk-call/features/sdk_wasm_load.feature
    targets:
      typescript:
        delivery: package
        packages: ["@brefwiz/api-bones-test-service-sdk"]
internal_behavior_owners:
  # The TypeScript test service is the loopback server a browser page under
  # test calls: it answers a preflight for loopback origins only, and its
  # control port serves only Host 127.0.0.1:<port> and POSTs of
  # application/json, with no CORS headers. The package exports no call that
  # reaches either check, so they are provider code no declared sdk_surface
  # exposes.
  - paths:
      - api-bones-test-service-ts/src/cors.ts
      - api-bones-test-service-ts/src/control-guard.ts
    feature: tests/features/test_service_preflight.feature
  # The automation manifest compiler and its DSSE signature check are library
  # code the service-kit, the CLI and the layer embed directly; no declared
  # sdk_surface exposes them.
  - paths:
      - api-bones-automation/src/lib.rs
      - api-bones-automation/src/compile.rs
      - api-bones-automation/src/digest.rs
      - api-bones-automation/src/document.rs
      - api-bones-automation/src/manifest.rs
      - api-bones-automation/src/pool.rs
      - api-bones-automation/src/signature.rs
    feature: tests/internal-bdd/tests/features/automation_manifest.feature
  # domain_to_connect attaches the error code a domain error names; callers
  # reach it directly as library code, not through a generated contract.
  - paths: [src/connect/domain_error.rs]
    feature: tests/internal-bdd/tests/features/connect_domain_error_code.feature
  # with_bearer sets transport-layer default headers no declared sdk_surface
  # exposes -- callers reach it directly as library code, not through a
  # generated contract.
  - paths: [src/connect/transport.rs]
    feature: tests/internal-bdd/tests/features/connect_client_headers.feature
  # MAX_LIMIT/DEFAULT_LIMIT and the validation/clamping logic that reads them
  # are library code no declared sdk_surface exposes -- callers reach them
  # directly as PaginationParams/CursorPaginationParams/KeysetPaginationParams
  # constructors and the Connect page builder, not through a generated
  # contract.
  - paths: [src/pagination.rs, src/connect/page.rs, src/fake_impls.rs]
    feature: tests/internal-bdd/tests/features/pagination_max_limit.feature
  # The Rust half of the default If-Match precondition lives in the root
  # crate, outside the api-bones-connect package root the TypeScript half
  # answers to as its SDK contract (connect-precondition, below) -- so this
  # half is library code no declared sdk_surface exposes.
  - paths: [src/connect/mod.rs, src/connect/precondition_client.rs]
    feature: tests/features/connect_precondition.feature
  # ConnectCode is the wire-level Connect error code; callers reach it
  # directly as library code, not through a generated contract.
  - paths: [src/connect/code.rs, src/connect/mod.rs]
    feature: tests/internal-bdd/tests/features/connect_code.feature
  # check_if_match/etag_from_updated_at are Connect adapter code no declared
  # sdk_surface exposes -- callers reach them directly as library code, not
  # through a generated contract.
  - paths: [src/connect/etag.rs]
    feature: tests/internal-bdd/tests/features/connect_if_match_precondition.feature
  # The Rust half of the browser transport's GET selection for eligible
  # reads: transport code callers wrap around a generated client directly,
  # outside any declared sdk_surface.
  - paths: [src/connect/get_read_client.rs, src/connect/mod.rs]
    feature: tests/internal-bdd/tests/features/connect_get_reads.feature
  # The error type URI mode (default URN namespace, application-set namespace
  # or base URL, runtime environment resolution) is library code no declared
  # sdk_surface exposes -- callers reach it directly as error_type_mode and
  # set_error_type_mode, not through a generated contract.
  - paths: [src/error.rs]
    feature: tests/internal-bdd/tests/features/error_type_mode.feature
  # The shipped proto vocabulary (resource, call shape, MCP opt-in, automation
  # surface declaration and signed manifest) is
  # library code callers embed directly; no generated contract reaches it.
  - paths: [api-bones-protos/src/lib.rs, api-bones-automation/src/batch_of.rs, api-bones-automation/src/nested_write_of.rs, api-bones-automation/src/method_shape.rs]
    feature: tests/internal-bdd/tests/features/proto_vocabulary.feature
  # The wasm load check and the boundary stamp of a generated TypeScript
  # wrapper fault only when the built artifact is altered (a flipped module
  # byte, another boundary version in the wrapper's init). Those faults change
  # the artifact itself, so no SDK call can produce them; the proof alters a
  # copy of the built package and opens it. The provenance a healthy load
  # records stays on the sdk-wasm-load surface.
  - paths: [api-bones-sdk-gen/src/wrapper_ts.rs]
    feature: tests/internal-bdd/tests/features/sdk_wasm_integrity.feature
  # The package a generated wrapper publishes is the name in the
  # `package.json` tracked beside it; the generator refuses a crate whose
  # exports or listing surfaces disagree with that package. It is build
  # tooling, so no SDK call can reach it.
  - paths: [api-bones-sdk-gen/src/wrapper_package.rs, api-bones-sdk-gen/src/build_ts.rs]
    feature: tests/internal-bdd/tests/features/sdk_wrapper_package.feature
library_crates:
  - api-bones
  - api-bones-automation
  - api-bones-batch
  - api-bones-fields
  # Independent of api-bones majors (decision 12d precedent): the error
  # catalogs and their renderer version on their own.
  - api-bones-i18n
  - api-bones-page
  - api-bones-progenitor
  - api-bones-protos
  - api-bones-reqwest
  - api-bones-rt
  - api-bones-sdk
  - api-bones-sdk-gen
  - api-bones-sdk-macros
  - api-bones-sdk-model
  - api-bones-stream
  - api-bones-test
  - api-bones-test-service-l2
  - api-bones-transport
  - api-bones-tower
rpc_protocols:
  supported: [proto, json]
  service:
    default: proto
  webapp:
    default: proto
capability_exposes: []
capability_consumes: []
ci_snowflakes: []
proto_packages: []
openapi_path: ~
publishes: [rust-crates, npm]
version_ecosystem: rust
# Ceilings, not targets: they only ratchet down (`--lower` rewrites them as the
# duplication goes), and a new copy-paste still fails against them. The existing
# volume is tracked in the issue below rather than accepted silently.
duplication_baseline:
  issue: 2
  adapter_construction: 0
  noop_in_production: 0
  test_double_in_production: 0
  duplicate_dispatch: 0
  duplicate_literals: 0
  clone_tokens: 2152
  test_clone_tokens: 309
ci_checks:
  license_headers:
    # Cargo metadata is authoritative here: these crates are MIT, and the
    # check's default expects the proprietary identifier every other repo uses.
    mode: cargo-per-crate
migration_baseline:
  utoipa_handler_count: 0
  baseline_commit: ~
migration_priority: ~
migration_eta: ~
---

# api-bones — Development Spec

api-bones is the shared error/response and Connect-transport layer every Brefwiz
service builds on. It owns no wire surface of its own: it supplies the types and
transport composition that consumer services use to define theirs, which is what
`wire_surface: library` records.

It publishes on two ecosystems, and they release on separate lineages:

- Rust crates to crates.io on `v*` tags.
- npm packages (`api-bones-connect`, `api-bones-axios`, `api-bones-otel`,
  `api-bones-types`, `api-bones-sdk-runtime`, `api-bones-test-service`,
  `api-bones-test-service-sdk`) on their own prefixed tags.
  `api-bones-connect` publishes to the brefwiz
  registry, because packages published there depend on it and npm resolves a
  registry per scope rather than per package.
- `@brefwiz/api-bones-test-service-sdk` is the generated TypeScript wrapper of
  `api-bones-test-service-l2` (directory `api-bones-test-service-sdk-ts`, outside
  `tests/` so the package lane discovers it). The generator writes its
  manifest: its `name` is the package, read back from the tracked
  `package.json` (one crate, one wrapper, one package); `private` follows the Layer 2 crate's `publish` setting, the sdk
  runtime is a peer dependency (`>=` the runtime floor) with the in-tree `file:`
  link as a dev dependency, and a published manifest names the registry
  its sdk runtime publishes to. Its `src/generated` is gitignored and built by
  `ci-sdk-autogenerate`; only `package.json`, `package-lock.json`,
  `l2-methods.json`, `api-surface.d.ts` and `LICENSE` are tracked. Its version is
  the fixed placeholder `0.0.0`; the package lane stamps the tag version into
  `package.json` at pack time.

## Coverage

Target 100%, floor 96 (lines and functions), raise the floor as gaps close.
The floor is set in the `Makefile` (`COVERAGE_FAIL_UNDER_LINES`,
`COVERAGE_FAIL_UNDER_FUNCTIONS`); CI and `make ci-coverage` apply the same
gate knobs.

## Canonical home

This repository lives in Gitea; the GitHub repository is a downstream push
mirror kept for public visibility. CI runs on CDS from `.cds/workflows/`.
