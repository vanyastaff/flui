//! RenderView - the root of the render tree.

use std::any::Any;
use std::fmt::Debug;

use flui_types::{Offset, Rect, Size};

use crate::constraints::{BoxConstraints, Constraints};

use super::ViewConfiguration;
use crate::hit_testing::{HitTestEntry, HitTestResult};
use crate::layer::TransformLayer;
use crate::parent_data::ParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::BoxHitTestResult;
use crate::traits::{RenderBox, RenderObject};

/// The root of the render tree.
///
/// The view represents the total output surface of the render tree and handles
/// bootstrapping the rendering pipeline. The view has a unique child
/// [`RenderBox`], which is required to fill the entire output surface.
///
/// # Bootstrapping Order
///
/// This object must be bootstrapped in a specific order:
///
/// 1. First, set the [`configuration`](Self::set_configuration)
/// 2. Second, [`attach`](RenderObject::attach) the object to a [`PipelineOwner`]
/// 3. Third, use [`prepare_initial_frame`](Self::prepare_initial_frame) to bootstrap
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `RenderView` class from `rendering/view.dart`.
pub struct RenderView {
    /// The view configuration.
    configuration: Option<ViewConfiguration>,

    /// The child render box.
    child: Option<Box<dyn RenderBox>>,

    /// The current size (in logical pixels).
    size: Size,

    /// The root transformation matrix.
    root_transform: Option<[f32; 16]>,

    /// The root layer.
    layer: Option<TransformLayer>,

    /// Whether automatic system UI adjustment is enabled.
    automatic_system_ui_adjustment: bool,

    /// The pipeline owner.
    owner: Option<*const PipelineOwner>,

    /// The depth in the tree (always 0 for root).
    depth: usize,

    /// Parent data (always None for root).
    parent_data: Option<Box<dyn ParentData>>,

    /// Whether layout is needed.
    needs_layout: bool,

    /// Whether paint is needed.
    needs_paint: bool,

    /// Whether compositing bits update is needed.
    needs_compositing_bits_update: bool,

    /// Whether semantics update is needed.
    needs_semantics_update: bool,

    /// Whether this render object or descendants need compositing.
    needs_compositing: bool,
}

impl Debug for RenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderView")
            .field("configuration", &self.configuration)
            .field("size", &self.size)
            .field("has_child", &self.child.is_some())
            .field("has_layer", &self.layer.is_some())
            .finish()
    }
}

// Safety: RenderView manages raw pointer to PipelineOwner carefully
unsafe impl Send for RenderView {}
unsafe impl Sync for RenderView {}

impl Default for RenderView {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView {
    /// Creates a new render view.
    ///
    /// The view starts without a configuration. You must call
    /// [`set_configuration`](Self::set_configuration) before using the view.
    pub fn new() -> Self {
        Self {
            configuration: None,
            child: None,
            size: Size::ZERO,
            root_transform: None,
            layer: None,
            automatic_system_ui_adjustment: true,
            owner: None,
            depth: 0,
            parent_data: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: true,
            needs_semantics_update: true,
            needs_compositing: true, // RenderView always needs compositing (is repaint boundary)
        }
    }

    /// Creates a new render view with a configuration.
    pub fn with_configuration(configuration: ViewConfiguration) -> Self {
        let mut view = Self::new();
        view.configuration = Some(configuration);
        view
    }

    /// Creates a new render view with a configuration and child.
    pub fn with_child(configuration: ViewConfiguration, child: Box<dyn RenderBox>) -> Self {
        let mut view = Self::with_configuration(configuration);
        view.child = Some(child);
        view
    }

    // ========================================================================
    // Configuration
    // ========================================================================

    /// Returns the current view configuration.
    ///
    /// # Panics
    ///
    /// Panics if no configuration has been set.
    pub fn configuration(&self) -> &ViewConfiguration {
        self.configuration
            .as_ref()
            .expect("RenderView configuration not set")
    }

    /// Returns whether a configuration has been set.
    pub fn has_configuration(&self) -> bool {
        self.configuration.is_some()
    }

    /// Sets the view configuration.
    ///
    /// This is typically called by the binding when the view is registered.
    pub fn set_configuration(&mut self, configuration: ViewConfiguration) {
        let old_configuration = self.configuration.take();

        if let Some(ref old) = old_configuration {
            if old == &configuration {
                self.configuration = old_configuration;
                return;
            }

            // Check if we need to update the root transform
            if self.root_transform.is_some() && configuration.should_update_matrix(old) {
                self.replace_root_layer();
            }
        }

        self.configuration = Some(configuration);

        if self.root_transform.is_some() {
            self.needs_layout = true;
        }
    }

    // ========================================================================
    // Size and Constraints
    // ========================================================================

