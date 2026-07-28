//! Dependency-inverted tree observation (ADR-0040).
//!
//! `flui-devtools` cannot depend on the tree crates (that would compile the
//! whole framework into any app enabling inspection and invert nothing), so
//! the core publishes observations *downward*: this module declares the
//! narrow [`TreeObserver`] trait and its typed event structs, emitters
//! (`BuildOwner` and the tree primitives it drives, in `flui-view`) call
//! into an installed observer, and devtools implements the trait with only
//! `flui-foundation` on its dependency list.
//!
//! The existing `flui::reconcile` tracing stream is deliberately NOT this
//! seam (typed payloads degrade to strings there, its subscriber lifecycle
//! is process-global, and it cannot be realm-scoped); it remains unchanged
//! for trace tooling.

use core::any::TypeId;

use crate::{ElementId, RebuildReasons};

/// A fresh element was created and mounted into the tree.
///
/// Emitted only when a new element is minted — a `GlobalKey` retake of an
/// existing element emits [`ElementMoved`] instead, never a second mount.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMounted {
    /// The new element. Generational (index + generation), so a devtools
    /// store keyed by the packed value can never confuse a recycled slab
    /// slot — the house ABA defense.
    pub element: ElementId,
    /// `None` for the root element (`mount_root*`); `Some` for every child
    /// minted by `ElementTree::insert`.
    pub parent: Option<ElementId>,
    /// Slot index under `parent` (0 for the root).
    pub slot: usize,
    /// `View::view_type_id()` of the configuring view — typed, not a Debug
    /// string. This is the *logical* view type: `view_type_id` is
    /// overridable and `BoxedView` forwards it to its inner view, so a
    /// boxed child reports the inner type, not the wrapper. Devtools
    /// grouping is therefore by logical type — the same identity the
    /// reconciler matches on.
    pub view_type_id: TypeId,
}

impl ElementMounted {
    /// Constructor for in-workspace emitters.
    ///
    /// The struct is `#[non_exhaustive]`, which protects *matchers*, not
    /// constructors: appending a field changes this signature. That is
    /// acceptable because events are constructed only by in-workspace
    /// emitters; out-of-workspace observers read events, never build them.
    #[must_use]
    pub fn new(
        element: ElementId,
        parent: Option<ElementId>,
        slot: usize,
        view_type_id: TypeId,
    ) -> Self {
        Self {
            element,
            parent,
            slot,
            view_type_id,
        }
    }
}

/// An existing element's position changed: a `GlobalKey` reparent (possibly
/// across parents) or a keyed reorder under the same parent.
///
/// Consumers holding a mirror update the element's parent/slot edge; the
/// element's state and identity survive.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementMoved {
    /// The element that moved.
    pub element: ElementId,
    /// Its parent after the move (may equal the previous parent for a
    /// same-parent reorder).
    pub parent: ElementId,
    /// Its slot under `parent` after the move.
    pub slot: usize,
}

impl ElementMoved {
    /// Constructor for in-workspace emitters (see [`ElementMounted::new`]).
    #[must_use]
    pub fn new(element: ElementId, parent: ElementId, slot: usize) -> Self {
        Self {
            element,
            parent,
            slot,
        }
    }
}

/// An element's build completed, with its accumulated causes.
///
/// Emitted only for builds that ran to completion (including builds
/// recovered into an error view — those completed with substitute output).
/// A build pass that unwinds emits nothing for that element. First builds
/// are included: every mount is followed by a rebuilt event whose reasons
/// contain [`crate::RebuildReason::InitialMount`], so `mounts + rebuilds`
/// intentionally counts first builds in both — consumers wanting
/// "re-builds only" filter on the reason set.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementRebuilt {
    /// The rebuilt element.
    pub element: ElementId,
    /// Logical view type (same semantics as [`ElementMounted::view_type_id`]).
    pub view_type_id: TypeId,
    /// Every distinct cause accumulated since the last build — the same set
    /// the build drain consumes.
    pub reasons: RebuildReasons,
}

impl ElementRebuilt {
    /// Constructor for in-workspace emitters (see [`ElementMounted::new`]).
    #[must_use]
    pub fn new(element: ElementId, view_type_id: TypeId, reasons: RebuildReasons) -> Self {
        Self {
            element,
            view_type_id,
            reasons,
        }
    }
}

