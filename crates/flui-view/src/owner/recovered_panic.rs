//! Panics contained by a per-child lifecycle seam, and the drain that
//! collects them.
//!
//! A per-child containment seam — `build_or_recover` today; a fresh
//! mount's `create_element`/`mount` window, a `GlobalKey` retake's
//! `activate`/`update` window, `dispose`, `deactivate`, and
//! `did_unmount_render_object` as each seam lands —
//! catches a user lifecycle-hook panic and substitutes an
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
//! and records separately. The realm drains once per presentation per
//! pump, after the build segment, through
//! `WidgetsBinding::take_recovered_panics`, and forwards the drain as a
//! frame-failure report; tests drain directly through
//! [`BuildOwner::take_recovered_panics`](super::BuildOwner::take_recovered_panics).
//! The queue is frame-scoped scratch state: nothing here outlives the
//! drain that reads it.

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
    /// A fresh element's `View::create_element` + `RenderView::
    /// create_render_object` + `did_mount_render_object`. The
    /// [`FlutterError`] `details` breadcrumb carries which of the three
    /// actually panicked.
    Mount,
    /// A `GlobalKey` retake's `activate_subtree` call, reactivating an
    /// element that was queued inactive.
    Activate,
    /// `ViewState::did_update_view` / `RenderView::update_render_object`,
    /// or `InheritedBehavior::on_view_updated`'s re-read of `listenable()`
    /// on the old and new view.
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
}

impl fmt::Display for LifecycleHook {
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
        })
    }
}

/// One lifecycle-hook panic a per-child containment seam caught and
/// substituted, recorded so it can be forwarded as a frame-failure report
/// instead of vanishing into `tracing` alone.
///
/// `#[non_exhaustive]`: the realm maps this into a `FrameFailureKind`
/// variant that is expected to grow further fields without this type's
/// existing ones changing shape underneath it.
#[non_exhaustive]
#[derive(Debug)]
pub struct RecoveredPanic {
    /// The element whose lifecycle hook panicked.
    pub element: ElementId,
    /// The parent of `element` at the time of the panic, when the seam
    /// that caught it can see one. `None` at the unmount-side seams
    /// (`dispose` / `deactivate` / `did_unmount_render_object`) —
    /// `ElementCore` does not record its own parent, and those hooks see
    /// only `core`, never the tree.
    pub parent: Option<ElementId>,
    /// `TypeId` of the `View` mounted at `element` — identifies which
    /// view type panicked without pinning a generic parameter onto this
    /// type.
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
    /// Build a [`RecoveredPanic`] directly from a caught panic payload,
    /// converting it to a [`FlutterError`] and classifying it in the same
    /// step.
    ///
    /// `context` becomes the `FlutterError::details` breadcrumb (e.g.
    /// `"disposing StatefulElement"`) — never user data. Use this when the
    /// seam has not already built a `FlutterError` for another purpose
    /// (e.g. to render the substituted `ErrorView`); a seam that has one
    /// already should build `Self` directly instead of re-deriving the
    /// message from the payload a second time.
    pub(crate) fn from_payload(
        element: ElementId,
        parent: Option<ElementId>,
        view_type_id: TypeId,
        hook: LifecycleHook,
        payload: &(dyn Any + Send),
        context: impl Into<String>,
    ) -> Self {
        Self {
            element,
            parent,
            view_type_id,
            hook,
            internal_invariant: payload_text(payload).is_some_and(is_internal_invariant),
            error: FlutterError::from_panic(payload, context),
        }
    }
}
