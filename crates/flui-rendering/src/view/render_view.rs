//! RenderView - the root of the render tree.

use std::fmt::Debug;
use std::sync::{Arc, Weak};

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_layer::TransformLayer;
use flui_types::{Matrix4, Offset, Pixels, Rect, Size};
use parking_lot::RwLock;

use super::ViewConfiguration;
use crate::{
    constraints::BoxConstraints,
    context::CanvasContext,
    // Cycle 4 U-4: `HitTestResult` is re-exported from
    // `flui_interaction::routing` via `hit_testing::mod`; the import
    // path stays the same but the underlying type is now the
    // interaction-side canonical one. The previous `HitTestTarget` +
    // `PointerEvent` imports were dropped here alongside the deletion
    // of `impl HitTestTarget for RenderView`. PR #110 review feedback
    // dropped `HitTestEntry` here too once the root-sentinel add went
    // away.
    hit_testing::HitTestResult,
    pipeline::PipelineOwner,
};

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
/// 2. Second, [`attach`](Self::attach) the object to a
///    [`PipelineOwner`]
/// 3. Third, use [`prepare_initial_frame`](Self::prepare_initial_frame) to
///    bootstrap
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `RenderView` class from `rendering/view.dart`.
pub struct RenderView {
    /// The view configuration.
    configuration: Option<ViewConfiguration>,

    /// The current size (in logical pixels).
    pub(crate) size: Size,

    /// The root transformation matrix.
    root_transform: Option<Matrix4>,

    /// The root layer.
    layer: Option<TransformLayer>,

    /// Whether automatic system UI adjustment is enabled.
    automatic_system_ui_adjustment: bool,

    /// The pipeline owner (weak reference to avoid preventing cleanup).
    ///
    /// Uses `Weak<RwLock<PipelineOwner>>` because `PipelineOwner` is stored as
    /// `Arc<RwLock<PipelineOwner>>` in the application layer. A weak reference
    /// allows the render view to reference the owner without preventing cleanup
    /// when the owner is dropped.
    owner: Option<Weak<RwLock<PipelineOwner>>>,
    // Cycle 4 R-14: the 9-field `#[allow(dead_code)]` placeholder
    // block (depth / needs_layout / needs_paint /
    // needs_compositing_bits_update / needs_semantics_update /
    // is_repaint_boundary / needs_compositing / cached_constraints /
    // parent_data) was deleted. Workspace audit:
    //   - 5 fields (needs_compositing_bits_update, needs_semantics_update,
    //     needs_compositing, cached_constraints, parent_data) had zero
    //     writes AND zero reads -- pure placeholders.
    //   - 2 fields (needs_layout, needs_paint) had writes
    //     (set_configuration / schedule_initial_*_internal /
    //     perform_layout) but ZERO reads -- the framework never
    //     consulted them when scheduling frames.
    //   - 2 fields (depth, is_repaint_boundary) were constants set at
    //     construction and read only by tests asserting the field
    //     value (test-the-field-not-the-behavior).
    // Re-introduce concrete fields with concrete consumers when the
    // full RenderView lifecycle plumbing materializes (RenderState<P>
    // already carries the equivalent atomic flags via
    // `crates/flui-rendering/src/storage/flags.rs`).
    //
    // U2 exemplar refactor note (preserved): the previous
    // `was_repaint_boundary` field lived here as a mirror of the
    // (removed) `RenderObject::set_was_repaint_boundary` trait method.
    // The bit now lives on `RenderState<P>::flags` as
    // `WAS_REPAINT_BOUNDARY` (see flags.rs + ARCHITECTURE.md).
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
        // Cycle 4 R-14: the previous `self.needs_layout = true` write
        // here had zero readers. When RenderView's full lifecycle
        // plumbing lands, the equivalent invalidation flips on
        // `RenderState<P>::flags::NEEDS_LAYOUT` (the atomic version
        // in `crates/flui-rendering/src/storage/flags.rs`).
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

