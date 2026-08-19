//! Hit testing infrastructure (Flutter-like)
//!
//! This module provides base hit testing types following Flutter's
//! architecture:
//!
//! - **`HitTestResult`** - Base result with transform stack
//!   (gestures/hit_test.dart)
//! - **`HitTestEntry`** - Single hit entry with transform
//!
//! Protocol-specific types (`BoxHitTestResult`, `SliverHitTestResult`) are
//! defined in `flui_rendering` crate, following Flutter's organization where:
//! - `BoxHitTestResult` is in `rendering/box.dart`
//! - `SliverHitTestResult` is in `rendering/sliver.dart`
//!
//! # Flutter References
//!
//! - HitTestResult: gestures/hit_test.dart
//! - HitTestEntry: gestures/hit_test.dart

pub use flui_foundation::RenderId;
use flui_types::geometry::{Matrix4, Offset, Pixels};

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
/// receives its locally transformed event in leaf-first order (ADR-0027,
/// Flutter `GestureBinding.dispatchEvent` parity). Only the two arbitrated
/// walks carry a claiming result — the pointer-signal / scroll resolver
/// (mirroring Flutter's separate `PointerSignalResolver`) and the trackpad
/// pan-zoom walk (standing in for Flutter's scale gesture arena until FLUI
/// has a scale recognizer).
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

/// Hit test behavior (Flutter's HitTestBehavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Receive events only if a child is hit (Flutter's `deferToChild`).
    #[default]
    DeferToChild,
    /// Hit within bounds even with no child hit, and block targets visually
    /// behind from receiving the event (Flutter's `opaque`).
    Opaque,
    /// Hit within bounds while still letting targets visually behind receive
    /// the event too (Flutter's `translucent`).
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
// HIT TEST ENTRY (Base - Flutter's HitTestEntry<T>)
// ============================================================================

/// Base hit test entry.
///
/// Data-only (`Send + Sync`): executable pointer callbacks live in the
/// owner-local interaction lane and are addressed through the entry's
/// [`PointerTarget`] identity, never stored here.
///
/// Flutter equivalent: `HitTestEntry<T extends HitTestTarget>`
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
    /// to local -- no further inversion is needed at delivery. Flutter
    /// parity: `pushTransform(Matrix4.tryInvert(...))` (`rendering/box.dart`,
    /// `addWithPaintTransform`/`addWithPaintOffset`), folded by
    /// `HitTestResult._globalizeTransforms` (`gestures/hit_test.dart`).
    ///
    /// Set automatically when added to HitTestResult.
    pub transform: Option<Matrix4>,

    /// Data-plane identity of this target's owner-local pointer handler.
    pub pointer_target: Option<PointerTarget>,

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
        }
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
// HIT TEST RESULT (Base - Flutter's HitTestResult)
// ============================================================================

/// Result of hit testing (base class).
///
/// Flutter equivalent: `class HitTestResult` from gestures/hit_test.dart
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

/// Transform part for lazy globalization (Flutter's _TransformPart).
#[derive(Debug, Clone)]
enum TransformPart {
    Matrix(Matrix4),
    Offset(Offset<Pixels>),
}

