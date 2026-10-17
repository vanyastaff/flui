//! Owned render subscriptions, including cleanup during exceptional retirement.

use std::{cell::Cell, fmt, rc::Rc};

use flui_foundation::{Listenable, ListenerCallback, ListenerId, panic::PanicRecovery};
use flui_rendering::{RenderUpdateImpact, pipeline::LocalRenderInvalidationHandle};

pub(crate) struct RenderSubscription {
    source: Option<Rc<dyn Listenable>>,
    id: ListenerId,
    active: Rc<Cell<bool>>,
}

impl fmt::Debug for RenderSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSubscription")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl RenderSubscription {
    pub(crate) fn subscribe(
        resolve: impl FnOnce() -> Option<Rc<dyn Listenable>>,
        handle: Option<&LocalRenderInvalidationHandle>,
        impact: RenderUpdateImpact,
        recovery: &mut PanicRecovery,
    ) -> Option<Self> {
        let mark = handle?.clone();
        Self::subscribe_callback(
            resolve,
            Rc::new(move || {
                let _ = mark.request_update(impact);
            }),
            recovery,
        )
    }

    pub(crate) fn subscribe_callback(
        resolve: impl FnOnce() -> Option<Rc<dyn Listenable>>,
        callback: ListenerCallback,
        recovery: &mut PanicRecovery,
    ) -> Option<Self> {
        let mut source = None;
        recovery.run(|| source = resolve());
        let source = source?;
        let active = Rc::new(Cell::new(true));
        let admission = active.clone();
        let mut id = None;
        recovery.run(|| {
            id = Some(source.add_listener(Rc::new(move || {
                if admission.get() {
                    callback();
                }
            })));
        });
        if let Some(id) = id {
            Some(Self {
                source: Some(source),
                id,
                active,
            })
        } else {
            // A source that panics before returning an ID has not provided a
            // cancellation capability. Retain its opaque ownership on failure.
            active.set(false);
            recovery.retire(source);
            None
        }
    }

    pub(crate) fn close(mut self, recovery: &mut PanicRecovery) {
        self.close_source(recovery);
    }

    fn close_source(&mut self, recovery: &mut PanicRecovery) {
        self.active.set(false);
        if let Some(source) = self.source.take() {
            recovery.run(|| source.remove_listener(self.id));
            // Retain the cancellation capability too if the source's panic
            // leaves completion unproven. The revoked relay is already inert.
            recovery.retire((source, self.id));
        }
    }
}

impl Drop for RenderSubscription {
    fn drop(&mut self) {
        let mut recovery = PanicRecovery::new();
        self.close_source(&mut recovery);
        recovery.finish();
    }
}
