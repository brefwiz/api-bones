// SPDX-License-Identifier: MIT
//! The two directions of a request's body on an h2 stream: the pump that
//! writes the request body into the stream under its flow control, and the
//! response body that reads the stream and returns its capacity.

use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use connectrpc::client::ClientBody;
use h2::{Reason, RecvStream, SendStream};
use http_body::{Body, Frame};
use http_body_util::BodyExt as _;

use crate::conn::InFlight;

type BoxError = Box<dyn Error + Send + Sync>;

/// Writes `body` into `stream`: data frames within the stream's flow-control
/// capacity, then trailers or the end of the stream. A failure resets the
/// stream; the request's reply carries the verdict, so it is only logged here.
pub(crate) async fn pump(mut body: ClientBody, mut stream: SendStream<Bytes>) {
    if let Err(failure) = forward(&mut body, &mut stream).await {
        tracing::debug!(
            target: "api_bones_transport",
            event = "request_body_failed",
            error = %failure,
        );
        stream.send_reset(Reason::CANCEL);
    }
}

async fn forward(body: &mut ClientBody, stream: &mut SendStream<Bytes>) -> Result<(), BoxError> {
    while let Some(frame) = body.frame().await {
        match frame?.into_data() {
            Ok(data) => {
                let last = body.is_end_stream();
                send_data(stream, data, last).await?;
                if last {
                    return Ok(());
                }
            }
            Err(frame) => {
                if let Ok(trailers) = frame.into_trailers() {
                    stream.send_trailers(trailers)?;
                    return Ok(());
                }
            }
        }
    }
    stream.send_data(Bytes::new(), true)?;
    Ok(())
}

/// Sends `data` in the pieces the stream's capacity allows, ending the stream
/// with the last piece when `last`.
async fn send_data(
    stream: &mut SendStream<Bytes>,
    mut data: Bytes,
    last: bool,
) -> Result<(), BoxError> {
    if data.is_empty() {
        if last {
            stream.send_data(data, true)?;
        }
        return Ok(());
    }
    while !data.is_empty() {
        stream.reserve_capacity(data.len());
        let mut available = stream.capacity();
        if available == 0 {
            available = std::future::poll_fn(|cx| stream.poll_capacity(cx))
                .await
                .ok_or("the stream closed before the request body was sent")??;
        }
        if available == 0 {
            continue;
        }
        let piece = data.split_to(available.min(data.len()));
        stream.send_data(piece, last && data.is_empty())?;
    }
    Ok(())
}

/// The body of a response: the stream's data frames, then its trailers. Each
/// data frame returns its flow-control capacity as it is handed out, so a
/// consumer that reads slowly slows the peer down.
///
/// Holds the connection's in-flight count up until it is dropped.
pub struct ResponseBody {
    received: RecvStream,
    data_done: bool,
    finished: bool,
    _flight: InFlight,
}

impl ResponseBody {
    pub(crate) fn new(received: RecvStream, flight: InFlight) -> Self {
        Self {
            received,
            data_done: false,
            finished: false,
            _flight: flight,
        }
    }
}

// Nothing in the body is pinned: h2 polls its stream through `&mut`.
impl Unpin for ResponseBody {}

impl Body for ResponseBody {
    type Data = Bytes;
    type Error = h2::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, h2::Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }
        if !self.data_done {
            match ready!(self.received.poll_data(cx)) {
                Some(Ok(data)) => {
                    return Poll::Ready(Some(
                        self.received
                            .flow_control()
                            .release_capacity(data.len())
                            .map(|()| Frame::data(data)),
                    ));
                }
                Some(Err(error)) => return Poll::Ready(Some(Err(error))),
                None => self.data_done = true,
            }
        }
        let trailers = ready!(self.received.poll_trailers(cx));
        self.finished = true;
        Poll::Ready(trailers.transpose().map(|t| t.map(Frame::trailers)))
    }
}
