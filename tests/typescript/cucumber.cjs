// SPDX-License-Identifier: MIT
// The contracts live beside this lane, at ../features. The Rust lane answers
// the retry-eligibility file too; workload identity, the public lane, the CSRF synchronizer echo, the
// default precondition, retry observability and undelivered-request replay are TypeScript-only entries.
// The rate-limit retry contract is answered by both lanes; its relative-base-URL contract is TypeScript-only.
// The list-call mapping contract is TypeScript-only: it is a Layer 2 adapter of the TS package.
// The package-shaking contract is TypeScript-only: Rust has no bundler.
// The catalog-loading contract is TypeScript-only: Rust embeds every locale at compile time.
// It runs before error_localization.feature, whose Background loads every catalog into the one module instance.
// The SDK runtime contract is TypeScript-only: Rust callers use api-bones-sdk directly.
// The test service preflight contract is TypeScript-only: the Rust test service serves no browser page.
// The error-info, CSRF replay-rule, pinned-peer, field-labels and stream-rotation, stream-keepalive and error-localization contracts are answered by both lanes.
module.exports = {
  default: {
    paths: [
      "../features/connect_retry_eligibility.feature",
      "../features/connect_workload_identity.feature",
      "../features/connect_public_read.feature",
      "../features/connect_csrf_synchronizer.feature",
      "../features/connect_precondition.feature",
      "../features/connect_retry_observability.feature",
      "../features/connect_undelivered_retry.feature",
      "../features/connect_rate_limit_retry.feature",
      "../features/connect_rate_limit_relative_base.feature",
      "../features/connect_error_info.feature",
      "../features/connect_csrf_replay_rule.feature",
      "../features/connect_pinned_peer.feature",
      "../features/field_labels.feature",
      "../features/overridable_defaults.feature",
      "../features/connect_stream_rotation.feature",
      "../features/connect_pagination.feature",
      "../features/connect_batch.feature",
      "../features/connect_batch_client_construction.feature",
      "../features/connect_pagination_client_construction.feature",
      "../features/connect_batch_identity_guard.feature",
      "../features/connect_paged_without_batch.feature",
      "../features/typed_fields.feature",
      "../features/connect_stream_keepalive.feature",
      "../features/connect_tree_shaking.feature",
      "../features/connect_map_list_call.feature",
      "../features/error_localization_loading.feature",
      "../features/error_localization.feature",
      "../features/sdk_runtime.feature",
      "../features/test_service_preflight.feature",
    ],
    import: [
      "steps/retry_eligibility.ts",
      "steps/workload_identity.ts",
      "steps/public_read.ts",
      "steps/csrf_synchronizer.ts",
      "steps/precondition.ts",
      "steps/retry_observability.ts",
      "steps/undelivered_retry.ts",
      "steps/rate_limit_retry.ts",
      "steps/error_info.ts",
      "steps/csrf_replay_rule.ts",
      "steps/pinned_peer.ts",
      "steps/field_labels.ts",
      "steps/overridable_defaults.ts",
      "steps/stream_rotation.ts",
      "steps/pagination.ts",
      "steps/batch.ts",
      "steps/batch_client_construction.ts",
      "steps/pagination_client_construction.ts",
      "steps/batch_identity_guard.ts",
      "steps/paged_without_batch.ts",
      "steps/typed_fields.ts",
      "steps/stream_keepalive.ts",
      "steps/tree_shaking.ts",
      "steps/map_list_call.ts",
      "steps/error_localization_loading.ts",
      "steps/error_localization.ts",
      "steps/sdk_runtime.ts",
      "steps/test_service_preflight.ts",
    ],
    strict: true,
    // The summary formatter on stderr prints each failing scenario and its
    // assertion after the progress dots, which the streamed log otherwise cuts.
    format: ["progress", "summary:/dev/stderr"],
    publishQuiet: true,
  },
};
