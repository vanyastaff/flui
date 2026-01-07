//! RenderView - the root of the render tree.
//!
//! The view represents the total output surface of the render tree and handles
//! bootstrapping the rendering pipeline.
//!
//! # Bootstrapping Order
//!
//! 1. Set the [`configuration`](RenderView::set_configuration)
//! 2. [`attach`](RenderView::attach) to a [`PipelineOwner`]
//! 3. Call [`prepare_initial_frame`](RenderView::prepare_initial_frame)
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `RenderView` class from `rendering/view.dart`.
//!
//! # Child Management
//!
//! Unlike Flutter's RenderView which uses `RenderObjectWithChildMixin<RenderBox>`,
//! FLUI's RenderView currently operates as a leaf node. Child management will be
//! added when the storage system supports dynamic child relationships.

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_layer::TransformLayer;
use flui_types::{Matrix4, Offset, Point, Rect, Size};

use crate::arity::Leaf;
use crate::constraints::BoxConstraints;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::hit_testing::{HitTestEntry, HitTestResult, HitTestTarget, PointerEvent};
use crate::parent_data::BoxParentData;
use crate::pipeline::PipelineOwner;
use crate::traits::RenderBox;

use super::ViewConfiguration;

/// The root of the render tree.
///
/// RenderView represents the total output surface and bootstraps the rendering pipeline.
pub struct RenderView {
    /// The view configuration (logical/physical constraints, DPI).
    configuration: Option<ViewConfiguration>,

    /// Current size after layout (in logical pixels).
    size: Size,

    /// Root transformation matrix (applies devicePixelRatio).
    root_transform: Option<Matrix4>,

    /// Root compositing layer.
    layer: Option<TransformLayer>,

    /// Whether to automatically adjust system UI overlays.
    automatic_system_ui_adjustment: bool,

    /// Pipeline owner reference.
    owner: Option<*const PipelineOwner>,
}

// Safety: RenderView carefully manages the PipelineOwner pointer
unsafe impl Send for RenderView {}
unsafe impl Sync for RenderView {}

impl std::fmt::Debug for RenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderView")
            .field("configuration", &self.configuration)
            .field("size", &self.size)
            .field("has_layer", &self.layer.is_some())
            .finish()
    }
}

impl Default for RenderView {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView {
    /// Creates a new RenderView without a configuration.
    pub fn new() -> Self {
        Self {
            configuration: None,
            size: Size::ZERO,
            root_transform: None,
            layer: None,
            automatic_system_ui_adjustment: true,
            owner: None,
        }
    }

