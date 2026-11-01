// SPDX-License-Identifier: MIT
//! The stream-control signal and its control-only frame.
//!
//! The codec lives in the `api-bones-stream` crate, which is independent of any
//! `api-bones` major; this module re-exports it unchanged.

pub use api_bones_stream::{
    CONTROL_FIELD_JSON_NAME, MalformedControl, STREAM_CONTROL_FIELD_NUMBER, STREAM_CONTROL_HEADER,
    STREAM_CONTROL_HEADER_VALUE, StreamControl,
};

#[cfg(test)]
mod tests {
    use super::*;

    // An api-bones user reaches the very items of api-bones-stream, not copies:
    // the assignments below compile only if the types are the same.
    #[test]
    fn re_exports_the_api_bones_stream_items() {
        let control: api_bones_stream::StreamControl = StreamControl::Drain { cut_in_ms: 1 };
        let malformed: api_bones_stream::MalformedControl = MalformedControl;
        assert_eq!(
            StreamControl::from_proto_frame(&control.to_proto_frame()),
            Ok(Some(control))
        );
        assert_eq!(malformed, api_bones_stream::MalformedControl);
        assert_eq!(
            (
                STREAM_CONTROL_HEADER,
                STREAM_CONTROL_HEADER_VALUE,
                STREAM_CONTROL_FIELD_NUMBER,
                CONTROL_FIELD_JSON_NAME,
            ),
            (
                api_bones_stream::STREAM_CONTROL_HEADER,
                api_bones_stream::STREAM_CONTROL_HEADER_VALUE,
                api_bones_stream::STREAM_CONTROL_FIELD_NUMBER,
                api_bones_stream::CONTROL_FIELD_JSON_NAME,
            )
        );
    }
}
