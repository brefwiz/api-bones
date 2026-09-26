// SPDX-License-Identifier: MIT
//! The Cucumber World for the shared contract lane.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use api_bones_connect::{Idempotency, PreconditionedTransport};
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport};
use cucumber::World;
use http::{Request, Response};
use http_body_util::Empty;

#[derive(Debug, Default, World)]
pub struct RetryWorld {
    pub failure: Option<ConnectError>,
}

impl RetryWorld {
    /// The failure under test, or a panic naming the missing Given.
    pub fn failure(&self) -> &ConnectError {
        self.failure.as_ref().expect("no failure given")
    }
}

const PRECONDITION_METHOD: &str = "/pkg.v1.Svc/Method";

#[derive(Clone)]
struct RecordingTransport {
    seen: Arc<Mutex<Vec<http::HeaderMap>>>,
}

impl ClientTransport for RecordingTransport {
    type ResponseBody = Empty<bytes::Bytes>;
    type Error = ConnectError;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        self.seen.lock().unwrap().push(request.headers().clone());
        Box::pin(async { Ok(Response::new(Empty::new())) })
    }
}

#[derive(Debug, Default, World)]
pub struct PreconditionWorld {
    policy: Option<HashMap<String, Idempotency>>,
    caller_header: Option<String>,
    seen: Option<Arc<Mutex<Vec<http::HeaderMap>>>>,
}

impl PreconditionWorld {
    pub fn declare_method(&mut self, idempotency: Idempotency) {
        let mut policy = HashMap::new();
        policy.insert(PRECONDITION_METHOD.to_owned(), idempotency);
        self.policy = Some(policy);
    }

    pub fn declare_no_policy(&mut self) {
        self.policy = Some(HashMap::new());
    }

    pub fn set_caller_header(&mut self, value: &str) {
        self.caller_header = if value == "none" {
            None
        } else {
            Some(value.to_owned())
        };
    }

    pub async fn send(&mut self) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        self.seen = Some(seen.clone());
        let transport = PreconditionedTransport::new(
            RecordingTransport { seen },
            self.policy.take().expect("no method declared"),
        );
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("https://svc{PRECONDITION_METHOD}"));
        if let Some(value) = &self.caller_header {
            builder = builder.header("if-match", value.as_str());
        }
        let request = builder
            .body(connectrpc::client::full_body(bytes::Bytes::new()))
            .expect("build request");
        transport.send(request).await.expect("transport send");
    }

    pub fn sent_if_match(&self) -> Option<String> {
        let seen = self.seen.as_ref().expect("no call sent yet");
        let headers = seen.lock().unwrap();
        headers[0]
            .get("if-match")
            .map(|v| v.to_str().unwrap().to_owned())
    }
}
