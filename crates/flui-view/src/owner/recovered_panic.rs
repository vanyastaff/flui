//! Panics contained by a per-child lifecycle seam, and the drain that
//! collects them.
//!
//! Every per-child containment seam — `build_or_recover` (a `build()`
//! panic); a fresh mount's `create_element`/`mount` window
//! (`ElementTree::mount_or_substitute`); a `GlobalKey` retake's two
//! windows, `activate_subtree` and the retake's own `update`
//! (`ElementTree::update_or_substitute` and the retake fns in
//! `tree/element_tree.rs`); the removal-path hooks `dispose`,
//! bounded-retake `activate`, `deactivate`, and `did_unmount_render_object`
//! (`StatefulBehavior`/`RenderBehavior` in `element/behavior.rs`; a direct
//! public activation remains unbounded and unrecorded); and a
//! lazy sliver host's own delegate — the item builder
//! (`sparse_children::build_item_or_report`, on the mount path only; the
//! count-probe callers in `sliver_adaptor.rs` stay unreported by design,
//! see `build_item_or_error`'s doc) and `find_index_by_key`
//! (`sparse_children::find_index_or_none`) — catches
//! a user lifecycle-hook panic and substitutes an
//! [`ErrorView`](crate::view::ErrorView) rather than letting the unwind
//! abort the frame. Substituting is not reporting: a panic that only ever
//! reaches `tracing::error!` has nowhere a later consumer can read it back
//! from, act on it, or forward it. [`RecoveredPanic`] is the record a seam
//! pushes instead; [`BuildOwner::take_recovered_panics`](super::BuildOwner::take_recovered_panics)
//! is what drains the queue.
//!
//! # Who pushes, who drains
//!
//! Every per-child containment seam pushes through
//! [`ElementOwner::push_recovered_panic`](super::ElementOwner::push_recovered_panic),
//! which logs at error level and pushes in one call, so a seam never logs
//! and records separately. A host drains after the build segment through
//! `WidgetsBinding::take_recovered_panics`; tests drain directly through
//! [`BuildOwner::take_recovered_panics`](super::BuildOwner::take_recovered_panics).
//! The queue is frame-scoped scratch state by construction: if a host does
//! not drain it, the next `WidgetsBinding::draw_frame` entry discards the
//! stale records with one aggregate warning before new frame work begins.

use std::{
    any::{Any, TypeId},
    fmt,
};

use flui_foundation::{
    ElementId,
    panic::{is_internal_invariant, payload_text},
};

use crate::view::FlutterError;

/// Which `ViewState`/view lifecycle hook a [`RecoveredPanic`] was caught
/// inside.
///
/// `#[non_exhaustive]`: a frame-failure report maps this into its own
/// finer-grained failure-kind variant, expected to grow alongside the
/// containment seams that get added one at a time.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleHook {
    /// `StatelessView::build` / `ViewState::build` — the read half of
    /// `ElementBase::build_into_views`.
    Build,
    /// `ViewState::init_state`, run once at mount before the first build.
    InitState,
    /// `ViewState::did_change_dependencies`, run when an ancestor
    /// `InheritedView` this element depends on notifies a change.
    DidChangeDependencies,
    /// A fresh element's `View::create_element` (including `create_state`
    /// for a `StatefulView`) or `RenderView::create_render_object`. The
    /// [`FlutterError`] `details` breadcrumb carries which of the two
    /// actually panicked. `ViewState::init_state` is NOT this hook — it
    /// runs later, in the `build_scope` drain, not during `mount`. See
    /// [`Self::InitState`].
    Mount,
    /// A bounded `GlobalKey` retake's `activate_subtree` call, reactivating
    /// an element that was queued inactive. A direct public activation panic
    /// propagates without producing a recovered-panic record.
    Activate,
    /// `ViewState::did_update_view` / `RenderView::update_render_object`,
    /// or `AnimatedBehavior::on_view_updated`'s `listenable()` read. That
    /// read happens once, on the new view only: the cached `Arc` from the
    /// last subscribe already stands in for the old view's instance, so
    /// there is no second call to compare against.
    Update,
    /// `ViewState::deactivate`, run when a parent's rebuild drops this
    /// element without unmounting it (it may still be retaken this
    /// frame).
    Deactivate,
    /// `ViewState::dispose`, run when an inactive element is finalized at
    /// end-of-frame with no retake having claimed it.
    Dispose,
    /// `RenderView::did_unmount_render_object`, run while the render
    /// object is still attached, just before it is removed from the
    /// render tree.
    UnmountRenderObject,
    /// A lazy sliver delegate's `find_index_by_key` — a distinct user hook
    /// from the item builder (`Self::Build`): it searches the whole data
    /// source for a key rather than building one item, and a panic there
    /// declines the move (answers `None`, the same outcome a genuine
    /// "not found" produces) instead of substituting a view, since there is
    /// no single index to substitute at.
    LazyIndexLookup,
}

