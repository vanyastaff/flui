//! Box and Sliver Hit Test Types (Flutter Model)
//!
//! This module provides protocol-specific hit test types following Flutter's
//! architecture where `BoxHitTestResult` is defined in `rendering/box.dart`
//! and `SliverHitTestResult` in `rendering/sliver.dart`.
//!
//! # Architecture
//!
//! ```text
//! flui_interaction (gestures layer)
//!     └─ HitTestResult (base)
//!     └─ HitTestEntry (base)
//!
//! flui_rendering (rendering layer)
//!     └─ BoxHitTestResult (box protocol)
//!     └─ BoxHitTestEntry (box protocol)
//!     └─ SliverHitTestResult (sliver protocol)
//! ```
//!
//! # Flutter Equivalence
//!
//! - `BoxHitTestResult` → `rendering/box.dart`
//! - `BoxHitTestEntry` → `rendering/box.dart`
//! - `SliverHitTestResult` → `rendering/sliver.dart`

use std::ops::{Deref, DerefMut};

use flui_foundation::RenderId;
use flui_interaction::{HitTestEntry, HitTestResult, PointerEventHandler};
use flui_types::geometry::{Matrix4, Offset, Rect};

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Box hit test callback signature (Flutter's BoxHitTest typedef).
///
/// # Flutter Equivalence
///
/// ```dart
/// typedef BoxHitTest = bool Function(BoxHitTestResult result, Offset position);
/// ```
#[allow(dead_code)]
pub type BoxHitTest<'a> = &'a mut dyn FnMut(&mut BoxHitTestResult, Offset) -> bool;

/// Sliver hit test callback signature.
///
/// # Flutter Equivalence
///
/// ```dart
/// typedef SliverHitTest = bool Function(SliverHitTestResult result, double mainAxisPosition, double crossAxisPosition);
/// ```
#[allow(dead_code)]
pub type SliverHitTest<'a> = &'a mut dyn FnMut(&mut SliverHitTestResult, f64) -> bool;

// ============================================================================
// BOX HIT TEST ENTRY (Flutter's BoxHitTestEntry)
// ============================================================================

/// Hit test entry for RenderBox.
///
/// Flutter equivalent: `BoxHitTestEntry extends HitTestEntry<RenderBox>`
#[derive(Clone)]
pub struct BoxHitTestEntry {
    /// Base entry.
    pub entry: HitTestEntry,

    /// Local position within the target.
    pub local_position: Offset,

    /// Bounds of the target (for debugging).
    pub bounds: Rect,
}

impl std::fmt::Debug for BoxHitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxHitTestEntry")
            .field("target", &self.entry.target)
            .field("local_position", &self.local_position)
            .field("bounds", &self.bounds)
            .finish()
    }
}

impl BoxHitTestEntry {
    /// Creates a new box hit test entry.
    pub fn new(target: RenderId, local_position: Offset, bounds: Rect) -> Self {
        Self {
            entry: HitTestEntry::new(target),
            local_position,
            bounds,
        }
    }

    /// Creates entry with a handler.
    pub fn with_handler(
        target: RenderId,
        local_position: Offset,
        bounds: Rect,
        handler: PointerEventHandler,
    ) -> Self {
        Self {
            entry: HitTestEntry::with_handler(target, handler),
            local_position,
            bounds,
        }
    }
}

impl Deref for BoxHitTestEntry {
    type Target = HitTestEntry;

    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

impl DerefMut for BoxHitTestEntry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entry
    }
}

// ============================================================================
// BOX HIT TEST RESULT (Flutter's BoxHitTestResult)
// ============================================================================

/// Hit test result for RenderBox.
///
/// Flutter equivalent: `class BoxHitTestResult extends HitTestResult`
///
/// Provides convenience methods for hit testing box children with transforms.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    inner: HitTestResult,
}

impl BoxHitTestResult {
    /// Creates an empty box hit test result.
    pub fn new() -> Self {
        Self {
            inner: HitTestResult::new(),
        }
    }

    /// Wraps a HitTestResult.
    ///
    /// Flutter equivalent: `BoxHitTestResult.wrap(HitTestResult result)`
    pub fn wrap(result: HitTestResult) -> Self {
        Self { inner: result }
    }

    /// Returns the inner HitTestResult.
    pub fn into_inner(self) -> HitTestResult {
        self.inner
    }

    /// Adds a BoxHitTestEntry.
    pub fn add_box_entry(&mut self, entry: BoxHitTestEntry) {
        self.inner.add(entry.entry);
    }

    /// Hit tests a child with paint transform.
    ///
    /// Flutter equivalent:
    /// ```dart
    /// bool addWithPaintTransform({
    ///   required Matrix4? transform,
    ///   required Offset position,
    ///   required BoxHitTest hitTest,
    /// })
    /// ```
    pub fn add_with_paint_transform<F>(
        &mut self,
        transform: Option<Matrix4>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let Some(transform) = transform else {
            return hit_test(self, position);
        };

        // Remove perspective and invert
        let Some(inverse) = transform.try_inverse() else {
            return false;
        };

        self.add_with_raw_transform(Some(inverse), position, hit_test)
    }

