//! Frame-completion wake ownership, failure priority and contained reporting.
use crate::wake_delivery::FailureSignal;
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::task::Waker;

type Payload = Box<dyn Any + Send>;

fn report(payload: &(dyn Any + Send), context: &'static str) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: reporting runs tracing subscribers, contained"
    )]
    if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| {
        tracing::error!(
            context,
            panic_msg = flui_foundation::panic::payload_text(payload)
                .unwrap_or("(non-string panic payload)"),
            "frame completion delivery failed"
        );
    })) {
        flui_foundation::panic::discard_payload(secondary);
    }
}

pub(crate) fn discard_reported(payload: Payload, context: &'static str) {
    report(payload.as_ref(), context);
    flui_foundation::panic::discard_payload(payload);
}

pub(crate) struct WakeBatch<'a> {
    first: Option<Payload>,
    context: &'static str,
    failure_signal: Option<&'a FailureSignal>,
}

impl<'a> WakeBatch<'a> {
    pub(crate) const fn new(context: &'static str) -> Self {
        Self {
            first: None,
            context,
            failure_signal: None,
        }
    }

    pub(crate) const fn with_failure_signal(
        context: &'static str,
        failure_signal: &'a FailureSignal,
    ) -> Self {
        Self {
            first: None,
            context,
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
            flui_foundation::panic::discard_payload(payload);
        }
    }

    pub(crate) fn into_failure(self) -> Option<Payload> {
        self.first
    }

    /// Wake through the borrowed executor envelope, then destroy it; both run
    /// contained, and a failure of either competes for first (ADR-0281).
    pub(crate) fn wake(&mut self, waker: Waker) {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a waker's wake runs contained; the first failure is kept"
        )]
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| waker.wake_by_ref())) {
            self.failed(payload);
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a waker's drop runs contained; the first failure is kept"
        )]
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(waker))) {
            self.failed(payload);
        }
    }

    pub(crate) fn finish(self, propagate: bool) {
        if let Some(payload) = self.first {
            if propagate && !std::thread::panicking() {
                std::panic::resume_unwind(payload);
            }
            flui_foundation::panic::discard_payload(payload);
        }
    }
}
