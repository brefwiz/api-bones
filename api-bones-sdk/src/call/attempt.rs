// SPDX-License-Identifier: MIT
//! What one send of a call carries: the credential, the trace context and the
//! call's one deadline.

use std::fmt;

use api_bones_batch::{BONES_LIST_CACHE, Credential, HeaderPair};
use api_bones_page::ListMetadata;
use api_bones_transport::Deadline;
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, CallOptions, ClientBody, ClientTransport};
use http::header::{HeaderName, HeaderValue, IF_NONE_MATCH};
use http::{Request, Response};

use super::identity::CREDENTIAL_HEADERS;

/// One send of a call, handed to the caller's fetch closure.
///
/// The headers are the credential read just before this send, `traceparent`
/// and, when the call has one, `tracestate`. The deadline is the one instant
/// the whole call shares: every retry sees the same [`Deadline`]. A stream's
/// open carries none: the server is not told a budget it would apply to the
/// whole stream.
#[derive(Clone)]
pub struct Attempt {
    headers: Vec<HeaderPair>,
    deadline: Option<Deadline>,
}

/// Names the headers and shows no credential: the values of the credential
/// headers are redacted.
impl fmt::Debug for Attempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(name, value)| {
                if CREDENTIAL_HEADERS
                    .iter()
                    .any(|secret| name.eq_ignore_ascii_case(secret))
                {
                    format!("{name}: <redacted>")
                } else {
                    format!("{name}: {}", String::from_utf8_lossy(value))
                }
            })
            .collect();
        f.debug_struct("Attempt")
            .field("headers", &headers)
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl Attempt {
    pub(crate) fn new(
        credential: Credential,
        trace: &[HeaderPair],
        deadline: Option<Deadline>,
    ) -> Self {
        let mut headers = credential.headers;
        headers.extend_from_slice(trace);
        Self { headers, deadline }
    }

    /// This send with a list's cache metadata: `bones-list-cache: 1` when the
    /// list opts in, and `if-none-match` when it holds a revision to confirm.
    #[must_use]
    pub fn with_list(mut self, list: &ListMetadata) -> Self {
        if list.cache_opt_in {
            self.headers
                .push((BONES_LIST_CACHE.as_str().to_owned(), b"1".to_vec()));
        }
        if let Some(revision) = &list.if_none_match {
            self.headers.push((
                IF_NONE_MATCH.as_str().to_owned(),
                revision.as_bytes().to_vec(),
            ));
        }
        self
    }

    /// The headers this send carries, names lower-case.
    #[must_use]
    pub fn headers(&self) -> &[HeaderPair] {
        &self.headers
    }

    /// When the call's budget runs out; `None` for a stream's open, which
    /// states no budget to the server.
    #[must_use]
    pub fn deadline(&self) -> Option<Deadline> {
        self.deadline
    }

    /// The headers of this send as the generated client's call options.
    ///
    /// # Errors
    /// `invalid_argument` when a header does not fit HTTP.
    pub fn options(&self) -> Result<CallOptions, ConnectError> {
        let mut pairs = Vec::with_capacity(self.headers.len());
        for (name, value) in &self.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ConnectError::invalid_argument("invalid header name"))?;
            let value = HeaderValue::from_bytes(value)
                .map_err(|_| ConnectError::invalid_argument("invalid header value"))?;
            pairs.push((name, value));
        }
        Ok(CallOptions::default().with_headers(pairs))
    }

    /// `inner`, with this send's deadline, when it has one, on every request
    /// it sends.
    #[must_use]
    pub fn transport<T: ClientTransport>(&self, inner: T) -> WithDeadline<T> {
        WithDeadline {
            inner,
            deadline: self.deadline,
        }
    }
}

/// A transport that puts one call's [`Deadline`] in the extensions of every
/// request, so the transport states the time left in the timeout header and
/// gives up when it passes. A send without a deadline leaves the requests as
/// they are.
#[derive(Clone)]
pub struct WithDeadline<T> {
    inner: T,
    deadline: Option<Deadline>,
}

impl<T: ClientTransport> ClientTransport for WithDeadline<T> {
    type ResponseBody = T::ResponseBody;
    type Error = T::Error;