impl fmt::Display for LifecycleHook {
    /// These strings are the `hook` log-field identifiers
    /// `ElementOwner::push_recovered_panic` emits (`hook = %panic.hook`)
    /// and are stable — a log query or downstream forwarder may match on
    /// them by name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Build => "build",
            Self::InitState => "init_state",
            Self::DidChangeDependencies => "did_change_dependencies",
            Self::Mount => "mount",
            Self::Activate => "activate",
            Self::Update => "update",
            Self::Deactivate => "deactivate",
            Self::Dispose => "dispose",
            Self::UnmountRenderObject => "did_unmount_render_object",
            Self::LazyIndexLookup => "find_index_by_key",
        })
    }
}

/// Where a [`RecoveredPanic`] was attributed and, when applicable, which
/// substitute location the containing seam produced.
///
/// The variants distinguish three attribution contexts: an exact element,
/// a window-level substitution, or a lazy delegate call made before an item
/// element existed. They do not by themselves promise that the attributed
/// element survived or describe every cleanup action an outer seam took.
///
/// `#[non_exhaustive]`: containment seams are added one at a time, and a
/// later one may need a shape none of these three cover.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveredAt {
    /// The hook was attributed to this exact element rather than to a
    /// substitute slot. This is location, not a promise that the element
    /// survives recovery: an `Activate` panic is recorded here by the
    /// behavior for accurate descendant attribution, then the retake removes
    /// the failed subtree and mounts a substitute. For removal hooks
    /// (`Deactivate`, `Dispose`, `UnmountRenderObject`), a later drain may
    /// likewise see a post-mortem identity rather than a live handle.
    #[non_exhaustive]
    Element {
        /// The element whose lifecycle hook panicked.
        element: ElementId,
        /// `element`'s parent at the time of the panic, when the seam that
        /// caught it can see one. `None` at the unmount-side seams
        /// (`Deactivate` / `Dispose` / `UnmountRenderObject`) —
        /// `ElementCore` does not record its own parent, and those hooks
        /// see only `core`, never the tree.
        parent: Option<ElementId>,
    },
    /// The element at `(parent, slot)` was discarded or removed and
    /// `substitute` now stands in its place.
    #[non_exhaustive]
    Substituted {
        /// The element that panicked, when one existed to discard. `None`
        /// when the panic came from `View::create_element` itself —
        /// nothing had been minted yet for the window to undo.
        element: Option<ElementId>,
        /// The `ErrorView` (or custom recovery view) now mounted at
        /// `(parent, slot)` in `element`'s place. A post-mortem identity
        /// the same way `Element::element` is: a later drain sees the
        /// substitute, not the panic site.
        substitute: ElementId,
        /// The parent `element`/`substitute` is mounted under.
        parent: ElementId,
        /// The slot `element`/`substitute` occupies under `parent`.
        slot: usize,
    },
    /// A lazy host's delegate panicked before any element existed for it:
    /// the item builder (`index: Some`) or the delegate's `find_index_by_key`
    /// (`index: None`). The host mounts the substitute itself (builder) or
    /// declines the move (key lookup) — neither path has an `ElementId` of
    /// its own to report.
    #[non_exhaustive]
    LazyDelegate {
        /// The lazy sliver host whose delegate panicked.
        host: ElementId,
        /// The logical index the panicking item builder was building, or
        /// `None` for a `find_index_by_key` panic (which has no single
        /// index — it searches the whole data source).
        index: Option<usize>,
    },
}

impl RecoveredAt {
    /// The panicking element, when one existed at the time of the panic.
    ///
    /// `None` for [`Self::Substituted`] with no stranded element (a
    /// `create_element` panic) and for [`Self::LazyDelegate`] (no element
    /// ever existed to name).
    pub fn element(&self) -> Option<ElementId> {
        match self {
            Self::Element { element, .. } => Some(*element),
            Self::Substituted { element, .. } => *element,
            Self::LazyDelegate { .. } => None,
        }
    }

    /// The parent of the panicking element, when this variant carries one.
    ///
    /// A [`Self::LazyDelegate`] record returns `None`: its `host` identifies
    /// the owner of the delegate, not the host's parent, and must not be
    /// reinterpreted as an element-tree edge.
    pub fn parent(&self) -> Option<ElementId> {
        match self {
            Self::Element { parent, .. } => *parent,
            Self::Substituted { parent, .. } => Some(*parent),
            Self::LazyDelegate { .. } => None,
        }
    }
}

