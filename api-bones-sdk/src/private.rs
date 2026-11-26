// SPDX-License-Identifier: MIT
//! What the code of `#[client]` reaches through `api_bones_sdk::__private`.
//!
//! Not part of the API: the macro output names these items, so a consumer
//! needs no dependency of its own on them.

pub use serde_json;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use {js_sys, wasm_bindgen, web_sys};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use browser::{Lazy, Opened, PageHold, on_abort, promise};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use std::any::Any;
    use std::cell::RefCell;
    use std::future::Future;
    use std::mem;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::Arc;

    use api_bones_page::Page;
    use futures_util::future::{Either, select};
    use wasm_bindgen::JsValue;
    use wasm_bindgen::prelude::wasm_bindgen;

    use crate::call::AbortFlag;
    use crate::error::Error;
    use crate::handle::Handle;

    /// Keeps the items of one page alive for as long as the host's page
    /// object is. The revision cache holds a page's items weakly and replays a
    /// chain only while every page is still held; a page that reached the host
    /// as JSON would otherwise be held by no one.
    #[wasm_bindgen(js_name = "api_bones__PageHold")]
    pub struct PageHold {
        _items: Box<dyn Any>,
    }

    impl PageHold {
        /// A hold on the items of `page`.
        pub fn new<T: 'static>(page: &Page<T>) -> Self {
            Self {
                _items: Box::new(Arc::clone(&page.items)),
            }
        }
    }

    /// What the host can do to an opened handle.
    pub trait Opened {
        /// Ends the handle.
        fn cancel(&self);
        /// The id of the handle's span.
        fn span(&self) -> Option<String>;
    }

    impl<T: 'static> Opened for Handle<T> {
        fn cancel(&self) {
            Handle::cancel(self);
        }

        fn span(&self) -> Option<String> {
            Handle::span(self)
        }
    }

    type Opening<H> = Pin<Box<dyn Future<Output = Result<H, Error>>>>;

    enum State<H> {
        /// The call that opens the handle has not started.
        Pending(Opening<H>),
        /// The call that opens the handle is running; `canceled` when the
        /// host canceled meanwhile.
        Opening {
            canceled: bool,
        },
        Open(Rc<H>),
        /// Canceled before the handle opened; the next read says so.
        Canceled,
        /// Nothing more to deliver.
        Closed,
    }

    /// A handle that opens on the first read: the host gets it at once and the
    /// call that opens it runs when the first `next()` asks.
    pub struct Lazy<H>(Rc<RefCell<State<H>>>);

    impl<H> Clone for Lazy<H> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<H: Opened + 'static> Lazy<H> {
        /// A handle that `opening` opens on the first read.
        pub fn new(opening: impl Future<Output = Result<H, Error>> + 'static) -> Self {
            Self(Rc::new(RefCell::new(State::Pending(Box::pin(opening)))))
        }

        /// The opened handle, opening it first when this is the first read.
        ///
        /// # Errors
        /// `HANDLE_BUSY` while another read is opening it, `canceled` when it
        /// was canceled before it opened, or the refusal of the opening call;
        /// every read after a refusal or a cancellation ends the stream.
        pub async fn open(&self) -> Result<Option<Rc<H>>, Error> {
            let opening = {
                let mut state = self.0.borrow_mut();
                match mem::replace(&mut *state, State::Closed) {
                    State::Pending(opening) => {
                        *state = State::Opening { canceled: false };
                        opening
                    }
                    State::Opening { canceled } => {
                        *state = State::Opening { canceled };
                        return Err(Error::handle_busy());
                    }
                    State::Open(handle) => {
                        *state = State::Open(handle.clone());
                        return Ok(Some(handle));
                    }
                    State::Canceled => return Err(Error::canceled()),
                    State::Closed => return Ok(None),
                }
            };
            let outcome = opening.await;
            let mut state = self.0.borrow_mut();
            let canceled = matches!(*state, State::Opening { canceled: true });
            match outcome {
                Ok(handle) => {
                    if canceled {
                        handle.cancel();
                    }
                    let handle = Rc::new(handle);
                    *state = State::Open(handle.clone());
                    Ok(Some(handle))
                }
                Err(error) => {
                    *state = State::Closed;
                    Err(error)
                }
            }
        }

        /// Ends the handle: a call that has not opened is dropped unstarted.
        pub fn cancel(&self) {
            let mut state = self.0.borrow_mut();
            match &mut *state {
                State::Pending(_) => *state = State::Canceled,
                State::Opening { canceled } => *canceled = true,
                State::Open(handle) => handle.cancel(),
                State::Canceled | State::Closed => {}
            }
        }

        /// The id of the span of the opened handle; none before it opened.
        pub fn span(&self) -> Option<String> {
            match &*self.0.borrow() {
                State::Open(handle) => handle.span(),
                _ => None,
            }
        }
    }

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
