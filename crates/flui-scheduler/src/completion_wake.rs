//! Frame-completion wake ownership, failure priority and contained reporting.
use std::any::Any;
use crate::wake_delivery::FailureSignal;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::task::Waker;

type Payload = Box<dyn Any + Send>;

fn report(payload: &(dyn Any + Send), context: &'static str) {
    if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| {
        tracing::error!(
            context,
            panic_msg = flui_foundation::panic::payload_text(payload)
                .unwrap_or("(non-string panic payload)"),
            "frame completion delivery failed"
        );
    })) {
        flui_foundation::panic::retain_opaque_payload(secondary);
    }
}

pub(crate) fn retain_reported(payload: Payload, context: &'static str) {
    report(payload.as_ref(), context);
    flui_foundation::panic::retain_opaque_payload(payload);
}

pub(crate) struct WakeBatch<'a> {
    first: Option<Payload>,
    context: &'static str,
    preserve_failure: bool,
    failure_signal: Option<&'a FailureSignal>,
}

impl<'a> WakeBatch<'a> {
    pub(crate) const fn new(context: &'static str, preserve_failure: bool) -> Self {
        Self {
            first: None,
            context,
            preserve_failure,
            failure_signal: None,
        }
    }

    pub(crate) const fn with_failure_signal(
        context: &'static str,
        preserve_failure: bool,
        failure_signal: &'a FailureSignal,
    ) -> Self {
        Self {
            first: None,
            context,
            preserve_failure,
            failure_signal: Some(failure_signal),
        }
    }

    fn failed(&mut self, payload: Payload) {
        if let Some(signal) = self.failure_signal {
            signal.set(true);
        }
        report(payload.as_ref(), self.context);
        if self.first.is_none() {
            self.first = Some(payload);
        } else {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    pub(crate) fn into_failure(self) -> Option<Payload> {
        self.first
    }

    pub(crate) fn wake(&mut self, waker: Waker) {
        let called = catch_unwind(AssertUnwindSafe(|| waker.wake_by_ref()));
        if let Err(payload) = called {
            // Keep the owning opaque executor envelope outside the invocation.
            std::mem::forget(waker);
            self.failed(payload);
        } else if self.preserve_failure
            || self.first.is_some()
            || self.failure_signal.is_some_and(FailureSignal::get)
            || std::thread::panicking()
        {
            std::mem::forget(waker);
        } else if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(waker))) {
            self.failed(payload);
        }
    }

    pub(crate) fn finish(self, propagate: bool) {
        if let Some(payload) = self.first {
            if propagate && !std::thread::panicking() {
                std::panic::resume_unwind(payload);
            }
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}
