// SPDX-License-Identifier: MIT
//! The binding generator, run against the built library to write the Kotlin
//! and Swift sources.

fn main() {
    uniffi::uniffi_bindgen_main();
}
