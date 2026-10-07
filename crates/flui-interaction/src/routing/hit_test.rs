//! Hit testing infrastructure
//!
//! This module provides the base hit testing types:
//!
//! - **`HitTestResult`** - Base result with transform stack
//! - **`HitTestEntry`** - Single hit entry with transform
//!
//! Protocol-specific types (`BoxHitTestResult`, `SliverHitTestResult`) are
//! defined in the `flui_rendering` crate, next to the render protocols they
//! serve.

pub use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Point};
use flui_platform_api::pointer::{
    ButtonChange, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerMove, PointerPosition,
    PointerSample, ScrollDelta, ScrollEvent, ScrollUnit,
};

use crate::{
    events::{CursorIcon, PointerEvent},
    routing::MouseTrackerAnnotation,
    routing::interaction_lane::{
        PanZoomDispatch, PanZoomTarget, PointerTarget, RoutePanic, ScrollTarget,
        active_dispatch_handle,
    },
};

// ============================================================================
// EVENT PROPAGATION (claim walks only)
// ============================================================================

/// Claim-walk propagation control.
///
/// Ordinary pointer delivery has no propagation result: every hit target
/// receives its locally transformed event in leaf-first order (ADR-0027).
/// Only the two arbitrated walks carry a claiming result — the
/// pointer-signal / scroll resolver and the trackpad pan-zoom walk
/// (standing in for a scale gesture arena until FLUI has a scale
/// recognizer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventPropagation {
    /// Keep dispatching to the remaining entries on the walk.
    #[default]
    Continue,
    /// Claim the event; entries further out on the walk do not see it.
    Stop,
}

impl EventPropagation {
    /// Returns `true` if dispatch should continue to the next entry.
    #[inline]
    pub const fn should_continue(self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Returns `true` if dispatch should stop at this entry.
    #[inline]
    pub const fn should_stop(self) -> bool {
        matches!(self, Self::Stop)
    }
}

// ============================================================================
// HIT TEST BEHAVIOR
// ============================================================================

/// How an element takes part in hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Receive events only if a child is hit.
    #[default]
    DeferToChild,
    /// Hit within bounds even with no child hit, and block targets visually
    /// behind from receiving the event.
    Opaque,
    /// Hit within bounds while still letting targets visually behind receive
    /// the event too.
    Translucent,
}

impl HitTestBehavior {
    /// Returns `true` if the element adds itself to the hit-test result even
    /// when no child was hit (`Opaque` and `Translucent`).
    #[inline]
    pub const fn registers_self(self) -> bool {
        matches!(self, Self::Opaque | Self::Translucent)
    }

    /// Returns `true` if a hit on this element prevents targets visually
    /// behind it from being hit (`Opaque` only).
    #[inline]
    pub const fn blocks_below(self) -> bool {
        matches!(self, Self::Opaque)
    }
}

// ============================================================================
// HIT TEST ENTRY (Base)
// ============================================================================

/// A hit target's mouse cursor contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorRequest {
    /// Let the next target in the hit path choose the cursor.
    #[default]
    Defer,
    /// Select this icon, including the platform's default arrow.
    Icon(CursorIcon),
}

/// Base hit test entry.
///
/// Data-only (`Send + Sync`): executable pointer callbacks live in the
/// owner-local interaction lane and are addressed through the entry's
/// [`PointerTarget`] identity, never stored here.
#[derive(Clone)]
pub struct HitTestEntry {
    /// Element/render ID.
    pub target: RenderId,

    /// The composed global-to-local transform for this entry's coordinate
    /// space.
    ///
    /// Assembled by [`HitTestResult`] as the walk descends. The raw
    /// primitives [`HitTestResult::push_offset`] and
    /// [`HitTestResult::push_transform`] push whatever they are given
    /// VERBATIM -- they do not invert. It is the higher-level scope helpers
    /// [`HitTestResult::with_paint_offset`] and
    /// [`HitTestResult::with_paint_transform`] that push each level's OWN
    /// INVERSE, so `HitTestResult::last_transform` folds those inverses
    /// left-multiplied in descent order and the result already maps global
    /// to local -- no further inversion is needed at delivery.
    ///
    /// Set automatically when added to HitTestResult.
    pub transform: Option<Matrix4>,

