//! RenderView - the root of the render tree.

use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use parking_lot::RwLock;

use flui_types::{Matrix4, Offset, Rect, Size};

use crate::constraints::{BoxConstraints, Constraints};
use crate::lifecycle::BaseRenderObject;

use super::ViewConfiguration;
use crate::hit_testing::{HitTestEntry, HitTestResult};
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::BoxHitTestResult;
use crate::traits::{RenderBox, RenderObject};
use flui_layer::TransformLayer;

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
    /// Base render object state (lifecycle, dirty flags, parent data, debug info).
    base: BaseRenderObject,

    /// The view configuration.
    configuration: Option<ViewConfiguration>,

    /// The child render box (owned version - legacy).
    child: Option<Box<dyn RenderBox>>,

    /// The child render box (shared version - for Flutter-like element tree).
    /// This is used when the child RenderObject is owned by an Element
    /// but needs to be accessible from the render tree.
    child_shared: Option<Arc<RwLock<dyn RenderBox>>>,

    /// The current size (in logical pixels).
    size: Size,

    /// The root transformation matrix.
    root_transform: Option<Matrix4>,

    /// The root layer.
    layer: Option<TransformLayer>,

    /// Whether automatic system UI adjustment is enabled.
    automatic_system_ui_adjustment: bool,

    /// The pipeline owner (raw pointer for direct access).
    owner: Option<*const PipelineOwner>,
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
        let mut base = BaseRenderObject::new();
        // RenderView is always a repaint boundary and needs compositing
        base.state_mut().set_repaint_boundary(true);
        base.state_mut().set_needs_compositing(true);

        Self {
            base,
            configuration: None,
            child: None,
            child_shared: None,
            size: Size::ZERO,
            root_transform: None,
            layer: None,
            automatic_system_ui_adjustment: true,
            owner: None,
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
            self.base.mark_needs_layout();
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

    /// Sets the child render box (owned version).
    pub fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child = child;
        self.child_shared = None; // Clear shared child when setting owned
        self.base.mark_needs_layout();
    }

    /// Sets the child render box (shared version).
    ///
    /// This is used when the child RenderObject is owned by an Element
    /// but needs to be referenced from the render tree for layout/paint.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, RenderObjects are stored in Elements and referenced
    /// in the parent's child field. This method enables the same pattern.
    pub fn set_child_shared(&mut self, child: Option<Arc<RwLock<dyn RenderBox>>>) {
        self.child_shared = child;
        self.child = None; // Clear owned child when setting shared
        self.base.mark_needs_layout();
    }

    /// Returns the shared child, if any.
    pub fn child_shared(&self) -> Option<&Arc<RwLock<dyn RenderBox>>> {
        self.child_shared.as_ref()
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

        self.schedule_initial_layout_internal();
        self.schedule_initial_paint_internal();
    }

    /// Schedules the initial layout.
    fn schedule_initial_layout_internal(&mut self) {
        self.base.mark_needs_layout();
    }

    /// Schedules the initial paint and creates the root layer.
    fn schedule_initial_paint_internal(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        self.base.mark_needs_paint();
    }

    /// Prepare the initial frame without requiring a PipelineOwner.
    ///
    /// This is useful when bootstrapping the render tree before
    /// the PipelineOwner is fully attached.
    pub fn prepare_initial_frame_without_owner(&mut self) {
        if self.root_transform.is_some() {
            // Already prepared
            return;
        }
        self.schedule_initial_layout_internal();
        self.schedule_initial_paint_internal();
    }

    /// Internal method for testing.
    #[cfg(test)]
    fn prepare_initial_frame_internal(&mut self) {
        self.prepare_initial_frame_without_owner();
    }

    // ========================================================================
    // Layout
    // ========================================================================

    /// Performs layout on this render view.
    pub fn perform_layout(&mut self) {
        assert!(self.root_transform.is_some());

        let constraints = self.constraints();
        let sized_by_child = !constraints.is_tight();

        // Try owned child first, then shared child
        let child_size = if let Some(child) = &mut self.child {
            Some(child.perform_layout(constraints))
        } else if let Some(child_shared) = &self.child_shared {
            let mut child = child_shared.write();
            Some(child.perform_layout(constraints))
        } else {
            None
        };

        if let Some(child_size) = child_size {
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

        self.base.clear_needs_layout();
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Determines the set of render objects located at the given position.
    ///
    /// The `position` argument is in the coordinate system of the render view,
    /// which is in logical pixels.
    pub fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Try owned child first, then shared child
        if let Some(child) = &self.child {
            let mut box_result = BoxHitTestResult::new();
            child.hit_test(&mut box_result, position);
            // Note: In a full implementation, we would merge box_result into result
        } else if let Some(child_shared) = &self.child_shared {
            let child = child_shared.read();
            let mut box_result = BoxHitTestResult::new();
            child.hit_test(&mut box_result, position);
        }
        result.add(HitTestEntry::new_render_view());
        true
    }

    /// Performs box-specific hit testing.
    ///
    /// This is a convenience method that directly returns a BoxHitTestResult.
    pub fn hit_test_box(&self, position: Offset) -> BoxHitTestResult {
        let mut result = BoxHitTestResult::new();
        // Try owned child first, then shared child
        if let Some(child) = &self.child {
            child.hit_test(&mut result, position);
        } else if let Some(child_shared) = &self.child_shared {
            let child = child_shared.read();
            child.hit_test(&mut result, position);
        }
        result
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this render view.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Try owned child first, then shared child
        if let Some(child) = &self.child {
            context.paint_child(child.as_ref(), offset);
        } else if let Some(child_shared) = &self.child_shared {
            let child = child_shared.read();
            context.paint_child(&*child, offset);
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
            // Get scale factors from the matrix diagonal (column-major: [0]=m00, [5]=m11)
            let scale_x = transform[0];
            let scale_y = transform[5];
            Rect::from_ltwh(
                bounds.min.x * scale_x,
                bounds.min.y * scale_y,
                bounds.width() * scale_x,
                bounds.height() * scale_y,
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
    pub fn apply_paint_transform(&self, transform: &mut Matrix4) {
        if let Some(root_transform) = &self.root_transform {
            *transform = *root_transform * *transform;
        }
    }
}

// ============================================================================
// RenderObject Implementation
// ============================================================================

impl RenderObject for RenderView {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
    }

    fn parent(&self) -> Option<&dyn RenderObject> {
        None // RenderView is the root
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

    fn adopt_child(&mut self, child: &mut dyn RenderObject) {
        self.setup_parent_data(child);
        self.base.mark_needs_layout();
        self.base.mark_needs_compositing_bits_update();
        self.base.mark_needs_semantics_update();
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
        self.base.mark_needs_layout();
        self.base.mark_needs_compositing_bits_update();
        self.base.mark_needs_semantics_update();
    }

    fn redepth_child(&mut self, child: &mut dyn RenderObject) {
        let my_depth = self.base.depth();
        if child.depth() <= my_depth {
            child.set_depth(my_depth + 1);
            child.redepth_children();
        }
    }

    fn redepth_children(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let my_depth = self.base.depth();
            if child.depth() <= my_depth {
                child.set_depth(my_depth + 1);
                child.redepth_children();
            }
        }
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
        self.base.mark_needs_layout();
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
        self.base.mark_needs_layout();
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
        self.base.mark_needs_paint();
        // In a real implementation, this would add to owner's nodes needing paint
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
    fn test_apply_paint_transform() {
        let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame_internal();

        let mut transform = Matrix4::identity();
        view.apply_paint_transform(&mut transform);

        // Should have DPR scaling applied (column-major: [0] = scale_x, [5] = scale_y)
        assert!((transform[0] - 2.0).abs() < 1e-6);
        assert!((transform[5] - 2.0).abs() < 1e-6);
    }
}