/// Whether an inner behavior seam already recorded the panic currently
/// unwinding into a tree-level containment window.
///
/// This stays crate-private: it is a handoff between two implementation
/// layers, not part of the diagnostic API. A domain enum makes the two states
/// explicit at every read and prevents a bare boolean from acquiring a second
/// interpretation later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HookPanicRecording {
    /// The tree-level window is the first seam able to record this panic.
    #[default]
    Unrecorded,
    /// An inner behavior seam already recorded the panic with finer attribution.
    Recorded,
}

/// One lifecycle-hook panic a per-child containment seam caught and recovered
/// from, recorded so it can be forwarded as a frame-failure report
/// instead of vanishing into `tracing` alone.
///
/// `#[non_exhaustive]`: the realm maps this into a `FrameFailureKind`
/// variant that is expected to grow further fields without this type's
/// existing ones changing shape underneath it.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RecoveredPanic {
    /// What the panic happened to — which element, and what the seam did
    /// about it. See [`RecoveredAt`].
    pub at: RecoveredAt,
    /// `TypeId` of the `View` that panicked — identifies which view type
    /// failed without pinning a generic parameter onto this type. For
    /// [`RecoveredAt::LazyDelegate`] this is the HOST's `view_type_id`:
    /// a delegate panic has no item view to name (the builder never
    /// returned one; `find_index_by_key` has no view at all), so the host
    /// that owns the delegate is the closest identity available.
    pub view_type_id: TypeId,
    /// Which lifecycle hook was running when the panic happened.
    pub hook: LifecycleHook,
    /// The caught panic, converted to a [`FlutterError`]. `error.message`
    /// is the user's panic payload text — a consumer forwarding this
    /// report may redact it; `error.details` is the framework's own
    /// breadcrumb (which hook, which behavior) and never carries user
    /// data.
    pub error: FlutterError,
    /// Whether the payload text started with `BUG:` — FLUI's own
    /// internal-invariant convention (`docs/PANIC-POLICY.md`). Computed
    /// here, at push time, before any consumer has a chance to redact
    /// `error.message`: a release build that redacts the message must
    /// still classify correctly, so classification cannot depend on the
    /// message surviving intact.
    ///
    /// This field **classifies only** — a `BUG:`-prefixed panic inside a
    /// containment window is still contained, exactly like any other; it
    /// never decides whether a panic is routed, only how it reads once
    /// routed.
    pub internal_invariant: bool,
}

impl RecoveredPanic {
    /// The panicking element, when one existed — see [`RecoveredAt::element`].
    pub fn element(&self) -> Option<ElementId> {
        self.at.element()
    }

    /// The panicking element's parent, when this record's [`RecoveredAt`]
    /// carries one — see [`RecoveredAt::parent`].
    pub fn parent(&self) -> Option<ElementId> {
        self.at.parent()
    }

    /// Build a [`RecoveredPanic`] directly from a caught panic payload,
    /// converting it to a [`FlutterError`] and classifying it in the same
    /// step.
    ///
    /// `context` becomes the `FlutterError::details` breadcrumb (e.g.
    /// `"disposing StatefulElement"`) — never user data. Use this when the
    /// seam has not already built a `FlutterError` for another purpose
    /// (e.g. to render the substituted `ErrorView`); a seam that has one
    /// already should call [`Self::with_error`] instead of re-deriving the
    /// message from the payload a second time.
    pub(crate) fn from_payload(
        at: RecoveredAt,
        view_type_id: TypeId,
        hook: LifecycleHook,
        payload: &(dyn Any + Send),
        context: impl Into<String>,
    ) -> Self {
        Self::with_error(
            at,
            view_type_id,
            hook,
            payload,
            FlutterError::from_panic(payload, context),
        )
    }

    /// Build a [`RecoveredPanic`] from a panic payload and an
    /// **already-built** [`FlutterError`] — for a seam that built the
    /// error first for another purpose (typically to render the
    /// substituted `ErrorView` before it knows whether it will push a
    /// record at all) and must not re-derive the message from the payload
    /// a second time. Classifies `payload` the same way
    /// [`Self::from_payload`] does.
    pub(crate) fn with_error(
        at: RecoveredAt,
        view_type_id: TypeId,
        hook: LifecycleHook,
        payload: &(dyn Any + Send),
        error: FlutterError,
    ) -> Self {
        Self {
            at,
            view_type_id,
            hook,
            internal_invariant: payload_text(payload).is_some_and(is_internal_invariant),
            error,
        }
    }
}
