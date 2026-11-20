//! Owned render subscriptions, including cleanup during exceptional retirement.

use std::{cell::Cell, fmt, rc::Rc};

use flui_foundation::{Listenable, ListenerCallback, panic::FirstPanic};
use flui_rendering::{RenderUpdateImpact, pipeline::LocalRenderInvalidationHandle};

pub(crate) struct RenderSubscription {
    source: Option<Rc<dyn Listenable>>,
    subscription: Option<flui_foundation::Subscription>,
    /// Set false when this subscription revokes; a delivery of a
    /// registration the source could not withdraw stays silent.
    active: Rc<Cell<bool>>,
}

impl fmt::Debug for RenderSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSubscription")
            .field("revoked", &!self.active.get())
            .finish_non_exhaustive()
    }
}

impl RenderSubscription {
    pub(crate) fn subscribe(
        resolve: impl FnOnce() -> Option<Rc<dyn Listenable>>,
        handle: Option<&LocalRenderInvalidationHandle>,
        impact: RenderUpdateImpact,
        failure: &mut FirstPanic,
    ) -> Option<Self> {
        let mark = handle?.clone();
        Self::subscribe_callback(
            resolve,
            Rc::new(move || {
                let _ = mark.request_update(impact);
            }),
            failure,
        )
    }

    pub(crate) fn subscribe_callback(
        resolve: impl FnOnce() -> Option<Rc<dyn Listenable>>,
        callback: ListenerCallback,
        failure: &mut FirstPanic,
    ) -> Option<Self> {
        let source = failure.contain(resolve).flatten()?;
        // The admission gate: once the enclosing owner revokes this
        // subscription, the relay stays silent even if the source's own
        // removal panicked before it could take the entry.
        let active = Rc::new(Cell::new(true));
        let admitted = Rc::clone(&active);
        let listened = failure.contain(|| {
            source.listen({
                let callback = Rc::clone(&callback);
                Rc::new(move || {
                    if admitted.get() {
                        callback();
                    }
                })
            })
        });
        let Some(subscription) = listened else {
            // A source that panics before handing out a subscription has not
            // provided a cancellation capability. Retain its opaque ownership
            // on failure.
            active.set(false);
            failure.retire(source);
            return None;
        };
        Some(Self {
            source: Some(source),
            subscription: Some(subscription),
            active,
        })
    }

    pub(crate) fn close(mut self, failure: &mut FirstPanic) {
        self.close_source(failure);
    }

    fn close_source(&mut self, failure: &mut FirstPanic) {
        self.active.set(false);
        if let Some(subscription) = self.subscription.take() {
            failure.contain(|| subscription.detach());
        }
        if let Some(source) = self.source.take() {
            // The source is held only so its later destruction stays under
            // containment: its captures may run user destructors on the way
            // out. The revoked relay never fires again.
            failure.retire(source);
        }
    }
}

impl Drop for RenderSubscription {
    fn drop(&mut self) {
        let mut failure = FirstPanic::new();
        self.close_source(&mut failure);
        failure.resume();
    }
}