    /// Data-plane identity of this target's owner-local pointer handler.
    pub pointer_target: Option<PointerTarget>,
    /// Opaque payload this render object attaches to anything that hits it.
    ///
    /// The mechanism `RenderMetaData` uses to make a widget findable from a
    /// hit-test path without the searcher knowing anything about it — how
    /// `DragTarget` is discovered by a drag that has moved over it, since a
    /// drag cannot ask the element tree "who is under this point".
    ///
    /// Rides on the entry rather than being resolved afterwards because the
    /// only place a `RenderId` can be turned back into a render object is
    /// inside the pipeline, holding the tree; a caller resolving it later
    /// would need the pipeline itself, and would be reading a tree that may
    /// have moved on.
    pub metadata: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,

    /// Data-plane identity of this target's owner-local scroll handler.
    pub scroll_target: Option<ScrollTarget>,

    /// Data-plane identity of this target's owner-local pan-zoom handler.
    pub pan_zoom_target: Option<PanZoomTarget>,

    /// Mouse cursor for this target.
    pub cursor: CursorRequest,

    /// Mouse-tracker annotation contributed by this target, if it wants
    /// enter/exit/hover tracking.
    pub mouse_annotation: Option<MouseTrackerAnnotation>,
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("target", &self.target)
            .field("has_transform", &self.transform.is_some())
            .field("has_pointer_target", &self.pointer_target.is_some())
            .field("cursor", &self.cursor)
            .field("has_scroll_target", &self.scroll_target.is_some())
            .field("has_pan_zoom_target", &self.pan_zoom_target.is_some())
            .field("has_mouse_annotation", &self.mouse_annotation.is_some())
            // Presence, not contents: the payload is `dyn Any` and has no
            // useful `Debug`. It is the flag that separates "no tag here" from
            // "tagged, but the downcast asked for another type" -- two states a
            // hit-test log otherwise cannot tell apart.
            .field("has_metadata", &self.metadata.is_some())
            .finish_non_exhaustive()
    }
}

impl HitTestEntry {
    /// Creates a new entry with just a target.
    pub fn new(target: RenderId) -> Self {
        Self {
            target,
            transform: None,
            pointer_target: None,
            scroll_target: None,
            pan_zoom_target: None,
            cursor: CursorRequest::Defer,
            mouse_annotation: None,
            metadata: None,
        }
    }

    /// Builder: attach an opaque payload for searchers to find.
    #[must_use]
    pub fn metadata(mut self, payload: std::sync::Arc<dyn std::any::Any + Send + Sync>) -> Self {
        self.metadata = Some(payload);
        self
    }

    /// The payload downcast to `T`, or `None` if absent or a different type.
    #[must_use]
    pub fn metadata_as<T: std::any::Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.metadata.as_ref()?.downcast_ref::<T>()
    }

    /// Builder: set cursor.
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = CursorRequest::Icon(cursor);
        self
    }

    /// Builder: contribute an explicit cursor or defer to the next entry.
    pub fn cursor_request(mut self, cursor: CursorRequest) -> Self {
        self.cursor = cursor;
        self
    }

    /// Builder: set mouse-tracker annotation.
    pub fn mouse_annotation(mut self, annotation: MouseTrackerAnnotation) -> Self {
        self.mouse_annotation = Some(annotation);
        self
    }

    /// Builder: set the owner-local pointer target identity.
    pub fn pointer_target(mut self, target: PointerTarget) -> Self {
        self.pointer_target = Some(target);
        self
    }

    /// Builder: set the owner-local scroll target identity.
    pub fn scroll_target(mut self, target: ScrollTarget) -> Self {
        self.scroll_target = Some(target);
        self
    }

    /// Builder: set the owner-local pan-zoom target identity.
    pub fn pan_zoom_target(mut self, target: PanZoomTarget) -> Self {
        self.pan_zoom_target = Some(target);
        self
    }

    /// Builder: set the entry's transform directly, bypassing the
    /// `HitTestResult`'s transform stack.
    ///
    /// Use this when the caller has already computed the
    /// global-to-local transform out-of-band (for example, from a
    /// protocol-side `BoxHitTestResult` adapter that owns the
    /// transform graph itself). The standard `HitTestResult::add`
    /// captures the current transform stack via `last_transform()`;
    /// this builder lets callers preserve a transform that the stack
    /// does not currently hold.
    ///
    /// "Unchecked" here means the transform is not validated against
    /// the result's transform stack -- not that it bypasses any
    /// safety invariant. The receiver is still `&mut self` because
    /// the field is private.
    #[must_use]
    pub fn with_transform_unchecked(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }
}

