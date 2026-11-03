//! `VisualUpdateNotifier` -- consolidated callbacks for pipeline events.
//!
//! `PipelineOwner` used to carry three separate `Box<dyn Fn() + Send + Sync>`
//! callback fields (visual-update, semantics-owner-created,
//! semantics-owner-disposed) directly on the struct. That shape paid the
//! "three pointers + the if-let dance" cost in every constructor + Debug
//! impl + setter group, with zero structural benefit.
//!
//! `VisualUpdateNotifier` packages them into one struct that owns its own
//! invariants ("at most one callback per event kind, fired-when-set,
//! silently ignored when not"). The struct lives as a single field on the
//! pipeline owner.
//!
//! The shape stays callback-per-event rather than `Vec<listener>` because
//! every production caller registers exactly one listener -- multi-listener
//! observability would force the pipeline to choose between unbounded
//! listener vectors or capped registration, neither of which earns its
//! complexity at this scale. If the need arises later, the notifier grows
//! into a real observer pattern without rippling through `PipelineOwner`.

/// A callback the notifier holds. Shared, so a wake or a lifecycle event takes
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

/// Holds the three pipeline-event callbacks that `PipelineOwner` exposes
/// to its embedding application.
///
/// Each event holds at most one callback; when unset, `fire_*` is a no-op.
/// When set, `fire_*` calls the closure synchronously. Callbacks are
/// `Send + Sync` so the notifier itself is `Send + Sync`, matching the
/// pipeline-owner trait bound.
///
/// The pipeline shares its notifier through [`SharedNotifier`] and never
/// fires through it: it takes an owned snapshot of a callback and runs it
/// after the lock is released. `fire_*` is for a notifier the caller owns
/// outright.
///
/// Dropping the notifier drops its references to the visual-update, created,
/// then disposed captures in that order, as std drops fields. A capture is
/// destroyed with the last reference, which may be a snapshot a wake in
/// flight still holds: that wake's thread destroys it then. If one capture
/// panics as it is destroyed the rest are still dropped, and one that panics
/// during that unwind aborts (ADR-0281).
#[derive(Default)]
pub struct VisualUpdateNotifier {
    need_visual_update: Option<SharedCallback>,
    semantics_owner_created: Option<SharedCallback>,
    semantics_owner_disposed: Option<SharedCallback>,
}

impl std::fmt::Debug for VisualUpdateNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualUpdateNotifier")
            .field("need_visual_update", &self.need_visual_update.is_some())
            .field(
                "semantics_owner_created",
                &self.semantics_owner_created.is_some(),
            )
            .field(
                "semantics_owner_disposed",
                &self.semantics_owner_disposed.is_some(),
            )
            .finish()
    }
}

impl VisualUpdateNotifier {
    /// Creates a notifier with no callbacks set.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    // -- visual update --

    /// Sets (or replaces) the visual-update callback.
    pub fn set_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        drop(self.replace_need_visual_update(callback));
    }

    /// Commit a replacement and transfer outgoing custody to the caller so a
    /// host can release its infrastructure guard before retiring captures.
    pub(crate) fn replace_need_visual_update<F>(&mut self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.need_visual_update
            .replace(std::sync::Arc::new(callback))
    }

    /// Fires the visual-update callback if one is set; otherwise no-op.
    #[inline]
    pub fn fire_need_visual_update(&self) {
        if let Some(callback) = &self.need_visual_update {
            callback();
        }
    }

    // -- semantics owner created --

    /// Sets (or replaces) the semantics-owner-created callback.
    pub fn set_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        drop(self.replace_semantics_owner_created(callback));
    }

    pub(crate) fn replace_semantics_owner_created<F>(
        &mut self,
        callback: F,
    ) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.semantics_owner_created
            .replace(std::sync::Arc::new(callback))
    }

    /// Fires the semantics-owner-created callback if one is set.
    #[inline]
    pub fn fire_semantics_owner_created(&self) {
        if let Some(callback) = &self.semantics_owner_created {
            callback();
        }
    }

    // -- semantics owner disposed --

    /// Sets (or replaces) the semantics-owner-disposed callback.
    pub fn set_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        drop(self.replace_semantics_owner_disposed(callback));
    }

    pub(crate) fn replace_semantics_owner_disposed<F>(
        &mut self,
        callback: F,
    ) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.semantics_owner_disposed
            .replace(std::sync::Arc::new(callback))
    }

    /// Fires the semantics-owner-disposed callback if one is set.
    #[inline]
    pub fn fire_semantics_owner_disposed(&self) {
        if let Some(callback) = &self.semantics_owner_disposed {
            callback();
        }
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

    /// A snapshot of the semantics-owner-created callback.
    pub(crate) fn semantics_owner_created(&self) -> DeferredCallback {
        let callback = self.0.read().semantics_owner_created.clone();
        DeferredCallback(callback)
    }

    /// A snapshot of the semantics-owner-disposed callback.
    pub(crate) fn semantics_owner_disposed(&self) -> DeferredCallback {
        let callback = self.0.read().semantics_owner_disposed.clone();
        DeferredCallback(callback)
    }

    /// Commits a visual-update callback and hands back the outgoing one for
    /// the caller to retire with no guard held.
    pub(crate) fn replace_need_visual_update<F>(&self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.0.write().replace_need_visual_update(callback)
    }

    /// Commits a semantics-owner-created callback; see
    /// [`Self::replace_need_visual_update`].
    pub(crate) fn replace_semantics_owner_created<F>(&self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.0.write().replace_semantics_owner_created(callback)
    }

    /// Commits a semantics-owner-disposed callback; see
    /// [`Self::replace_need_visual_update`].
    pub(crate) fn replace_semantics_owner_disposed<F>(&self, callback: F) -> Option<SharedCallback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.0.write().replace_semantics_owner_disposed(callback)
    }
}
