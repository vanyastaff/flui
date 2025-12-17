//! RenderOpacity - applies alpha transparency to its child.
//!
//! This render object multiplies its child's opacity by a given value,
//! creating transparency effects.
//!
//! # Flutter Hierarchy
//!
//! ```text
//! RenderObject
//!     └── RenderBox
//!         └── SingleChildRenderBox
//!             └── RenderProxyBox
//!                 └── RenderOpacity
//! ```
//!
//! # Architecture
//!
//! Following Flutter's `RenderProxyBox` pattern:
//! - Child stored directly (not in container)
//! - Size equals child's size (pass-through)
//! - Child painted at same offset (no offset in parentData)

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::{Offset, Size};

use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::lifecycle::BaseRenderObject;
use crate::parent_data::ParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{
    BoxHitTestEntry, BoxHitTestResult, HitTestBehavior, RenderBox, RenderObject, RenderProxyBox,
    SingleChildRenderBox, TextBaseline,
};

/// Simple parent data for proxy boxes.
///
/// RenderProxyBox doesn't need BoxParentData since child is painted
/// at the same position as the parent.
#[derive(Debug, Default)]
struct SimpleParentData;

impl ParentData for SimpleParentData {}

/// A render object that applies opacity to its child.
///
/// Opacity values should be between 0.0 (fully transparent) and 1.0 (fully opaque).
/// Values outside this range are clamped.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderOpacity` class which extends
/// `RenderProxyBox`.
///
/// # Trait Chain
///
/// RenderObject → RenderBox → SingleChildRenderBox → RenderProxyBox
///
/// # Polymorphism
///
/// `RenderOpacity` can be used as:
/// - `Box<dyn RenderObject>` - for generic render tree operations
/// - `Box<dyn RenderBox>` - for box layout operations
/// - `Box<dyn SingleChildRenderBox>` - for single-child operations
/// - `Box<dyn RenderProxyBox>` - for proxy box operations
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderOpacity;
///
/// // 50% opacity
/// let mut opacity = RenderOpacity::new(0.5);
///
/// // Fully transparent (invisible)
/// let mut hidden = RenderOpacity::new(0.0);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Single child using type-safe container.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// The opacity value (0.0 to 1.0).
    opacity: f32,

    /// Whether the child should be included in hit testing when invisible.
    always_include_semantics: bool,
}

impl Default for RenderOpacity {
    fn default() -> Self {
        Self::opaque()
    }
}

impl RenderOpacity {
    /// Creates a new opacity render object.
    ///
    /// The opacity is clamped to [0.0, 1.0].
    pub fn new(opacity: f32) -> Self {
        Self {
            child: BoxChild::new(),
            size: Size::ZERO,
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
        }
    }

    /// Creates a new opacity render object with a child.
    pub fn with_child(opacity: f32, child: Box<dyn RenderBox>) -> Self {
        let mut child = child;
        Self::setup_child_parent_data(&mut *child);
        Self {
            child: BoxChild::with(child),
            size: Size::ZERO,
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
        }
    }

    /// Creates a fully opaque render object.
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully transparent render object.
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// The value is clamped to [0.0, 1.0].
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() > f32::EPSILON {
            self.opacity = clamped;
            self.mark_needs_paint();
        }
    }

    /// Returns whether semantics are always included.
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Sets whether semantics should always be included.
    pub fn set_always_include_semantics(&mut self, value: bool) {
        if self.always_include_semantics != value {
            self.always_include_semantics = value;
            // In full implementation: self.mark_needs_semantics_update();
        }
    }

    /// Returns whether the child is effectively invisible.
    pub fn is_invisible(&self) -> bool {
        self.opacity < 0.001
    }

    /// Returns whether the opacity creates any effect.
    pub fn is_opaque(&self) -> bool {
        self.opacity > 0.999
    }

    /// Sets up SimpleParentData on a child.
    fn setup_child_parent_data(child: &mut dyn RenderBox) {
        let needs_setup = child
            .parent_data()
            .map(|pd| pd.as_any().downcast_ref::<SimpleParentData>().is_none())
            .unwrap_or(true);

        if needs_setup {
            child.set_parent_data(Box::new(SimpleParentData));
        }
    }
}

// ============================================================================
// RenderObject trait implementation
// ============================================================================

impl RenderObject for RenderOpacity {
    fn base(&self) -> &BaseRenderObject {
        unimplemented!("RenderOpacity::base() - need BaseRenderObject storage")
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        unimplemented!("RenderOpacity::base_mut() - need BaseRenderObject storage")
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        None
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        if let Some(child) = self.child.get_mut() {
            child.attach(owner);
        }
    }

