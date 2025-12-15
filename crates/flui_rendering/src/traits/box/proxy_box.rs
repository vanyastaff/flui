//! RenderProxyBox trait - single child where size equals child size.

use ambassador::delegatable_trait;
use flui_types::{Offset, Rect, Size};

use crate::constraints::BoxConstraints;
use crate::parent_data::ParentData;
use crate::traits::RenderObject;

use super::{BoxHitTestResult, SingleChildRenderBox, TextBaseline};
use crate::pipeline::PaintingContext;

/// Trait for render boxes that pass through to a single child.
///
/// RenderProxyBox is used for render objects that:
/// - Apply visual effects (opacity, color filters)
/// - Apply transformations
/// - Simply wrap a child without changing size
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderProxyBox` and `RenderProxyBoxMixin` in Flutter.
///
/// # Default Behavior
///
/// All methods delegate to the child by default:
/// - `perform_layout`: Returns child's size or `constraints.smallest()`
/// - `paint`: Paints the child at the given offset
/// - `hit_test_children`: Delegates to child
/// - Intrinsic dimensions: Delegates to child
#[delegatable_trait]
pub trait RenderProxyBox: SingleChildRenderBox {
    // ========================================================================
    // Parent Data Setup
    // ========================================================================

    /// Sets up parent data for a child.
    ///
    /// RenderProxyBox uses simple ParentData since it doesn't need
    /// offset information (child is painted at the same position).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.setupParentData`.
    fn proxy_setup_parent_data(&self, child: &mut dyn RenderObject) {
        // ProxyBox doesn't use BoxParentData because we don't need offset
        // Just ensure some ParentData exists
        if child.parent_data().is_none() {
            child.set_parent_data(Box::new(SimpleParentData));
        }
    }

    // ========================================================================
    // Layout
    // ========================================================================
    /// Performs layout by delegating to the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.performLayout`.
    fn proxy_perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(|| self.compute_size_for_no_child(constraints))
    }

    /// Computes the size when there is no child.
    ///
    /// By default returns `constraints.smallest()`. Subclasses can override
    /// this to provide different behavior.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.computeSizeForNoChild`.
    fn compute_size_for_no_child(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints by delegating to the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.paint`.
    fn proxy_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }

    /// Applies paint transform for the child.
    ///
    /// RenderProxyBox uses identity transform since child is painted
    /// at the same position.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.applyPaintTransform`.
    fn proxy_apply_paint_transform(&self, _child: &dyn RenderObject, _transform: &mut [f32; 16]) {
        // Identity transform - child is at the same position
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests by delegating to the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.hitTestChildren`.
    fn proxy_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.child()
            .map(|c| c.hit_test(result, position))
            .unwrap_or(false)
    }

    /// Computes min intrinsic width by delegating to child.
    fn proxy_compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes max intrinsic width by delegating to child.
    fn proxy_compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes min intrinsic height by delegating to child.
    fn proxy_compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Computes max intrinsic height by delegating to child.
    fn proxy_compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Computes dry layout by delegating to child.
    fn proxy_compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child()
            .map(|c| c.compute_dry_layout(constraints))
            .unwrap_or_else(|| constraints.smallest())
    }

    /// Computes baseline by delegating to child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.computeDistanceToActualBaseline`.
    fn proxy_compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.child()
            .and_then(|c| c.compute_distance_to_actual_baseline(baseline))
    }

    /// Computes dry baseline by delegating to child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxMixin.computeDryBaseline`.
    fn proxy_compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
    ) -> Option<f32> {
        self.child()
            .and_then(|c| c.compute_dry_baseline(constraints, baseline))
    }

    // ========================================================================
    // Semantics
    // ========================================================================

    /// Returns the semantic bounds by delegating to child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's semantic bounds handling in proxy boxes.
    fn proxy_semantic_bounds(&self) -> Rect {
        self.child()
            .map(|c| c.semantic_bounds())
            .unwrap_or(Rect::ZERO)
    }
}

/// Simple parent data for proxy boxes.
///
/// RenderProxyBox doesn't need offset information since the child
/// is painted at the same position as the parent.
#[derive(Debug, Default)]
pub struct SimpleParentData;

impl ParentData for SimpleParentData {
    fn detach(&mut self) {
        // Nothing to detach
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// How to behave during hit testing.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `HitTestBehavior` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Targets that defer to their children receive events within their bounds
    /// only if one of their children is hit by the hit test.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `HitTestBehavior.deferToChild`.
    #[default]
    DeferToChild,

    /// Opaque targets can be hit by hit tests, causing them to both receive
    /// events within their bounds and prevent targets visually behind them from
    /// also receiving events.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `HitTestBehavior.opaque`.
    Opaque,

    /// Translucent targets both receive events within their bounds and permit
    /// targets visually behind them to also receive events.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `HitTestBehavior.translucent`.
    Translucent,
}

impl HitTestBehavior {
    /// Returns whether this behavior allows the hit to pass through.
    pub fn allows_pass_through(&self) -> bool {
        matches!(self, Self::Translucent)
    }

    /// Returns whether this behavior is opaque (absorbs all hits).
    pub fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque)
    }

    /// Returns whether this behavior defers to children.
    pub fn defers_to_child(&self) -> bool {
        matches!(self, Self::DeferToChild)
    }
}

// ============================================================================
// RenderProxyBoxWithHitTestBehavior
// ============================================================================

/// A RenderProxyBox subclass that allows customizing hit-testing behavior.
///
/// This trait extends `RenderProxyBox` with customizable hit test behavior.
/// Use this for render objects that need to control how hit tests propagate
/// to children and whether to consider targets behind this one.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderProxyBoxWithHitTestBehavior` class.
///
/// # Example
///
/// ```ignore
/// impl RenderProxyBoxWithHitTestBehavior for MyRenderObject {
///     fn behavior(&self) -> HitTestBehavior {
///         self.hit_test_behavior
///     }
///
///     fn set_behavior(&mut self, behavior: HitTestBehavior) {
///         self.hit_test_behavior = behavior;
///     }
/// }
/// ```
pub trait RenderProxyBoxWithHitTestBehavior: RenderProxyBox {
    /// Returns the hit test behavior for this render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxWithHitTestBehavior.behavior` getter.
    fn behavior(&self) -> HitTestBehavior;

    /// Sets the hit test behavior for this render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxWithHitTestBehavior.behavior` setter.
    fn set_behavior(&mut self, behavior: HitTestBehavior);

    /// Performs hit testing with the configured behavior.
    ///
    /// This method implements the hit test logic based on the behavior:
    /// - `DeferToChild`: Only hit if a child is hit
    /// - `Opaque`: Always hit if position is within bounds
    /// - `Translucent`: Hit and allow pass-through
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxWithHitTestBehavior.hitTest`.
    fn hit_test_with_behavior(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
        size: Size,
    ) -> bool {
        let mut hit_target = false;

        // Check if position is within bounds
        if position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
        {
            // Test children and self
            hit_target = self.proxy_hit_test_children(result, position)
                || self.proxy_hit_test_self(position);

            // Add to result based on behavior
            if hit_target || self.behavior() == HitTestBehavior::Translucent {
                result.add(super::BoxHitTestEntry::new(position));
            }
        }

        hit_target
    }

    /// Tests whether this render object itself is hit.
    ///
    /// Returns true for `Opaque` behavior, false otherwise.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderProxyBoxWithHitTestBehavior.hitTestSelf`.
    fn proxy_hit_test_self(&self, _position: Offset) -> bool {
        self.behavior() == HitTestBehavior::Opaque
    }
}
