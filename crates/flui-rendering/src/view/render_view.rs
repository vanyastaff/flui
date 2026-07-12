//! RenderView - the root of the render tree.

use std::fmt::Debug;
use std::sync::{Arc, Weak};

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_layer::TransformLayer;
use flui_types::{Matrix4, Pixels, Rect, Size};
use parking_lot::RwLock;

use super::ViewConfiguration;
use crate::{constraints::BoxConstraints, pipeline::PipelineOwner};

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
    // Exemplar refactor note (preserved): the previous
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
            .finish_non_exhaustive()
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
    ///
    /// # Flutter Protocol
    ///
    /// Mirrors `RenderView.configuration`'s setter (`.flutter/.../view.dart:173-186`):
    /// the new configuration is installed *before* the root layer is
    /// rebuilt, since rebuilding it reads the new configuration to compute
    /// the updated matrix.
    pub fn set_configuration(&mut self, configuration: ViewConfiguration) {
        if self.configuration.as_ref() == Some(&configuration) {
            return;
        }

        let old_configuration = self.configuration.replace(configuration);

        if self.root_transform.is_none() {
            // prepare_initial_frame has not been called yet — nothing more to do.
            return;
        }

        let should_replace_layer = match &old_configuration {
            None => true,
            Some(old) => self.configuration().should_update_matrix(old),
        };
        if should_replace_layer {
            self.replace_root_layer_internal();
        }
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

        Self::schedule_initial_layout_internal();
        self.schedule_initial_paint_internal();
    }

    fn schedule_initial_layout_internal() {
        // Cycle 4 R-14: the previous `self.needs_layout = true` write
        // had zero readers. RenderState<P>::flags::NEEDS_LAYOUT is
        // the load-bearing equivalent; the full plumbing lands when
        // RenderView grows its own RenderState (or attaches to one) —
        // at which point this becomes a `&mut self` method again.
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
        Self::schedule_initial_layout_internal();
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

    // ========================================================================
    // Painting
    // ========================================================================

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

impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for RenderViewAdapter {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>>
    {
        // The INCOMING constraints are authoritative — they carry the
        // live window size from the binding (set_root_constraints every
        // frame). The mount-time ViewConfiguration is a snapshot that
        // goes stale on the first resize; sizing from it left every
        // newly exposed pixel unpainted. The root fills whatever the
        // window gives it (Flutter parity: tight root constraints), and
        // children get that size as tight constraints at the origin.
        let typed_inner = crate::protocol::BoxLayoutCtx::<
            flui_tree::Variable,
            crate::parent_data::BoxParentData,
        >::from_erased(ctx);
        let mut layout_ctx = crate::context::BoxLayoutContext::<
            flui_tree::Variable,
            crate::parent_data::BoxParentData,
        >::new(typed_inner);

        let constraints = *layout_ctx.constraints();
        let size = constraints.biggest();
        if !size.is_finite() {
            // Root constraints come from the window surface and must be
            // bounded; letting INF through would poison every descendant
            // geometry and paint bound downstream of `view.size`. A
            // typed error keeps the failure diagnosable in release
            // builds (a debug_assert would silently propagate there).
            tracing::error!(?constraints, "root constraints must be bounded");
            return Err(crate::error::RenderError::unbounded_constraint(
                "RenderViewAdapter",
            ));
        }
        self.view.size = size;

        let child_constraints = crate::constraints::BoxConstraints::tight(size);
        for i in 0..layout_ctx.child_count() {
            let _ = layout_ctx.layout_child(i, child_constraints);
            layout_ctx.position_child(i, flui_types::Offset::ZERO);
        }

        Ok(size)
    }

    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_types::Size,
    ) {
        // Root pass-through: the view draws nothing itself and splices
        // every child subtree in order — `size` is only forwarded to
        // the child-painting context.
        let mut cx =
            crate::context::PaintCx::<flui_tree::Variable>::new(recorder, child_count, size);
        cx.paint_children();
    }

    fn hit_test_raw(
        &self,
        _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
        child_count: usize,
        _size: flui_types::Size,
        hit_child: &mut (
                 dyn FnMut(
            usize,
            Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
        ) -> bool
                     + Send
                     + Sync
             ),
    ) -> crate::traits::HitTestOutcome {
        // Root pass-through: test children topmost-first (later
        // siblings paint on top). The view itself claims no hit — an
        // empty window region reports a miss instead of a phantom
        // root target.
        for index in (0..child_count).rev() {
            if hit_child(index, None) {
                return crate::traits::HitTestOutcome::from_hit(true);
            }
        }
        crate::traits::HitTestOutcome::miss()
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn is_relayout_boundary(&self) -> bool {
        true
    }

    // 2B field dedup: geometry / paint_bounds removed from
    // RenderObject<P>. The committed root size lives on
    // `RenderState<BoxProtocol>` (set from `perform_layout_raw`'s
    // returned `size`). `RenderView::size` is retained as the view's own
    // window-size input (set from the incoming root constraints), not a
    // render-state mirror; the engine reads root paint bounds via
    // `RenderView::physical_paint_bounds`.
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

    #[test]
    fn layer_accessors_are_none_before_and_some_after_initial_frame() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 1.0);
        let mut view = RenderView::with_configuration(config);

        assert!(view.layer().is_none());
        assert!(view.layer_mut().is_none());

        view.prepare_initial_frame_internal();

        assert!(view.layer().is_some());
        assert!(view.layer_mut().is_some());
    }

    #[test]
    fn set_configuration_is_noop_when_identical() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config.clone());
        view.prepare_initial_frame_internal();

        // Setting an identical configuration must not panic, must not clear
        // the already-established layer, and must preserve the config value.
        view.set_configuration(config.clone());

        assert_eq!(view.configuration(), &config);
        assert!(view.layer().is_some());
    }

    #[test]
    fn set_configuration_replaces_root_layer_when_device_pixel_ratio_changes() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame_internal();

        let new_config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 3.0);
        view.set_configuration(new_config.clone());

        assert_eq!(view.configuration(), &new_config);
        assert!(view.layer().is_some());

        // The replaced root layer must carry the NEW device pixel ratio, not
        // the stale one from the original configuration.
        let mut transform = Matrix4::identity();
        view.apply_paint_transform(&mut transform);
        assert!((transform[0] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn attach_detach_and_owner_liveness_lifecycle() {
        use std::sync::Arc;

        use parking_lot::RwLock;

        use crate::pipeline::PipelineOwner;

        let mut view = RenderView::new();
        assert!(!view.has_owner());

        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        view.attach(&owner);
        assert!(view.has_owner());

        view.detach();
        assert!(!view.has_owner());

        // Re-attach, then drop the strong reference: the weak pointer must
        // report no owner rather than upgrading to a dangling value.
        view.attach(&owner);
        assert!(view.has_owner());
        drop(owner);
        assert!(!view.has_owner());
    }

    #[test]
    #[should_panic(expected = "attach the RenderView to a PipelineOwner")]
    fn prepare_initial_frame_panics_without_owner() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 1.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame();
    }

    #[test]
    #[should_panic(expected = "set a configuration")]
    fn prepare_initial_frame_panics_without_configuration() {
        use std::sync::Arc;

        use parking_lot::RwLock;

        use crate::pipeline::PipelineOwner;

        let mut view = RenderView::new();
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        view.attach(&owner);
        view.prepare_initial_frame();
    }

    #[test]
    #[should_panic(expected = "must only be called once")]
    fn prepare_initial_frame_panics_on_second_call() {
        use std::sync::Arc;

        use parking_lot::RwLock;

        use crate::pipeline::PipelineOwner;

        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 1.0);
        let mut view = RenderView::with_configuration(config);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        view.attach(&owner);

        view.prepare_initial_frame();
        view.prepare_initial_frame();
    }

    #[test]
    fn prepare_initial_frame_without_owner_is_idempotent() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);

        view.prepare_initial_frame_without_owner();
        let mut first_transform = Matrix4::identity();
        view.apply_paint_transform(&mut first_transform);

        // A second call must be a no-op (early return on `root_transform.is_some()`),
        // not a silent re-bootstrap that could reset accumulated frame state.
        view.prepare_initial_frame_without_owner();
        let mut second_transform = Matrix4::identity();
        view.apply_paint_transform(&mut second_transform);

        assert_eq!(first_transform, second_transform);
    }

    #[test]
    fn perform_layout_sizes_to_the_smallest_logical_constraint() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config.clone());
        view.prepare_initial_frame_internal();

        view.perform_layout();

        assert_eq!(view.size(), config.logical_constraints().smallest());
    }

    #[test]
    fn physical_paint_bounds_scales_logical_size_by_device_pixel_ratio() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame_internal();
        view.perform_layout();

        let bounds = view.physical_paint_bounds();
        assert_eq!(bounds.width(), view.size().width * 2.0);
        assert_eq!(bounds.height(), view.size().height * 2.0);
    }

    #[test]
    fn semantic_bounds_is_unscaled_before_root_transform_is_established() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        // `size` is crate-visible; set it directly to probe the pre-bootstrap
        // (no root transform yet) branch of `semantic_bounds` in isolation.
        view.size = Size::new(px(100.0), px(50.0));

        let bounds = view.semantic_bounds();
        assert_eq!(bounds.width(), px(100.0));
        assert_eq!(bounds.height(), px(50.0));
    }

    #[test]
    fn semantic_bounds_scales_by_root_transform_once_established() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.size = Size::new(px(100.0), px(50.0));
        view.prepare_initial_frame_internal();

        let bounds = view.semantic_bounds();
        assert_eq!(bounds.width(), px(200.0));
        assert_eq!(bounds.height(), px(100.0));
    }

    #[test]
    fn composite_frame_reports_physical_and_logical_size() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let mut view = RenderView::with_configuration(config);
        view.prepare_initial_frame_internal();
        view.perform_layout();

        let result = view.composite_frame();

        assert_eq!(result.logical_size, view.size());
        assert_eq!(result.device_pixel_ratio, 2.0);
        assert_eq!(
            result.physical_size,
            Size::new(view.size().width * 2.0, view.size().height * 2.0)
        );
    }

    #[test]
    #[should_panic(expected = "self.root_transform.is_some()")]
    fn composite_frame_panics_before_initial_frame_is_prepared() {
        let config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let view = RenderView::with_configuration(config);
        let _ = view.composite_frame();
    }
}
