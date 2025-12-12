//! Proxy box trait for pass-through render objects

use crate::traits::r#box::SingleChildRenderBox;
use crate::traits::{BoxHitTestResult, PaintingContext, TextBaseline};
use crate::constraints::BoxConstraints;
use crate::geometry::Size;
use flui_types::Offset;

/// Trait for render boxes where parent size equals child size
///
/// RenderProxyBox is used for render objects that simply pass their constraints
/// to their child and adopt the child's size. This is common for effects like
/// opacity, transforms, and clipping that don't change layout.
///
/// # Key Characteristic
///
/// The defining feature of a proxy box is:
/// ```ignore
/// parent.size == child.size
/// ```
///
/// # Ambassador Support
///
/// This trait works seamlessly with ambassador delegation:
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(RenderProxyBox, target = "proxy")]
/// struct RenderOpacity {
///     proxy: ProxyBox,
///     opacity: f32,
/// }
///
/// // Marker trait implementation
/// impl RenderProxyBox for RenderOpacity {}
///
/// // Automatically get SingleChildRenderBox + RenderBox!
/// ```
///
/// # Default Implementations
///
/// This trait provides complete default implementations for:
/// - Layout (pass constraints to child, adopt child's size)
/// - Hit testing (delegate to child)
/// - Painting (paint child at same offset)
/// - Intrinsic dimensions (delegate to child)
///
/// Override only what you need to customize (typically just `paint`).
pub trait RenderProxyBox: SingleChildRenderBox {
    // Note: child() and child_mut() are inherited from SingleChildRenderBox
    // No need to duplicate them here to avoid ambiguity

    // ============================================================
    // RenderBox methods with proxy-specific defaults
    // ============================================================

    /// Layout by passing constraints to child and adopting its size
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = self.child_mut() {
            child.perform_layout(constraints)
        } else {
            constraints.smallest()
        }
    }

    /// Returns the current size of this render object
    fn size(&self) -> Size;

    /// Paint child at the same offset
    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }

    /// Hit test delegates to child
    fn hit_test(&self, result: &mut dyn BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Hit test self (default: false)
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Hit testing delegates to child at same position
    fn hit_test_children(&self, result: &mut dyn BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Minimum intrinsic width delegates to child
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Maximum intrinsic width delegates to child
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Minimum intrinsic height delegates to child
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Maximum intrinsic height delegates to child
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Baseline distance delegates to child
    fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.child()
            .and_then(|c| c.compute_distance_to_baseline(baseline))
    }

    /// Dry layout delegates to child
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child()
            .map(|c| c.compute_dry_layout(constraints))
            .unwrap_or_else(|| constraints.smallest())
    }

    // Note: RenderObject methods (depth, attached, attach, detach, etc.)
    // are inherited from the RenderBox -> RenderObject trait chain
    // No need to duplicate them here
}