    fn send(
        &self,
        mut request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        if let Some(deadline) = self.deadline {
            request.extensions_mut().insert(deadline);
        }
        self.inner.send(request)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use api_bones_batch::Identity;
    use api_bones_rt::Instant;
    use bytes::Bytes;
    use http_body_util::{BodyExt as _, Empty};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn attempt(headers: &[(&str, &[u8])]) -> Attempt {
        attempt_with(headers, true)
    }

    fn attempt_with(headers: &[(&str, &[u8])], deadline: bool) -> Attempt {
        let credential = Credential {
            identity: Identity::default(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.to_vec()))
                .collect(),
        };
        let trace = vec![("traceparent".to_owned(), b"00-a-b-01".to_vec())];
        Attempt::new(
            credential,
            &trace,
            deadline.then(|| Deadline(Instant::now() + Duration::from_secs(1))),
        )
    }

    #[test]
    fn the_credential_comes_first_and_the_trace_context_follows() {
        let sent = attempt(&[("authorization", b"Bearer t")]);
        let names: Vec<_> = sent
            .headers()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["authorization", "traceparent"]);
    }

    #[test]
    fn list_metadata_adds_the_cache_headers_it_names() {
        let both = attempt(&[]).with_list(&ListMetadata {
            cache_opt_in: true,
            if_none_match: Some("\"r1\"".to_owned()),
        });
        let sent: Vec<_> = both
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_slice()))
            .collect();
        assert!(sent.contains(&("bones-list-cache", b"1".as_slice())));
        assert!(sent.contains(&("if-none-match", b"\"r1\"".as_slice())));
        let none = attempt(&[]).with_list(&ListMetadata::default());
        assert_eq!(none.headers().len(), 1, "only the trace context");
        let opted_in = attempt(&[]).with_list(&ListMetadata {
            cache_opt_in: true,
            if_none_match: None,
        });
        assert!(
            !opted_in
                .headers()
                .iter()
                .any(|(name, _)| name == "if-none-match")
        );
    }

    #[test]
    fn debug_names_every_header_and_shows_no_credential() {
        let sent = attempt(&[
            ("authorization", b"Bearer s3cret-token"),
            ("Brefwiz-Relayed-Credential", b"relayed-s3cret"),
            ("x-request", b"visible"),
        ]);
        let shown = format!("{sent:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        for name in [
            "authorization",
            "Brefwiz-Relayed-Credential",
            "traceparent",
            "x-request",
        ] {
            assert!(shown.contains(name), "{name} in {shown}");
        }
        assert!(shown.contains("visible"));
    }

    #[test]
    fn the_headers_become_call_options() {
        assert!(attempt(&[("authorization", b"Bearer t")]).options().is_ok());
    }

    #[test]
    fn a_header_that_does_not_fit_http_is_an_invalid_argument() {
        for bad in [attempt(&[("bad name", b"x")]), attempt(&[("x", b"a\nb")])] {
            let refusal = bad.options().expect_err("not HTTP");
            assert_eq!(refusal.code, connectrpc::ErrorCode::InvalidArgument);
        }
    }

    #[derive(Clone, Default)]
    struct Recorder {
        seen: Arc<Mutex<Vec<Option<Deadline>>>>,
    }

    impl ClientTransport for Recorder {
        type ResponseBody = Empty<Bytes>;
        type Error = ConnectError;

        fn send(
            &self,
            request: Request<ClientBody>,
        ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
            self.seen
                .lock()
                .expect("recorder")
                .push(request.extensions().get::<Deadline>().copied());
            Box::pin(async { Err(ConnectError::unavailable("recorded")) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_transport_stamps_the_calls_deadline_on_the_request() {
        let sent = attempt(&[]);
        let recorder = Recorder::default();
        let transport = sent.transport(recorder.clone());
        let body: ClientBody = Empty::<Bytes>::new()
            .map_err(|never| match never {})
            .boxed();
        let request = Request::new(body);
        transport.send(request).await.expect_err("recorded failure");
        assert_eq!(
            *recorder.seen.lock().expect("recorder"),
            vec![sent.deadline()]
        );
        assert!(sent.deadline().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn an_attempt_without_a_deadline_stamps_none_on_the_request() {
        let sent = attempt_with(&[], false);
        assert_eq!(sent.deadline(), None);
        let recorder = Recorder::default();
        let transport = sent.transport(recorder.clone());
        let body: ClientBody = Empty::<Bytes>::new()
            .map_err(|never| match never {})
            .boxed();
        transport
            .send(Request::new(body))
            .await
            .expect_err("recorded failure");
        assert_eq!(*recorder.seen.lock().expect("recorder"), vec![None]);
    }
}
