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
use flui_foundation::geometry::{Matrix4, Offset};

use crate::pan_zoom::PointerPanZoomEvent;
use crate::{
    events::{CursorIcon, PointerEvent, ScrollEventData},
    routing::MouseTrackerAnnotation,
    routing::interaction_lane::{
        PanZoomTarget, PointerTarget, RoutePanic, ScrollTarget, active_dispatch_handle,
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
    pub cursor: CursorIcon,

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
            cursor: CursorIcon::Default,
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
    /// If `f` panics, the transform is **not** popped (no `Drop`-
    /// based guard). The hit-test framework runs inside the
    /// pipeline owner's `catch_unwind` boundary, so a panicked
    /// `HitTestResult` is dropped wholesale on the next frame;
    /// per-call transform balance is therefore not load-bearing.
    /// Callers wanting strict panic-safe transform balance should
    /// pop manually with `push_offset` + `pop_transform`.
    pub fn with_paint_offset<F, R>(&mut self, offset: Offset<f64>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.push_offset(-offset);
        let result = f(self);
        self.pop_transform();
        result
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
    /// # Known limitations
    ///
    /// 1. **Non-invertible transforms.** A hard refusal (returning `false`
    ///    to say the subtree is not hittable) would need a `bool` threaded
    ///    through every caller. This method instead falls
    ///    back to pushing the still-singular forward matrix, the same
    ///    convention `PipelineOwner::hit_test_subtree` already uses for
    ///    `RenderBox::hit_test_transform`
    ///    (`crates/flui-rendering/src/pipeline/owner/accessors.rs`:
    ///    `t.try_inverse().unwrap_or(t)`): when the determinant is exactly
    ///    zero, the composed chain stays singular, so delivery still
    ///    detects and skips it (`LocalEventTransform::capture`) without
    ///    this method needing to thread a `bool` result back through every
    ///    caller. The skip is only threshold-relative, not guaranteed, for
    ///    a merely near-singular transform (`0 < |det| < f64::EPSILON`,
    ///    which `Matrix4::is_invertible` also rejects): determinants
    ///    compose multiplicatively, so a large-determinant ancestor
    ///    elsewhere in the chain can lift the product back above
    ///    `f64::EPSILON`. In that case delivery sees an invertible
    ///    composite and delivers the entry with a garbage local position --
    ///    a wider gap than the still-singular fallback above covers by
    ///    itself.
    /// 2. **No perspective removal.** Stripping the perspective row/column
    ///    before inverting would let a perspective-projected transform still
    ///    invert to a usable affine map. This method calls
    ///    `transform.try_inverse()` directly, with no perspective removal.
    ///    Low reachability today: nothing in the widget layer constructs a
    ///    perspective (non-affine) transform, so every `transform` reaching
    ///    this method in practice is already affine. Perspective removal is
    ///    intentionally not implemented here (out of scope); a
    ///    perspective-producing widget added later would make this
    ///    limitation live and worth revisiting.
    pub fn with_paint_transform<F, R>(&mut self, transform: Matrix4, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.push_transform(transform.try_inverse().unwrap_or(transform));
        let result = f(self);
        self.pop_transform();
        result
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
    pub fn dispatch_scroll(&self, event: &ScrollEventData) -> bool {
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
                        transform_scroll_event(event, transform)
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
    /// Routing trackpad pan-zoom through the SCALE GESTURE ARENA would
    /// resolve the same contention with full gesture arbitration. This claim
    /// walk is the interim arbitration FLUI has until a recognizer takes
    /// pan-zoom input; it is deliberately shaped like
    /// the pointer-signal claim walk, which is the arbitration primitive this
    /// codebase already has.
    ///
    /// Returns `true` when a target claimed the event.
    pub fn dispatch_pan_zoom(&self, event: &PointerPanZoomEvent) -> bool {
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
                        transform_pan_zoom_event(event, transform)
                    } else {
                        continue;
                    }
                } else {
                    *event
                };

                match handle.invoke_pan_zoom_target(target, &local_event) {
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
    /// Returns the first non-default cursor in the path, or
    /// `CursorIcon::Default`.
    pub fn resolve_cursor(&self) -> CursorIcon {
        for entry in &self.path {
            if entry.cursor != CursorIcon::Default {
                return entry.cursor;
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
/// Automatically pops transform when dropped.
#[must_use = "TransformGuard must be held to maintain the transform"]
#[derive(Debug)]
pub struct TransformGuard<'a> {
    result: &'a mut HitTestResult,
}

impl<'a> TransformGuard<'a> {
    /// Creates a guard that will pop on drop.
    pub fn new(result: &'a mut HitTestResult) -> Self {
        Self { result }
    }
}

impl Drop for TransformGuard<'_> {
    fn drop(&mut self) {
        self.result.pop_transform();
    }
}

// ============================================================================
// HIT TESTABLE TRAIT
// ============================================================================

/// Trait for objects that can be hit-tested.
pub trait HitTestable: crate::sealed::hit_testable::Sealed {
    /// Performs hit testing at the given position.
    fn hit_test(&self, position: Offset<f64>, result: &mut HitTestResult) -> bool;

    /// Returns the hit test behavior.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::DeferToChild
    }
}

impl<T: crate::sealed::CustomHitTestable> HitTestable for T {
    fn hit_test(&self, position: Offset<f64>, result: &mut HitTestResult) -> bool {
        self.perform_hit_test(position, result)
    }

    fn hit_test_behavior(&self) -> HitTestBehavior {
        self.get_hit_test_behavior()
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

pub(crate) fn transform_pointer_event(event: &PointerEvent, transform: &Matrix4) -> PointerEvent {
    use ui_events::pointer::{PointerButtonEvent, PointerScrollEvent, PointerUpdate};

    let transform_position = |pos: dpi::PhysicalPosition<f64>| -> dpi::PhysicalPosition<f64> {
        let (x, y) = transform.transform_point(pos.x, pos.y);
        dpi::PhysicalPosition::new(x, y)
    };

    match event {
        PointerEvent::Down(e) => {
            let mut new_state = e.state.clone();
            new_state.position = transform_position(e.state.position);
            PointerEvent::Down(PointerButtonEvent {
                button: e.button,
                pointer: e.pointer,
                state: new_state,
            })
        }
        PointerEvent::Up(e) => {
            let mut new_state = e.state.clone();
            new_state.position = transform_position(e.state.position);
            PointerEvent::Up(PointerButtonEvent {
                button: e.button,
                pointer: e.pointer,
                state: new_state,
            })
        }
        PointerEvent::Move(e) => {
            let mut new_current = e.current.clone();
            new_current.position = transform_position(e.current.position);
            PointerEvent::Move(PointerUpdate {
                pointer: e.pointer,
                current: new_current,
                coalesced: e.coalesced.clone(),
                predicted: e.predicted.clone(),
            })
        }
        PointerEvent::Scroll(e) => {
            let mut new_state = e.state.clone();
            new_state.position = transform_position(e.state.position);
            PointerEvent::Scroll(PointerScrollEvent {
                pointer: e.pointer,
                state: new_state,
                delta: e.delta,
            })
        }
        PointerEvent::Gesture(e) => {
            // A gesture's focal point localizes exactly like a scroll's
            // position — without this, a pinch consumer under any offset or
            // transform scales around a window-global point and the content
            // jumps instead of staying under the fingers.
            let mut new_state = e.state.clone();
            new_state.position = transform_position(e.state.position);
            PointerEvent::Gesture(ui_events::pointer::PointerGestureEvent {
                pointer: e.pointer,
                gesture: e.gesture.clone(),
                state: new_state,
            })
        }
        // Cancel, Enter, Leave don't have position - just clone
        other => other.clone(),
    }
}

/// Re-express a pan-zoom event in an entry's local space.
///
/// Each field is localized differently:
///
/// - `position` and `pan` are **positions**: transformed as points.
/// - `pan_delta` is a **delta anchored at `pan`**: the delta's start and end
///   points are transformed separately and subtracted, rather than mapping
///   the offset directly — mathematically equivalent for an affine matrix,
///   but it also stays correct under perspective and carries less precision
///   error.
/// - `scale` and `rotation` are dimensionless and pass through untouched.
///
/// Localizing `pan`/`pan_delta` matters even though today's W3C adapter
/// synthesizes both as zero (`convert_gesture` has no upstream pan field to
/// read): `PointerPanZoomEvent` is public and `dispatch_pan_zoom` accepts a
/// fully populated one, so a richer producer must not silently observe
/// global-space offsets inside a scaled or rotated subtree.
fn transform_pan_zoom_event(
    event: &PointerPanZoomEvent,
    transform: &Matrix4,
) -> PointerPanZoomEvent {
    let localize = |point: Offset<f64>| {
        let (x, y) = transform.transform_point(point.dx, point.dy);
        Offset::new(x, y)
    };
    match *event {
        PointerPanZoomEvent::Start {
            pointer_id,
            position,
            timestamp_nanos,
            device_kind,
        } => PointerPanZoomEvent::Start {
            pointer_id,
            position: localize(position),
            timestamp_nanos,
            device_kind,
        },
        PointerPanZoomEvent::Update {
            pointer_id,
            position,
            pan,
            pan_delta,
            scale,
            rotation,
            timestamp_nanos,
            device_kind,
        } => {
            let local_pan = localize(pan);
            PointerPanZoomEvent::Update {
                pointer_id,
                position: localize(position),
                pan: local_pan,
                // `transformDeltaViaPositions`: end minus start, both mapped
                // as positions, with `pan` as the delta's end point.
                pan_delta: local_pan - localize(pan - pan_delta),
                scale,
                rotation,
                timestamp_nanos,
                device_kind,
            }
        }
        PointerPanZoomEvent::End {
            pointer_id,
            position,
            timestamp_nanos,
            device_kind,
        } => PointerPanZoomEvent::End {
            pointer_id,
            position: localize(position),
            timestamp_nanos,
            device_kind,
        },
    }
}

fn transform_scroll_event(event: &ScrollEventData, transform: &Matrix4) -> ScrollEventData {
    let (x, y) = transform.transform_point(event.position.dx, event.position.dy);

    ScrollEventData {
        position: Offset::new(x, y),
        delta: event.delta,
        modifiers: event.modifiers,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::PointerType;

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

            let event = crate::events::make_down_event(Offset::new(50.0, 50.0), PointerType::Mouse);
            result.dispatch(&event);
        });
        assert_eq!(&*order.borrow(), &["leaf", "root"]);
    }
}
