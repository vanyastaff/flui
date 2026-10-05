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

/// Type alias for the boxed-closure callbacks the notifier holds.
type Callback = Box<dyn Fn() + Send + Sync>;

/// Independently retired event envelopes, after their physical owner is empty.
struct RetiringCallbacks([Option<Callback>; 3]);

impl Drop for RetiringCallbacks {
    fn drop(&mut self) {
        if std::thread::panicking() {
            for callback in &mut self.0 {
                std::mem::forget(callback.take());
            }
        }
    }
}

/// Holds the three pipeline-event callbacks that `PipelineOwner` exposes
/// to its embedding application.
///
/// Each event is `Option<Callback>`; when unset, `fire_*` is a no-op.
/// When set, `fire_*` calls the closure synchronously. Callbacks are
/// `Send + Sync` so the notifier itself is `Send + Sync`, matching the
/// pipeline-owner trait bound.
///
/// Dropping the notifier drops the visual-update, created, then disposed
/// captures; once one of them panics, or if the thread is already panicking,
/// the rest are retained (ADR-0127).
#[derive(Default)]
pub struct VisualUpdateNotifier {
    need_visual_update: Option<Callback>,
    semantics_owner_created: Option<Callback>,
    semantics_owner_disposed: Option<Callback>,
}

impl Drop for VisualUpdateNotifier {
    fn drop(&mut self) {
        let mut retiring = RetiringCallbacks([
            self.need_visual_update.take(),
            self.semantics_owner_created.take(),
            self.semantics_owner_disposed.take(),
        ]);
        if !std::thread::panicking() {
            for callback in &mut retiring.0 {
                drop(callback.take());
            }
        }
    }
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
    pub(crate) fn replace_need_visual_update<F>(&mut self, callback: F) -> Option<Callback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.need_visual_update.replace(Box::new(callback))
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

    pub(crate) fn replace_semantics_owner_created<F>(&mut self, callback: F) -> Option<Callback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.semantics_owner_created.replace(Box::new(callback))
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

    pub(crate) fn replace_semantics_owner_disposed<F>(&mut self, callback: F) -> Option<Callback>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.semantics_owner_disposed.replace(Box::new(callback))
    }

    /// Fires the semantics-owner-disposed callback if one is set.
    #[inline]
    pub fn fire_semantics_owner_disposed(&self) {
        if let Some(callback) = &self.semantics_owner_disposed {
            callback();
        }
    }
}
