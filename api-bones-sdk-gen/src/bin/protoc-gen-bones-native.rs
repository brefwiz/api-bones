// SPDX-License-Identifier: MIT
//! Buf/protoc plugin entry point for callable native clients.

use api_bones_sdk_gen::native;

fn main() {
    if let Err(error) = native::run_plugin() {
        native::write_plugin_error(&error.to_string());
    }
}