    fn detach(&mut self) {
        if let Some(child) = self.child.get_mut() {
            child.detach();
        }
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {}

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {}

    fn redepth_child(&mut self, _child: &mut dyn RenderObject) {}

    fn mark_parent_needs_layout(&mut self) {}

    fn schedule_initial_layout(&mut self) {}

    fn schedule_initial_paint(&mut self) {}

    fn paint_bounds(&self) -> flui_types::Rect {
        flui_types::Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = self.child.get() {
            visitor(child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = self.child.get_mut() {
            visitor(child);
        }
    }
}

// ============================================================================
// RenderBox trait implementation
// ============================================================================

impl RenderBox for RenderOpacity {
    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Performs layout using RenderProxyBox pattern.
    ///
    /// Size equals child's size (pass-through).
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Use RenderProxyBox default implementation
        self.size = self.proxy_perform_layout(constraints);
        self.size
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.is_invisible() {
            // Don't paint anything when fully transparent
            return;
        }

        if self.is_opaque() {
            // Paint child directly without opacity layer
            self.proxy_paint(context, offset);
        } else {
            // Paint child through opacity layer
            // In full implementation:
            // context.push_opacity(offset, (self.opacity * 255.0) as i32, |ctx, off| {
            //     self.proxy_paint(ctx, off);
            // });
            self.proxy_paint(context, offset);
        }
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Don't hit test invisible children unless always_include_semantics
        if self.is_invisible() && !self.always_include_semantics {
            return false;
        }

        // Delegate to default implementation from trait
        let size = self.size();
        if position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
        {
            let child_hit = self.hit_test_children(result, position);
            let self_hit = self.hit_test_self(position);

            match self.hit_test_behavior() {
                HitTestBehavior::DeferToChild => {
                    if child_hit {
                        result.add(BoxHitTestEntry::new(position));
                    }
                    child_hit
                }
                HitTestBehavior::Opaque => {
                    result.add(BoxHitTestEntry::new(position));
                    true
                }
                HitTestBehavior::Translucent => {
                    result.add(BoxHitTestEntry::new(position));
                    child_hit || self_hit
                }
            }
        } else {
            false
        }
    }

    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Use RenderProxyBox implementation
        self.proxy_hit_test_children(result, position)
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.proxy_compute_min_intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.proxy_compute_max_intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.proxy_compute_min_intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.proxy_compute_max_intrinsic_height(width)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.proxy_compute_distance_to_actual_baseline(baseline)
    }
}

// ============================================================================
// SingleChildRenderBox trait implementation
// ============================================================================

impl SingleChildRenderBox for RenderOpacity {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child.clear();

        if let Some(mut new_child) = child {
            Self::setup_child_parent_data(&mut *new_child);
            self.child.set(new_child);
        }

        self.mark_needs_layout();
    }

    fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }
}

// ============================================================================
// RenderProxyBox trait implementation
// ============================================================================

impl RenderProxyBox for RenderOpacity {
    // All methods use default implementations from the trait
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert!((opacity.opacity() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_opacity_clamping() {
        let under = RenderOpacity::new(-0.5);
        assert!((under.opacity() - 0.0).abs() < f32::EPSILON);

        let over = RenderOpacity::new(1.5);
        assert!((over.opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_opacity_opaque() {
        let opaque = RenderOpacity::opaque();
        assert!(opaque.is_opaque());
        assert!(!opaque.is_invisible());
    }

    #[test]
    fn test_opacity_transparent() {
        let transparent = RenderOpacity::transparent();
        assert!(transparent.is_invisible());
        assert!(!transparent.is_opaque());
    }

    #[test]
    fn test_layout_no_child() {
        let mut opacity = RenderOpacity::new(0.5);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);

        let size = opacity.perform_layout(constraints);

        // Without child, size is smallest (0, 0)
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_trait_polymorphism() {
        let opacity = RenderOpacity::new(0.5);

        // Should compile - RenderOpacity implements all these traits
        let _: &dyn RenderObject = &opacity;
        let _: &dyn RenderBox = &opacity;
        let _: &dyn SingleChildRenderBox = &opacity;
        let _: &dyn RenderProxyBox = &opacity;
    }

    #[test]
    fn test_default() {
        let opacity = RenderOpacity::default();
        assert!(opacity.is_opaque());
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for RenderOpacity {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("opacity", format!("{:.2}", self.opacity));
        if self.always_include_semantics {
            properties.add("alwaysIncludeSemantics", true);
        }
    }
}

impl HitTestTarget for RenderOpacity {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}