    /// Returns the current size of the view (in logical pixels).
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the constraints for layout.
    ///
    /// # Panics
    ///
    /// Panics if no configuration has been set.
    pub fn constraints(&self) -> BoxConstraints {
        self.configuration().logical_constraints()
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Returns the child render box, if any.
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.as_ref().map(|c| c.as_ref())
    }

    /// Returns a mutable reference to the child render box, if any.
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.as_mut().map(|c| c.as_mut())
    }

    /// Sets the child render box.
    pub fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child = child;
        self.needs_layout = true;
    }

    // ========================================================================
    // Layer Management
    // ========================================================================

    /// Returns the root layer.
    pub fn layer(&self) -> Option<&TransformLayer> {
        self.layer.as_ref()
    }

    /// Returns a mutable reference to the root layer.
    pub fn layer_mut(&mut self) -> Option<&mut TransformLayer> {
        self.layer.as_mut()
    }

    /// Replaces the root layer with a new one.
    fn replace_root_layer(&mut self) {
        let new_layer = self.update_matrices_and_create_new_root_layer();
        self.layer = Some(new_layer);
    }

    /// Updates the transformation matrices and creates a new root layer.
    fn update_matrices_and_create_new_root_layer(&mut self) -> TransformLayer {
        let config = self.configuration();
        let matrix = config.to_matrix();
        self.root_transform = Some(matrix);
        TransformLayer::new(matrix)
    }

    // ========================================================================
    // System UI
    // ========================================================================

    /// Whether Flutter should automatically compute the desired system UI.
    ///
    /// When enabled, the system will hit-test the layer tree to find
    /// system UI overlay styles at the top and bottom of the screen.
    pub fn automatic_system_ui_adjustment(&self) -> bool {
        self.automatic_system_ui_adjustment
    }

    /// Sets whether automatic system UI adjustment is enabled.
    pub fn set_automatic_system_ui_adjustment(&mut self, value: bool) {
        self.automatic_system_ui_adjustment = value;
    }

    // ========================================================================
    // Initialization
    // ========================================================================

    /// Bootstrap the rendering pipeline by preparing the first frame.
    ///
    /// This should only be called once. It is typically called immediately after
    /// setting the configuration and attaching to a pipeline owner.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - No pipeline owner is attached
    /// - Already called before
    /// - No configuration is set
    pub fn prepare_initial_frame(&mut self) {
        assert!(
            self.owner.is_some(),
            "attach the RenderView to a PipelineOwner before calling prepare_initial_frame"
        );
        assert!(
            self.root_transform.is_none(),
            "prepare_initial_frame must only be called once"
        );
        assert!(
            self.has_configuration(),
            "set a configuration before calling prepare_initial_frame"
        );

        self.schedule_initial_layout();
        self.schedule_initial_paint();
    }

    /// Schedules the initial layout.
    fn schedule_initial_layout(&mut self) {
        self.needs_layout = true;
    }

    /// Schedules the initial paint and creates the root layer.
    fn schedule_initial_paint(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        self.needs_paint = true;
    }

    // ========================================================================
    // Layout
    // ========================================================================

    /// Performs layout on this render view.
    pub fn perform_layout(&mut self) {
        assert!(self.root_transform.is_some());

        let constraints = self.constraints();
        let sized_by_child = !constraints.is_tight();

        if let Some(child) = &mut self.child {
            let child_size = child.perform_layout(constraints);
            if sized_by_child {
                self.size = child_size;
            } else {
                self.size = constraints.smallest();
            }
        } else {
            self.size = constraints.smallest();
        }

        assert!(
            self.size.is_finite(),
            "RenderView size must be finite: {:?}",
            self.size
        );
        assert!(
            constraints.is_satisfied_by(self.size),
            "RenderView size {:?} does not satisfy constraints {:?}",
            self.size,
            constraints
        );

        self.needs_layout = false;
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Determines the set of render objects located at the given position.
    ///
    /// The `position` argument is in the coordinate system of the render view,
    /// which is in logical pixels.
    pub fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = &self.child {
            let mut box_result = BoxHitTestResult::new();
            child.hit_test(&mut box_result, position);
            // Note: In a full implementation, we would merge box_result into result
        }
        result.add(HitTestEntry::new_render_view());
        true
    }

    /// Performs box-specific hit testing.
    ///
    /// This is a convenience method that directly returns a BoxHitTestResult.
    pub fn hit_test_box(&self, position: Offset) -> BoxHitTestResult {
        let mut result = BoxHitTestResult::new();
        if let Some(child) = &self.child {
            child.hit_test(&mut result, position);
        }
        result
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this render view.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = &self.child {
            context.paint_child(child.as_ref(), offset);
        }
    }

    /// Returns the paint bounds for this render view.
    ///
    /// The bounds are in physical pixels (accounting for device pixel ratio).
    pub fn paint_bounds(&self) -> Rect {
        let config = self.configuration();
        let dpr = config.device_pixel_ratio();
        Rect::from_ltwh(0.0, 0.0, self.size.width * dpr, self.size.height * dpr)
    }

    /// Returns the semantic bounds for this render view.
    pub fn semantic_bounds(&self) -> Rect {
        if let Some(transform) = &self.root_transform {
            // Apply the root transform to the logical bounds
            let bounds = Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height);
            // Simplified: just scale by DPR (transform is diagonal)
            Rect::from_ltwh(
                bounds.min.x * transform[0],
                bounds.min.y * transform[5],
                bounds.width() * transform[0],
                bounds.height() * transform[5],
            )
        } else {
            Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
        }
    }

    // ========================================================================
    // Frame Composition
    // ========================================================================

    /// Uploads the composited layer tree to the engine.
    ///
    /// This actually causes the output of the rendering pipeline to appear
    /// on screen.
    ///
    /// # Panics
    ///
    /// Panics if the view has not been properly initialized.
    pub fn composite_frame(&self) -> CompositeResult {
        assert!(
            self.has_configuration(),
            "set the RenderView configuration before calling composite_frame"
        );
        assert!(
            self.root_transform.is_some(),
            "call prepare_initial_frame before calling composite_frame"
        );
        assert!(
            self.layer.is_some(),
            "call prepare_initial_frame before calling composite_frame"
        );

        let config = self.configuration();
        let physical_size = config.to_physical_size(self.size);

        CompositeResult {
            physical_size,
            logical_size: self.size,
            device_pixel_ratio: config.device_pixel_ratio(),
        }
    }

    // ========================================================================
    // Transforms
    // ========================================================================

    /// Applies the paint transform for a child.
    pub fn apply_paint_transform(&self, transform: &mut [f32; 16]) {
        if let Some(root_transform) = &self.root_transform {
            // Multiply transform by root_transform
            multiply_matrices(transform, root_transform);
        }
    }
}

