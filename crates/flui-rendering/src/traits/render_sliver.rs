//! RenderSliver trait for scrollable content layout.

use crate::constraints::AxisDirection;
use flui_foundation::Arity;
use flui_foundation::geometry::Size;

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::ParentData,
    protocol::SliverProtocol,
    traits::{HitTestOutcome, PaintEffects, RenderObject},
};

// ============================================================================
// RenderSliver Trait
// ============================================================================

/// Trait for render objects that provide scrollable content.
///
/// RenderSliver is the layout protocol for scrollable content. Slivers:
/// - Receive [`SliverConstraints`] with scroll position and viewport info
/// - Compute what portion is visible and space consumed
/// - Return [`SliverGeometry`] with scroll/paint extents
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliver` abstract class in
/// `rendering/sliver.dart`.
///
/// # Layout Protocol
///
/// 1. Parent (viewport) calls `perform_layout()` with context
/// 2. Sliver determines visible portion based on scroll offset
/// 3. Sliver returns the computed `SliverGeometry` as the return value
/// 4. Viewport composes geometries to build scrollable view
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
///
/// # Example
///
/// ```ignore
/// impl RenderSliver for MySliverList {
///     type Arity = Variable;
///     type ParentData = SliverMultiBoxAdaptorParentData;
///
///     fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<Variable, Self::ParentData>) -> SliverGeometry {
///         let scroll_offset = ctx.constraints().scroll_offset;
///         // ... compute visible items ...
///         SliverGeometry { ... }
///     }
/// }
/// ```
///
/// Implementations are automatically bridged to `RenderObject<SliverProtocol>`
/// via blanket impl.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a `RenderSliver`",
    label = "missing `impl RenderSliver for {Self}`",
    note = "a sliver-protocol render object implements `RenderSliver` (its `Arity`, `perform_layout` against `SliverConstraints`, `paint`, hit-testing) and `flui_foundation::Diagnosticable`; `RenderObject<SliverProtocol>` is then derived automatically"
)]
pub trait RenderSliver: flui_foundation::Diagnosticable + 'static {
    /// The arity of this render sliver (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render sliver.
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this sliver and returns the resulting geometry.
    ///
    /// The context provides:
    /// - Constraints via `ctx.constraints()`
    /// - Child layout via `ctx.layout_child()`
    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> SliverGeometry;

    // 2B field dedup: `SliverGeometry` and `SliverConstraints` live
    // **only** on `RenderState<SliverProtocol>` (committed from the
    // `perform_layout` return value and the layout pass). The former
    // `geometry()` / `constraints()` / `set_geometry()` accessors — which
    // forced every sliver to mirror committed state in fields and risked
    // desync — are gone. `perform_layout` returns its geometry directly;
    // positioning / size helpers take `&SliverConstraints` as an argument
    // (the layout/paint driver supplies it from `RenderState`).

    // ========================================================================
    // Positioning
    // ========================================================================

    /// Computes how much of the `[from, to]` range lies inside the viewport
    /// paint window `[scroll_offset, scroll_offset + remaining_paint_extent]`.
    ///
    /// Returns the **extent** (length) of the visible intersection, not a
    /// coordinate offset — matching Flutter's `calculatePaintOffset` naming.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculatePaintOffset` in Flutter.
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f64, to: f64) -> f64 {
        debug_assert!(from <= to);
        let remaining_painted_extent = constraints.remaining_paint_extent;
        let scroll_offset = constraints.scroll_offset;

        let a = scroll_offset;
        let b = scroll_offset + remaining_painted_extent;

        (to.min(b) - from.max(a)).max(0.0)
    }

    /// Computes the portion of this sliver that is in the cache area.
    ///
    /// Similar to `calculate_paint_offset` but includes the cache extent.
    ///
    /// # Arguments
    ///
    /// * `from` - Start of the range in sliver coordinates
    /// * `to` - End of the range in sliver coordinates
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.calculateCacheOffset` in Flutter.
    fn calculate_cache_offset(&self, constraints: &SliverConstraints, from: f64, to: f64) -> f64 {
        debug_assert!(from <= to);
        let remaining_cache_extent = constraints.remaining_cache_extent;
        let cache_origin = constraints.cache_origin;
        let scroll_offset = constraints.scroll_offset;

        let a = scroll_offset + cache_origin;
        let b = scroll_offset + remaining_cache_extent;

        (to.min(b) - from.max(a))
            .max(0.0)
            .min(remaining_cache_extent)
    }

    /// Returns the position of a child along the main axis.
    ///
    /// `constraints` are this sliver's layout constraints, supplied by the
    /// caller from [`RenderState`](crate::storage::RenderState) — these
    /// positioning hooks take it as an argument rather than caching it on
    /// the object (2B field dedup).
    ///
    /// # Arguments
    ///
    /// * `constraints` - This sliver's layout constraints
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childMainAxisPosition` in Flutter.
    fn child_main_axis_position(
        &self,
        constraints: &SliverConstraints,
        child: &dyn RenderObject<SliverProtocol>,
    ) -> f64 {
        let _ = (constraints, child);
        0.0
    }

    /// Returns the position of a child along the cross axis.
    ///
    /// # Arguments
    ///
    /// * `constraints` - This sliver's layout constraints
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childCrossAxisPosition` in Flutter.
    fn child_cross_axis_position(
        &self,
        constraints: &SliverConstraints,
        child: &dyn RenderObject<SliverProtocol>,
    ) -> f64 {
        let _ = (constraints, child);
        0.0
    }

    /// Returns the scroll offset of a child.
    ///
    /// Returns the scroll offset needed to bring the leading edge
    /// of the given child into view.
    ///
    /// # Arguments
    ///
    /// * `constraints` - This sliver's layout constraints
    /// * `child` - The child to query
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.childScrollOffset` in Flutter.
    fn child_scroll_offset(
        &self,
        constraints: &SliverConstraints,
        child: &dyn RenderObject<SliverProtocol>,
    ) -> Option<f64> {
        let _ = (constraints, child);
        None
    }

    // ========================================================================
    // Size Helpers
    // ========================================================================

    /// Returns the absolute size in the main and cross axis.
    ///
    /// Given a paint extent and cross axis extent, returns the
    /// absolute size as (width, height) based on the axis direction.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSize` in Flutter.
    fn get_absolute_size(&self, constraints: &SliverConstraints, paint_extent: f64) -> Size {
        let cross_axis_extent = constraints.cross_axis_extent;

        match constraints.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                Size::new(cross_axis_extent, paint_extent)
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                Size::new(paint_extent, cross_axis_extent)
            }
        }
    }

    /// Returns the absolute size relative to the origin.
    ///
    /// Like `get_absolute_size`, but takes into account the growth direction
    /// and axis direction to position relative to origin. Dimensions along
    /// the effective up/left direction may be negative.
    ///
    /// # Arguments
    ///
    /// * `paint_extent` - The extent along the main axis
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to `RenderSliver.getAbsoluteSizeRelativeToOrigin` in
    /// Flutter.
    fn get_absolute_size_relative_to_origin(
        &self,
        constraints: &SliverConstraints,
        paint_extent: f64,
    ) -> Size {
        match constraints
            .growth_direction
            .apply_to_axis_direction(constraints.axis_direction)
        {
            AxisDirection::TopToBottom => Size::new(constraints.cross_axis_extent, paint_extent),
            AxisDirection::BottomToTop => Size::new(constraints.cross_axis_extent, -paint_extent),
            AxisDirection::LeftToRight => Size::new(paint_extent, constraints.cross_axis_extent),
            AxisDirection::RightToLeft => Size::new(-paint_extent, constraints.cross_axis_extent),
        }
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Records this sliver's paint fragment.
    ///
    /// Same sans-IO fragment model as
    /// [`RenderBox::paint`](crate::traits::RenderBox::paint): the
    /// canvas is pre-translated to the sliver's origin (draw in local
    /// coordinates) and children are spliced via the arity-gated
    /// `paint_child` surface. Visibility culling stays the sliver's
    /// job — splice only the visible child range.
    ///
    /// The default implementation splices all children in tree order.
    fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Self::Arity>) {
        ctx.paint_children_in_order();
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this sliver.
    ///
    /// The context provides:
    /// - Position via `ctx.main_axis()`, `ctx.cross_axis()`
    /// - Child testing via `ctx.hit_test_child()`
    ///
    /// Mirrors Flutter's `RenderSliver.hitTest` dispatcher shape:
    /// children first, then [`Self::hit_test_self`]. The pipeline owns
    /// the geometry/cross-axis gate and appends the sliver's hit entry
    /// when this method returns `true`.
    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool {
        self.hit_test_children(ctx) || self.hit_test_self(ctx.main_axis(), ctx.cross_axis())
    }

    /// Hit tests this sliver's children.
    ///
    /// Container slivers should override this in reverse paint order. Leaf
    /// slivers normally leave it as `false` and override
    /// [`Self::hit_test_self`] instead.
    fn hit_test_children(
        &self,
        _ctx: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }

    /// Hit tests just this sliver (not children).
    fn hit_test_self(&self, _main: f64, _cross: f64) -> bool {
        false
    }

    // ========================================================================
    // Effect Layers
    // ========================================================================
    //
    // Override `paint_effects` to have the pipeline wrap children in this
    // node's own effect layers. The blanket `impl RenderObject<SliverProtocol>
    // for T` forwards every call from the `RenderObject<P>` surface to these
    // RenderSliver methods — concrete types override here.

    /// Opaque payload this sliver attaches to any hit that lands on it — see
    /// [`RenderObject::metadata`].
    ///
    /// Present so the hook is honoured on both protocols. The pipeline's sliver
    /// hit walk reads it exactly as the box walk does; without this forward it
    /// would read the `RenderObject` default and no sliver could ever attach a
    /// payload, which is a quieter failure than not offering the hook at all.
    fn metadata(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        None
    }

    /// Whether this render object should suppress all child painting.
    ///
    /// Default: `false`. See
    /// [`RenderObject::skip_paint`].
    fn skip_paint(&self) -> bool {
        false
    }

    /// This node's own paint effects — opacity, clip, and transform — as one
    /// value.
    ///
    /// Default: [`PaintEffects::NONE`]. See [`RenderObject::paint_effects`]
    /// for the full contract (pure in `(self, size)`, no user code).
    fn paint_effects(&self, size: flui_foundation::geometry::Size) -> PaintEffects {
        let _ = size;
        PaintEffects::NONE
    }

    /// Returns the transform matrix for hit testing.
    ///
    /// Default: `None`. See
    /// [`RenderObject::hit_test_transform`].
    fn hit_test_transform(
        &self,
        size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Matrix4> {
        let _ = size;
        None
    }

    // ========================================================================
    // Compositing / Layer Boundaries
    // ========================================================================
    //
    // Mirror the RenderBox trait methods of the same names (render_box.rs).
    // The blanket `impl<T: RenderSliver …> RenderObject<SliverProtocol> for T`
    // forwards each of these so overrides here are visible through the
    // `&dyn RenderObject<SliverProtocol>` vtable — which is what the pipeline
    // compositing-bits walk (`owner/mod.rs:2355`) and `RenderNode::debug_name`
    // (`storage/node.rs:592`) both call.

    /// Whether this node is a repaint boundary.
    ///
    /// Override and return `true` to have the pipeline allocate a dedicated
    /// compositing layer for this subtree. Default: `false`. See
    /// [`RenderObject::is_repaint_boundary`].
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this node always needs its own compositing layer.
    ///
    /// Override and return `true` for sliver nodes that apply an effect
    /// requiring a dedicated layer (e.g. `RenderSliverOpacity` when alpha is
    /// in `(0, 255)`). The pipeline compositing-bits walk at
    /// `PipelineOwner::update_subtree_compositing_bits` reads this via the
    /// `dyn RenderObject<SliverProtocol>` vtable, so the blanket impl must
    /// forward the call here — concrete types override here to be visible to
    /// the pipeline. Default: `false`. See
    /// [`RenderObject::always_needs_compositing`].
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Short human-readable name for diagnostics and error messages.
    ///
    /// Default: [`core::any::type_name::<Self>()`]. Override to return a
    /// stable short name independent of crate layout. See
    /// [`RenderObject::debug_name`].
    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
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

    /// Whether the semantics assembly walk should skip this sliver's entire
    /// child subtree.
    ///
    /// Default: `false`. See [`RenderObject::excludes_semantics_subtree`].
    fn excludes_semantics_subtree(&self) -> bool {
        false
    }

    /// Whether the child in `child_slot` reaches the semantics tree at all.
    ///
    /// The per-CHILD counterpart of [`Self::excludes_semantics_subtree`]: that
    /// one answers "none of my descendants", this one "this child of mine". A
    /// sliver that keeps children it does not present overrides it.
    ///
    /// Default: `true`. See
    /// [`RenderBox::visits_child_for_semantics`](crate::traits::RenderBox::visits_child_for_semantics)
    /// for why the placed-generation stamp cannot answer this — it excludes a
    /// child a parent *stopped* laying out, so one skipped from its very first
    /// pass was never stamped and reads as placed.
    fn visits_child_for_semantics(&self, _child_slot: usize) -> bool {
        true
    }

    /// The rect, in this sliver's coordinates, outside which child paint is
    /// not visible.
    ///
    /// Default: `None`. See [`RenderObject::describe_approximate_paint_clip`].
    fn describe_approximate_paint_clip(
        &self,
        _child_slot: usize,
        _size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Rect<f64>> {
        None
    }

    /// The rect, in this sliver's coordinates, outside which a child carries
    /// no accessibility presence.
    ///
    /// Default: `None`. See [`RenderObject::describe_semantics_clip`].
    fn describe_semantics_clip(
        &self,
        _child_slot: usize,
        _size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Rect<f64>> {
        None
    }

    /// Marks this render object for reprocessing after hot reload.
    ///
    /// Default: no-op. See
    /// [`RenderObject::reassemble`].
    fn reassemble(&mut self) {}

    // ========================================================================
    // Tree Lifecycle (ADR-0013)
    // ========================================================================

    /// Hands this render object a generational, least-privilege self-dirty
    /// handle when it enters the tree.
    ///
    /// Override to subscribe to a `dyn Listenable` this object owns or
    /// holds and self-mark on notify via the handle. Default: no-op. See
    /// [`RenderObject::attach`].
    fn attach(&mut self, handle: crate::pipeline::RenderInvalidationHandle) {
        let _ = handle;
    }

    /// Tears down whatever [`Self::attach`] subscribed to, before this
    /// render object leaves the tree.
    ///
    /// Default: no-op. See
    /// [`RenderObject::detach`].
    fn detach(&mut self) {}

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }
}

// ============================================================================
// Blanket Implementation of RenderObject<SliverProtocol> for RenderSliver
// ============================================================================

/// Automatic implementation of `RenderObject<SliverProtocol>` for all
/// RenderSliver types.
///
/// This blanket impl bridges the typed RenderSliver API (with Arity/ParentData)
/// and the protocol-specific `RenderObject<P>` trait needed for storage.
///
/// # Architecture Note
///
/// The `perform_layout_raw` and `hit_test_raw` methods are **protocol bridges
/// only**. See the RenderBox blanket impl documentation for detailed
/// explanation.
impl<T> RenderObject<SliverProtocol> for T
where
    T: RenderSliver + flui_foundation::Diagnosticable,
{
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <SliverProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<SliverProtocol>> {
        // Core.2 W3 — live bridge, mirroring the Box analog in
        // `render_box.rs`.
        //
        // The pipeline hands us `&mut dyn SliverLayoutCtxErased`
        // (the GAT `SliverProtocol::LayoutCtxErased<'_>` resolves to
        // exactly this). We reconstruct a typed
        // `SliverLayoutCtx<T::Arity, T::ParentData>` via `from_erased`
        // (Proxy storage — caches constraints, delegates child ops
        // back through the erased ctx), wrap it in the ergonomic
        // `SliverLayoutContext` so the user's `perform_layout` body
        // receives `ctx.constraints()`, `ctx.layout_child()`, etc.,
        // and call `T::perform_layout`.
        //
        // `T::perform_layout` now returns `SliverGeometry` directly —
        // a missing completion is a compile error, not a runtime error.
        let typed_inner =
            crate::protocol::SliverLayoutCtx::<T::Arity, T::ParentData>::from_erased(ctx);
        let mut layout_ctx =
            crate::context::SliverLayoutContext::<T::Arity, T::ParentData>::new(typed_inner);
        Ok(T::perform_layout(self, &mut layout_ctx))
    }

    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_foundation::geometry::Size,
    ) {
        // Same paint bridge shape as the BoxProtocol blanket: wrap the
        // recorder in the typed PaintCx<T::Arity> and call the user's
        // RenderSliver::paint. `size` is the sliver's absolute paint size
        // (`get_absolute_size(paint_extent)`), resolved by the driver
        // from `RenderState` so paint reads `ctx.size()` (2B field dedup).
        let mut cx = crate::context::PaintCx::<T::Arity>::new(recorder, child_count, size);
        T::paint(self, &mut cx);
    }

    fn hit_test_raw(
        &self,
        position: crate::protocol::ProtocolPosition<SliverProtocol>,
        _child_count: usize,
        size: flui_foundation::geometry::Size,
        hit_child: &mut dyn FnMut(
            usize,
            Option<crate::protocol::ProtocolPosition<SliverProtocol>>,
            Option<flui_foundation::geometry::Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        // The sliver hit gate is driver-owned (geometry / cross-axis
        // range), so `size` is threaded for signature uniformity but the
        // sliver context does not read it.
        let inner =
            crate::protocol::SliverHitTestCtx::<T::Arity, T::ParentData>::with_child_callback(
                position, hit_child,
            );
        let mut ctx = crate::context::SliverHitTestContext::new(inner, size);
        let blocks_below = T::hit_test(self, &mut ctx);
        HitTestOutcome::new(
            ctx.self_hit_entry_registered() || blocks_below,
            blocks_below,
        )
    }

    // Effect-layer and lifecycle forwards — same pattern as the BoxProtocol
    // blanket: call into the RenderSliver method so overrides are visible
    // through `&dyn RenderObject<SliverProtocol>`.
    fn metadata(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        <T as RenderSliver>::metadata(self)
    }

    fn skip_paint(&self) -> bool {
        <T as RenderSliver>::skip_paint(self)
    }

    fn paint_effects(&self, size: flui_foundation::geometry::Size) -> PaintEffects {
        <T as RenderSliver>::paint_effects(self, size)
    }

    fn hit_test_transform(
        &self,
        size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Matrix4> {
        <T as RenderSliver>::hit_test_transform(self, size)
    }

    fn describe_semantics_configuration(
        &self,
        config: &mut crate::semantics::SemanticsConfiguration,
    ) {
        <T as RenderSliver>::describe_semantics_configuration(self, config);
    }

    fn visits_child_for_semantics(&self, child_slot: usize) -> bool {
        <T as RenderSliver>::visits_child_for_semantics(self, child_slot)
    }

    fn excludes_semantics_subtree(&self) -> bool {
        <T as RenderSliver>::excludes_semantics_subtree(self)
    }

    fn describe_approximate_paint_clip(
        &self,
        child_slot: usize,
        size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Rect<f64>> {
        <T as RenderSliver>::describe_approximate_paint_clip(self, child_slot, size)
    }

    fn describe_semantics_clip(
        &self,
        child_slot: usize,
        size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Rect<f64>> {
        <T as RenderSliver>::describe_semantics_clip(self, child_slot, size)
    }

    fn reassemble(&mut self) {
        <T as RenderSliver>::reassemble(self);
    }

    fn attach(&mut self, handle: crate::pipeline::RenderInvalidationHandle) {
        <T as RenderSliver>::attach(self, handle);
    }

    fn detach(&mut self) {
        <T as RenderSliver>::detach(self);
    }

    // Compositing / layer-boundary forwards — mirror the RenderBox blanket
    // (render_box.rs:626-667).  UFCS calls prevent recursion: each method
    // resolves to the `RenderSliver` trait method on `T`, not back to this
    // `RenderObject<SliverProtocol>` impl.
    fn is_repaint_boundary(&self) -> bool {
        <T as RenderSliver>::is_repaint_boundary(self)
    }

    fn always_needs_compositing(&self) -> bool {
        <T as RenderSliver>::always_needs_compositing(self)
    }

    fn debug_name(&self) -> &'static str {
        <T as RenderSliver>::debug_name(self)
    }

    fn child_parent_data_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<T::ParentData>()
    }
}

// ============================================================================
// Proxy Sliver
// ============================================================================

/// Trait for slivers with a single sliver child.
///
/// Generic over the child type `C` which must implement `RenderSliver`.
pub trait RenderProxySliver<C: RenderSliver>: RenderSliver {
    /// Returns the child sliver, if any.
    fn child(&self) -> Option<&C>;

    /// Returns the child sliver mutably, if any.
    fn child_mut(&mut self) -> Option<&mut C>;

    /// Sets the child sliver.
    fn set_child(&mut self, child: Option<C>);
}
