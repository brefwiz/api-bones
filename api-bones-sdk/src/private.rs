// SPDX-License-Identifier: MIT
//! What the code of `#[client]` reaches through `api_bones_sdk::__private`.
//!
//! Not part of the API: the macro output names these items, so a consumer
//! needs no dependency of its own on them.

use std::time::{Duration, SystemTime};

use buffa::{DecodeOptions, Message};
use buffa_types::google::protobuf::{
    BoolValue, BytesValue, DoubleValue, Duration as ProtoDuration, Int32Value, Int64Value,
    StringValue, Timestamp, UInt32Value, UInt64Value,
};

use crate::BOUNDARY;
use crate::error::Error;

pub use buffa::MessageName;
pub use serde_json;

pub use crate::boundary::{
    credential_read, decode_call_error, encode_batch_chunk, encode_build_info, encode_call_error,
    encode_format_refused, encode_format_text, encode_page, span_end, span_start, span_started,
};

/// Decodes the message a host sent, refusing any field the message does not
/// declare.
///
/// Every value that crosses is a message declared in proto, and the generated
/// wrapper encodes it from that declaration, so a field this core does not know
/// means the wrapper and the core were built from different protos: the call is
/// refused rather than run with a parameter dropped.
///
/// # Errors
/// `invalid_argument` when `bytes` is not the encoding of an `M` or carries a
/// field `M` does not declare.
pub fn decode_strict<M: Message + Default>(bytes: &[u8]) -> Result<M, Error> {
    DecodeOptions::new()
        .with_unknown_field_limit(0)
        .decode_from_slice::<M>(bytes)
        .map_err(|error| Error::invalid_argument(format!("the message does not decode: {error}")))
}

/// The options message of a client, decoded as [`decode_strict`] does.
///
/// # Errors
/// As [`decode_strict`].
pub fn decode_options<M: Message + Default>(bytes: &[u8]) -> Result<M, Error> {
    decode_strict(bytes)
}

/// Refuses arguments for a method that takes none.
///
/// # Errors
/// `invalid_argument` when `bytes` is not empty.
pub fn no_args(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(Error::invalid_argument(
            "this method takes no arguments, send an empty message",
        ))
    }
}

/// Whether two names are the same, evaluable in a `const` so the macro output
/// can check at compile time that the message it names is the one the model
/// recorded.
#[must_use]
pub const fn same_name(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0;
    while at < left.len() {
        if left[at] != right[at] {
            return false;
        }
        at += 1;
    }
    true
}

/// Refuses options for a client that takes none.
///
/// # Errors
/// `invalid_argument` when `bytes` is not empty.
pub fn no_options(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(Error::invalid_argument(
            "this client takes no options, send an empty message",
        ))
    }
}

/// Refuses a client whose wrapper or runtime was built for another boundary
/// version than this core.
///
/// # Errors
/// `SDK_BOUNDARY_MISMATCH` naming the three versions.
pub fn check_boundary(wrapper: u32, runtime: u32) -> Result<(), Error> {
    if wrapper == BOUNDARY && runtime == BOUNDARY {
        Ok(())
    } else {
        Err(Error::boundary_mismatch(BOUNDARY, wrapper, runtime))
    }
}

/// The protobuf encoding of a message: what a result crosses as.
#[must_use]
pub fn encode_message<M: Message>(message: &M) -> Vec<u8> {
    message.encode_to_vec()
}

/// A scalar that crosses as the well-known wrapper message of its type, never
/// as an implicit field 1 of a message of the crate's own.
pub trait Wrapped {
    /// The `google.protobuf` message the scalar crosses as.
    type Wrapper: Message + Default;

    /// The scalar in its wrapper.
    fn wrap(&self) -> Self::Wrapper;
}

macro_rules! wrapped {
    ($($scalar:ty => $wrapper:ident),* $(,)?) => {$(
        impl Wrapped for $scalar {
            type Wrapper = $wrapper;

            fn wrap(&self) -> $wrapper {
                $wrapper { value: self.clone(), ..$wrapper::default() }
            }
        }
    )*};
}

wrapped! {
    bool => BoolValue,
    i32 => Int32Value,
    u32 => UInt32Value,
    i64 => Int64Value,
    u64 => UInt64Value,
    f64 => DoubleValue,
    String => StringValue,
    Vec<u8> => BytesValue,
}

impl Wrapped for SystemTime {
    type Wrapper = Timestamp;

    fn wrap(&self) -> Timestamp {
        Timestamp::from(*self)
    }
}

impl Wrapped for Duration {
    type Wrapper = ProtoDuration;

    fn wrap(&self) -> ProtoDuration {
        ProtoDuration::from(*self)
    }
}

