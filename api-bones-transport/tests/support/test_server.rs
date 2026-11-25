// SPDX-License-Identifier: MIT
//! The test server's address, shared by the wasm suites that call it
//! (`api-bones-transport`'s and `api-bones-sdk`'s). Each includes this file
//! as a module; it is not a test target of its own.

use js_sys::Reflect;
use wasm_bindgen::JsValue;

/// The test server's base URL, from `API_BONES_TEST_SERVER`.
pub fn server() -> String {
    let process = Reflect::get(&js_sys::global(), &JsValue::from_str("process"))
        .expect("the suite runs in Node");
    let env = Reflect::get(&process, &JsValue::from_str("env")).expect("Node has process.env");
    let url = Reflect::get(&env, &JsValue::from_str("API_BONES_TEST_SERVER"))
        .ok()
        .and_then(|value| value.as_string())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .expect(
            "API_BONES_TEST_SERVER must name the test server's base URL; \
             run the suite through `make ci-wasm-test`",
        );
    if url.contains("://") {
        url
    } else {
        format!("http://{url}")
    }
}
