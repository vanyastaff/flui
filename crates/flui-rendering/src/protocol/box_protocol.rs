//! Box protocol for 2D cartesian layout.
//!
//! This module provides the BoxProtocol and its capability implementations:
//! - [`BoxProtocol`]: Main protocol type
//! - [`BoxLayout`]: Layout capability (BoxConstraints → Size)
//! - [`BoxHitTest`]: Hit test capability (Offset → BoxHitTestResult)

use flui_foundation::RenderId;
use flui_tree::Arity;
use flui_types::{
    Size,
    geometry::{Matrix4, Offset, Point, Rect},
};

use crate::{
    constraints::{BoxConstraints, Constraints},
    parent_data::{BoxParentData, ParentData},
    protocol::{
        capabilities::{HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi},
        protocol::{BidirectionalProtocol, Protocol, ProtocolCompatible, sealed},
    },
};

// ============================================================================
// CHILD STATE
// ============================================================================
//
// Per-child layout-time bookkeeping owned by `BoxLayoutCtx`. Previously
// lived in `crates/flui-rendering/src/children_access.rs` alongside a
// 500-LOC closure-based iterator (`ChildrenAccess`) and the
// `ChildHandle` wrapper in `child_handle.rs` -- both fought the borrow
// checker for users that never appeared, so Mythos Step 5b deleted them
// outright. `ChildState<P>` itself stays because it IS the data shape
// `BoxLayoutContextApi::layout_child` / `position_child` /
// `child_geometry` / `child_parent_data` need.

/// Per-child layout-time state held by [`BoxLayoutCtx`].
///
/// Created by the pipeline before invoking a parent's `perform_layout`,
/// mutated through `BoxLayoutContextApi::layout_child` /
/// `position_child`, and read during the subsequent paint phase.
#[derive(Debug)]
pub struct ChildState<P: ParentData + Default> {
    /// Render ID of this child.
    pub id: RenderId,
    /// Computed size after layout.
    pub size: Size,
    /// Position offset set by parent.
    pub offset: Offset,
    /// Parent data for this child.
    pub parent_data: P,
}

impl<P: ParentData + Default> ChildState<P> {
    /// Creates a new child state with default values.
    pub fn new(id: RenderId) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: P::default(),
        }
    }

    /// Creates a new child state with specific parent data.
    pub fn with_parent_data(id: RenderId, parent_data: P) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data,
        }
    }
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box protocol using 2D constraints and sizes.
///
/// This is the most common protocol for 2D layout with width/height
/// constraints. Used by most widgets: containers, buttons, text, images, etc.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl sealed::Sealed for BoxProtocol {}

impl Protocol for BoxProtocol {
    type Layout = BoxLayout;
    type HitTest = BoxHitTest;
    type DefaultParentData = BoxParentData;

    fn name() -> &'static str {
        "box"
    }

    /// D-block PR-A1 U17 — override the default no-op with the actual
    /// Flutter-parity `compute_relayout_boundary` call. `parent_uses_size`
    /// and `sized_by_parent` are wired as `false` for now; full Flutter
    /// parity for those parameters lands later in Core.2 alongside the
    /// intrinsic-dimension protocol.
    fn bootstrap_relayout_boundary(state: &crate::storage::RenderState<Self>, has_parent: bool) {
        state.compute_relayout_boundary(false, false, has_parent);
    }
}

impl BidirectionalProtocol for BoxProtocol {}

// Self-compatibility
impl ProtocolCompatible<BoxProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// ============================================================================
// BOX LAYOUT CAPABILITY
// ============================================================================

/// Layout capability for box (2D) layout.
///
/// Uses `BoxConstraints` for input and `Size` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxLayout;

/// Cache key for BoxConstraints.
///
/// Uses integer representation of floats (bits) for reliable hashing.
/// This handles -0.0/+0.0 and provides exact equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxConstraintsCacheKey {
    min_width_bits: u32,
    max_width_bits: u32,
    min_height_bits: u32,
    max_height_bits: u32,
}