/// An element was unmounted — its `unmount` ran and its slab slot was freed.
///
/// This is a fact, not a scheduling artifact: it fires for eager inline
/// removals during reconcile, deferred keyed removals drained by
/// `finalize_tree`, lazy-sliver evictions, and root detachment alike. A
/// soft-removed keyed element parked in the inactive queue has NOT been
/// unmounted and produces no event until it is either retaken
/// ([`ElementMoved`]) or finalized (this event).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementUnmounted {
    /// The unmounted element.
    pub element: ElementId,
}

impl ElementUnmounted {
    /// Constructor for in-workspace emitters (see [`ElementMounted::new`]).
    #[must_use]
    pub fn new(element: ElementId) -> Self {
        Self { element }
    }
}

/// Dependency-inverted tree observation (ADR-0040).
///
/// Implemented by consumers (devtools); emitted into by owners
/// (`BuildOwner` and the tree primitives it drives). All methods default to
/// no-ops so adding an observation kind is never a breaking change for
/// implementors.
///
/// # Ordering contract
///
/// The stream is a **totally-ordered tree-mutation log**, emitted
/// synchronously on the realm's owner thread in the exact order the
/// mutations happen. Per element it is causal: `element_mounted` first,
/// then any interleaving of `element_rebuilt` / `element_moved`, then
/// `element_unmounted` last. **No phase bucketing is promised**: builds run
/// mid-layout-settling and post-layout, eager unmounts happen inline during
/// any reconcile, and finalization runs at several points per frame — so
/// "mounts during build, unmounts at end of frame" would be false and is
/// not the contract.
///
/// # Threading, re-entrancy, and deadlock
///
/// Callbacks run on the realm's owner thread, inside frame phases, while
/// the realm's locks are held. The contract is absolute: an observer
/// callback must not call **any** flui-view binding, realm, or owner API —
/// the natural accessors take a non-reentrant read lock while the frame
/// drive holds the write lock, which deadlocks the realm thread in release
/// builds. A callback may touch only its own state (atomics, lock-free
/// structures, `try_send` into a channel it owns) and must return promptly.
/// Inspection-driven mutation must go through out-of-frame channels (a
/// `RebuildHandle` used from *outside* the callback), never synchronously
/// from an observer method.
///
/// # Panic policy
///
/// A panicking observer is detached: the emission wraps every call in
/// `catch_unwind`; on unwind the observer is removed from its slot,
/// [`detached`](Self::detached) is NOT called (the observer just proved it
/// cannot be trusted to run), and the detachment is logged at error level.
/// The frame survives.
///
/// # Teardown
///
/// The seam does not synthesize `element_unmounted` for elements alive at
/// teardown. [`detached`](Self::detached) is the end-of-life signal — and
/// it only fires if the embedder clears the observer (install/clear
/// symmetry); an owner dropped without clearing emits nothing.
pub trait TreeObserver: Send + Sync {
    /// A fresh element was minted and mounted.
    fn element_mounted(&self, event: &ElementMounted) {
        let _ = event;
    }
    /// An existing element moved (`GlobalKey` reparent or keyed reorder).
    fn element_moved(&self, event: &ElementMoved) {
        let _ = event;
    }
    /// An element's build completed.
    fn element_rebuilt(&self, event: &ElementRebuilt) {
        let _ = event;
    }
    /// An element was unmounted and its slot freed.
    fn element_unmounted(&self, event: &ElementUnmounted) {
        let _ = event;
    }
    /// End of stream: the observer was replaced or explicitly cleared. No
    /// further events will arrive from this owner. Consumers drop or
    /// archive their mirror here — this, not mount/unmount balance, is the
    /// end-of-life signal. Not called when the observer is detached after
    /// a panic (see the trait docs).
    fn detached(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorder(std::sync::Mutex<Vec<&'static str>>);

    impl TreeObserver for Recorder {
        fn element_mounted(&self, _event: &ElementMounted) {
            self.0.lock().unwrap().push("mounted");
        }
        fn detached(&self) {
            self.0.lock().unwrap().push("detached");
        }
    }

    #[test]
    fn defaulted_methods_are_no_ops_and_overrides_receive_events() {
        let recorder = Recorder(std::sync::Mutex::new(Vec::new()));
        let observer: &dyn TreeObserver = &recorder;
        let id = ElementId::new(1);
        observer.element_mounted(&ElementMounted::new(id, None, 0, TypeId::of::<()>()));
        // Defaulted methods must be callable and silent.
        observer.element_moved(&ElementMoved::new(id, id, 3));
        observer.element_unmounted(&ElementUnmounted::new(id));
        observer.detached();
        assert_eq!(*recorder.0.lock().unwrap(), ["mounted", "detached"]);
    }
}
