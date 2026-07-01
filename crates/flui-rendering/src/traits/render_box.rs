//! RenderBox trait for 2D box layout with Arity-based child management.

use flui_tree::Arity;
use flui_types::{Point, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestBehavior,
    parent_data::ParentData,
    protocol::BoxProtocol,
    traits::RenderObject,
};

// ============================================================================
// RenderBox Trait with Arity and ParentData
// ============================================================================

/// Trait for render objects that use 2D cartesian coordinates.
///
/// ## Associated Types
///
/// - `Arity` - Defines child count at compile time (Leaf, Optional, Variable)
/// - `ParentData` - Metadata type that parent stores on children
///
/// ## Example
///
/// ```ignore
/// // Simple leaf with default BoxParentData
/// struct RenderColoredBox { color: Color, size: Size }
///
/// impl RenderBox for RenderColoredBox {
///     type Arity = Leaf;
///     type ParentData = BoxParentData;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) -> Size {
///         ctx.constraints().constrain(self.size)
///     }
/// }
///
/// // Flex container with FlexParentData on children
/// struct RenderFlex { children: Vec<...> }
///
/// impl RenderBox for RenderFlex {
///     type Arity = Variable;
///     type ParentData = FlexParentData;  // Children get FlexParentData
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
///         for child in ctx.iter_children() {
///             // Type-safe access to FlexParentData
///             let flex = child.parent_data().flex;
///             let fit = child.parent_data().fit;
///         }
///     }
/// }
///
/// // Stack container with StackParentData on children
/// struct RenderStack { ... }
///
/// impl RenderBox for RenderStack {
///     type Arity = Variable;
///     type ParentData = StackParentData;  // Children get positioning info
///     ...
/// }
/// ```
/// Trait for render objects that use 2D cartesian coordinates.
///
/// Users implement this trait for their custom render objects.
/// Render objects are automatically converted to `RenderObject<BoxProtocol>`
/// for storage in `RenderTree` via the From trait.
///
/// # Features
///
/// - Intrinsic dimension queries (min/max width/height)
/// - Baseline support for text alignment
/// - Dry layout (compute size without actual layout)
/// - Coordinate conversion (local ↔ global)
pub trait RenderBox: RenderObject<BoxProtocol> + flui_foundation::Diagnosticable {
    /// The arity of this render box (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render box.
    ///
    /// This determines what metadata the parent can store on each child:
    /// - `BoxParentData` - Basic offset only (default for simple containers)
    /// - `FlexParentData` - Flex factor, fit mode (for Row/Column)
    /// - `StackParentData` - Positioning constraints (for Stack)
    /// - `TableCellParentData` - Row/column span (for Table)
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this render object and returns the resulting
    /// size.
    ///
    /// The context provides:
    /// - Constraints from parent via `ctx.constraints()`
    /// - Type-safe child access via `ctx.layout_child()`,
    ///   `ctx.position_child()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) -> Size {
    ///     let child_size = ctx.layout_single_child_loose();
    ///     ctx.position_single_child_at_origin();
    ///     ctx.constrain(child_size)
    /// }
    /// ```
    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> Size;