    /// Returns whether a pipeline owner is attached and still alive.
    pub fn has_owner(&self) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|weak| weak.strong_count() > 0)
    }

    /// Attaches this render view to a pipeline owner.
    ///
    /// The render view holds a weak reference to the owner, so it does not
    /// prevent cleanup when the owner is dropped.
    pub fn attach(&mut self, owner: &Arc<RwLock<PipelineOwner>>) {
        self.owner = Some(Arc::downgrade(owner));
    }

    /// Detaches this render view from its pipeline owner.
    pub fn detach(&mut self) {
        self.owner = None;
    }

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
            self.has_owner(),
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
        // Cycle 4 R-14: the previous `self.needs_layout = true` write
        // had zero readers. RenderState<P>::flags::NEEDS_LAYOUT is
        // the load-bearing equivalent; the full plumbing lands when
        // RenderView grows its own RenderState (or attaches to one).
    }

    fn schedule_initial_paint_internal(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        // Cycle 4 R-14: the previous `self.needs_paint = true` write
        // had zero readers. RenderState<P>::flags::NEEDS_PAINT carries
        // the live signal post-plumbing.
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
        // Cycle 4 R-14: the previous `self.needs_layout = false`
        // clear had zero readers. The atomic flag lives on
        // `RenderState<P>::flags::NEEDS_LAYOUT`; clearing it will
        // happen at the state-flip site when RenderView's lifecycle
        // plumbing wires up.
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Determines the set of render objects located at the given position.
    ///
    /// Mirrors Flutter's `RenderView.hitTest`: returns `true` because
    /// the view covers the entire surface, then delegates to child
    /// traversal to populate the result with real hit entries.
    ///
    /// # Why no root-sentinel entry
    ///
    /// PR #110 review feedback: the pre-fix body added
    /// `result.add(HitTestEntry::new(RenderId::new(1)))` as a "root
    /// sentinel" mirroring the pre-U-4 `HitTestEntry::new_render_view()`
    /// shape. The `RenderId::new(1)` value collides with whichever
    /// real render node gets slab index 0 (FLUI's
    /// Slab-0-based + IDs-1-based convention), so the sentinel
    /// masquerades as a real node ID and makes the dispatch path
    /// ambiguous. Post-fix the function adds NO sentinel; the
    /// post-U-4 interaction-side `HitTestResult` carries handler
    /// closures on entries, so an entry-less result correctly
    /// dispatches zero handlers (the previous trait-dispatch shape
    /// the sentinel was load-bearing for is gone).
    ///
    /// Child-traversal that populates the result with real hit
    /// entries lands when the RenderView → child-render-object
    /// dispatch plumbing materializes (separate audit item).
    pub fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
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
        Rect::from_ltwh(
            Pixels::ZERO,
            Pixels::ZERO,
            self.size.width * dpr,
            self.size.height * dpr,
        )
    }

    /// Returns the semantic bounds for this render view.
    pub fn semantic_bounds(&self) -> Rect {
        if let Some(transform) = &self.root_transform {
            let bounds = Rect::from_ltwh(
                Pixels::ZERO,
                Pixels::ZERO,
                self.size.width,
                self.size.height,
            );
            let scale_x = transform[0];
            let scale_y = transform[5];
            Rect::from_ltwh(
                bounds.min.x * scale_x,
                bounds.min.y * scale_y,
                bounds.width() * scale_x,
                bounds.height() * scale_y,
            )
        } else {
            Rect::from_ltwh(
                Pixels::ZERO,
                Pixels::ZERO,
                self.size.width,
                self.size.height,
            )
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
// RenderViewAdapter - Storage-compatible wrapper for RenderView
// ============================================================================

/// Adapter that makes `RenderView` compatible with `RenderObject<BoxProtocol>`
/// for storage in `RenderTree`.
///
/// `RenderView` is the root of the render tree and manages its own layout/paint
/// lifecycle. This adapter provides the minimal `RenderObject<BoxProtocol>`
/// implementation needed for `RenderNode::new_box()` storage. The pipeline
/// drives `RenderView` methods directly rather than through the standard
/// protocol dispatch.
pub struct RenderViewAdapter {
    /// The wrapped RenderView.
    pub view: RenderView,
}

impl std::fmt::Debug for RenderViewAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderViewAdapter")
            .field("view", &self.view)
            .finish()
    }
}