// ============================================================================
// RenderObject Implementation
// ============================================================================

impl RenderObject for RenderView {
    fn parent(&self) -> Option<&dyn RenderObject> {
        None // RenderView is the root
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        self.owner.map(|p| unsafe { &*p })
    }

    fn set_parent(&mut self, _parent: Option<*const dyn RenderObject>) {
        // RenderView is always the root, so parent is always None
        // This is a no-op for RenderView
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        self.owner = Some(owner as *const PipelineOwner);

        // Attach child if present
        if let Some(child) = &mut self.child {
            child.attach(owner);
        }
    }

    fn detach(&mut self) {
        // Detach child if present
        if let Some(child) = &mut self.child {
            child.detach();
        }

        self.owner = None;
    }

    fn dispose(&mut self) {
        self.layer = None;
        self.child = None;
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    fn mark_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = true;
    }

    fn mark_needs_semantics_update(&mut self) {
        self.needs_semantics_update = true;
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint
    }

    fn needs_compositing_bits_update(&self) -> bool {
        self.needs_compositing_bits_update
    }

    fn clear_needs_layout(&mut self) {
        self.needs_layout = false;
    }

    fn clear_needs_paint(&mut self) {
        self.needs_paint = false;
    }

    fn clear_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = false;
    }

    fn adopt_child(&mut self, child: &mut dyn RenderObject) {
        self.setup_parent_data(child);
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
        self.mark_needs_semantics_update();
        child.set_parent(Some(self as *const dyn RenderObject));
        if let Some(owner) = self.owner() {
            child.attach(owner);
        }
        self.redepth_child(child);
    }

    fn drop_child(&mut self, child: &mut dyn RenderObject) {
        child.set_parent(None);
        if self.attached() {
            child.detach();
        }
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
        self.mark_needs_semantics_update();
    }

    fn redepth_child(&mut self, child: &mut dyn RenderObject) {
        if child.depth() <= self.depth {
            child.set_depth(self.depth + 1);
            child.redepth_children();
        }
    }

    fn redepth_children(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let my_depth = self.depth;
            if child.depth() <= my_depth {
                child.set_depth(my_depth + 1);
                child.redepth_children();
            }
        }
    }

    fn is_repaint_boundary(&self) -> bool {
        true // RenderView is always a repaint boundary
    }

    fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_ref().map(|p| p.as_ref())
    }

    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_mut().map(|p| p.as_mut())
    }

    fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.parent_data = Some(data);
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(child.as_mut());
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn mark_parent_needs_layout(&mut self) {
        // RenderView is the root, so no parent to mark
        // Just mark self as needing layout
        self.needs_layout = true;
    }

    fn schedule_initial_layout(&mut self) {
        assert!(
            self.attached(),
            "RenderView must be attached before scheduling initial layout"
        );
        assert!(
            self.parent().is_none(),
            "RenderView must be root to schedule initial layout"
        );
        self.needs_layout = true;
        // In a real implementation, this would add to owner's nodes needing layout
    }

    fn schedule_initial_paint(&mut self) {
        assert!(
            self.attached(),
            "RenderView must be attached before scheduling initial paint"
        );
        assert!(
            self.is_repaint_boundary(),
            "RenderView must be a repaint boundary"
        );
        self.needs_paint = true;
        // In a real implementation, this would add to owner's nodes needing paint
    }

    fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    fn set_needs_compositing(&mut self, value: bool) {
        self.needs_compositing = value;
    }

    fn paint_bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.size.width, self.size.height)
    }
}

