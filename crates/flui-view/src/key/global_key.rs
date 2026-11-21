//! Global key - provides access to the element from anywhere.
//!
//! This module is part of the widgets layer.

use std::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    num::NonZeroU64,
};

use flui_foundation::{AtomicIdMint, ElementId, IdExhausted, ViewKey};

use crate::view::ElementBase;

/// The outcome of a [`GlobalKey::current_element`](GlobalKey::current_element)
/// lookup: a miss says why it missed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyLookup {
    /// An element is mounted for the key.
    Mounted(ElementId),
    /// A registry was consulted and no element is mounted for the key in it.
    NotMounted,
    /// No registry is active: the call ran outside a UI runtime scope
    /// (`UiRuntime::enter`, or a binding's `with_global_key_registry`), so
    /// nothing was consulted and the key may well be mounted.
    NoActiveRuntime,
    /// The registry was consulted but could not be read: a presentation's
    /// frame is running on this thread (the caller is inside it, from `build`,
    /// a lifecycle hook, `dispose` or a layout-builder build), so its tree is
    /// not read re-entrantly. The key may be mounted there; a key held by
    /// another presentation of the runtime resolves normally.
    RegistryBusy,
}

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
/// // The view holds the key in a field marked `#[view(key)]`
/// #[derive(Clone, StatefulView)]
/// struct Form {
///     #[view(key)]
///     key: GlobalKey<FormState>,
/// }
/// let form = Form { key: form_key.clone() };
///
/// // Access from anywhere
/// form_key.with_current_state(|state| state.validate());
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
    /// Panics after every nonzero `u64` identity has been issued; use
    /// [`Self::try_new`] to handle that. Exhaustion is permanent: catching the
    /// panic cannot make an old key available again.
    pub fn new() -> Self {
        Self::try_new().expect("GlobalKey identities exhausted; try_new reports it as IdExhausted")
    }

    /// Create a new global key, or [`IdExhausted`] once every nonzero `u64`
    /// identity has been issued (permanently, from every thread).
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] when the process has issued its last key identity.
    pub fn try_new() -> Result<Self, IdExhausted> {
        static MINT: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();
        Self::minted_from(&MINT)
    }

    /// The local-mint seam keeps terminal tests away from the shared mint.
    fn minted_from(mint: &AtomicIdMint<NonZeroU64>) -> Result<Self, IdExhausted> {
        mint.next().map(|id| Self {
            id: id.get(),
            _marker: PhantomData,
        })
    }

    /// Get the unique ID of this key.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Get the current element ID associated with this key.
    ///
    /// Returns [`KeyLookup::Mounted`] with the element's id, or the reason
    /// the key did not resolve: [`KeyLookup::NotMounted`] when the registry
    /// was consulted and holds no element for this key,
    /// [`KeyLookup::NoActiveRuntime`] when there was no registry to consult,
    /// [`KeyLookup::RegistryBusy`] when the registry could not be read.
    ///
    /// # Registry access
    ///
    /// Reads the registry handle activated by the current owner-thread UI runtime
    /// scope. When no UI runtime is active
    /// the method returns [`KeyLookup::NoActiveRuntime`]: the call ran outside
    /// `UiRuntime::enter`, as in a unit test that bypasses the framework binding,
    /// and says nothing about whether the key is mounted.
    ///
    /// Resolution is by key identity: the registries behind the handle
    /// bucket entries by
    /// [`ViewKey::key_hash`] and then decide membership with
    /// [`ViewKey::key_eq`], because `Box<dyn ViewKey>` has no blanket
    /// `Hash + Eq` to hand a `FxHashMap` directly.
    ///
    /// # During a frame
    ///
    /// Called from inside the frame of the presentation that hosts the key
    /// (from `build`, a lifecycle hook, `dispose`, or a layout-builder build),
    /// this returns [`KeyLookup::RegistryBusy`] (logged at `debug`)
    /// instead of the element: that presentation's tree is locked for the frame
    /// and is not read re-entrantly. Keys held by other presentations of the UI
    /// runtime resolve normally.
    #[must_use]
    pub fn current_element(&self) -> KeyLookup {
        match crate::key::registry::with_registry(|registry| registry.lookup_element(self)) {
            Some(Ok(Some(id))) => KeyLookup::Mounted(id),
            Some(Err(crate::key::registry::RegistryBusy)) => {
                self.report_skipped_busy_presentation();
                KeyLookup::RegistryBusy
            }
            Some(Ok(None)) => KeyLookup::NotMounted,
            None => KeyLookup::NoActiveRuntime,
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
    /// - [`Self::current_element`] does not resolve the key (not mounted, no
    ///   active UI runtime, or the registry busy: this method folds all three
    ///   into `None`; call [`Self::current_element`] to tell them apart), OR
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
        let element_id = match self.current_element() {
            KeyLookup::Mounted(id) => id,
            KeyLookup::NotMounted | KeyLookup::NoActiveRuntime | KeyLookup::RegistryBusy => {
                return None;
            }
        };

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
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        (other as &dyn Any)
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
        let mint = AtomicIdMint::<NonZeroU64>::starting_after(u64::MAX - 2);
        let penultimate = Self::minted_from(&mint).expect("one identity before the last");
        let last = Self::minted_from(&mint).expect("the last identity");
        assert_eq!(penultimate.id(), u64::MAX - 1);
        assert_eq!(last.id(), u64::MAX);
        assert_ne!(penultimate, last);
        for _ in 0..8 {
            assert_eq!(
                Self::minted_from(&mint).err(),
                Some(IdExhausted),
                "an exhausted mint never reissues zero or an earlier identity"
            );
        }
        let healthy = AtomicIdMint::<NonZeroU64>::new();
        let first = Self::minted_from(&healthy).expect("a fresh mint issues");
        let second = Self::minted_from(&healthy).expect("a fresh mint issues");
        assert_eq!(first.id(), 1);
        assert_eq!(second.id(), 2);
        assert_ne!(first, second);
        assert_ne!(first, last);
    }
}