impl BoxConstraintsCacheKey {
    /// Creates a cache key from constraints.
    ///
    /// Returns `None` if any value is NaN.
    pub fn from_constraints(c: &BoxConstraints) -> Option<Self> {
        // NaN check using is_nan()
        if c.min_width.is_nan()
            || c.max_width.is_nan()
            || c.min_height.is_nan()
            || c.max_height.is_nan()
        {
            return None;
        }

        Some(Self {
            min_width_bits: c.min_width.to_bits(),
            max_width_bits: c.max_width.to_bits(),
            min_height_bits: c.min_height.to_bits(),
            max_height_bits: c.max_height.to_bits(),
        })
    }
}

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type CacheKey = BoxConstraintsCacheKey;
    type Context<'ctx, A: Arity, P: ParentData + Default>
        = BoxLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        Size::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey> {
        BoxConstraintsCacheKey::from_constraints(constraints)
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Box layout context implementation.
///
/// This context provides access to constraints and children during layout.
/// Callback type for synchronous child layout.
///
/// Called when parent's `layout_child()` is invoked. The callback receives
/// the child's `RenderId` and constraints, performs layout on the child via
/// the RenderTree, and returns the child's size.
pub type LayoutChildCallback<'a> =
    &'a (dyn Fn(flui_foundation::RenderId, BoxConstraints) -> Size + Send + Sync);

/// The children reference allows `position_child` to store offsets that
/// will be used during painting.
pub struct BoxLayoutCtx<'ctx, A: Arity, P: ParentData + Default> {
    constraints: BoxConstraints,
    geometry: Option<Size>,
    /// Reference to children states for position_child to update offsets.
    children: Option<&'ctx mut Vec<ChildState<P>>>,
    /// Child render IDs for tree lookup during layout_child.
    child_ids: Option<&'ctx [flui_foundation::RenderId]>,
    /// Callback to perform synchronous child layout through RenderTree.
    layout_child_callback: Option<LayoutChildCallback<'ctx>>,
    _phantom: std::marker::PhantomData<A>,
}

impl<'ctx, A: Arity, P: ParentData + Default> BoxLayoutCtx<'ctx, A, P> {
    /// Creates a new box layout context with given constraints (no children
    /// access).
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            geometry: None,
            children: None,
            child_ids: None,
            layout_child_callback: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new box layout context with children access.
    pub fn with_children(
        constraints: BoxConstraints,
        children: &'ctx mut Vec<ChildState<P>>,
    ) -> Self {
        Self {
            constraints,
            geometry: None,
            children: Some(children),
            child_ids: None,
            layout_child_callback: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new box layout context with full access for synchronous child
    /// layout.
    ///
    /// This constructor enables proper Flutter-style layout where parent's
    /// `layout_child()` triggers synchronous child layout through the
    /// RenderTree.
    pub fn with_layout_callback(
        constraints: BoxConstraints,
        children: &'ctx mut Vec<ChildState<P>>,
        child_ids: &'ctx [flui_foundation::RenderId],
        layout_child_callback: LayoutChildCallback<'ctx>,
    ) -> Self {
        Self {
            constraints,
            geometry: None,
            children: Some(children),
            child_ids: Some(child_ids),
            layout_child_callback: Some(layout_child_callback),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Gets the current geometry if layout is complete.
    pub fn geometry(&self) -> Option<&Size> {
        self.geometry.as_ref()
    }
}

impl<'ctx, A: Arity, P: ParentData + Default> LayoutContextApi<'ctx, BoxLayout, A, P>
    for BoxLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &BoxConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.geometry.is_some()
    }

    fn complete_layout(&mut self, geometry: Size) {
        self.geometry = Some(geometry);
    }

    fn child_count(&self) -> usize {
        self.children.as_ref().map(|c| c.len()).unwrap_or(0)
    }

    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        // Try to use the layout callback for synchronous child layout
        if let (Some(child_ids), Some(callback)) =
            (self.child_ids, self.layout_child_callback.as_ref())
            && let Some(&child_id) = child_ids.get(index)
        {
            // Perform synchronous layout through RenderTree
            let size = callback(child_id, constraints);

            // Update cached size in children state
            if let Some(children) = &mut self.children
                && let Some(child) = children.get_mut(index)
            {
                child.size = size;
            }

            return size;
        }

        // Fallback: return cached size if available
        if let Some(children) = &self.children
            && let Some(child) = children.get(index)
        {
            return child.size;
        }
        Size::ZERO
    }

    fn position_child(&mut self, index: usize, offset: Offset) {
        // Store the offset in the child's state
        if let Some(children) = &mut self.children
            && let Some(child) = children.get_mut(index)
        {
            child.offset = offset;
        }
    }

    fn child_geometry(&self, index: usize) -> Option<&Size> {
        self.children
            .as_ref()
            .and_then(|c| c.get(index))
            .map(|child| &child.size)
    }

    fn child_parent_data(&self, index: usize) -> Option<&P> {
        self.children
            .as_ref()
            .and_then(|c| c.get(index))
            .map(|child| &child.parent_data)
    }

    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P> {
        self.children
            .as_mut()
            .and_then(|c| c.get_mut(index))
            .map(|child| &mut child.parent_data)
    }
}

// ============================================================================
// BOX HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for box (2D) layout.
///
/// Uses `Offset` for position and standard hit test result.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxHitTest;

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Hit test result for box protocol.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    /// Path of hit test entries from leaf to root.
    pub path: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self { path: Vec::new() }
    }

    /// Adds an entry to the hit test path.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.path.push(entry);
    }

    /// Returns whether any targets were hit.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the number of hit entries.
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Clears all hit entries.
    pub fn clear(&mut self) {
        self.path.clear();
    }
}