    // 2B field dedup: the box `Size` lives **only** on
    // `RenderState<BoxProtocol>` (committed from the `perform_layout`
    // return value). The former `size()` / `size_mut()` / `has_size()`
    // accessors — which forced every render object to mirror its
    // committed size in a field and risked desync — are gone. Paint and
    // hit_test read the driver-supplied size via `ctx.size()` /
    // `ctx.own_size()`; `perform_layout` returns its size directly.

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Returns the hit test behavior for this render object.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }

    /// Hit tests this render object.
    ///
    /// The default implementation checks if the hit position is within the
    /// render object's size bounds (Flutter parity: `RenderBox.hitTest`
    /// in `box.dart:2916-2959`). Subclasses can override to add children
    /// testing or special hit behavior.
    ///
    /// The context provides:
    /// - Position via `ctx.position()` or `ctx.x()`, `ctx.y()`
    /// - Bounds checking via `ctx.is_within_size(w, h)`
    /// - Child testing via `ctx.hit_test_child()`, `ctx.hit_test_child_at_layout_offset()`
    /// - Result management via `ctx.add_hit()`, `ctx.result_mut()`
    /// - Transform stack via `ctx.push_offset()`, `ctx.push_transform()`
    ///
    /// # Default behavior
    ///
    /// Returns true iff the hit position is within this object's bounds
    /// (size.contains check). Override this method to add child testing,
    /// transform recording, or special hit behaviors.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
    ///     // Start with the default bounds gate (size from RenderState).
    ///     if !ctx.is_within_own_size() {
    ///         return false;
    ///     }
    ///     // Then test children
    ///     if ctx.hit_test_child_at_layout_offset(0) {
    ///         return true;
    ///     }
    ///     // This object is the hit target
    ///     ctx.result_mut().add(HitTestEntry::new(self.id()));
    ///     true
    /// }
    /// ```
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool {
        ctx.is_within_own_size()
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    ///
    /// Called when a child is adopted. Override if you need custom
    /// initialization.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    fn global_to_local(&self, point: Point) -> Point {
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    fn local_to_global(&self, point: Point) -> Point {
        point
    }

    // ========================================================================
    // Intrinsic Dimensions
    // ========================================================================
    //
    // Pure functions of the object's configuration plus the SAME
    // queries on children (through the ctx — objects hold no child
    // pointers). Callers go through the pipeline
    // (`PipelineOwner::box_intrinsic_dimension`), which memoizes every
    // level in the per-node layout cache and clears it on
    // `mark_needs_layout` with boundary-crossing escalation
    // (Flutter `_LayoutCacheStorage`, box.dart:2840). The Flutter
    // `getMinIntrinsicWidth` wrapper layer IS the pipeline here; there
    // is deliberately no uncached `get_*` mirror on the trait.

    /// Computes the minimum intrinsic width for a given height.
    ///
    /// Default: `0.0` (Flutter parity — `RenderBox` itself reports no
    /// intrinsic extent; containers override and fold their children's
    /// answers via `ctx`).
    fn compute_min_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    fn compute_max_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    fn compute_min_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    fn compute_max_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        0.0
    }

    // ========================================================================
    // Dry Layout
    // ========================================================================

    /// Computes the size this box would have given the constraints,
    /// without laying out — a pure mirror of `perform_layout`'s sizing
    /// logic. Children are probed through `ctx` (memoized by the
    /// pipeline). Queried via `PipelineOwner::box_dry_layout`.
    fn compute_dry_layout(
        &self,
        _constraints: BoxConstraints,
        _ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        Size::ZERO
    }

    // ========================================================================
    // Baseline
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline.
    fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Computes the distance from the top of the box to its first baseline.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Computes the dry baseline for the given constraints — where the
    /// first baseline WOULD sit after a layout with these constraints.
    /// Memoized per `(constraints, baseline)` by the pipeline
    /// (`PipelineOwner::box_dry_baseline`); the cached entry includes a
    /// computed `None`. Children are probed through `ctx`.
    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
        _ctx: &mut crate::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        None
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Records this render object's paint fragment.
    ///
    /// The context provides:
    /// - A recording canvas pre-translated to this node's origin via
    ///   `ctx.canvas()` — **draw in local coordinates**, no offset
    ///   arithmetic
    /// - Child splicing via `ctx.paint_child()` (arity-gated: `Leaf`
    ///   has no child methods at compile time)
    /// - Clip scopes that cover children via `ctx.with_clip_rect()`
    ///   and friends
    ///
    /// Paint is a sans-IO encoder pass: the recorded fragment is
    /// replayed into the layer tree by the pipeline; this method never
    /// touches the live render tree.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
    ///     // Draw background in local coordinates (size from RenderState).
    ///     ctx.canvas()
    ///         .draw_rect(Rect::from_origin_size(Point::ZERO, ctx.size()), &paint);
    ///     // Splice the child at its laid-out offset.
    ///     ctx.paint_child();
    /// }
    /// ```
    ///
    /// The default implementation splices all children in tree order
    /// (Flutter's `RenderProxyBox.paint` parity) — pass-through
    /// containers need no override. An override that does NOT call any
    /// child-painting method hides its subtree (offstage semantics).
    fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Self::Arity>) {
        ctx.paint_children_in_order();
    }

    // ========================================================================
    // Effect Layers
    // ========================================================================
    //
    // Override these to have the pipeline wrap children in OpacityLayer /
    // TransformLayer. The blanket `impl RenderObject<BoxProtocol> for T`
    // forwards every call from the `RenderObject<P>` surface to these
    // RenderBox methods — concrete types override here, not on RenderObject.

    /// Returns the alpha value to apply to children.
    ///
    /// Override to have the pipeline wrap children in an `OpacityLayer`.
    /// Default: `None` (no opacity effect). See
    /// [`RenderObject::paint_alpha`].
    fn paint_alpha(&self) -> Option<u8> {
        None
    }

    /// Returns the blend mode for the opacity layer wrapping children.
    ///
    /// Default: `None` (= `SrcOver`). See
    /// [`RenderObject::paint_layer_blend`].
    fn paint_layer_blend(&self) -> Option<flui_types::painting::BlendMode> {
        None
    }

    /// Whether this node is a repaint boundary.
    ///
    /// Override and return `true` to have the pipeline allocate a dedicated
    /// compositing layer for this subtree.  When the child tree repaints, only
    /// this subtree's layer is invalidated — siblings and ancestors keep their
    /// cached paint output.
    ///
    /// Default: `false`. See [`RenderObject::is_repaint_boundary`].
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this node always needs its own compositing layer.
    ///
    /// Override and return `true` for nodes that apply an effect (clip,
    /// backdrop filter, etc.) that requires a dedicated layer even when no
    /// children need repainting.
    ///
    /// Default: `false`. See [`RenderObject::always_needs_compositing`].
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Whether this render object should suppress all child painting.
    ///
    /// Default: `false`. See
    /// [`RenderObject::skip_paint`].
    fn skip_paint(&self) -> bool {
        false
    }

    /// Returns the transform matrix to apply to children during painting.
    ///
    /// Default: `None`. See
    /// [`RenderObject::paint_transform`].
    fn paint_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        let _ = size;
        None
    }

    /// Returns the transform matrix for hit testing.
    ///
    /// Default: `None`. See
    /// [`RenderObject::hit_test_transform`].
    fn hit_test_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        let _ = size;
        None
    }

    /// The pointer-event handler this box contributes to its hit entry.
    ///
    /// Default `None`; override (e.g. `RenderListener`) to receive pointer
    /// events that land on this box. The blanket `RenderObject<BoxProtocol>`
    /// impl forwards to this — concrete types override here, not on
    /// `RenderObject`. See [`RenderObject::pointer_event_handler`].
    fn pointer_event_handler(&self) -> Option<crate::hit_testing::PointerEventHandler> {
        None
    }

    // ========================================================================
    // Semantics / Hot Reload
    // ========================================================================

    /// Describes semantic properties for accessibility.
    ///
    /// Default: no-op. See
    /// [`RenderObject::describe_semantics_configuration`].
    fn describe_semantics_configuration(
        &self,
        _config: &mut crate::semantics::SemanticsConfiguration,
    ) {
    }

    /// Marks this render object for reprocessing after hot reload.
    ///
    /// Default: no-op. See
    /// [`RenderObject::reassemble`].
    fn reassemble(&mut self) {}

    /// Short human-readable name for diagnostics and error messages.
    ///
    /// Default: [`core::any::type_name::<Self>()`] (the fully-qualified Rust
    /// type name). Override to return a stable short name (e.g.
    /// `"RenderPadding"`) that is independent of crate layout.
    /// See [`RenderObject::debug_name`].
    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }
}