    /// Hit tests a child with paint offset.
    ///
    /// Flutter equivalent:
    /// ```dart
    /// bool addWithPaintOffset({
    ///   required Offset? offset,
    ///   required Offset position,
    ///   required BoxHitTest hitTest,
    /// })
    /// ```
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let transformed_position = match offset {
            Some(o) => position - o,
            None => position,
        };

        if let Some(o) = offset {
            self.inner.push_offset(-o);
        }

        let is_hit = hit_test(self, transformed_position);

        if offset.is_some() {
            self.inner.pop_transform();
        }

        is_hit
    }

    /// Hit tests a child with raw transform (already inverted).
    ///
    /// Flutter equivalent:
    /// ```dart
    /// bool addWithRawTransform({
    ///   required Matrix4? transform,
    ///   required Offset position,
    ///   required BoxHitTest hitTest,
    /// })
    /// ```
    pub fn add_with_raw_transform<F>(
        &mut self,
        transform: Option<Matrix4>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let transformed_position = match &transform {
            Some(t) => {
                let (x, y) = t.transform_point(position.dx, position.dy);
                Offset::new(x, y)
            }
            None => position,
        };

        if let Some(t) = transform {
            self.inner.push_transform(t);
        }

        let is_hit = hit_test(self, transformed_position);

        if transform.is_some() {
            self.inner.pop_transform();
        }

        is_hit
    }
}

impl Deref for BoxHitTestResult {
    type Target = HitTestResult;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for BoxHitTestResult {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// ============================================================================
// SLIVER HIT TEST RESULT (Flutter's SliverHitTestResult)
// ============================================================================

/// Hit test result for RenderSliver.
///
/// Flutter equivalent: `class SliverHitTestResult extends HitTestResult`
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    inner: HitTestResult,
}

impl SliverHitTestResult {
    /// Creates an empty sliver hit test result.
    pub fn new() -> Self {
        Self {
            inner: HitTestResult::new(),
        }
    }

    /// Wraps a HitTestResult.
    pub fn wrap(result: HitTestResult) -> Self {
        Self { inner: result }
    }

    /// Wraps a BoxHitTestResult.
    pub fn wrap_box(result: BoxHitTestResult) -> Self {
        Self {
            inner: result.into_inner(),
        }
    }

    /// Returns the inner HitTestResult.
    pub fn into_inner(self) -> HitTestResult {
        self.inner
    }

    /// Hit tests a child with axis offset.
    ///
    /// Flutter equivalent:
    /// ```dart
    /// bool addWithAxisOffset({
    ///   required Offset? paintOffset,
    ///   required double mainAxisOffset,
    ///   required double crossAxisOffset,
    ///   required SliverHitTest hitTest,
    /// })
    /// ```
    pub fn add_with_axis_offset<F>(
        &mut self,
        paint_offset: Option<Offset>,
        main_axis_position: f64,
        _cross_axis_position: f64,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut SliverHitTestResult, f64) -> bool,
    {
        if let Some(offset) = paint_offset {
            self.inner.push_offset(-offset);
        }

        let is_hit = hit_test(self, main_axis_position);

        if paint_offset.is_some() {
            self.inner.pop_transform();
        }

        is_hit
    }
}

impl Deref for SliverHitTestResult {
    type Target = HitTestResult;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SliverHitTestResult {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_hit_test_result_new() {
        let result = BoxHitTestResult::new();
        assert!(result.is_empty());
    }

    #[test]
    fn test_box_hit_test_result_add_with_paint_offset() {
        let mut result = BoxHitTestResult::new();

        let hit = result.add_with_paint_offset(
            Some(Offset::new(10.0, 10.0)),
            Offset::new(50.0, 50.0),
            |r, pos| {
                assert_eq!(pos, Offset::new(40.0, 40.0)); // 50 - 10
                r.add(HitTestEntry::new(RenderId::new(1)));
                true
            },
        );

        assert!(hit);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_box_hit_test_result_add_with_paint_transform() {
        let mut result = BoxHitTestResult::new();

        let transform = Matrix4::translation(10.0, 20.0, 0.0);
        let hit =
            result.add_with_paint_transform(Some(transform), Offset::new(50.0, 60.0), |r, pos| {
                // Position should be transformed by inverse
                assert!((pos.dx - 40.0).abs() < 0.001);
                assert!((pos.dy - 40.0).abs() < 0.001);
                r.add(HitTestEntry::new(RenderId::new(1)));
                true
            });

        assert!(hit);
    }

    #[test]
    fn test_sliver_hit_test_result() {
        let mut result = SliverHitTestResult::new();

        let hit = result.add_with_axis_offset(
            Some(Offset::new(0.0, 100.0)),
            50.0,
            0.0,
            |r, main_axis| {
                r.add(HitTestEntry::new(RenderId::new(1)));
                main_axis < 100.0
            },
        );

        assert!(hit);
    }

    #[test]
    fn test_box_hit_test_entry() {
        let entry = BoxHitTestEntry::new(
            RenderId::new(1),
            Offset::new(10.0, 20.0),
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        );

        assert_eq!(entry.local_position, Offset::new(10.0, 20.0));
        assert_eq!(entry.target, RenderId::new(1));
    }
}