/// Individual hit test entry for box protocol.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Transform from target to root coordinates.
    pub transform: Matrix4,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(target_id: u64, transform: Matrix4) -> Self {
        Self {
            target_id,
            transform,
        }
    }

    /// Creates a hit test entry with identity transform.
    pub fn with_id(target_id: u64) -> Self {
        Self::new(target_id, Matrix4::IDENTITY)
    }
}

/// Box hit test context implementation.
///
/// # Transform accumulation
///
/// Cycle 4 wave 5 R-24: `current_transform()` previously folded the
/// entire `transform_stack: Vec<Matrix4>` via
/// `iter().fold(IDENTITY, |acc, t| acc * t)` -- O(N) matrix-multiply
/// chain on every hit-test entry. Hit testing is hot-path; a 30-deep
/// tree paid 30 mat-mults per entry.
///
/// The fix mirrors Flutter's `HitTestResult._localTransforms` cache:
/// alongside the explicit `transform_stack`, the ctx maintains
/// `composed_transform: Matrix4` updated incrementally on
/// `push_transform` (one mat-mult) and recomputed on `pop_transform`
/// (one full re-fold over the now-shorter stack). Per-call cost
/// drops from O(stack_depth) to O(1) for queries, and pops stay
/// O(stack_depth) but amortize across the matched push.
pub struct BoxHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: Offset,
    result: BoxHitTestResult,
    transform_stack: Vec<Matrix4>,
    /// Cached composition of `transform_stack` in push-order. Kept in
    /// sync with the stack via `push_transform` (multiply in) and
    /// `pop_transform` (full re-fold over the truncated stack).
    composed_transform: Matrix4,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> BoxHitTestCtx<'ctx, A, P> {
    /// Creates a new box hit test context.
    pub fn new(position: Offset) -> Self {
        Self {
            position,
            result: BoxHitTestResult::new(),
            transform_stack: Vec::new(),
            composed_transform: Matrix4::IDENTITY,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the current accumulated transform.
    ///
    /// O(1) -- reads the cached composition. See type-level doc for
    /// the R-24 incremental-composition design.
    pub fn current_transform(&self) -> Matrix4 {
        self.composed_transform
    }

    /// Recomputes [`Self::composed_transform`] from `transform_stack`.
    /// Used by `pop_transform` because matrix inversion to "subtract"
    /// the popped factor is more expensive (and more numerically
    /// fraught) than a full re-fold over a typically-shallow stack.
    #[inline]
    fn recompute_composed(&mut self) {
        self.composed_transform = self
            .transform_stack
            .iter()
            .fold(Matrix4::IDENTITY, |acc, t| acc * *t);
    }

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        let transform = self.current_transform();
        self.result.add(BoxHitTestEntry::new(target_id, transform));
    }
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, BoxHitTest, A, P>
    for BoxHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &Offset {
        &self.position
    }

    fn result(&self) -> &BoxHitTestResult {
        &self.result
    }

    fn result_mut(&mut self) -> &mut BoxHitTestResult {
        &mut self.result
    }

    fn add_hit(&mut self, entry: BoxHitTestEntry) {
        self.result.add(entry);
    }

    fn is_hit(&self, bounds: Rect) -> bool {
        bounds.contains(Point::new(self.position.dx, self.position.dy))
    }

    fn hit_test_child(&mut self, _index: usize, _position: Offset) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, transform: Matrix4) {
        // R-24: keep the cached composition in sync. One mat-mult
        // per push amortizes O(stack_depth) hit-test queries down
        // to O(1).
        self.transform_stack.push(transform);
        self.composed_transform *= transform;
    }

    fn pop_transform(&mut self) {
        // R-24: a popped factor cannot be "un-multiplied" cheaply
        // (would require matrix inverse + multiply, ~5x cost of a
        // forward fold and numerically fragile). Full re-fold over
        // the now-shorter stack is the cleanest fix; hit-test stacks
        // measure ~20-40 deep in practice, well within
        // matrix-multiply burst budgets.
        self.transform_stack.pop();
        self.recompute_composed();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_tree::Leaf;
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_box_protocol_name() {
        assert_eq!(BoxProtocol::name(), "box");
    }

    #[test]
    fn test_box_layout_default_geometry() {
        let size = BoxLayout::default_geometry();
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add(BoxHitTestEntry::with_id(1));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_box_hit_test_context() {
        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(50.0), px(50.0)));

        let bounds = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
        assert!(ctx.is_hit(bounds));

        let outside = Rect::from_ltrb(px(100.0), px(100.0), px(200.0), px(200.0));
        assert!(!ctx.is_hit(outside));
    }

    /// Cycle 4 wave 5 R-24: incremental transform composition must
    /// stay numerically identical to the prior O(N) fold path.
    /// Builds a 3-deep stack and asserts the cached
    /// `current_transform()` equals the explicit `fold(IDENTITY, |a, t| a * t)`.
    #[test]
    fn test_box_hit_test_context_incremental_transform_matches_fold() {
        let mut ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));

        // Mat₁: translate (10, 0)
        let t1 = Matrix4::translation(10.0, 0.0, 0.0);
        // Mat₂: rotation 90° about Z
        let t2 = Matrix4::rotation_z(std::f32::consts::FRAC_PI_2);
        // Mat₃: scale 2x
        let t3 = Matrix4::scaling(2.0, 2.0, 1.0);

        ctx.push_transform(t1);
        ctx.push_transform(t2);
        ctx.push_transform(t3);

        let expected = Matrix4::IDENTITY * t1 * t2 * t3;
        let got = ctx.current_transform();
        // Bit-exact: cache and explicit fold do the same mat-mults
        // in the same order.
        assert_eq!(got, expected);
    }

    /// Pop must restore the prior composed state. Push A, push B,
    /// pop B → composed == A.
    #[test]
    fn test_box_hit_test_context_pop_restores_composition() {
        let mut ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));

        let t1 = Matrix4::translation(5.0, 5.0, 0.0);
        let t2 = Matrix4::scaling(3.0, 3.0, 1.0);

        ctx.push_transform(t1);
        let after_t1 = ctx.current_transform();

        ctx.push_transform(t2);
        ctx.pop_transform();

        assert_eq!(ctx.current_transform(), after_t1);
    }

    /// Empty stack returns identity.
    #[test]
    fn test_box_hit_test_context_empty_stack_is_identity() {
        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(px(0.0), px(0.0)));
        assert_eq!(ctx.current_transform(), Matrix4::IDENTITY);
    }

    #[test]
    fn test_box_layout_context() {
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let mut ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);

        assert!(!ctx.is_complete());
        assert_eq!(ctx.constraints().max_width, 100.0);

        ctx.complete_layout(Size::new(px(100.0), px(100.0)));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_box_constraints_cache_key_equality() {
        let c1 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c2 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c3 = BoxConstraints::tight(Size::new(px(200.0), px(100.0)));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_box_constraints_cache_key_nan() {
        let c = BoxConstraints::new(px(f32::NAN), px(100.0), px(0.0), px(100.0));
        assert!(BoxConstraintsCacheKey::from_constraints(&c).is_none());
    }

    #[test]
    fn test_box_constraints_cache_key_negative_zero() {
        // -0.0 and +0.0 should produce different cache keys (bit-exact)
        let c1 = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));
        let c2 = BoxConstraints::new(px(-0.0), px(100.0), px(0.0), px(100.0));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();

        // They have different bits, so different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_box_constraints_cache_key_hash() {
        use std::collections::HashSet;

        let c1 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c2 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c3 = BoxConstraints::tight(Size::new(px(200.0), px(100.0)));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        let mut set = HashSet::new();
        set.insert(key1);

        // key2 is equal to key1, so set size should stay 1
        set.insert(key2);
        assert_eq!(set.len(), 1);

        // key3 is different, so set size should become 2
        set.insert(key3);
        assert_eq!(set.len(), 2);
    }
}