/// Text baseline types for baseline alignment.
///
/// Re-exported from [`flui_types`] — the single canonical definition for the
/// workspace. The former parallel enum here was consolidated into `flui-types`
/// (its lower, owning layer) in 2026-06.
pub use flui_types::layout::TextBaseline;

// ============================================================================
// Blanket Implementation of RenderObject<BoxProtocol> for RenderBox
// ============================================================================

/// Automatic implementation of `RenderObject<BoxProtocol>` for all RenderBox
/// types.
///
/// This blanket impl bridges the typed RenderBox API (with Arity/ParentData)
/// and the protocol-specific `RenderObject<P>` trait needed for storage.
///
/// # Architecture Note (D-block PR-A1b U19 / companion memo D5)
///
/// The `perform_layout_raw` body **is** the real bridge — it receives a
/// protocol-erased `&mut dyn BoxLayoutCtxErased`, reconstructs a typed
/// `BoxLayoutCtx<T::Arity, T::ParentData>` via the in-crate
/// `BoxLayoutCtx::from_erased` ctor (`pub(crate)` — see
/// [`crate::protocol::BoxLayoutCtx`]), wraps it in a `BoxLayoutContext`
/// (the rich ergonomic wrapper), and calls
/// [`RenderBox::perform_layout`]. The completion size is read back from
/// the inner context's geometry and returned to the pipeline.
///
/// Pre-U19 this method returned `*self.size()` as a no-op placeholder
/// (companion memo §D5: D-1 AE1 demonstrably returned `Size::ZERO` for
/// fresh boxes). The real bridge is now live.
///
/// `hit_test_raw` is still a placeholder — hit testing flows through
/// `RenderBox::hit_test()` with `BoxHitTestContext` and is wired
/// separately by the hit-test pipeline. `paint_raw` is the live paint
/// bridge: it wraps the pipeline's `FragmentRecorder` in the typed
/// `PaintCx<T::Arity>` and calls `RenderBox::paint`.
///
/// Note: This requires T to also implement Diagnosticable since `RenderObject<P>`
/// requires it.
impl<T> RenderObject<BoxProtocol> for T
where
    T: RenderBox + flui_foundation::Diagnosticable,
{
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<BoxProtocol>> {
        // D-block PR-A1b U19 / memo D5 — the real bridge.
        //
        // The pipeline / `RenderEntry::layout_leaf_only` hands us a
        // `&mut dyn BoxLayoutCtxErased` (the GAT for `BoxProtocol`
        // resolves to exactly this). We reconstruct a typed
        // `BoxLayoutCtx<T::Arity, T::ParentData>` via the `from_erased`
        // ctor (Proxy storage that delegates child ops back through the
        // erased trait), wrap it in the ergonomic `BoxLayoutContext` so
        // user widgets get nice helpers, and call `T::perform_layout`.
        //
        // `T::perform_layout` now returns `Size` directly — a missing
        // completion is a compile error, not a runtime `ContractViolation`.
        //
        // `catch_unwind` in `RenderEntry::layout_leaf_only` is retained
        // only for genuine third-party panics (panic! / unwrap in user
        // widget code) which still surface as `RenderError::Poisoned`.
        let typed_inner =
            crate::protocol::BoxLayoutCtx::<T::Arity, T::ParentData>::from_erased(ctx);
        let mut layout_ctx =
            crate::context::BoxLayoutContext::<T::Arity, T::ParentData>::new(typed_inner);
        Ok(T::perform_layout(self, &mut layout_ctx))
    }

    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_types::Size,
    ) {
        // The paint bridge: wrap the recorder in the typed, arity-gated
        // PaintCx and call the user's RenderBox::paint. Unlike the
        // layout bridge there is no GAT erasure — the recorder is a
        // concrete type (no ParentData in the paint surface), so the
        // re-typing is a zero-cost PhantomData re-tag. `size` is the
        // node's committed `RenderState` geometry, threaded by the
        // pipeline so paint reads `ctx.size()` (2B field dedup).
        let mut cx = crate::context::PaintCx::<T::Arity>::new(recorder, child_count, size);
        T::paint(self, &mut cx);
    }

    fn hit_test_raw(
        &self,
        position: crate::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        size: flui_types::Size,
        hit_child: &mut (
                 dyn FnMut(usize, Option<crate::protocol::ProtocolPosition<BoxProtocol>>) -> bool
                     + Send
                     + Sync
             ),
    ) -> bool {
        // The hit-test bridge: wrap the driver's child recursion in
        // the typed, arity-gated BoxHitTestContext and call the user's
        // RenderBox::hit_test. Same shape as the paint bridge — no GAT
        // erasure needed, the position/callback types are concrete.
        // `size` is the node's committed `RenderState` geometry, threaded
        // by the driver so the default bounds gate reads
        // `ctx.is_within_own_size()` (2B field dedup).
        let inner = crate::protocol::BoxHitTestCtx::<T::Arity, T::ParentData>::with_child_callback(
            position, hit_child,
        );
        let mut ctx = crate::context::BoxHitTestContext::new(inner, size);
        T::hit_test(self, &mut ctx)
    }

    fn intrinsic_raw(
        &self,
        dimension: crate::storage::IntrinsicDimension,
        extent: f32,
        child_count: usize,
        child_parent_data: &[Option<&dyn crate::parent_data::ParentData>],
        child_query: &mut (
                 dyn FnMut(usize, crate::storage::IntrinsicDimension, f32) -> f32 + Send + Sync
             ),
    ) -> f32 {
        // The intrinsics bridge: wrap the driver's memoizing child
        // recursion in the typed ctx and dispatch the dimension to the
        // matching typed compute_* — same shape as the paint/hit
        // bridges, no GAT erasure needed.
        use crate::storage::IntrinsicDimension as Dim;
        let mut ctx =
            crate::context::BoxIntrinsicsCtx::new(child_count, child_parent_data, child_query);
        match dimension {
            Dim::MinWidth => T::compute_min_intrinsic_width(self, extent, &mut ctx),
            Dim::MaxWidth => T::compute_max_intrinsic_width(self, extent, &mut ctx),
            Dim::MinHeight => T::compute_min_intrinsic_height(self, extent, &mut ctx),
            Dim::MaxHeight => T::compute_max_intrinsic_height(self, extent, &mut ctx),
        }
    }

    fn dry_layout_raw(
        &self,
        constraints: crate::protocol::ProtocolConstraints<BoxProtocol>,
        child_count: usize,
        child_parent_data: &[Option<&dyn crate::parent_data::ParentData>],
        child_dry: &mut (
                 dyn FnMut(
            usize,
            crate::protocol::ProtocolConstraints<BoxProtocol>,
        ) -> crate::protocol::ProtocolGeometry<BoxProtocol>
                     + Send
                     + Sync
             ),
    ) -> crate::protocol::ProtocolGeometry<BoxProtocol> {
        let mut ctx =
            crate::context::BoxDryLayoutCtx::new(child_count, child_parent_data, child_dry);
        T::compute_dry_layout(self, constraints, &mut ctx)
    }

    fn dry_baseline_raw(
        &self,
        constraints: crate::protocol::ProtocolConstraints<BoxProtocol>,
        baseline: crate::traits::TextBaseline,
        child_count: usize,
        child_parent_data: &[Option<&dyn crate::parent_data::ParentData>],
        child_query: &mut (
                 dyn FnMut(
            usize,
            crate::context::DryBaselineChildRequest,
        ) -> crate::context::DryBaselineChildResponse
                     + Send
                     + Sync
             ),
    ) -> Option<f32> {
        let mut ctx =
            crate::context::BoxDryBaselineCtx::new(child_count, child_parent_data, child_query);
        T::compute_dry_baseline(self, constraints, baseline, &mut ctx)
    }

    fn actual_baseline_raw(&self, baseline: crate::traits::TextBaseline) -> Option<f32> {
        T::compute_distance_to_actual_baseline(self, baseline)
    }

    // Effect-layer and lifecycle forwards — mirror the `actual_baseline_raw`
    // pattern: call into the RenderBox method so overrides on RenderBox are
    // visible through `&dyn RenderObject<BoxProtocol>`.
    fn is_repaint_boundary(&self) -> bool {
        <T as RenderBox>::is_repaint_boundary(self)
    }

    fn always_needs_compositing(&self) -> bool {
        <T as RenderBox>::always_needs_compositing(self)
    }

    fn paint_alpha(&self) -> Option<u8> {
        <T as RenderBox>::paint_alpha(self)
    }

    fn paint_layer_blend(&self) -> Option<flui_types::painting::BlendMode> {
        <T as RenderBox>::paint_layer_blend(self)
    }

    fn skip_paint(&self) -> bool {
        <T as RenderBox>::skip_paint(self)
    }

    fn paint_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        <T as RenderBox>::paint_transform(self, size)
    }

    fn hit_test_transform(&self, size: flui_types::Size) -> Option<flui_types::Matrix4> {
        <T as RenderBox>::hit_test_transform(self, size)
    }

    fn pointer_event_handler(&self) -> Option<crate::hit_testing::PointerEventHandler> {
        <T as RenderBox>::pointer_event_handler(self)
    }

    fn describe_semantics_configuration(
        &self,
        config: &mut crate::semantics::SemanticsConfiguration,
    ) {
        <T as RenderBox>::describe_semantics_configuration(self, config)
    }

    fn reassemble(&mut self) {
        <T as RenderBox>::reassemble(self)
    }

    fn debug_name(&self) -> &'static str {
        <T as RenderBox>::debug_name(self)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_behavior_default() {
        // HitTestBehavior is now imported from flui_interaction via hit_testing
        let behavior = HitTestBehavior::default();
        assert_eq!(behavior, HitTestBehavior::DeferToChild);
    }

    // BoxHitTestResult and BoxHitTestEntry tests are now in
    // hit_testing/result.rs
}