impl RenderViewAdapter {
    /// Creates a new adapter wrapping the given `RenderView`.
    pub fn new(view: RenderView) -> Self {
        Self { view }
    }
}

impl Diagnosticable for RenderViewAdapter {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        self.view.debug_fill_properties(properties);
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl crate::traits::PaintEffectsCapability for RenderViewAdapter {}
impl crate::traits::SemanticsCapability for RenderViewAdapter {}
impl crate::traits::HotReloadCapability for RenderViewAdapter {}

impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for RenderViewAdapter {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
        // D-block PR-A1b U19 — RenderView is the root and manages its own
        // layout via the `perform_layout()` method on the embedded
        // `RenderView`. The erased ctx is unused — root layout is driven
        // by `configuration().preferred_size` rather than parent-supplied
        // constraints (Flutter parity, `.flutter/.../view.dart`).
        self.view.perform_layout();
        self.view.size()
    }

    fn paint(&self, context: &mut CanvasContext, offset: Offset) {
        self.view.paint_view(context, offset);
    }

    fn hit_test_raw(
        &self,
        _result: &mut crate::protocol::ProtocolHitResult<crate::protocol::BoxProtocol>,
        _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
    ) -> bool {
        // RenderView always hits (it's the root)
        true
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn is_relayout_boundary(&self) -> bool {
        true
    }

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
        // Return reference to view's size field
        // SAFETY: This is a valid reference to the size field in RenderView
        &self.view.size
    }

    fn set_geometry(
        &mut self,
        geometry: crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
    ) {
        self.view.size = geometry;
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_ltwh(
            Pixels::ZERO,
            Pixels::ZERO,
            self.view.size.width,
            self.view.size.height,
        )
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

// Cycle 4 U-4: `impl HitTestTarget for RenderView` was deleted. The
// pre-cycle body was a no-op (`let _ = (event, entry);`) -- the view
// implemented the trait only to satisfy the trait-dispatch shape
// `flui_rendering::hit_testing::HitTestResult` required. Post-U-4 the
// result type is the data-typed `flui_interaction::routing::HitTestResult`
// where entries carry handler closures directly; no trait impl is
// needed on RenderView. The `HitTestTarget` trait itself is U-5's
// deletion target.

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_render_view_new() {
        let view = RenderView::new();
        assert!(!view.has_configuration());
        assert_eq!(view.size(), Size::ZERO);
    }

    #[test]
    fn test_render_view_with_configuration() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let view = RenderView::with_configuration(config.clone());
        assert!(view.has_configuration());
        assert_eq!(view.configuration(), &config);
    }

    #[test]
    fn test_render_view_constraints() {
        let config = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 2.0);
        let view = RenderView::with_configuration(config);
        let constraints = view.constraints();
        assert_eq!(
            constraints,
            BoxConstraints::tight(Size::new(px(960.0), px(540.0)))
        );
    }

    // Cycle 4 R-14: tests for `is_repaint_boundary` and `depth`
    // fields were removed alongside the field deletions -- the tests
    // asserted the field VALUE (a literal `0` / `true`), not any
    // behavior driven by the field. Both fields had zero production
    // readers, so the assertions tested the test itself.

    #[test]
    fn test_render_view_owner_is_none() {
        let view = RenderView::new();
        assert!(!view.has_owner());
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
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame_internal();

        let mut transform = Matrix4::identity();
        view.apply_paint_transform(&mut transform);

        assert!((transform[0] - 2.0).abs() < 1e-6);
        assert!((transform[5] - 2.0).abs() < 1e-6);
    }
}
