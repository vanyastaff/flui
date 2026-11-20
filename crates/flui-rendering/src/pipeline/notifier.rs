//! The pipeline's visual-update callback and the wake snapshot taken from it.
//!
//! The owner, its scheduler and every invalidation handle share one
//! [`SharedNotifier`]; a wake takes an owned snapshot of the callback and
//! delivers it after every guard is released.

/// A callback the notifier holds. Shared, so a wake takes
/// an owned snapshot and runs it after every guard is released.
type SharedCallback = std::sync::Arc<dyn Fn() + Send + Sync>;

/// An owned snapshot of one notifier callback, taken under the notifier's
/// lock and delivered after it is released.
#[must_use = "deliver the event after releasing every guard"]
pub(crate) struct DeferredCallback(Option<SharedCallback>);

impl DeferredCallback {
    /// Delivers the event and retires the snapshot inside the caller's
    /// first-failure and retirement boundary.
    pub(crate) fn notify_with(self, recovery: &mut flui_foundation::panic::RecoveryScope<'_>) {
        if let Some(callback) = self.0.as_ref() {
            // Every snapshot producer has already admitted its dirty work or
            // capability update. Callback delivery cannot undo that admission;
            // arbitrary capture retirement below has separate Unknown custody.
            recovery.run_classified(flui_foundation::panic::FailureClass::Delivery, |_| {
                callback();
            });
        }
        recovery.retire(self.0);
    }
}

/// An owned wake snapshot delivered after a compound publication releases its guards.
#[must_use = "deliver the visual update after releasing all publication guards"]
pub struct DeferredVisualUpdate(DeferredCallback);

impl std::fmt::Debug for DeferredVisualUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredVisualUpdate")
            .finish_non_exhaustive()
    }
}

impl DeferredVisualUpdate {
    /// Deliver and retire the snapshot without holding the notifier's lock.
    pub fn notify(self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        self.notify_with(&mut recovery.scope());
        recovery.finish();
    }

    /// Join an enclosing publication's first-failure and retirement boundary.
    pub fn notify_with(self, recovery: &mut flui_foundation::panic::RecoveryScope<'_>) {
        self.0.notify_with(recovery);
    }
}

/// The visual-update callback a pipeline's host installs
/// ([`PipelineOwner::set_on_need_visual_update`](super::PipelineOwner::set_on_need_visual_update)).
///
/// The callback is `Send + Sync` so the notifier is `Send + Sync`, matching
/// the pipeline-owner trait bound. The pipeline never calls it through the
/// notifier: it takes an owned snapshot and runs it after the lock is
/// released. A capture is destroyed with the last reference, which may be a
/// snapshot a wake in flight still holds: that wake's thread destroys it then.
#[derive(Default)]
pub(crate) struct VisualUpdateNotifier {
    need_visual_update: Option<SharedCallback>,
}

impl std::fmt::Debug for VisualUpdateNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualUpdateNotifier")
            .field("need_visual_update", &self.need_visual_update.is_some())
            .finish()
    }
}

impl VisualUpdateNotifier {
    /// Commit a replacement and transfer outgoing custody to the caller so a
    /// host can release its infrastructure guard before retiring captures.
    pub(crate) fn replace_need_visual_update<F>(&mut self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.need_visual_update
            .replace(std::sync::Arc::new(callback))
    }
}

/// The pipeline's one notifier, shared by the owner, its scheduler and every
/// invalidation handle, including the `Send + Sync` ones on worker threads.
///
/// The lock is private to this module and every method releases it before it
/// returns: a caller gets an owned snapshot and delivers it with no guard
/// held, so a callback that replaces a callback, re-enters a handle or waits
/// for the owner thread cannot deadlock against a setter (ADR-0287 §1; the
/// notifier borrows end before the wake callback, ADR-0190). A setter commits
/// its replacement for every wake that starts after it returns; a wake already
/// in flight finishes with the callback it took.
#[derive(Clone, Default)]
pub(crate) struct SharedNotifier(std::sync::Arc<parking_lot::RwLock<VisualUpdateNotifier>>);

impl std::fmt::Debug for SharedNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedNotifier").finish_non_exhaustive()
    }
}

impl SharedNotifier {
    /// A snapshot of the visual-update callback.
    pub(crate) fn visual_update(&self) -> DeferredVisualUpdate {
        let callback = self.0.read().need_visual_update.clone();
        DeferredVisualUpdate(DeferredCallback(callback))
    }

    /// Commits a visual-update callback and hands back the outgoing one for
    /// the caller to retire with no guard held.
    pub(crate) fn replace_need_visual_update<F>(&self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.0.write().replace_need_visual_update(callback)
    }
}
