//! RenderView - the root of the render tree.

use std::fmt::Debug;

use flui_foundation::geometry::{Matrix4, Rect, Size};
use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_layer::TransformLayer;

use super::ViewConfiguration;
use crate::constraints::BoxConstraints;

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
/// 2. Second, use
///    [`prepare_initial_frame_without_owner`](Self::prepare_initial_frame_without_owner)
///    to bootstrap
///
/// There is no owner-attach step: a `RenderView` no longer holds a
/// [`PipelineOwner`](crate::pipeline::PipelineOwner) back-reference -- the
/// owner reaches the tree through the regular insert path, not through the
/// view.
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
    // A 9-field `#[allow(dead_code)]` placeholder block used to live here
    // (depth / needs_layout / needs_paint / needs_compositing_bits_update /
    // needs_semantics_update / is_repaint_boundary / needs_compositing /
    // cached_constraints / parent_data). It was removed after an audit
    // found none of the fields were pulling their weight:
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
        // A `self.needs_layout = true` write used to happen here, but it had
        // zero readers, so it was removed. When RenderView's full lifecycle
        // plumbing lands, the equivalent invalidation should flip on
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

    fn schedule_initial_layout_internal() {
        // A `self.needs_layout = true` write used to happen here, but it had
        // zero readers, so it was removed. RenderState<P>::flags::NEEDS_LAYOUT
        // is the load-bearing equivalent; the full plumbing lands when
        // RenderView grows its own RenderState (or attaches to one) —
        // at which point this becomes a `&mut self` method again.
    }

    fn schedule_initial_paint_internal(&mut self) {
        self.layer = Some(self.update_matrices_and_create_new_root_layer());
        // A `self.needs_paint = true` write used to happen here, but it had
        // zero readers, so it was removed. RenderState<P>::flags::NEEDS_PAINT
        // carries the live signal post-plumbing.
    }

    /// Prepare the initial frame without requiring a PipelineOwner.
    pub fn prepare_initial_frame_without_owner(&mut self) {
        if self.root_transform.is_some() {
            return;
        }
        Self::schedule_initial_layout_internal();
        self.schedule_initial_paint_internal();
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
        // A `self.needs_layout = false` clear used to happen here, but it
        // had zero readers, so it was removed. The atomic flag lives on
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
            flui_foundation::Variable,
            crate::parent_data::BoxParentData,
        >::from_erased(ctx);
        let mut layout_ctx = crate::context::BoxLayoutContext::<
            flui_foundation::Variable,
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
            layout_ctx.position_child(i, flui_foundation::geometry::Offset::ZERO);
        }

        Ok(size)
    }

    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_foundation::geometry::Size,
    ) {
        // Root pass-through: the view draws nothing itself and splices
        // every child subtree in order — `size` is only forwarded to
        // the child-painting context.
        let mut cx =
            crate::context::PaintCx::<flui_foundation::Variable>::new(recorder, child_count, size);
        cx.paint_children();
    }

    fn hit_test_raw(
        &self,
        _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
        child_count: usize,
        _size: flui_foundation::geometry::Size,
        hit_child: &mut dyn FnMut(
            usize,
            Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
            Option<flui_foundation::geometry::Matrix4>,
        ) -> bool,
    ) -> crate::traits::HitTestOutcome {
        // Root pass-through: test children topmost-first (later
        // siblings paint on top). The view itself claims no hit — an
        // empty window region reports a miss instead of a phantom
        // root target.
        for index in (0..child_count).rev() {
            if hit_child(index, None, None) {
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
    pub device_pixel_ratio: f64,
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

// `impl HitTestTarget for RenderView` used to live here, but was deleted.
// Its body was a no-op (`let _ = (event, entry);`) -- the view only
// implemented the trait to satisfy the trait-dispatch shape that the
// old rendering-side `crate::hit_testing::HitTestResult` type
// required. Hit testing now produces the data-typed
// `flui_interaction::routing::HitTestResult`, whose entries carry handler
// closures directly, so no trait impl is needed on RenderView. The
// `HitTestTarget` trait itself has since been removed entirely, since it
// no longer had a production implementor.

#[cfg(test)]
mod tests {

    // Tests for the `is_repaint_boundary` and `depth` fields were removed
    // alongside the field deletions above -- the tests asserted the field
    // VALUE (a literal `0` / `true`), not any behavior driven by the field.
    // Both fields had zero production readers, so the assertions tested
    // the test itself.
    //
    // `test_render_view_owner_is_none` (and the `attach`/`detach`/`has_owner`
    // lifecycle tests below it) were removed alongside the `owner` field
    // `RenderView` no longer holds a `PipelineOwner` back-reference
    // at all, so there is nothing left to assert liveness of.
}