// ============================================================================
// CompositeResult
// ============================================================================

/// The result of compositing a frame.
#[derive(Debug, Clone)]
pub struct CompositeResult {
    /// The physical size of the frame (in device pixels).
    pub physical_size: Size,

    /// The logical size of the frame (in logical pixels).
    pub logical_size: Size,

    /// The device pixel ratio.
    pub device_pixel_ratio: f32,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Multiplies two 4x4 matrices (column-major order).
fn multiply_matrices(a: &mut [f32; 16], b: &[f32; 16]) {
    let result = [
        a[0] * b[0] + a[4] * b[1] + a[8] * b[2] + a[12] * b[3],
        a[1] * b[0] + a[5] * b[1] + a[9] * b[2] + a[13] * b[3],
        a[2] * b[0] + a[6] * b[1] + a[10] * b[2] + a[14] * b[3],
        a[3] * b[0] + a[7] * b[1] + a[11] * b[2] + a[15] * b[3],
        a[0] * b[4] + a[4] * b[5] + a[8] * b[6] + a[12] * b[7],
        a[1] * b[4] + a[5] * b[5] + a[9] * b[6] + a[13] * b[7],
        a[2] * b[4] + a[6] * b[5] + a[10] * b[6] + a[14] * b[7],
        a[3] * b[4] + a[7] * b[5] + a[11] * b[6] + a[15] * b[7],
        a[0] * b[8] + a[4] * b[9] + a[8] * b[10] + a[12] * b[11],
        a[1] * b[8] + a[5] * b[9] + a[9] * b[10] + a[13] * b[11],
        a[2] * b[8] + a[6] * b[9] + a[10] * b[10] + a[14] * b[11],
        a[3] * b[8] + a[7] * b[9] + a[11] * b[10] + a[15] * b[11],
        a[0] * b[12] + a[4] * b[13] + a[8] * b[14] + a[12] * b[15],
        a[1] * b[12] + a[5] * b[13] + a[9] * b[14] + a[13] * b[15],
        a[2] * b[12] + a[6] * b[13] + a[10] * b[14] + a[14] * b[15],
        a[3] * b[12] + a[7] * b[13] + a[11] * b[14] + a[15] * b[15],
    ];
    *a = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_view_new() {
        let view = RenderView::new();
        assert!(!view.has_configuration());
        assert!(view.child().is_none());
        assert_eq!(view.size(), Size::ZERO);
    }

    #[test]
    fn test_render_view_with_configuration() {
        let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 2.0);
        let view = RenderView::with_configuration(config.clone());
        assert!(view.has_configuration());
        assert_eq!(view.configuration(), &config);
    }

    #[test]
    fn test_render_view_constraints() {
        let config = ViewConfiguration::from_size(Size::new(1920.0, 1080.0), 2.0);
        let view = RenderView::with_configuration(config);
        let constraints = view.constraints();
        assert_eq!(constraints, BoxConstraints::tight(Size::new(960.0, 540.0)));
    }

    #[test]
    fn test_render_view_is_repaint_boundary() {
        let view = RenderView::new();
        assert!(view.is_repaint_boundary());
    }

    #[test]
    fn test_render_view_depth() {
        let view = RenderView::new();
        assert_eq!(view.depth(), 0);
    }

    #[test]
    fn test_render_view_parent_is_none() {
        let view = RenderView::new();
        assert!(view.parent().is_none());
    }

    #[test]
    fn test_render_view_automatic_system_ui() {
        let mut view = RenderView::new();
        assert!(view.automatic_system_ui_adjustment());

        view.set_automatic_system_ui_adjustment(false);
        assert!(!view.automatic_system_ui_adjustment());
    }

    #[test]
    fn test_multiply_matrices_identity() {
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut a = identity;
        multiply_matrices(&mut a, &identity);
        assert_eq!(a, identity);
    }

    #[test]
    fn test_multiply_matrices_scale() {
        let mut a = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let scale = [
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        multiply_matrices(&mut a, &scale);
        assert!((a[0] - 2.0).abs() < 1e-6);
        assert!((a[5] - 3.0).abs() < 1e-6);
    }
}
