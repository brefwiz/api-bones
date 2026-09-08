// SPDX-License-Identifier: MIT
//
// The contract lives at the repository root, not under this package: one file
// answered by BOTH languages is the point, so neither owns it. The Rust half
// runs the same path from api-bones-connect/tests.
module.exports = {
  default: {
    paths: ["../tests/features/connect_retry_eligibility.feature"],
    require: ["features/steps/retry_eligibility.ts"],
    requireModule: ["tsx/cjs"],
    strict: true,
    format: ["progress"],
    publishQuiet: true,
  },
};
