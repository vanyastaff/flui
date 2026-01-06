//! RenderView - the root of the render tree.

use std::fmt::Debug;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};

use crate::hit_testing::{HitTestEntry, HitTestResult, HitTestTarget, PointerEvent};

use flui_types::{Matrix4, Offset, Rect, Size};

use crate::constraints::BoxConstraints;
use crate::parent_data::ParentData;

use super::ViewConfiguration;
use crate::context::CanvasContext;
use crate::pipeline::PipelineOwner;
use flui_layer::TransformLayer;

/// The root of the render tree.
///
/// The view represents the total output surface of the render tree and handles
/// bootstrapping the rendering pipeline.
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

    // ========================================================================
    // Render Object State
    // ========================================================================
    #[allow(dead_code)] // Placeholder for full RenderView implementation
    depth: usize,
    #[allow(dead_code)]
    needs_layout: bool,
    #[allow(dead_code)]
    needs_paint: bool,
    #[allow(dead_code)]
    needs_compositing_bits_update: bool,
    #[allow(dead_code)]
    needs_semantics_update: bool,
    #[allow(dead_code)]
    is_repaint_boundary: bool,
    #[allow(dead_code)]
    was_repaint_boundary: bool,
    #[allow(dead_code)]
    needs_compositing: bool,
    #[allow(dead_code)]
    cached_constraints: Option<BoxConstraints>,
    #[allow(dead_code)]
    parent_data: Option<Box<dyn ParentData>>,
}

