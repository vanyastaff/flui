//! Global key - provides access to the element from anywhere.
//!
//! This module is part of the widgets layer.

use std::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

use flui_foundation::{ElementId, ViewKey};

use crate::view::ElementBase;

/// A key that provides access to the element from anywhere.
///
/// Unlike regular keys which only affect reconciliation, `GlobalKey`
/// also allows you to access the element's state from outside the tree.
///
/// # Use Cases
///
/// - Access state of a widget from a parent or sibling
/// - Trigger methods on a widget programmatically
/// - Get the render object for measurements/positioning
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::GlobalKey;
///
/// // Create a global key
/// let form_key = GlobalKey::<FormState>::new();
///
/// // Use in widget tree
/// Form::new().with_view_key(form_key.clone())
///
/// // Access from anywhere
/// if let Some(state) = form_key.current_state() {
///     state.validate();
/// }
/// ```
///
/// # Performance Note
///
/// Global keys have overhead compared to local keys because they
/// maintain a registry. Use sparingly.
pub struct GlobalKey<T: 'static> {
    id: u64,
    /// `fn() -> T` keeps `GlobalKey<T>` covariant in `T` without
    /// requiring `T: Send + Sync`. The marker is also why the manual
    /// `Clone` impl below sidesteps the `T: Clone` bound a derive
    /// would impose — `PhantomData<fn() -> T>` is always `Clone +
    /// Copy` regardless of `T`.
    _marker: PhantomData<fn() -> T>,
}

// Manual `Clone` impl: do NOT require `T: Clone`. A `GlobalKey<T>` is
// just an `id` + a phantom marker, so cloning is trivial.
impl<T: 'static> Clone for GlobalKey<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: PhantomData,
        }
    }
}

