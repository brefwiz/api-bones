// SPDX-License-Identifier: MIT
use super::native_generated::*;
use cucumber::World;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Wake},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct Recording {
    pub(crate) calls: Arc<Mutex<Vec<String>>>,
    pub(crate) denied: bool,
    pub(crate) options: Arc<Mutex<Option<CallOptions>>>,
    pub(crate) publication: Arc<Mutex<Option<Publication>>>,
    pub(crate) handle_bytes: Arc<Mutex<Option<Vec<u8>>>>,
    pub(crate) fail_ack_once: Arc<AtomicBool>,
    pub(crate) interrupt_ack_once: Arc<AtomicBool>,
    pub(crate) fail_nak_once: Arc<AtomicBool>,
}

impl Recording {
    fn record_handle_call(&self, operation: &'static str, handle: AckHandle, options: CallOptions) {
        self.calls.lock().unwrap().push(operation.into());
        *self.handle_bytes.lock().unwrap() = Some(handle.value);
        *self.options.lock().unwrap() = Some(options);
    }

    fn handle_result(fail: bool, code: &'static str) -> BoxNativeFuture<'static, ()> {
        Box::pin(async move {
            if fail {
                Err(NativeError::Transport { code: code.into() })
            } else {
                Ok(())
            }
        })
    }
}

struct PendingAck;
impl Future for PendingAck {
    type Output = Result<(), NativeError>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

pub(crate) struct NoopWake;
impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

struct Once(Option<Delivery>);
impl DeliveryStream for Once {
    fn next(&mut self) -> BoxNativeFuture<'_, Option<Delivery>> {
        let item = self.0.take();
        Box::pin(async move { Ok(item) })
    }
}

impl NativeTransport for Recording {
    fn publish(
        &self,
        operation: &'static str,
        request: Publication,
        options: CallOptions,
    ) -> BoxNativeFuture<'_, PublishResult> {
        self.calls.lock().unwrap().push(operation.into());
        *self.publication.lock().unwrap() = Some(request);
        *self.options.lock().unwrap() = Some(options);
        let denied = self.denied;
        Box::pin(async move {
            if denied {
                Err(NativeError::PermissionDenied {
                    code: "permission_denied".into(),
                })
            } else {
                Ok(PublishResult { duplicate: false })
            }
        })
    }

    fn deliver(
        &self,
        operation: &'static str,
        _: CallOptions,
    ) -> BoxNativeFuture<'_, BoxDeliveryStream> {
        self.calls.lock().unwrap().push(operation.into());
        Box::pin(async {
            Ok(Box::new(Once(Some(Delivery {
                message_id: "m1".into(),
                entity_id: "e1".into(),
                payload: vec![1],
                ack_handle: AckHandle::new(vec![7]),
            }))) as BoxDeliveryStream)
        })
    }

    fn acknowledge(
        &self,
        operation: &'static str,
        handle: AckHandle,
        options: CallOptions,
    ) -> BoxNativeFuture<'_, ()> {
        self.record_handle_call(operation, handle, options);
        let fail = self.fail_ack_once.swap(false, Ordering::Relaxed);
        if self.interrupt_ack_once.swap(false, Ordering::Relaxed) {
            Box::pin(PendingAck)
        } else {
            Self::handle_result(fail, "ack_failed")
        }
    }

    fn negative_acknowledge(
        &self,
        operation: &'static str,
        handle: AckHandle,
        options: CallOptions,
    ) -> BoxNativeFuture<'_, ()> {
        self.record_handle_call(operation, handle, options);
        let fail = self.fail_nak_once.swap(false, Ordering::Relaxed);
        Self::handle_result(fail, "nak_failed")
    }
}

#[derive(Debug, Default, World)]
pub struct NativeWorld {
    pub(crate) transport: Option<Recording>,
    pub(crate) error: Option<NativeError>,
    pub(crate) handle: Option<AckHandle>,
    pub(crate) first_generation: Option<(String, String)>,
    pub(crate) second_generation: Option<(String, String)>,
    pub(crate) generation_failed: bool,
    pub(crate) generation_error: Option<String>,
    pub(crate) stale_errors: Vec<String>,
    pub(crate) identifier_errors: Vec<String>,
    pub(crate) disambiguated: bool,
    pub(crate) publish_result: Option<PublishResult>,
    pub(crate) delivery: Option<Delivery>,
    pub(crate) expected_cancel: Option<Arc<AtomicBool>>,
    pub(crate) expected_deadline: Option<Instant>,
    pub(crate) plugin_empty: bool,
    pub(crate) plugin_multi: bool,
}
