//! Frame-completion wake ownership, failure priority and contained reporting.
use crate::wake_delivery::FailureSignal;
use flui_foundation::panic::FirstPanic;
use std::any::Any;
use std::task::Waker;

type Payload = Box<dyn Any + Send>;

fn report(payload: &(dyn Any + Send), context: &'static str) {
    // A tracing subscriber is user code: its own panic is contained and
    // discarded when `reporting` drops, so it never displaces `payload`.
    let mut reporting = FirstPanic::new();
    reporting.contain(|| {
        tracing::error!(
            context,
            panic_msg = flui_foundation::panic::payload_text(payload)
                .unwrap_or("(non-string panic payload)"),
            "frame completion delivery failed"
        );
    });
}

pub(crate) fn discard_reported(payload: Payload, context: &'static str) {
    report(payload.as_ref(), context);
    flui_foundation::panic::discard_payload(payload);
}

pub(crate) struct WakeBatch<'a> {
    first: FirstPanic,
    context: &'static str,
    failure_signal: Option<&'a FailureSignal>,
}

impl<'a> WakeBatch<'a> {
    pub(crate) const fn new(context: &'static str) -> Self {
        Self {
            first: FirstPanic::new(),
            context,
            failure_signal: None,
        }
    }

    pub(crate) const fn with_failure_signal(
        context: &'static str,
        failure_signal: &'a FailureSignal,
    ) -> Self {
        Self {
            first: FirstPanic::new(),
            context,
            failure_signal: Some(failure_signal),
        }
    }

    /// Run `step` contained: a failure is reported and raises the failure
    /// signal, and the first one is kept (ADR-0281).
    fn contain(&mut self, step: impl FnOnce()) {
        let mut attempt = FirstPanic::new();
        attempt.contain(step);
        if let Some(payload) = attempt.held() {
            if let Some(signal) = self.failure_signal {
                signal.set(true);
            }
            report(payload, self.context);
        }
        self.first.absorb(attempt);
    }

    pub(crate) fn into_failure(self) -> Option<Payload> {
        self.first.into_payload()
    }

    /// Wake through the borrowed executor envelope, then destroy it; both run
    /// contained, and a failure of either competes for first (ADR-0281).
    pub(crate) fn wake(&mut self, waker: Waker) {
        self.contain(|| waker.wake_by_ref());
        self.contain(move || drop(waker));
    }

    /// Raise the first failure when `propagate` is set and the thread is not
    /// already unwinding; otherwise destroy it under containment.
    pub(crate) fn finish(self, propagate: bool) {
        if propagate {
            self.first.resume();
        }
    }
}
