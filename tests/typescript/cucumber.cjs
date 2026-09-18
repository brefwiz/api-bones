// SPDX-License-Identifier: MIT
// The contracts live beside this lane, at ../features. The Rust lane answers
// the retry-eligibility file too; workload identity is a TypeScript-only entry.
module.exports = {
  default: {
    paths: [
      "../features/connect_retry_eligibility.feature",
      "../features/connect_workload_identity.feature",
    ],
    import: ["steps/retry_eligibility.ts", "steps/workload_identity.ts"],
    strict: true,
    format: ["progress"],
    publishQuiet: true,
  },
};
