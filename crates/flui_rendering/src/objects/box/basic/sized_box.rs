//! RenderSizedBox - forces a specific size on its child.
//!
//! This render object forces its child to have a specific width and/or height.
//! Unlike ConstrainedBox, it always uses tight constraints for the specified
//! dimensions.

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::{Offset, Size};

use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::lifecycle::BaseRenderObject;
use crate::parent_data::BoxParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{BoxHitTestResult, RenderBox, RenderObject, TextBaseline};

/// A render object that forces a specific size.
///
/// If width or height is None, that dimension uses the child's size
/// (or 0 if there's no child).
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderConstrainedBox` with tight constraints.
/// Like Flutter, this stores child directly and delegates size to child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderSizedBox;
///
/// // Fixed 100x100 size
/// let mut sized = RenderSizedBox::new(Some(100.0), Some(100.0));
///
/// // Only fixed width
/// let mut sized = RenderSizedBox::new(Some(100.0), None);
/// ```
#[derive(Debug)]
pub struct RenderSizedBox {
    /// Base render object for lifecycle management.
    base: BaseRenderObject,

    /// The child render object using type-safe container.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// The fixed width, if any.
    width: Option<f32>,

    /// The fixed height, if any.
    height: Option<f32>,
}

impl RenderSizedBox {
    /// Creates a new sized box with optional fixed dimensions.
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            base: BaseRenderObject::new(),
            child: BoxChild::new(),
            size: Size::ZERO,
            width,
            height,
        }
    }

    /// Creates a sized box with both dimensions fixed.
    pub fn fixed(width: f32, height: f32) -> Self {
        Self::new(Some(width), Some(height))
    }

    /// Creates an expand box that fills available space.
    pub fn expand() -> Self {
        Self::new(Some(f32::INFINITY), Some(f32::INFINITY))
    }

    /// Creates a shrink box that takes minimum space.
    pub fn shrink() -> Self {
        Self::new(Some(0.0), Some(0.0))
    }

    /// Creates a sized box with a child.
    pub fn with_child(
        width: Option<f32>,
        height: Option<f32>,
        mut child: Box<dyn RenderBox>,
    ) -> Self {
        let mut this = Self::new(width, height);
        Self::setup_child_parent_data(&mut *child);
        this.child.set(child);
        this
    }

    /// Creates a fixed sized box with a child.
    pub fn fixed_with_child(width: f32, height: f32, mut child: Box<dyn RenderBox>) -> Self {
        Self::with_child(Some(width), Some(height), child)
    }

    /// Sets up BoxParentData on a child.
    fn setup_child_parent_data(child: &mut dyn RenderBox) {
        let needs_setup = child
            .parent_data()
            .map(|pd| pd.as_any().downcast_ref::<BoxParentData>().is_none())
            .unwrap_or(true);

        if needs_setup {
            child.set_parent_data(Box::new(BoxParentData::default()));
        }
    }

    // ========================================================================
    // Child access (using type-safe BoxChild container)
    // ========================================================================

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    /// Sets the child.
    pub fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child.clear();
        if let Some(c) = child {
            self.child.set(c);
        }
    }

    /// Takes the child out of the container.
    pub fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }

    // ========================================================================
    // Size configuration
    // ========================================================================

    /// Returns the fixed width, if any.
    pub fn width(&self) -> Option<f32> {
        self.width
    }

    /// Sets the fixed width.
    pub fn set_width(&mut self, width: Option<f32>) {
        if self.width != width {
            self.width = width;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the fixed height, if any.
    pub fn height(&self) -> Option<f32> {
        self.height
    }

    /// Sets the fixed height.
    pub fn set_height(&mut self, height: Option<f32>) {
        if self.height != height {
            self.height = height;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Computes the effective constraints.
    ///
    /// - None: pass through parent constraints (loose)
    /// - Some(INFINITY): use max from parent (expand)
    /// - Some(0.0): use min from parent (shrink)
    /// - Some(value): use the fixed value (clamped to parent)
    fn get_effective_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        let (min_width, max_width) = match self.width {
            Some(w) if w == f32::INFINITY => (constraints.max_width, constraints.max_width),
            Some(w) => {
                let clamped = w.clamp(constraints.min_width, constraints.max_width);
                (clamped, clamped)
            }
            None => (constraints.min_width, constraints.max_width),
        };
        let (min_height, max_height) = match self.height {
            Some(h) if h == f32::INFINITY => (constraints.max_height, constraints.max_height),
            Some(h) => {
                let clamped = h.clamp(constraints.min_height, constraints.max_height);
                (clamped, clamped)
            }
            None => (constraints.min_height, constraints.max_height),
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Performs layout without a child.
    /// Performs layout without a child (legacy helper - prefer using RenderBox::perform_layout).
    pub fn layout_without_child(&mut self, constraints: BoxConstraints) -> Size {
        let effective = self.get_effective_constraints(constraints);
        self.size = effective.constrain(Size::new(
            self.width.unwrap_or(0.0),
            self.height.unwrap_or(0.0),
        ));
        self.size
    }

    /// Performs layout with a child size (legacy helper - prefer using RenderBox::perform_layout).
    pub fn layout_with_child_size(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let effective = self.get_effective_constraints(constraints);
        self.size = effective.constrain(Size::new(
            self.width.unwrap_or(child_size.width),
            self.height.unwrap_or(child_size.height),
        ));
        self.size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.get_effective_constraints(constraints)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset
        let _ = (context, offset);
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        self.width.unwrap_or_else(|| child_width.unwrap_or(0.0))
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        self.height.unwrap_or_else(|| child_height.unwrap_or(0.0))
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        self.compute_min_intrinsic_height(width, child_height)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

// ============================================================================
// RenderObject trait implementation
// ============================================================================

impl RenderObject for RenderSizedBox {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
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

impl RenderBox for RenderSizedBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let effective = self.get_effective_constraints(constraints);

        if let Some(child) = self.child.get_mut() {
            let child_constraints = effective;
            let child_size = child.perform_layout(child_constraints);
            self.size = effective.constrain(Size::new(
                self.width.unwrap_or(child_size.width),
                self.height.unwrap_or(child_size.height),
            ));
        } else {
            self.size = effective.constrain(Size::new(
                self.width.unwrap_or(0.0),
                self.height.unwrap_or(0.0),
            ));
        }
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.get() {
            context.paint_child(child, offset);
        }
    }

    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child.get() {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        let child_width = self.child.get().map(|c| c.get_min_intrinsic_width(height));
        self.width.unwrap_or_else(|| child_width.unwrap_or(0.0))
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        let child_width = self.child.get().map(|c| c.get_max_intrinsic_width(height));
        self.width.unwrap_or_else(|| child_width.unwrap_or(0.0))
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        let child_height = self.child.get().map(|c| c.get_min_intrinsic_height(width));
        self.height.unwrap_or_else(|| child_height.unwrap_or(0.0))
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        let child_height = self.child.get().map(|c| c.get_max_intrinsic_height(width));
        self.height.unwrap_or_else(|| child_height.unwrap_or(0.0))
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.child
            .get()
            .and_then(|c| c.get_distance_to_baseline(baseline, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sized_box_fixed() {
        let mut sized = RenderSizedBox::fixed(100.0, 50.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        // Use trait method to avoid name conflict with inherent method
        let size = RenderBox::perform_layout(&mut sized, constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_sized_box_shrink() {
        let mut sized = RenderSizedBox::shrink();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        // Use trait method to avoid name conflict with inherent method
        let size = RenderBox::perform_layout(&mut sized, constraints);

        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_sized_box_expand() {
        let mut sized = RenderSizedBox::expand();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);

        // Use trait method to avoid name conflict with inherent method
        let size = RenderBox::perform_layout(&mut sized, constraints);

        assert_eq!(size, Size::new(200.0, 150.0));
    }

    #[test]
    fn test_sized_box_partial() {
        let mut sized = RenderSizedBox::new(Some(100.0), None);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        // Use trait method to avoid name conflict with inherent method
        let size = RenderBox::perform_layout(&mut sized, constraints);

        // Width is fixed, height is 0 (no child)
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn test_sized_box_intrinsics() {
        let sized = RenderSizedBox::fixed(100.0, 50.0);

        assert_eq!(sized.compute_min_intrinsic_width(0.0, None), 100.0);
        assert_eq!(sized.compute_max_intrinsic_width(0.0, None), 100.0);
        assert_eq!(sized.compute_min_intrinsic_height(0.0, None), 50.0);
        assert_eq!(sized.compute_max_intrinsic_height(0.0, None), 50.0);
    }

    #[test]
    fn test_layout_with_child_size() {
        let mut sized = RenderSizedBox::new(Some(100.0), None);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let child_size = Size::new(50.0, 75.0);

        let size = sized.layout_with_child_size(constraints, child_size);

        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 75.0);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for RenderSizedBox {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("width", format!("{:?}", self.width));
        properties.add("height", format!("{:?}", self.height));
    }
}

impl HitTestTarget for RenderSizedBox {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}