impl TransformPart {
    /// Multiply this transform part with a matrix (left multiplication).
    fn multiply(&self, rhs: Matrix4) -> Matrix4 {
        match self {
            TransformPart::Matrix(m) => *m * rhs,
            TransformPart::Offset(o) => {
                // Left multiply: Translation * rhs
                Matrix4::translation(o.dx.0, o.dy.0, 0.0) * rhs
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
    ///
    /// Flutter equivalent: `HitTestResult.wrap(HitTestResult result)`
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
    ///
    /// Flutter equivalent: `void add(HitTestEntry entry)`
    pub fn add(&mut self, mut entry: HitTestEntry) {
        entry.transform = Some(self.last_transform());
        self.path.push(entry);
    }

    /// Pushes a transform matrix onto the stack, VERBATIM -- no inversion.
    ///
    /// This is the raw primitive: it pushes exactly the matrix it is given.
    /// [`HitTestEntry::transform`] is documented (and Flutter's own
    /// `pushTransform`/`_globalizeTransforms` contract requires) that the
    /// stack accumulates the GLOBAL-TO-LOCAL mapping as the walk descends,
    /// so the CALLER is responsible for passing this method the inverse of
    /// whatever forward (paint-direction) transform the level represents.
    /// Prefer [`HitTestResult::with_paint_transform`], which computes and
    /// pushes that inverse for you and pops it automatically. Pushing the
    /// forward matrix here by mistake is exactly the composition-order bug
    /// `with_paint_offset`/`with_paint_transform` exist to prevent -- see
    /// their docs.
    ///
    /// Flutter equivalent: `@protected void pushTransform(Matrix4 transform)`
    /// (callers invert before calling, e.g. `addWithPaintTransform` in
    /// `rendering/box.dart`).
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
    ///
    /// Flutter equivalent: `@protected void pushOffset(Offset offset)`
    /// (callers negate before calling, e.g. `addWithPaintOffset` in
    /// `rendering/box.dart:839` calls `pushOffset(-offset)`).
    pub fn push_offset(&mut self, offset: Offset<Pixels>) {
        self.local_transforms.push(TransformPart::Offset(offset));
    }

    /// Pops the last transform from the stack.
    ///
    /// Flutter equivalent: `@protected void popTransform()`
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
    /// Mirrors `BoxHitTestResult::addWithPaintOffset` in Flutter's
    /// `rendering/box.dart`: the Flutter code uses a try/finally
    /// pair around the pushOffset/popTransform sequence; Rust
    /// expresses the same scope via a closure.
    ///
    /// # Why the offset is negated
    ///
    /// `box.dart:839` calls `pushOffset(-offset)`, not `pushOffset(offset)`:
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
    pub fn with_paint_offset<F, R>(&mut self, offset: Offset<Pixels>, f: F) -> R
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
    /// See [`with_paint_offset`](Self::with_paint_offset) for the
    /// Flutter-parity rationale and the closure-vs-guard discussion
    /// (closure-vs-guard rationale); this is the matrix-typed sibling for
    /// callers that need a full 4x4 transform rather than a paint-offset --
    /// same caller-supplies-the-forward-matrix, callee-inverts-it contract.
    /// Flutter parity: `BoxHitTestResult.addWithPaintTransform`
    /// (`rendering/box.dart:799-812`), which inverts via `Matrix4.tryInvert`
    /// at line 805 before delegating at line 811 to `addWithRawTransform`.
    ///
    /// # Known divergences from Flutter
    ///
    /// 1. **Non-invertible transforms.** Flutter's `addWithPaintTransform`
    ///    returns `false` outright when the transform cannot be inverted
    ///    (the subtree is not visible/hittable). This method instead falls
    ///    back to pushing the still-singular forward matrix, the same
    ///    convention `PipelineOwner::hit_test_subtree` already uses for
    ///    `RenderBox::hit_test_transform`
    ///    (`crates/flui-rendering/src/pipeline/owner/accessors.rs`:
    ///    `t.try_inverse().unwrap_or(t)`): when the determinant is exactly
    ///    zero, the composed chain stays singular, so delivery still
    ///    detects and skips it (`LocalEventTransform::capture`) without
    ///    this method needing to thread a `bool` result back through every
    ///    caller. The skip is only threshold-relative, not guaranteed, for
    ///    a merely near-singular transform (`0 < |det| < f32::EPSILON`,
    ///    which `Matrix4::is_invertible` also rejects): determinants
    ///    compose multiplicatively, so a large-determinant ancestor
    ///    elsewhere in the chain can lift the product back above
    ///    `f32::EPSILON`. In that case delivery sees an invertible
    ///    composite and delivers the entry with a garbage local position --
    ///    a wider divergence from Flutter's hard refusal than the
    ///    still-singular fallback above covers by itself.
    /// 2. **No perspective removal.** `box.dart:805` inverts
    ///    `PointerEvent.removePerspectiveTransform(transform)`, not
    ///    `transform` itself -- Flutter strips the perspective row/column
    ///    before inverting so a perspective-projected transform still
    ///    inverts to a usable affine map. This method calls
    ///    `transform.try_inverse()` directly, with no perspective removal.
    ///    Low reachability today: nothing in the widget layer constructs a
    ///    perspective (non-affine) transform, so every `transform` reaching
    ///    this method in practice is already affine. Perspective removal is
    ///    intentionally not implemented here (out of scope); a
    ///    perspective-producing widget added later would make this
    ///    divergence live and worth revisiting.
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
    /// have no propagation result (Flutter `GestureBinding.dispatchEvent`
    /// parity).
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
    /// hit-test order relative to EACH OTHER (Flutter's single per-entry
    /// `entry.target.handleEvent` loop, `gestures/binding.dart:496`); calling
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
    /// Flutter routes trackpad pan-zoom through the SCALE GESTURE ARENA
    /// (`PointerPanZoomStartEvent` opens a `ScaleGestureRecognizer`'s arena
    /// entry, `gestures/scale.dart`), which resolves the same contention with
    /// full gesture arbitration. This claim walk is the interim arbitration
    /// FLUI has until that recognizer lands; it is deliberately shaped like
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
    fn hit_test(&self, position: Offset<Pixels>, result: &mut HitTestResult) -> bool;

    /// Returns the hit test behavior.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::DeferToChild
    }
}

impl<T: crate::sealed::CustomHitTestable> HitTestable for T {
    fn hit_test(&self, position: Offset<Pixels>, result: &mut HitTestResult) -> bool {
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
        let (x, y) = transform.transform_point(Pixels(pos.x as f32), Pixels(pos.y as f32));
        dpi::PhysicalPosition::new(x.0 as f64, y.0 as f64)
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
/// Ported from Flutter's `_TransformedPointerPanZoomUpdateEvent`
/// (`gestures/events.dart`), which is the oracle for exactly this
/// localization and treats each field differently:
///
/// - `position` and `pan` are **positions**: `transformPosition`.
/// - `pan_delta` is a **delta anchored at `pan`**: the oracle's
///   `transformDeltaViaPositions` transforms the delta's start and end points
///   separately and subtracts, rather than mapping the offset directly —
///   mathematically equivalent for an affine matrix, but it also stays
///   correct under perspective and, as the oracle's own comment records,
///   carries less precision error.
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
    let localize = |point: Offset<Pixels>| {
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
    fn test_hit_test_result_new() {
        let result = HitTestResult::new();
        assert!(result.is_empty());
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_hit_test_result_add() {
        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(RenderId::new(1)));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_hit_test_result_transform_stack() {
        let mut result = HitTestResult::new();

        // `push_offset` is the raw primitive -- it pushes exactly what it is
        // given, no negation (see its doc). The composed entry transform
        // must therefore be the FORWARD translation(10, 20), not just
        // "present": asserting only `.is_some()` (as this test used to)
        // would pass even if the composition folded the wrong matrix in --
        // the exact vacuous shape that let the `with_paint_offset`/
        // `with_paint_transform` direction bug ship undetected.
        result.push_offset(Offset::new(Pixels(10.0), Pixels(20.0)));
        result.add(HitTestEntry::new(RenderId::new(1)));

        let transform = result.path()[0]
            .transform
            .expect("add() must record a transform");
        assert_eq!(
            transform.translation_component(),
            (10.0, 20.0, 0.0),
            "push_offset(10, 20) must compose to the forward translation, unnegated"
        );
        let (local_x, local_y) = transform.transform_point(Pixels(0.0), Pixels(0.0));
        assert_eq!(
            (local_x.0, local_y.0),
            (10.0, 20.0),
            "the composed matrix must actually map the origin to (10, 20)"
        );

        result.pop_transform();
    }

    #[test]
    fn test_event_propagation() {
        assert!(EventPropagation::Continue.should_continue());
        assert!(!EventPropagation::Continue.should_stop());
        assert!(EventPropagation::Stop.should_stop());
        assert!(!EventPropagation::Stop.should_continue());
    }

    #[test]
    fn test_hit_test_behavior() {
        assert!(!HitTestBehavior::DeferToChild.registers_self());
        assert!(HitTestBehavior::Opaque.registers_self());
        assert!(HitTestBehavior::Translucent.registers_self());

        assert!(!HitTestBehavior::DeferToChild.blocks_below());
        assert!(HitTestBehavior::Opaque.blocks_below());
        assert!(!HitTestBehavior::Translucent.blocks_below());
    }

    #[test]
    fn dispatch_delivers_through_the_active_lane() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let delivered = Rc::new(Cell::new(false));
        lane.enter(|| {
            let probe = Rc::clone(&delivered);
            let target = handle
                .register_pointer(move |_| probe.set(true))
                .expect("register");
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));

            let event = crate::events::make_down_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            result.dispatch(&event);
        });
        assert!(delivered.get());
    }

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

            let event = crate::events::make_down_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            result.dispatch(&event);
        });
        assert_eq!(&*order.borrow(), &["leaf", "root"]);
    }

    #[test]
    fn dispatch_applies_the_entry_local_transform() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::PointerEventExt as _;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(Offset::new(Pixels(0.0), Pixels(0.0))));
        lane.enter(|| {
            let position_probe = Rc::clone(&observed);
            let target = handle
                .register_pointer(move |event| position_probe.set(event.position()))
                .expect("register");
            let mut result = HitTestResult::new();
            // The entry sits in a subtree translated by (10, 20): the handler
            // must observe the event mapped into its local space.
            // `with_paint_offset` is the production path (the paint-offset
            // child descent in `PipelineOwner::hit_test_subtree`) -- it
            // pushes the offset's inverse internally, so the entry's
            // recorded transform is already global-to-local.
            result.with_paint_offset(Offset::new(Pixels(10.0), Pixels(20.0)), |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            });

            let event = crate::events::make_down_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            result.dispatch(&event);
        });
        assert_eq!(observed.get(), Offset::new(Pixels(40.0), Pixels(30.0)));
    }

    #[test]
    fn dispatch_applies_the_entry_local_transform_via_paint_transform() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::PointerEventExt as _;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(Offset::new(Pixels(0.0), Pixels(0.0))));
        lane.enter(|| {
            let position_probe = Rc::clone(&observed);
            let target = handle
                .register_pointer(move |event| position_probe.set(event.position()))
                .expect("register");
            let mut result = HitTestResult::new();
            // The entry sits in a subtree scaled 2x from its parent --
            // matrix-typed sibling of `dispatch_applies_the_entry_local_transform`.
            // `with_paint_transform` takes the FORWARD (paint-direction)
            // matrix and must invert it internally before pushing, exactly
            // like `with_paint_offset` does for a pure translation; without
            // that inversion the handler would observe the position
            // multiplied by the scale instead of divided by it. This method
            // has no production caller yet, so this is the only regression
            // coverage for the fix.
            let mut forward = Matrix4::identity();
            forward.scale(2.0, 2.0, 1.0);
            result.with_paint_transform(forward, |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            });

            let event = crate::events::make_down_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            result.dispatch(&event);
        });
        assert_eq!(observed.get(), Offset::new(Pixels(25.0), Pixels(25.0)));
    }

    #[test]
    fn dispatch_scroll_uses_owner_local_scroll_target() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::{PointerEvent, ScrollEventData, make_scroll_event};
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let delivered = Rc::new(Cell::new(false));
        lane.enter(|| {
            let probe = Rc::clone(&delivered);
            let target = handle
                .register_scroll(move |_| {
                    probe.set(true);
                    EventPropagation::Stop
                })
                .expect("register");
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).scroll_target(target));

            let event = make_scroll_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                Offset::new(Pixels(0.0), Pixels(10.0)),
            );
            let PointerEvent::Scroll(event) = event else {
                panic!("expected scroll event");
            };
            let scroll = ScrollEventData::from(&event);
            assert!(result.dispatch_scroll(&scroll));
        });
        assert!(delivered.get());
    }

    #[test]
    fn dispatch_scroll_stops_at_first_claiming_target() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use crate::events::{PointerEvent, ScrollEventData, make_scroll_event};
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let order = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let first_order = Rc::clone(&order);
            let first = handle
                .register_scroll(move |_| {
                    first_order.borrow_mut().push("first");
                    EventPropagation::Stop
                })
                .expect("register first");
            let second_order = Rc::clone(&order);
            let second = handle
                .register_scroll(move |_| {
                    second_order.borrow_mut().push("second");
                    EventPropagation::Continue
                })
                .expect("register second");

            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).scroll_target(first));
            result.add(HitTestEntry::new(RenderId::new(2)).scroll_target(second));

            let event = make_scroll_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                Offset::new(Pixels(0.0), Pixels(10.0)),
            );
            let PointerEvent::Scroll(event) = event else {
                panic!("expected scroll event");
            };
            let scroll = ScrollEventData::from(&event);
            assert!(result.dispatch_scroll(&scroll));
        });
        assert_eq!(&*order.borrow(), &["first"]);
    }

    #[test]
    fn dispatch_pan_zoom_stops_at_the_first_claiming_target() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use crate::events::make_pinch_gesture_event;
        use crate::pan_zoom::from_w3c_event;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let order = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let leaf_order = Rc::clone(&order);
            let leaf = handle
                .register_pan_zoom(move |_| {
                    leaf_order.borrow_mut().push("leaf");
                    EventPropagation::Stop
                })
                .expect("register leaf");
            let root_order = Rc::clone(&order);
            let root = handle
                .register_pan_zoom(move |_| {
                    root_order.borrow_mut().push("root");
                    EventPropagation::Continue
                })
                .expect("register root");

            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pan_zoom_target(leaf));
            result.add(HitTestEntry::new(RenderId::new(2)).pan_zoom_target(root));

            let event = make_pinch_gesture_event(Offset::new(Pixels(50.0), Pixels(50.0)), 0.5);
            let pan_zoom = from_w3c_event(&event).expect("gesture converts");
            assert!(result.dispatch_pan_zoom(&pan_zoom));
        });
        assert_eq!(&*order.borrow(), &["leaf"]);
    }

    #[test]
    fn dispatch_pan_zoom_passes_an_unclaimed_tick_outward() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use crate::events::make_pinch_gesture_event;
        use crate::pan_zoom::from_w3c_event;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let order = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let leaf_order = Rc::clone(&order);
            let leaf = handle
                .register_pan_zoom(move |_| {
                    leaf_order.borrow_mut().push("leaf");
                    EventPropagation::Continue
                })
                .expect("register leaf");
            let root_order = Rc::clone(&order);
            let root = handle
                .register_pan_zoom(move |_| {
                    root_order.borrow_mut().push("root");
                    EventPropagation::Stop
                })
                .expect("register root");

            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pan_zoom_target(leaf));
            result.add(HitTestEntry::new(RenderId::new(2)).pan_zoom_target(root));

            let event = make_pinch_gesture_event(Offset::new(Pixels(50.0), Pixels(50.0)), 0.5);
            let pan_zoom = from_w3c_event(&event).expect("gesture converts");
            assert!(result.dispatch_pan_zoom(&pan_zoom));
        });
        assert_eq!(&*order.borrow(), &["leaf", "root"]);
    }

    #[test]
    fn dispatch_pan_zoom_localizes_pan_as_a_position_and_pan_delta_as_a_delta() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::make_pinch_gesture_event;
        use crate::pan_zoom::from_w3c_event;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(None));
        lane.enter(|| {
            let probe = Rc::clone(&observed);
            let target = handle
                .register_pan_zoom(move |event| {
                    if let PointerPanZoomEvent::Update { pan, pan_delta, .. } = *event {
                        probe.set(Some((pan, pan_delta)));
                    }
                    EventPropagation::Stop
                })
                .expect("register");

            let mut result = HitTestResult::new();
            // A subtree scaled 2x in paint: its global-to-local transform
            // halves. `pan` is a POSITION and `pan_delta` a DELTA anchored at
            // it, so both must halve here — the oracle's
            // `_TransformedPointerPanZoomUpdateEvent` maps `pan` through
            // `transformPosition` and `panDelta` through
            // `transformDeltaViaPositions` (`gestures/events.dart`).
            let mut forward = Matrix4::identity();
            forward.scale(2.0, 2.0, 1.0);
            result.with_paint_transform(forward, |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).pan_zoom_target(target));
            });

            // Build on a real converted tick so pointer identity and device
            // kind come from the production adapter, then supply the nonzero
            // pan payload that adapter cannot yet produce.
            let converted = from_w3c_event(&make_pinch_gesture_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                0.0,
            ))
            .expect("gesture converts");
            let PointerPanZoomEvent::Update {
                pointer_id,
                scale,
                rotation,
                timestamp_nanos,
                device_kind,
                ..
            } = converted
            else {
                panic!("convert_gesture yields an Update");
            };
            let event = PointerPanZoomEvent::Update {
                pointer_id,
                position: Offset::new(Pixels(50.0), Pixels(50.0)),
                pan: Offset::new(Pixels(40.0), Pixels(20.0)),
                pan_delta: Offset::new(Pixels(10.0), Pixels(4.0)),
                scale,
                rotation,
                timestamp_nanos,
                device_kind,
            };
            assert!(result.dispatch_pan_zoom(&event));
        });

        let (pan, pan_delta) = observed.get().expect("an Update reached the target");
        assert_eq!(pan, Offset::new(Pixels(20.0), Pixels(10.0)), "pan halves");
        assert_eq!(
            pan_delta,
            Offset::new(Pixels(5.0), Pixels(2.0)),
            "pan_delta halves with the subtree, not passed through in global space"
        );
    }

    #[test]
    fn dispatch_pan_zoom_applies_the_entry_local_transform() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::make_pinch_gesture_event;
        use crate::pan_zoom::from_w3c_event;
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(Offset::new(Pixels(0.0), Pixels(0.0))));
        lane.enter(|| {
            let position_probe = Rc::clone(&observed);
            let target = handle
                .register_pan_zoom(move |event| {
                    position_probe.set(event.position());
                    EventPropagation::Stop
                })
                .expect("register");
            let mut result = HitTestResult::new();
            // Same production `with_paint_offset` path the scroll and
            // pointer transform tests use: a claimant in a subtree
            // translated by (10, 20) must see the focal point in ITS space,
            // or it scales around a point that is not under the fingers.
            result.with_paint_offset(Offset::new(Pixels(10.0), Pixels(20.0)), |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).pan_zoom_target(target));
            });

            let event = make_pinch_gesture_event(Offset::new(Pixels(50.0), Pixels(50.0)), 0.5);
            let pan_zoom = from_w3c_event(&event).expect("gesture converts");
            assert!(result.dispatch_pan_zoom(&pan_zoom));
        });
        assert_eq!(observed.get(), Offset::new(Pixels(40.0), Pixels(30.0)));
    }

    #[test]
    fn dispatch_scroll_applies_the_entry_local_transform() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::events::{PointerEvent, ScrollEventData, make_scroll_event};
        use crate::routing::InteractionLane;

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(Offset::new(Pixels(0.0), Pixels(0.0))));
        lane.enter(|| {
            let position_probe = Rc::clone(&observed);
            let target = handle
                .register_scroll(move |event| {
                    position_probe.set(event.position);
                    EventPropagation::Stop
                })
                .expect("register");
            let mut result = HitTestResult::new();
            // See `dispatch_applies_the_entry_local_transform`: exercise the
            // production `with_paint_offset` path, not the raw push/pop pair,
            // so this proves the fixed (inverse-pushing) contract.
            result.with_paint_offset(Offset::new(Pixels(10.0), Pixels(20.0)), |result| {
                result.add(HitTestEntry::new(RenderId::new(1)).scroll_target(target));
            });

            let event = make_scroll_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                Offset::new(Pixels(0.0), Pixels(10.0)),
            );
            let PointerEvent::Scroll(event) = event else {
                panic!("expected scroll event");
            };
            let scroll = ScrollEventData::from(&event);
            assert!(result.dispatch_scroll(&scroll));
        });
        assert_eq!(observed.get(), Offset::new(Pixels(40.0), Pixels(30.0)));
    }
}