// ============================================================================
// HIT TEST RESULT (Base)
// ============================================================================

/// Result of hit testing (base result type).
///
/// Contains the path of hit targets and manages the transform stack.
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Path of hit entries (most specific first).
    path: Vec<HitTestEntry>,

    /// Global transform stack.
    transforms: Vec<Matrix4>,

    /// Local transform parts (optimization - not globalized yet).
    local_transforms: Vec<TransformPart>,
}

/// Transform part for lazy globalization.
#[derive(Debug, Clone)]
enum TransformPart {
    Matrix(Matrix4),
    Offset(Offset<f64>),
}

impl TransformPart {
    /// Multiply this transform part with a matrix (left multiplication).
    fn multiply(&self, rhs: Matrix4) -> Matrix4 {
        match self {
            TransformPart::Matrix(m) => *m * rhs,
            TransformPart::Offset(o) => {
                // Left multiply: Translation * rhs
                Matrix4::translation(o.dx, o.dy, 0.0) * rhs
            }
        }
    }
}

impl HitTestResult {
    /// Creates an empty hit test result.
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            transforms: vec![Matrix4::identity()],
            local_transforms: Vec::new(),
        }
    }

    /// Wraps another result (shares the same path).
    pub fn wrap(other: &mut HitTestResult) -> &mut Self {
        other
    }

    /// Returns the path of hit entries.
    #[inline]
    pub fn path(&self) -> &[HitTestEntry] {
        &self.path
    }

    /// Returns mutable path.
    #[inline]
    pub fn path_mut(&mut self) -> &mut Vec<HitTestEntry> {
        &mut self.path
    }

    /// Globalizes all local transforms.
    fn globalize_transforms(&mut self) {
        if self.local_transforms.is_empty() {
            return;
        }

        let mut last = *self.transforms.last().unwrap_or(&Matrix4::identity());
        for part in &self.local_transforms {
            last = part.multiply(last);
            self.transforms.push(last);
        }
        self.local_transforms.clear();
    }

    /// Returns the current (last) transform.
    fn last_transform(&mut self) -> Matrix4 {
        self.globalize_transforms();
        *self.transforms.last().unwrap_or(&Matrix4::identity())
    }

    /// Adds an entry to the path.
    pub fn add(&mut self, mut entry: HitTestEntry) {
        entry.transform = Some(self.last_transform());
        self.path.push(entry);
    }

    /// Pushes a transform matrix onto the stack, VERBATIM -- no inversion.
    ///
    /// This is the raw primitive: it pushes exactly the matrix it is given.
    /// [`HitTestEntry::transform`] is documented to be the
    /// stack accumulating the GLOBAL-TO-LOCAL mapping as the walk descends,
    /// so the CALLER is responsible for passing this method the inverse of
    /// whatever forward (paint-direction) transform the level represents.
    /// Prefer [`HitTestResult::with_paint_transform`], which computes and
    /// pushes that inverse for you and pops it automatically. Pushing the
    /// forward matrix here by mistake is exactly the composition-order bug
    /// `with_paint_offset`/`with_paint_transform` exist to prevent -- see
    /// their docs.
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.local_transforms.push(TransformPart::Matrix(transform));
    }

    /// Pushes an offset translation onto the stack, VERBATIM -- no negation.
    ///
    /// This is the raw primitive: `push_offset(o)` pushes `translate(+o)`
    /// exactly as given. Same caller-must-invert contract as
    /// [`HitTestResult::push_transform`] -- for a pure translation the
    /// inverse of "translate by `offset`" is "translate by `-offset`", so a
    /// caller composing a global-to-local stack must pass `-offset`, not
    /// `offset`. Prefer [`HitTestResult::with_paint_offset`], which negates
    /// and pops for you.
    pub fn push_offset(&mut self, offset: Offset<f64>) {
        self.local_transforms.push(TransformPart::Offset(offset));
    }

    /// Pops the last transform from the stack.
    pub fn pop_transform(&mut self) {
        if !self.local_transforms.is_empty() {
            self.local_transforms.pop();
        } else if self.transforms.len() > 1 {
            self.transforms.pop();
        }
    }

    /// Runs `f` with `offset` pushed onto the transform stack and
    /// pops the transform before returning, regardless of `f`'s
    /// return value.
    ///
    /// The push/pop pair is expressed as a scope via a closure.
    ///
    /// # Why the offset is negated
    ///
    /// This pushes `-offset`, not `offset`:
    /// the transform stack accumulates the GLOBAL-TO-LOCAL mapping as the
    /// walk descends, so each level must push its own inverse. For a pure
    /// translation the inverse of "translate by `offset`" is "translate by
    /// `-offset`". Pushing the forward (un-negated) offset here recorded a
    /// composed transform that was `inv(B)·inv(A)`'s mirror-image
    /// `inv(A)·inv(B)` for any chain mixing offsets with a non-commuting
    /// matrix transform (e.g. a scaled `Transform` ancestor) -- correct only
    /// when every part in the chain is a pure translation, where the two
    /// compositions coincide. See `crates/flui-widgets/tests/parity/
    /// pointer_local_position_test.rs` for the regression this fixes.
    ///
    /// # Why a closure and not a guard
    ///
    /// The pre-fix
    /// `paint_offset_scope -> TransformGuard<'_>` API held an
    /// exclusive `&'a mut HitTestResult` borrow for the guard's
    /// lifetime. Calls like
    /// `let _g = result.paint_offset_scope(off); result.add(entry);`
    /// did **not** compile -- the second mutating call was rejected
    /// because the guard still held the borrow. The closure-based
    /// shape sidesteps the borrow conflict: `f` receives
    /// `&mut Self` and can call any mutating method
    /// (`add`, `push_transform`, nested `with_paint_*`) freely
    /// inside the scope.
    ///
    /// # Panic semantics
    ///
    /// The entry transform depth is restored both on return and on unwind.
    /// A caller may catch a descendant's panic and continue the same hit walk
    /// without giving the next entry the failed descendant's coordinate space.
    /// Non-finite offsets return `None` without invoking the subtree.
    pub fn with_paint_offset<F, R>(&mut self, offset: Offset<f64>, f: F) -> Option<R>
    where
        F: FnOnce(&mut Self) -> R,
    {
        if !offset.dx.is_finite() || !offset.dy.is_finite() {
            return None;
        }
        let depth = self.transforms.len() + self.local_transforms.len();
        self.push_offset(-offset);
        let guard = TransformGuard {
            result: self,
            depth,
        };
        Some(f(&mut *guard.result))
    }

    /// Runs `f` with the INVERSE of `transform` pushed onto the transform
    /// stack and pops it before returning.
    ///
    /// See [`with_paint_offset`](Self::with_paint_offset) for the negation
    /// rationale and the closure-vs-guard discussion; this is the
    /// matrix-typed sibling for
    /// callers that need a full 4x4 transform rather than a paint-offset --
    /// same caller-supplies-the-forward-matrix, callee-inverts-it contract.
    ///
    /// Returns `None` without invoking `f` or changing the transform stack if
    /// the computed inverse is not admitted by [`Matrix4::try_inverse`].
    /// Otherwise returns `Some(f(...))`. The inverse uses the full matrix,
    /// including perspective; this scope does not remove perspective terms.
    ///
    /// The entry transform depth is restored on return and unwind, just as for
    /// [`with_paint_offset`](Self::with_paint_offset).
    pub fn with_paint_transform<F, R>(&mut self, transform: Matrix4, f: F) -> Option<R>
    where
        F: FnOnce(&mut Self) -> R,
    {
        let inverse = transform.try_inverse()?;
        let depth = self.transforms.len() + self.local_transforms.len();
        self.push_transform(inverse);
        let guard = TransformGuard {
            result: self,
            depth,
        };
        Some(f(&mut *guard.result))
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Returns true if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns an iterator over the entries.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.path.iter()
    }

    /// Returns an iterator over entries with scroll targets.
    pub fn entries_with_scroll_targets(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.path.iter().filter(|e| e.scroll_target.is_some())
    }

    /// Returns an iterator over entries with pan-zoom targets.
    pub fn entries_with_pan_zoom_targets(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.path.iter().filter(|e| e.pan_zoom_target.is_some())
    }

    /// Clears all entries and transforms.
    pub fn clear(&mut self) {
        self.path.clear();
        self.transforms.clear();
        self.transforms.push(Matrix4::identity());
        self.local_transforms.clear();
    }

    /// Dispatches a pointer event to every entry, leaf-first, through the
    /// active owner lane.
    ///
    /// Resolves an ephemeral route over the path's pointer targets, invokes it
    /// synchronously with per-entry local transforms and per-target panic
    /// isolation, and releases the route before returning (or before resuming
    /// a captured panic). Delivery never stops early: ordinary pointer events
    /// have no propagation result.
    ///
    /// Must run on the owner thread inside an active interaction lane scope
    /// (a binding's `dispatch_pointer` / owner scope). Without one, entries
    /// carrying pointer targets cannot be delivered; the typed boundary error
    /// is traced and the event is dropped.
    pub fn dispatch(&self, event: &PointerEvent) {
        if let Some(panic) = self.dispatch_capturing_panic(event) {
            panic.resume();
        }
    }

    /// Dispatch an ephemeral pointer route while returning the first target
    /// panic to a binding that still has later transaction phases to run.
    pub(crate) fn dispatch_capturing_panic(&self, event: &PointerEvent) -> Option<RoutePanic> {
        if !self.path.iter().any(|e| e.pointer_target.is_some()) {
            return None;
        }
        let handle = match active_dispatch_handle() {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "pointer dispatch outside an active interaction lane; event not delivered"
                );
                return None;
            }
        };
        let resolution = match handle.resolve_pointer_route(&self.path) {
            Ok(resolution) => resolution,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "pointer route resolution failed; event not delivered"
                );
                return None;
            }
        };
        for miss in resolution.misses() {
            tracing::debug!(
                path_index = miss.path_index(),
                "hit path target unregistered before resolution"
            );
        }
        let token = resolution.token();
        let delivery = handle.invoke_pointer_route(token, event);
        // Mandatory cleanup precedes any resumed panic: the ephemeral route is
        // released whether or not a target panicked.
        let release = RoutePanic::try_run(|| handle.release_route(token));
        let mut first_panic = match delivery {
            Ok(panic) => panic,
            Err(error) => {
                tracing::error!(?error, "pointer route invocation failed");
                None
            }
        };
        match release {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::error!(?error, "failed to release ephemeral pointer route");
            }
            Err(panic) => {
                RoutePanic::preserve_first(
                    &mut first_panic,
                    Some(panic),
                    "ephemeral route cleanup",
                );
            }
        }
        first_panic
    }

    /// Dispatch a pointer event to every ordinary pointer target AND every
    /// mouse-hover region on this path together, leaf-first, in a single
    /// per-entry pass — the interleaved counterpart to
    /// [`dispatch_capturing_panic`](Self::dispatch_capturing_panic), which
    /// only knows about ordinary pointer targets.
    ///
    /// A `Listener` and a nested `MouseRegion` sharing this path must fire in
    /// hit-test order relative to EACH OTHER (one per-entry loop over the
    /// leaf-first path); calling
    /// [`dispatch_capturing_panic`](Self::dispatch_capturing_panic) and
    /// [`MouseTracker::dispatch_hover`](super::mouse_tracker::MouseTracker::dispatch_hover)
    /// as two separate full passes always delivers every ordinary target
    /// before every region, regardless of which is actually the leaf.
    ///
    /// Used only by the coalesced ephemeral hover-move dispatch path (see
    /// [`InteractionDispatchHandle::dispatch_hover_interleaved`](super::interaction_lane::InteractionDispatchHandle::dispatch_hover_interleaved)
    /// for the resolve/invoke detail); every other event kind keeps
    /// dispatching through
    /// [`dispatch_capturing_panic`](Self::dispatch_capturing_panic)
    /// unchanged.
    pub(crate) fn dispatch_hover_interleaved_capturing_panic(
        &self,
        event: &PointerEvent,
    ) -> Option<RoutePanic> {
        if !self
            .path
            .iter()
            .any(|e| e.pointer_target.is_some() || e.mouse_annotation.is_some())
        {
            return None;
        }
        let handle = match active_dispatch_handle() {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "hover-interleaved dispatch outside an active interaction lane; \
                     event not delivered"
                );
                return None;
            }
        };
        handle.dispatch_hover_interleaved(&self.path, event)
    }

    /// Dispatches a scroll event to all entries.
    pub fn dispatch_scroll(&self, event: &ScrollEvent) -> bool {
        let handle = match active_dispatch_handle() {
            Ok(handle) => handle,
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "scroll dispatch skipped without an active owner lane"
                );
                return false;
            }
        };
        for entry in &self.path {
            if let Some(target) = entry.scroll_target {
                let local_event = if let Some(ref transform) = entry.transform {
                    // `transform` is already global-to-local (see
                    // `HitTestEntry::transform`'s doc) -- apply it directly.
                    // `is_invertible` is a well-formedness probe -- it skips
                    // computing (and discarding) the inverse itself: a
                    // degenerate ancestor transform makes the composed
                    // `transform` itself singular, and such an entry must
                    // still skip delivery rather than report a bogus point
                    // (unchanged pre-existing behavior).
                    if transform.is_invertible() {
                        let Some(local) = transform_scroll_event(event, transform) else {
                            continue;
                        };
                        local
                    } else {
                        continue;
                    }
                } else {
                    *event
                };

                match handle.invoke_scroll_target(target, &local_event) {
                    Ok(propagation) if propagation.should_stop() => return true,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(
                            ?error,
                            "scroll target unavailable during owner-lane dispatch"
                        );
                    }
                }
            }
        }
        false
    }

    /// Dispatches a trackpad pan-zoom event to the path's pan-zoom targets,
    /// leaf-first, stopping at the first one that claims it.
    ///
    /// The pan-zoom counterpart of
    /// [`dispatch_scroll`](Self::dispatch_scroll), and arbitrated for the
    /// same reason: ordinary pointer delivery has no propagation result, so
    /// without this walk every enabled consumer under the focal point acts on
    /// the same tick and two nested viewers both zoom. A consumer returns
    /// [`EventPropagation::Stop`] only when it will actually consume the tick,
    /// so a viewer already clamped at its scale extent hands the pinch to the
    /// one above it.
    ///
    /// A native gesture is admitted before its recognizer publishes callbacks.
    /// The claimant receives the localized gesture alongside its original
    /// root-space event, so both focal points retain their actual coordinates.
    ///
    /// Returns `true` when a target claimed the event.
    pub fn dispatch_pan_zoom(&self, event: &PanZoomEvent) -> bool {
        let handle = match active_dispatch_handle() {
            Ok(handle) => handle,
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "pan-zoom dispatch skipped without an active owner lane"
                );
                return false;
            }
        };
        for entry in &self.path {
            if let Some(target) = entry.pan_zoom_target {
                let local_event = if let Some(ref transform) = entry.transform {
                    // `transform` is already global-to-local (see
                    // `HitTestEntry::transform`'s doc). A degenerate ancestor
                    // transform makes the composed matrix singular; such an
                    // entry skips delivery rather than reporting a focal
                    // point that is not on screen, exactly as the scroll walk
                    // does.
                    if transform.is_invertible() {
                        let Some(local) = transform_pan_zoom_event(event, transform) else {
                            continue;
                        };
                        local
                    } else {
                        continue;
                    }
                } else {
                    *event
                };

                match handle.invoke_pan_zoom_target(
                    target,
                    PanZoomDispatch {
                        local: &local_event,
                        global: event,
                    },
                ) {
                    Ok(propagation) if propagation.should_stop() => return true,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(
                            ?error,
                            "pan-zoom target unavailable during owner-lane dispatch"
                        );
                    }
                }
            }
        }
        false
    }

    /// Resolves the active mouse cursor.
    ///
    /// Returns the first explicit cursor in the path, or
    /// `CursorIcon::Default`.
    pub fn resolve_cursor(&self) -> CursorIcon {
        for entry in &self.path {
            if let CursorRequest::Icon(cursor) = entry.cursor {
                return cursor;
            }
        }
        CursorIcon::Default
    }
}

