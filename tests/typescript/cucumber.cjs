// SPDX-License-Identifier: MIT
// The contracts live beside this lane, at ../features. The Rust lane answers
// the retry-eligibility file too; workload identity, the public lane, the CSRF synchronizer echo, the
// default precondition, retry observability and undelivered-request replay are TypeScript-only entries.
// The error-info, CSRF replay-rule and pinned-peer contracts are answered by both lanes.
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
      "../features/connect_error_info.feature",
      "../features/connect_csrf_replay_rule.feature",
      "../features/connect_pinned_peer.feature",
    ],
    import: [
      "steps/retry_eligibility.ts",
      "steps/workload_identity.ts",
      "steps/public_read.ts",
      "steps/csrf_synchronizer.ts",
      "steps/precondition.ts",
      "steps/retry_observability.ts",
      "steps/undelivered_retry.ts",
      "steps/error_info.ts",
      "steps/csrf_replay_rule.ts",
      "steps/pinned_peer.ts",
    ],
    strict: true,
    format: ["progress"],
    publishQuiet: true,
  },
};
