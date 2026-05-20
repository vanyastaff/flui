//! `VisualUpdateNotifier` -- consolidated callbacks for pipeline events.
//!
//! Per docs/designs/2026-05-20-mythos-flui-rendering-redesign.md Section 6,
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

/// Holds the three pipeline-event callbacks that `PipelineOwner` exposes
/// to its embedding application.
///
/// Each event is `Option<Callback>`; when unset, `fire_*` is a no-op.
/// When set, `fire_*` calls the closure synchronously. Callbacks are
/// `Send + Sync` so the notifier itself is `Send + Sync`, matching the
/// pipeline-owner trait bound.
#[derive(Default)]
pub struct VisualUpdateNotifier {
    on_need_visual_update: Option<Callback>,
    on_semantics_owner_created: Option<Callback>,
    on_semantics_owner_disposed: Option<Callback>,
}

impl std::fmt::Debug for VisualUpdateNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualUpdateNotifier")
            .field(
                "on_need_visual_update",
                &self.on_need_visual_update.is_some(),
            )
            .field(
                "on_semantics_owner_created",
                &self.on_semantics_owner_created.is_some(),
            )
            .field(
                "on_semantics_owner_disposed",
                &self.on_semantics_owner_disposed.is_some(),
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
        self.on_need_visual_update = Some(Box::new(callback));
    }

    /// Fires the visual-update callback if one is set; otherwise no-op.
    #[inline]
    pub fn fire_need_visual_update(&self) {
        if let Some(callback) = &self.on_need_visual_update {
            callback();
        }
    }

    // -- semantics owner created --

    /// Sets (or replaces) the semantics-owner-created callback.
    pub fn set_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_created = Some(Box::new(callback));
    }

    /// Fires the semantics-owner-created callback if one is set.
    #[inline]
    pub fn fire_semantics_owner_created(&self) {
        if let Some(callback) = &self.on_semantics_owner_created {
            callback();
        }
    }

    // -- semantics owner disposed --

    /// Sets (or replaces) the semantics-owner-disposed callback.
    pub fn set_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_disposed = Some(Box::new(callback));
    }

    /// Fires the semantics-owner-disposed callback if one is set.
    #[inline]
    pub fn fire_semantics_owner_disposed(&self) {
        if let Some(callback) = &self.on_semantics_owner_disposed {
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[test]
    fn default_notifier_is_silent() {
        let n = VisualUpdateNotifier::new();
        n.fire_need_visual_update();
        n.fire_semantics_owner_created();
        n.fire_semantics_owner_disposed();
    }

    #[test]
    fn each_callback_fires_independently() {
        let visual = Arc::new(AtomicUsize::new(0));
        let created = Arc::new(AtomicUsize::new(0));
        let disposed = Arc::new(AtomicUsize::new(0));

        let mut n = VisualUpdateNotifier::new();
        {
            let visual = Arc::clone(&visual);
            n.set_need_visual_update(move || {
                visual.fetch_add(1, Ordering::Relaxed);
            });
        }
        {
            let created = Arc::clone(&created);
            n.set_semantics_owner_created(move || {
                created.fetch_add(1, Ordering::Relaxed);
            });
        }
        {
            let disposed = Arc::clone(&disposed);
            n.set_semantics_owner_disposed(move || {
                disposed.fetch_add(1, Ordering::Relaxed);
            });
        }

        n.fire_need_visual_update();
        n.fire_need_visual_update();
        n.fire_semantics_owner_created();
        n.fire_semantics_owner_disposed();

        assert_eq!(visual.load(Ordering::Relaxed), 2);
        assert_eq!(created.load(Ordering::Relaxed), 1);
        assert_eq!(disposed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn replacing_a_callback_overwrites_the_prior_one() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut n = VisualUpdateNotifier::new();

        {
            let counter = Arc::clone(&counter);
            n.set_need_visual_update(move || {
                counter.fetch_add(1, Ordering::Relaxed);
            });
        }
        {
            let counter = Arc::clone(&counter);
            n.set_need_visual_update(move || {
                counter.fetch_add(10, Ordering::Relaxed);
            });
        }

        n.fire_need_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 10);
    }
}