impl<T: 'static> GlobalKey<T> {
    /// Create a new global key.
    ///
    /// # Panics
    ///
    /// Panics after every nonzero `u64` identity has been issued. Exhaustion
    /// is permanent; catching this panic cannot make an old key available again.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self::new_with_counter(&COUNTER)
    }

    // Zero is the permanent exhaustion sentinel; MAX itself remains assignable.
    // The local-counter seam keeps terminal tests away from the shared allocator.
    fn new_with_counter(counter: &AtomicU64) -> Self {
        let id = counter
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current == 0 {
                    None
                } else {
                    Some(current.wrapping_add(1))
                }
            })
            .expect("GlobalKey counter exhausted: all nonzero u64 identities issued");
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Get the unique ID of this key.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Get the current element ID associated with this key.
    ///
    /// Returns `None` if no element registered under this `GlobalKey`'s
    /// hash is currently mounted in the registered build owner.
    ///
    /// # Registry access
    ///
    /// Reads the registry handle activated by the current owner-thread UI runtime
    /// scope (or by the legacy test harness adapter). When no UI runtime is active
    /// the method returns `None` — this is the
    /// quiescent state expected in pure-unit tests that bypass the
    /// framework binding.
    ///
    /// Resolution is by key identity: the registries behind the handle
    /// bucket entries by
    /// [`ViewKey::key_hash`] and then decide membership with
    /// [`ViewKey::key_eq`], because `Box<dyn ViewKey>` has no blanket
    /// `Hash + Eq` to hand a `HashMap` directly.
    ///
    /// # During a frame
    ///
    /// Called from inside the frame of the presentation that hosts the key
    /// (from `build`, a lifecycle hook, `dispose`, or a layout-builder build),
    /// this returns `None` (logged at `debug`) instead of the element:
    /// that presentation's tree is locked for the frame and is
    /// not read re-entrantly. Keys held by other presentations of the UI runtime
    /// resolve normally.
    #[must_use]
    pub fn current_element(&self) -> Option<ElementId> {
        match crate::key::registry::with_registry(|registry| registry.lookup_element(self)) {
            None => None,
            Some(Ok(id)) => id,
            Some(Err(crate::key::registry::RegistryBusy)) => {
                self.report_skipped_busy_presentation();
                None
            }
        }
    }

    /// Report a read that found no element but skipped a presentation whose
    /// frame is running.
    ///
    /// `debug`, not `warn`: during a frame the running presentation is always
    /// busy, so this also fires for a key that is simply not mounted anywhere,
    /// which is an ordinary miss.
    fn report_skipped_busy_presentation(&self) {
        tracing::debug!(
            key = ?self,
            "GlobalKey read skipped the presentation whose frame is running; \
             the key may be mounted there, and resolves to None"
        );
    }

    /// Run `f` against the current state of the element registered under
    /// this key, downcasting to `R` first.
    ///
    /// Returns `None` if:
    /// - No element is currently registered for this key, OR
    /// - The matched element has no state (e.g. it's a `StatelessElement`), OR
    /// - The state's runtime type doesn't match `T`.
    ///
    /// `T` is the type the `GlobalKey<T>` was instantiated with — by
    /// convention this is the `ViewState` impl tied to the keyed
    /// `StatefulView`. The match is enforced via `Any::downcast_ref::<T>`
    /// at the dispatch boundary so non-`StatefulView` keys (e.g.
    /// `GlobalKey<i32>`) simply never resolve, no compile error.
    ///
    /// The callback shape (`R` returned, state borrowed for the duration
    /// of the call) lets callers extract a snapshot without leaking the
    /// borrow into the rest of `build()`. Same pattern as
    /// [`BuildContextExt::find_state`](crate::BuildContextExt::find_state).
    ///
    /// The state is reached after a runtime-type check. The closure-callback
    /// shape lets the read-lock on the element tree drop before the caller
    /// does anything substantial with the value.
    ///
    /// # During a frame
    ///
    /// Like [`Self::current_element`], this returns `None` without calling
    /// `f` when called from inside the frame of the presentation that hosts
    /// the key; keys held by other presentations resolve normally.
    #[must_use]
    pub fn with_current_state<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R>
    where
        T: 'static,
    {
        let element_id = self.current_element()?;

        // `with_registry` yields `None` when no ui_runtime/fixture handle is
        // active; `with_element` yields `Err` when the owning binding is busy
        // and `Ok(None)` when the id is no longer in the tree; the inner
        // closure yields `None` when the state downcast fails.
        let visited = crate::key::registry::with_registry(|registry| {
            registry.with_element(element_id, |element: &dyn ElementBase| {
                let state_any = element.state_as_any()?;
                let typed = state_any.downcast_ref::<T>()?;
                Some(f(typed))
            })
        })?;
        if let Ok(result) = visited {
            result.flatten()
        } else {
            self.report_skipped_busy_presentation();
            None
        }
    }
}

impl<T: 'static> Default for GlobalKey<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> fmt::Debug for GlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalKey")
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: 'static> PartialEq for GlobalKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: 'static> Eq for GlobalKey<T> {}

impl<T: 'static> Hash for GlobalKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T: 'static> ViewKey for GlobalKey<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }

    fn key_hash(&self) -> u64 {
        self.id
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(Self {
            id: self.id,
            _marker: PhantomData,
        })
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlobalKey<{}>({})", std::any::type_name::<T>(), self.id)
    }

    fn is_global_key(&self) -> bool {
        true
    }
}

#[cfg(test)]
impl GlobalKey<()> {
    pub(crate) fn exhausted_global_key_counter_never_reissues_an_identity() {
        let counter = AtomicU64::new(u64::MAX - 1);
        let penultimate = Self::new_with_counter(&counter);
        let last = Self::new_with_counter(&counter);
        assert_eq!(penultimate.id(), u64::MAX - 1);
        assert_eq!(last.id(), u64::MAX);
        assert_ne!(penultimate, last);
        for _ in 0..8 {
            assert!(
                std::panic::catch_unwind(|| Self::new_with_counter(&counter)).is_err(),
                "catching exhaustion must never reissue zero or an earlier identity"
            );
        }
        let healthy = AtomicU64::new(1);
        let first = Self::new_with_counter(&healthy);
        let second = Self::new_with_counter(&healthy);
        assert_eq!(first.id(), 1);
        assert_eq!(second.id(), 2);
        assert_ne!(first, second);
        assert_ne!(first, last);
    }
}
