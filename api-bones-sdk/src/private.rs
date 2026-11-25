// SPDX-License-Identifier: MIT
//! What the code of `#[client]` reaches through `api_bones_sdk::__private`.
//!
//! Not part of the API: the macro output names these items, so a consumer
//! needs no dependency of its own on them.

pub use serde_json;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use {js_sys, wasm_bindgen, web_sys};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use browser::{on_abort, promise};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use std::future::Future;

    use futures_util::future::{Either, select};
    use wasm_bindgen::JsValue;

    use crate::call::AbortFlag;
    use crate::error::Error;

    /// A promise that resolves to what `future` yields and rejects with the
    /// error JSON. An abort of `signal` drops `future` and rejects with
    /// `canceled`.
    pub fn promise(
        signal: Option<web_sys::AbortSignal>,
        future: impl Future<Output = Result<JsValue, Error>> + 'static,
    ) -> js_sys::Promise {
        let abort = AbortFlag::from_signal(signal.as_ref());
        wasm_bindgen_futures::future_to_promise(async move {
            let outcome = {
                match select(abort.aborted(), Box::pin(future)).await {
                    Either::Left(_) => Err(Error::canceled()),
                    Either::Right((outcome, _)) => outcome,
                }
            };
            outcome.map_err(|error| JsValue::from_str(&error.to_json()))
        })
    }

    /// Runs `f` once `signal` aborts, or on the next turn when it already has.
    /// With no signal `f` never runs.
    pub fn on_abort(signal: &Option<web_sys::AbortSignal>, f: impl FnOnce() + 'static) {
        if signal.is_none() {
            return;
        }
        let abort = AbortFlag::from_signal(signal.as_ref());
        wasm_bindgen_futures::spawn_local(async move {
            abort.aborted().await;
            f();
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::serde_json;

    #[test]
    fn serde_json_is_reexported_for_the_macro_output() {
        let value = serde_json::json!({ "name": "x", "n": 1 });
        assert_eq!(value.to_string(), r#"{"n":1,"name":"x"}"#);
    }
}