/// The encoding of a scalar in its wrapper message.
#[must_use]
pub fn encode_wrapped<T: Wrapped>(value: &T) -> Vec<u8> {
    value.wrap().encode_to_vec()
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use {js_sys, wasm_bindgen, web_sys};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub use browser::{Lazy, Opened, on_abort, promise};

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use std::cell::RefCell;
    use std::future::Future;
    use std::mem;
    use std::pin::Pin;
    use std::rc::Rc;

    use futures_util::future::{Either, select};
    use wasm_bindgen::JsValue;

    use crate::call::AbortFlag;
    use crate::error::Error;
    use crate::handle::Handle;

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
    /// `bones.sdk.v1.CallError` bytes as a `Uint8Array`. An abort of `signal`
    /// drops `future` and rejects with `canceled`.
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
            outcome.map_err(|error| {
                js_sys::Uint8Array::from(crate::boundary::encode_call_error(&error).as_slice())
                    .into()
            })
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
    use super::*;
    use crate::SDK_BOUNDARY_MISMATCH;

    #[test]
    fn a_message_decodes_strictly_and_an_undeclared_field_is_refused() {
        let known = UInt32Value {
            value: 5,
            ..UInt32Value::default()
        }
        .encode_to_vec();
        let decoded: UInt32Value = decode_strict(&known).expect("a declared field");
        assert_eq!(decoded.value, 5);
        let empty: UInt32Value = decode_strict(&[]).expect("the empty message");
        assert_eq!(empty.value, 0);

        let mut unknown = known.clone();
        unknown.extend([0x10, 0x01]);
        let error = decode_strict::<UInt32Value>(&unknown).expect_err("field 2 is not declared");
        assert!(error.message.contains("does not decode"), "{error}");
        assert!(decode_strict::<UInt32Value>(&[0x08]).is_err(), "truncated");
    }

    #[test]
    fn options_decode_as_a_strict_message_and_none_means_the_empty_message() {
        let bytes = BoolValue {
            value: true,
            ..BoolValue::default()
        }
        .encode_to_vec();
        assert!(decode_options::<BoolValue>(&bytes).expect("ok").value);
        assert!(decode_options::<BoolValue>(&[0x10, 0x01]).is_err());
        assert!(no_options(&[]).is_ok());
        let error = no_options(&bytes).expect_err("a client without options");
        assert!(error.message.contains("no options"), "{error}");
    }

    #[test]
    fn a_method_without_arguments_refuses_any_byte() {
        assert!(no_args(&[]).is_ok());
        let error = no_args(&[0x08, 0x01]).expect_err("a method without arguments");
        assert!(error.message.contains("no arguments"), "{error}");
    }

    #[test]
    fn names_compare_in_a_const_context() {
        const SAME: bool = same_name("google.protobuf.Empty", "google.protobuf.Empty");
        const OTHER: bool = same_name("a.B", "a.C");
        const SHORTER: bool = same_name("a.B", "a.Bc");
        const { assert!(SAME) };
        const { assert!(!OTHER) };
        const { assert!(!SHORTER) };
        assert!(same_name("", ""));
    }

    #[test]
    fn a_message_exposes_its_full_name() {
        assert_eq!(
            <UInt32Value as MessageName>::FULL_NAME,
            "google.protobuf.UInt32Value"
        );
        assert!(same_name(
            <UInt32Value as MessageName>::FULL_NAME,
            "google.protobuf.UInt32Value"
        ));
    }

    #[test]
    fn a_boundary_version_other_than_the_cores_is_refused() {
        assert!(check_boundary(BOUNDARY, BOUNDARY).is_ok());
        for (wrapper, runtime) in [(BOUNDARY + 1, BOUNDARY), (BOUNDARY, BOUNDARY + 1)] {
            let error = check_boundary(wrapper, runtime).expect_err("stale");
            assert_eq!(error.code, SDK_BOUNDARY_MISMATCH);
        }
    }

    #[test]
    fn a_message_encodes_as_its_own_bytes() {
        let message = StringValue {
            value: "x".to_owned(),
            ..StringValue::default()
        };
        assert_eq!(encode_message(&message), message.encode_to_vec());
        assert_eq!(encode_message(&message), [0x0a, 0x01, b'x']);
    }

    #[test]
    fn every_scalar_crosses_as_its_well_known_wrapper() {
        assert_eq!(encode_wrapped(&5_u32), [0x08, 5]);
        assert_eq!(encode_wrapped(&true), [0x08, 1]);
        assert_eq!(encode_wrapped(&7_i32), [0x08, 7]);
        assert_eq!(encode_wrapped(&7_i64), [0x08, 7]);
        assert_eq!(encode_wrapped(&7_u64), [0x08, 7]);
        assert_eq!(encode_wrapped(&1.0_f64).len(), 9);
        assert_eq!(encode_wrapped(&"ab".to_owned()), [0x0a, 2, b'a', b'b']);
        assert_eq!(encode_wrapped(&vec![1_u8, 2]), [0x0a, 2, 1, 2]);
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(3);
        let stamp = Timestamp::decode_from_slice(&encode_wrapped(&at)).expect("a timestamp");
        assert_eq!((stamp.seconds, stamp.nanos), (3, 0));
        let span = ProtoDuration::decode_from_slice(&encode_wrapped(&Duration::from_millis(1500)))
            .expect("a duration");
        assert_eq!((span.seconds, span.nanos), (1, 500_000_000));
    }

    #[test]
    fn serde_json_is_reexported_for_the_macro_output() {
        let value = serde_json::json!({ "name": "x", "n": 1 });
        assert_eq!(value.to_string(), r#"{"n":1,"name":"x"}"#);
    }
}
