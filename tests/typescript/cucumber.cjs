// SPDX-License-Identifier: MIT
// The contract lives beside this lane, at ../features, and the Rust lane
// answers the same file.
module.exports = {
  default: {
    paths: ["../features/connect_retry_eligibility.feature"],
    import: ["steps/retry_eligibility.ts"],
    strict: true,
    format: ["progress"],
    publishQuiet: true,
  },
};
