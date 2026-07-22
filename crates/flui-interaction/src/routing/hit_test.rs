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

use crate::{
    events::{CursorIcon, PointerEvent, ScrollEventData},
    routing::MouseTrackerAnnotation,
    routing::interaction_lane::{PointerTarget, ScrollTarget, active_dispatch_handle},
};

// ============================================================================
// EVENT PROPAGATION (scroll arbitration only)
// ============================================================================

/// Scroll-claim propagation control.
///
/// Ordinary pointer delivery has no propagation result: every hit target
/// receives its locally transformed event in leaf-first order (ADR-0027,
/// Flutter `GestureBinding.dispatchEvent` parity). Only the pointer-signal /
/// scroll resolver keeps a claiming result, mirroring Flutter's separate
/// `PointerSignalResolver` arbitration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventPropagation {
    /// Keep dispatching the scroll event to the remaining entries.
    #[default]
    Continue,
    /// Claim the scroll event; entries deeper on the route do not see it.
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

    /// Transform from global to local coordinates.
    /// Set automatically when added to HitTestResult.
    pub transform: Option<Matrix4>,

    /// Data-plane identity of this target's owner-local pointer handler.
    pub pointer_target: Option<PointerTarget>,

    /// Data-plane identity of this target's owner-local scroll handler.
    pub scroll_target: Option<ScrollTarget>,

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

    /// Pushes a transform matrix onto the stack.
    ///
    /// Flutter equivalent: `@protected void pushTransform(Matrix4 transform)`
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.local_transforms.push(TransformPart::Matrix(transform));
    }

    /// Pushes an offset translation onto the stack.
    ///
    /// Flutter equivalent: `@protected void pushOffset(Offset offset)`
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
        self.push_offset(offset);
        let result = f(self);
        self.pop_transform();
        result
    }

    /// Runs `f` with `transform` pushed onto the transform stack and
    /// pops the transform before returning.
    ///
    /// See [`with_paint_offset`](Self::with_paint_offset) for the
    /// Flutter-parity rationale and the closure-vs-guard discussion
    /// (closure-vs-guard rationale); this is the matrix-typed sibling
    /// for callers that need a full 4x4 transform rather than a
    /// paint-offset.
    pub fn with_paint_transform<F, R>(&mut self, transform: Matrix4, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.push_transform(transform);
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
        if !self.path.iter().any(|e| e.pointer_target.is_some()) {
            return;
        }
        let handle = match active_dispatch_handle() {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "pointer dispatch outside an active interaction lane; event not delivered"
                );
                return;
            }
        };
        let resolution = match handle.resolve_pointer_route(&self.path) {
            Ok(resolution) => resolution,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "pointer route resolution failed; event not delivered"
                );
                return;
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
        if let Err(error) = handle.release_route(token) {
            tracing::error!(?error, "failed to release ephemeral pointer route");
        }
        match delivery {
            Ok(Some(panic)) => panic.resume(),
            Ok(None) => {}
            Err(error) => {
                tracing::error!(?error, "pointer route invocation failed");
            }
        }
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
                    if let Some(inverse) = transform.try_inverse() {
                        transform_scroll_event(event, &inverse)
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
        // Cancel, Enter, Leave don't have position - just clone
        other => other.clone(),
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

        result.push_offset(Offset::new(Pixels(10.0), Pixels(20.0)));
        result.add(HitTestEntry::new(RenderId::new(1)));

        // Entry should have captured the transform
        assert!(result.path()[0].transform.is_some());

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
            result.push_offset(Offset::new(Pixels(10.0), Pixels(20.0)));
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
            result.pop_transform();

            let event = crate::events::make_down_event(
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            result.dispatch(&event);
        });
        assert_eq!(observed.get(), Offset::new(Pixels(40.0), Pixels(30.0)));
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
            result.push_offset(Offset::new(Pixels(10.0), Pixels(20.0)));
            result.add(HitTestEntry::new(RenderId::new(1)).scroll_target(target));
            result.pop_transform();

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