// ============================================================================
// TRANSFORM GUARD (RAII helper)
// ============================================================================

/// RAII guard for transform stack management.
///
/// Restores the preceding transform depth when dropped, including on unwind.
#[must_use = "TransformGuard must be held to maintain the transform"]
#[derive(Debug)]
pub struct TransformGuard<'a> {
    result: &'a mut HitTestResult,
    depth: usize,
}

impl<'a> TransformGuard<'a> {
    /// Creates a guard that removes the current top transform and any later
    /// pushes on drop. Create it immediately after pushing the scoped transform.
    pub fn new(result: &'a mut HitTestResult) -> Self {
        let depth = (result.transforms.len() + result.local_transforms.len()).saturating_sub(1);
        Self { result, depth }
    }
}

impl Drop for TransformGuard<'_> {
    fn drop(&mut self) {
        if self.result.transforms.len() > self.depth {
            self.result.transforms.truncate(self.depth);
            self.result.local_transforms.clear();
        } else {
            self.result
                .local_transforms
                .truncate(self.depth - self.result.transforms.len());
        }
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

pub(crate) fn transform_pointer_event(
    event: &PointerEvent,
    transform: &Matrix4,
) -> Option<PointerEvent> {
    let mut local = event.clone();
    match &mut local {
        PointerEvent::Down(event) => event.sample = transform_sample(event.sample, transform)?,
        PointerEvent::Up(event) => event.sample = transform_sample(event.sample, transform)?,
        PointerEvent::ButtonChange(ButtonChange::Pressed(event)) => {
            event.sample = transform_sample(event.sample, transform)?;
        }
        PointerEvent::ButtonChange(ButtonChange::Released(event)) => {
            event.sample = transform_sample(event.sample, transform)?;
        }
        PointerEvent::Move(event) => {
            let current = transform_sample(*event.current(), transform)?;
            let coalesced = event
                .coalesced()
                .iter()
                .copied()
                .map(|sample| transform_sample(sample, transform))
                .collect::<Option<Vec<_>>>()?;
            let predicted = event
                .predicted()
                .iter()
                .copied()
                .map(|sample| transform_sample(sample, transform))
                .collect::<Option<Vec<_>>>()?;
            *event = PointerMove::new(event.pointer, event.buttons, current)
                .with_modifiers(event.modifiers)
                .with_coalesced(coalesced)
                .with_predicted(predicted);
        }
        PointerEvent::Scroll(event) => *event = transform_scroll_event(event, transform)?,
        PointerEvent::PanZoom(event) => *event = transform_pan_zoom_event(event, transform)?,
        PointerEvent::Enter(event)
        | PointerEvent::Leave(event)
        | PointerEvent::ScrollInertiaCancel(event) => {
            if let Some(position) = event.position {
                event.position = Some(transform_position(position, transform)?);
            }
        }
        _ => {}
    }
    Some(local)
}

/// Refuse a local coordinate that cannot be represented by the checked vocabulary.
fn transform_position(position: PointerPosition, transform: &Matrix4) -> Option<PointerPosition> {
    let point = position.get();
    let (x, y) = transform.unproject_to_plane(point.x, point.y)?;
    PointerPosition::try_new(Point::new(x, y)).ok()
}

fn transform_sample(mut sample: PointerSample, transform: &Matrix4) -> Option<PointerSample> {
    sample.position = transform_position(sample.position, transform)?;
    Some(sample)
}

/// Localize cumulative pan as a chord anchored at this event's current focal.
///
/// No starting global focal is inferred from an Update. Scale and rotation
/// remain dimensionless, and the source event retains its cumulative values.
fn transform_pan_zoom_event(event: &PanZoomEvent, transform: &Matrix4) -> Option<PanZoomEvent> {
    let mut local = *event;
    local.position = transform_position(event.position, transform)?;
    if let PanZoomPhase::Update(value) = event.phase {
        let pan = transform_delta(transform, event.position, value.pan())?;
        local.phase = PanZoomPhase::Update(
            PanZoomTransform::try_new(pan, value.scale(), value.rotation()).ok()?,
        );
    }
    Some(local)
}

fn transform_scroll_event(event: &ScrollEvent, transform: &Matrix4) -> Option<ScrollEvent> {
    let mut local = *event;
    local.position = transform_position(event.position, transform)?;
    // Only Pixels describes a geometric displacement. Line/Page counts are
    // resolved by the consuming scrollable against its own metrics; treating
    // them as screen endpoints would silently change that quantity.
    if event.delta.unit() == ScrollUnit::Pixels {
        let delta = transform_delta(
            transform, event.position, Offset::new(event.delta.x(), event.delta.y()),
        )?;
        local.delta = ScrollDelta::try_new(ScrollUnit::Pixels, delta.dx, delta.dy).ok()?;
    }
    Some(local)
}

/// A projective displacement is a chord between two admitted plane points,
/// not a vector transformed at an unrelated global origin.
fn transform_delta(
    transform: &Matrix4,
    focal: PointerPosition,
    delta: Offset<f64>,
) -> Option<Offset<f64>> {
    let focal = focal.get();
    let endpoint_x = focal.x + delta.dx;
    let endpoint_y = focal.y + delta.dy;
    if !endpoint_x.is_finite() || !endpoint_y.is_finite() {
        return None;
    }
    let (origin_x, origin_y) = transform.unproject_to_plane(focal.x, focal.y)?;
    let (x, y) = transform.unproject_to_plane(endpoint_x, endpoint_y)?;
    let local = Offset::new(x - origin_x, y - origin_y);
    (local.dx.is_finite() && local.dy.is_finite()).then_some(local)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::PointerKind;

    #[test]
    fn dispatch_reaches_every_target_leaf_first_without_stopping() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let order = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let leaf_order = Rc::clone(&order);
            let leaf = handle
                .register_pointer(move |_| leaf_order.borrow_mut().push("leaf"))
                .expect("register leaf");
            let root_order = Rc::clone(&order);
            let root = handle
                .register_pointer(move |_| root_order.borrow_mut().push("root"))
                .expect("register root");

            // Leaf-first path order: children push their entries before the
            // ancestor. No propagation result exists to stop delivery early.
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(leaf));
            result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(root));

            let event = crate::events::make_down_event(Offset::new(50.0, 50.0), PointerKind::Mouse)
                .expect("finite input");
            result.dispatch(&event);
        });
        assert_eq!(&*order.borrow(), &["leaf", "root"]);
    }
}