impl Debug for RenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderView")
            .field("configuration", &self.configuration)
            .field("size", &self.size)
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
            size: Size::ZERO,
            root_transform: None,
            layer: None,
            automatic_system_ui_adjustment: true,
            owner: None,
            // RenderView is always a repaint boundary and needs compositing
            depth: 0,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            needs_semantics_update: false,
            is_repaint_boundary: true,
            was_repaint_boundary: true,
            needs_compositing: true,
            cached_constraints: None,
            parent_data: None,
        }
    }

    /// Creates a new render view with a configuration.
    pub fn with_configuration(configuration: ViewConfiguration) -> Self {
        let mut view = Self::new();
        view.configuration = Some(configuration);
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
                self.replace_root_layer_internal();
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
    fn replace_root_layer_internal(&mut self) {
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

    fn schedule_initial_layout_internal(&mut self) {
        self.needs_layout = true;
    }

    fn schedule_initial_paint_internal(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        self.needs_paint = true;
    }

    /// Prepare the initial frame without requiring a PipelineOwner.
    pub fn prepare_initial_frame_without_owner(&mut self) {
        if self.root_transform.is_some() {
            return;
        }
        self.schedule_initial_layout_internal();
        self.schedule_initial_paint_internal();
    }

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
        self.size = constraints.smallest();

        assert!(
            self.size.is_finite(),
            "RenderView size must be finite: {:?}",
            self.size
        );

        self.needs_layout = false;
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Determines the set of render objects located at the given position.
    pub fn hit_test(&self, result: &mut HitTestResult, _position: Offset) -> bool {
        result.add(HitTestEntry::new_render_view());
        true
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this render view.
    pub fn paint_view(&self, _context: &mut CanvasContext, _offset: Offset) {
        // No children to paint currently
    }

    /// Returns the paint bounds for this render view (in physical pixels).
    pub fn physical_paint_bounds(&self) -> Rect {
        let config = self.configuration();
        let dpr = config.device_pixel_ratio();
        Rect::from_ltwh(0.0, 0.0, self.size.width * dpr, self.size.height * dpr)
    }

    /// Returns the semantic bounds for this render view.
    pub fn semantic_bounds(&self) -> Rect {
        if let Some(transform) = &self.root_transform {
            let bounds = Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height);
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
    pub fn composite_frame(&self) -> CompositeResult {
        assert!(self.has_configuration());
        assert!(self.root_transform.is_some());
        assert!(self.layer.is_some());

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

// impl RenderObject for RenderView {
//     fn parent(&self) -> Option<&dyn RenderObject> {
//         None
//     }
//
//     fn depth(&self) -> usize {
//         self.depth
//     }
//
//     fn set_depth(&mut self, depth: usize) {
//         self.depth = depth;
//     }
//
//     fn owner(&self) -> Option<&PipelineOwner> {
//         self.owner.map(|p| unsafe { &*p })
//     }
//
//     fn set_parent(&mut self, _parent: Option<*const dyn RenderObject>) {
//         // RenderView is always the root
//     }
//
//     fn attach(&mut self, owner: &PipelineOwner) {
//         self.owner = Some(owner as *const PipelineOwner);
//     }
//
//     fn detach(&mut self) {
//         self.owner = None;
//     }
//
//     fn dispose(&mut self) {
//         self.layer = None;
//     }
//
//     fn adopt_child(&mut self, child: &mut dyn RenderObject) {
//         self.setup_parent_data(child);
//         self.needs_layout = true;
//         self.needs_compositing_bits_update = true;
//         self.needs_semantics_update = true;
//         child.set_parent(Some(self as *const dyn RenderObject));
//         if let Some(owner) = self.owner() {
//             child.attach(owner);
//         }
//         self.redepth_child(child);
//     }
//
//     fn drop_child(&mut self, child: &mut dyn RenderObject) {
//         child.set_parent(None);
//         if self.attached() {
//             child.detach();
//         }
//         self.needs_layout = true;
//         self.needs_compositing_bits_update = true;
//         self.needs_semantics_update = true;
//     }
//
//     fn redepth_child(&mut self, child: &mut dyn RenderObject) {
//         if child.depth() <= self.depth {
//             child.set_depth(self.depth + 1);
//             child.redepth_children();
//         }
//     }
//
//     fn needs_layout(&self) -> bool {
//         self.needs_layout
//     }
//
//     fn needs_paint(&self) -> bool {
//         self.needs_paint
//     }
//
//     fn needs_compositing_bits_update(&self) -> bool {
//         self.needs_compositing_bits_update
//     }
//
//     fn is_relayout_boundary(&self) -> bool {
//         true
//     }
//
//     fn mark_needs_layout(&mut self) {
//         self.needs_layout = true;
//     }
//
//     fn mark_needs_paint(&mut self) {
//         self.needs_paint = true;
//     }
//
//     fn mark_needs_compositing_bits_update(&mut self) {
//         self.needs_compositing_bits_update = true;
//     }
//
//     fn mark_needs_semantics_update(&mut self) {
//         self.needs_semantics_update = true;
//     }
//
//     fn clear_needs_layout(&mut self) {
//         self.needs_layout = false;
//     }
//
//     fn clear_needs_paint(&mut self) {
//         self.needs_paint = false;
//     }
//
//     fn clear_needs_compositing_bits_update(&mut self) {
//         self.needs_compositing_bits_update = false;
//     }
//
//     fn layout(&mut self, constraints: BoxConstraints, _parent_uses_size: bool) {
//         self.cached_constraints = Some(constraints);
//         self.perform_layout();
//     }
//
//     fn layout_without_resize(&mut self) {
//         self.perform_layout();
//     }
//
//     fn cached_constraints(&self) -> Option<BoxConstraints> {
//         self.cached_constraints
//     }
//
//     fn set_cached_constraints(&mut self, constraints: BoxConstraints) {
//         self.cached_constraints = Some(constraints);
//     }
//
//     fn mark_parent_needs_layout(&mut self) {
//         self.needs_layout = true;
//     }
//
//     fn schedule_initial_layout(&mut self) {
//         assert!(self.attached());
//         assert!(self.parent().is_none());
//         self.needs_layout = true;
//     }
//
//     fn schedule_initial_paint(&mut self) {
//         assert!(self.attached());
//         assert!(self.is_repaint_boundary());
//         self.needs_paint = true;
//     }
//
//     fn is_repaint_boundary(&self) -> bool {
//         self.is_repaint_boundary
//     }
//
//     fn was_repaint_boundary(&self) -> bool {
//         self.was_repaint_boundary
//     }
//
//     fn set_was_repaint_boundary(&mut self, value: bool) {
//         self.was_repaint_boundary = value;
//     }
//
//     fn needs_compositing(&self) -> bool {
//         self.needs_compositing
//     }
//
//     fn set_needs_compositing(&mut self, value: bool) {
//         self.needs_compositing = value;
//     }
//
//     fn has_layer(&self) -> bool {
//         self.layer.is_some()
//     }
//
//     fn layer_id(&self) -> Option<LayerId> {
//         None
//     }
//
//     fn replace_root_layer(&mut self) {
//         self.replace_root_layer_internal();
//     }
//
//     fn parent_data(&self) -> Option<&dyn ParentData> {
//         self.parent_data.as_ref().map(|p| p.as_ref())
//     }
//
//     fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
//         self.parent_data.as_mut().map(|p| p.as_mut())
//     }
//
//     fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
//         self.parent_data = Some(data);
//     }
//
//     fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
//         // No children
//     }
//
//     fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
//         // No children
//     }
//
//     fn paint_bounds(&self) -> Rect {
//         Rect::new(0.0, 0.0, self.size.width, self.size.height)
//     }
//
//     fn size(&self) -> Size {
//         self.size
//     }
//
//     fn paint(&self, context: &mut CanvasContext, offset: Offset) {
//         self.paint_view(context, offset);
//     }
// }

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

        assert!((transform[0] - 2.0).abs() < 1e-6);
        assert!((transform[5] - 2.0).abs() < 1e-6);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for RenderView {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("size", format!("{:?}", self.size));
        if let Some(ref config) = self.configuration {
            properties.add("devicePixelRatio", config.device_pixel_ratio());
        }
    }
}

impl HitTestTarget for RenderView {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        // RenderView doesn't handle events, just implements the trait
        let _ = (event, entry);
    }
}
