// SPDX-License-Identifier: MIT
// The contracts live beside this lane, at ../features. The Rust lane answers
// the retry-eligibility file too; workload identity and the public lane are
// TypeScript-only entries.
module.exports = {
  default: {
    paths: [
      "../features/connect_retry_eligibility.feature",
      "../features/connect_workload_identity.feature",
      "../features/connect_public_read.feature",
      "../features/connect_precondition.feature",
    ],
    import: [
      "steps/retry_eligibility.ts",
      "steps/workload_identity.ts",
      "steps/public_read.ts",
      "steps/precondition.ts",
    ],
    strict: true,
    format: ["progress"],
    publishQuiet: true,
  },
};
