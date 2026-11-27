// SPDX-License-Identifier: MIT
// The sdk-call contract lives beside this lane, at ../features. The Rust lane
// answers the same file; the wasm-load and package-values contracts are this lane's alone. This lane
// drives the generated wrapper of the test service over its wasm Layer 2 and
// nothing else.
module.exports = {
  default: {
    paths: [
      "../features/sdk_call.feature",
      "../features/sdk_wasm_load.feature",
      "../features/sdk_package_values.feature",
    ],
    import: ["steps/sdk_call.ts", "steps/sdk_wasm_load.ts", "steps/sdk_package_values.ts"],
    strict: true,
    // The summary formatter on stderr prints each failing scenario and its
    // assertion after the progress dots, which the streamed log otherwise cuts.
    format: ["progress", "summary:/dev/stderr"],
    publishQuiet: true,
  },
};