    /// Creates a RenderView with the given configuration.
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
    /// Updates the root transform if the devicePixelRatio changed.
    pub fn set_configuration(&mut self, configuration: ViewConfiguration) {
        let old_configuration = self.configuration.take();

        if let Some(ref old) = old_configuration {
            if old == &configuration {
                self.configuration = old_configuration;
                return;
            }

            // Update root transform if DPI changed
            if self.root_transform.is_some() && configuration.should_update_matrix(old) {
                self.replace_root_layer();
            }
        }

        self.configuration = Some(configuration);

        // Mark needs layout if already initialized
        if self.root_transform.is_some() {
            // TODO: mark_needs_layout() when RenderObject trait is implemented
        }
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
    fn replace_root_layer(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
    }

    /// Updates the transformation matrix and creates a new root layer.
    fn update_matrices_and_create_new_root_layer(&mut self) -> TransformLayer {
        let config = self.configuration();
        let matrix = config.to_matrix();
        self.root_transform = Some(matrix);
        TransformLayer::new(matrix)
    }

    // ========================================================================
    // System UI
    // ========================================================================

    /// Whether to automatically compute system UI overlays.
    pub fn automatic_system_ui_adjustment(&self) -> bool {
        self.automatic_system_ui_adjustment
    }

    /// Sets automatic system UI adjustment.
    pub fn set_automatic_system_ui_adjustment(&mut self, value: bool) {
        self.automatic_system_ui_adjustment = value;
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Attaches this view to a PipelineOwner.
    pub fn attach(&mut self, owner: &PipelineOwner) {
        self.owner = Some(owner as *const PipelineOwner);
    }

    /// Detaches this view from the PipelineOwner.
    pub fn detach(&mut self) {
        self.owner = None;
    }

    /// Bootstrap the rendering pipeline by preparing the first frame.
    ///
    /// Must be called after setting configuration and attaching to owner.
    ///
    /// # Panics
    ///
    /// - If no pipeline owner is attached
    /// - If already called before
    /// - If no configuration is set
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

        // Initialize root layer
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        // TODO: schedule_initial_layout() and schedule_initial_paint() when RenderObject is ready
    }

    /// Prepare initial frame without owner (for testing).
    #[cfg(test)]
    fn prepare_initial_frame_for_test(&mut self) {
        if self.root_transform.is_none() {
            self.layer = Some(self.update_matrices_and_create_new_root_layer());
        }
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Determines render objects located at the given position.
    ///
    /// Returns true and adds this view to the result.
    pub fn hit_test_view(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check if position is within bounds
        if position.dx < 0.0
            || position.dx > self.size.width
            || position.dy < 0.0
            || position.dy > self.size.height
        {
            return false;
        }

        // Always add self to result
        result.add(HitTestEntry::new_render_view());
        true
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Returns paint bounds in logical pixels.
    pub fn paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    /// Returns paint bounds in physical pixels (scaled by DPI).
    pub fn physical_paint_bounds(&self) -> Rect {
        let config = self.configuration();
        let dpr = config.device_pixel_ratio();
        Rect::from_ltwh(0.0, 0.0, self.size.width * dpr, self.size.height * dpr)
    }

    /// Returns semantic bounds (transformed by root matrix).
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

    /// Applies the root paint transform to the given transform.
    ///
    /// This multiplies the device pixel ratio transform.
    pub fn apply_paint_transform(&self, transform: &mut Matrix4) {
        if let Some(root_transform) = &self.root_transform {
            // Flutter uses: transform.multiply(_rootTransform!)
            // In Rust with matrix multiplication: result = left * right
            *transform = *root_transform * *transform;
        }
    }

    // ========================================================================
    // Frame Composition
    // ========================================================================

    /// Uploads the composited layer tree to the engine.
    ///
    /// Returns physical size and metadata for rendering to the screen.
    pub fn composite_frame(&self) -> CompositeResult {
        assert!(self.has_configuration(), "configuration must be set");
        assert!(
            self.root_transform.is_some(),
            "call prepare_initial_frame first"
        );
        assert!(self.layer.is_some(), "layer must exist");

        let config = self.configuration();
        let physical_size = config.to_physical_size(self.size);

        CompositeResult {
            physical_size,
            logical_size: self.size,
            device_pixel_ratio: config.device_pixel_ratio(),
        }
    }
}

// ============================================================================
// RenderBox Implementation
// ============================================================================

impl Diagnosticable for RenderView {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("size", format!("{:?}", self.size));
        if let Some(ref config) = self.configuration {
            properties.add("devicePixelRatio", config.device_pixel_ratio());
        }
    }
}

impl RenderBox for RenderView {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        assert!(
            self.root_transform.is_some(),
            "call prepare_initial_frame first"
        );

        let constraints = ctx.constraints();

        // RenderView uses the smallest size from constraints
        // In Flutter, it would layout a child and potentially use child's size
        // For now, as a Leaf, we just use constraints.smallest()
        self.size = constraints.smallest();

        assert!(self.size.is_finite(), "RenderView size must be finite");
        assert!(
            constraints.is_satisfied_by(self.size),
            "RenderView size must satisfy constraints"
        );

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {
        // RenderView doesn't paint anything itself
        // In the future, when it has a child, it would paint the child here
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        // Check if position is within bounds
        ctx.is_within_size(self.size.width, self.size.height)
    }

    fn box_paint_bounds(&self) -> Rect {
        self.paint_bounds()
    }
}

// ============================================================================
// CompositeResult
// ============================================================================

/// Result of compositing a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeResult {
    /// Physical size in device pixels.
    pub physical_size: Size,
    /// Logical size in logical pixels.
    pub logical_size: Size,
    /// Device pixel ratio.
    pub device_pixel_ratio: f32,
}

// ============================================================================
// HitTestTarget
// ============================================================================

impl HitTestTarget for RenderView {
    fn handle_event(&self, _event: &PointerEvent, _entry: &HitTestEntry) {
        // RenderView doesn't handle events directly
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let view = RenderView::new();
        assert!(!view.has_configuration());
        assert_eq!(view.size, Size::ZERO);
    }

    #[test]
    fn test_with_configuration() {
        let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 2.0);
        let view = RenderView::with_configuration(config.clone());
        assert!(view.has_configuration());
        assert_eq!(view.configuration(), &config);
    }

    #[test]
    fn test_constraints() {
        let config = ViewConfiguration::from_size(Size::new(1920.0, 1080.0), 2.0);
        let view = RenderView::with_configuration(config);
        let constraints = view.constraints();
        // Logical constraints = physical / DPI
        assert_eq!(constraints, BoxConstraints::tight(Size::new(960.0, 540.0)));
    }

    #[test]
    fn test_apply_paint_transform() {
        let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 2.0);
        let mut view = RenderView::with_configuration(config);

        // Initialize
        view.owner = Some(std::ptr::null());
        view.prepare_initial_frame_for_test();

        let mut transform = Matrix4::identity();
        view.apply_paint_transform(&mut transform);

        // Should apply 2x scale
        assert!((transform[0] - 2.0).abs() < 1e-6);
        assert!((transform[5] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_composite_frame() {
        let config = ViewConfiguration::from_size(Size::new(1920.0, 1080.0), 2.0);
        let mut view = RenderView::with_configuration(config);

        view.owner = Some(std::ptr::null());
        view.prepare_initial_frame_for_test();
        view.size = Size::new(960.0, 540.0);

        let result = view.composite_frame();
        assert_eq!(result.logical_size, Size::new(960.0, 540.0));
        assert_eq!(result.physical_size, Size::new(1920.0, 1080.0));
        assert_eq!(result.device_pixel_ratio, 2.0);
    }

    #[test]
    fn test_semantic_bounds() {
        let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 2.0);
        let mut view = RenderView::with_configuration(config);

        view.owner = Some(std::ptr::null());
        view.prepare_initial_frame_for_test();
        view.size = Size::new(400.0, 300.0);

        let bounds = view.semantic_bounds();
        // Bounds should be scaled by DPI (2x)
        assert_eq!(bounds, Rect::from_ltwh(0.0, 0.0, 800.0, 600.0));
    }
}
