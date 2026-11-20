//! The [`Protocol`] trait: the types and rules of one layout protocol.

use std::{fmt::Debug, hash::Hash};

use flui_foundation::Arity;

use super::context_api::{HitTestContextApi, LayoutContextApi};
use crate::parent_data::ParentData;

// ============================================================================
// SEALED TRAIT
// ============================================================================

/// Private module for sealed trait pattern.
pub(crate) mod sealed {
    /// Sealed marker trait preventing external Protocol implementations.
    pub trait Sealed {}
}

// ============================================================================
// PROTOCOL TRAIT
// ============================================================================

/// A layout protocol: what a parent passes down, what a child answers, and
/// how a position is tested against it.
///
/// Sealed: [`BoxProtocol`](super::BoxProtocol) (2D constraints and sizes) and
/// [`SliverProtocol`](super::SliverProtocol) (scroll-axis constraints and
/// sliver geometry) are the only implementors. Generic code names a
/// protocol's types as `P::Constraints`, `P::Geometry`, `P::Position`, ….
///
/// Paint is not part of the protocol: every render object paints through the
/// same canvas API whatever its protocol.
///
/// # Cache key
///
/// Constraints hold floats, which have no reliable `Hash + Eq`. Rather than
/// hash floats, the protocol separates the two roles:
/// - [`Constraints`](Self::Constraints): the semantic layout input (no
///   `Hash`/`Eq` required);
/// - [`CacheKey`](Self::CacheKey): the hashable fingerprint
///   [`cache_key`](Self::cache_key) derives from it.
///
/// # Thread confinement
///
/// No bound asks for `Send` or `Sync`: every value of a protocol lives in a
/// render tree that one pipeline owner drives on its own thread (ADR-0323 §1).
pub trait Protocol: Debug + Clone + Copy + sealed::Sealed + 'static {
    /// Layout constraints from the parent.
    ///
    /// No `Hash + Eq` required: caching goes through [`Self::CacheKey`].
    type Constraints: Clone + Debug + 'static;

    /// Layout geometry output, returned to the parent after layout.
    type Geometry: Clone + Debug + Default + 'static;

    /// Hashable cache key derived from constraints.
    ///
    /// Implementations compare floats by their bits, never by value.
    type CacheKey: Clone + Debug + Hash + Eq + 'static;

    /// The typed layout context a render object of this protocol lays out
    /// through, for its arity `A` and its children's parent data `PD`.
    type LayoutContext<'ctx, A: Arity, PD: ParentData + Default>: LayoutContextApi<'ctx, Self, A, PD>
    where
        Self: 'ctx;

    /// Hit test position (an [`Offset`](flui_foundation::geometry::Offset)
    /// for the box protocol, a main-axis position for slivers).
    type Position: Clone + Debug + Default + 'static;

    /// Hit test result accumulator.
    ///
    /// **Vocabulary only, with no accumulator behind it today.** Nothing
    /// outside the protocol modules reads a protocol-level result: the driver
    /// builds the real path from `register_self_hit_entry`, which needs no id
    /// because the driver already has one.
    ///
    /// A sliver assembling its own hit path is the consumer this shape is
    /// kept for. Wiring it means the driver bridge reads the result and a test
    /// fails when it stops; anything less is an `add_self` that compiles,
    /// runs and does nothing.
    type HitResult: Default + 'static;

    /// Individual hit test entry. See [`Self::HitResult`] for its status.
    type HitEntry: Clone + Debug + 'static;

    /// The typed hit-test context a render object of this protocol tests a
    /// position through, for its arity `A` and its children's parent data `PD`.
    type HitTestContext<'ctx, A: Arity, PD: ParentData>: HitTestContextApi<'ctx, Self, A, PD>
    where
        Self: 'ctx;

    /// Default parent data for child render objects.
    type DefaultParentData: ParentData + Default;

    /// Protocol-specific erased layout context — the trait-object form of
    /// [`Self::LayoutContext`] without the arity / parent-data type
    /// parameters.
    ///
    /// Used at the
    /// [`RenderObject<P>::perform_layout_raw`](crate::traits::RenderObject::perform_layout_raw)
    /// trait boundary so the pipeline can hand a typed layout context to
    /// a protocol-erased render-object trait method without per-protocol
    /// dispatch in the caller.
    ///
    /// For `BoxProtocol` this
    /// resolves to `dyn ErasedBoxLayoutContext + 'ctx`; for `SliverProtocol`
    /// to `dyn ErasedSliverLayoutContext + 'ctx`. Each per-protocol trait
    /// exposes the small protocol-shared surface (constraints, child ops)
    /// that the [`RenderObject`](crate::traits::RenderObject)
    /// blanket impl needs to reconstruct a typed layout context via a
    /// `Proxy` storage variant on the typed context type.
    type ErasedLayoutContext<'ctx>: ?Sized
    where
        Self: 'ctx;

    /// Per-node layout calculation cache stored on `RenderState<Self>`.
    ///
    /// For `BoxProtocol` this is the four-map
    /// [`BoxLayoutCache`](crate::storage::BoxLayoutCache) (intrinsic
    /// dimensions, dry layout, dry baselines); the sliver protocol carries no cache yet
    /// (`()`), so its `clear` never triggers the boundary-crossing
    /// invalidation escalation.
    type LayoutCache: crate::storage::ProtocolLayoutCache;

    /// Where `RenderState<Self>` keeps the constraints of a node's last
    /// layout and the geometry it produced: inline for the box protocol,
    /// behind one allocation made on first write for the sliver protocol,
    /// whose record would otherwise size every slot of the render slab.
    type LayoutRecord: crate::storage::LayoutRecord<Self::Geometry, Self::Constraints>;

    /// Protocol name for debugging and diagnostics.
    fn name() -> &'static str;

    /// Default geometry for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Validate constraints before layout (returns true if valid).
    fn validate_constraints(_constraints: &Self::Constraints) -> bool {
        true
    }

    /// Derives a cache key from constraints.
    ///
    /// Returns `None` if constraints are not cacheable (e.g., contain NaN).
    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey>;

    /// Normalizes constraints for layout (e.g., clamping negative values).
    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints
    }

    /// Bootstrap the per-instance `IS_RELAYOUT_BOUNDARY` storage flag after
    /// a successful layout pass.
    ///
    /// Default implementation is a no-op (slivers don't use relayout-boundary
    /// semantics today; that's deferred to Core.2). The `BoxProtocol`
    /// override calls [`RenderState::<BoxProtocol>::compute_relayout_boundary`]
    /// with `parent_uses_size = false` and `sized_by_parent = false` —
    /// deriving those parameters exactly lands later in Core.2 alongside
    /// the intrinsic-dimension protocol.
    ///
    /// This bootstrap exists so that
    /// [`PipelineOwner::mark_needs_layout`] has a meaningful
    /// `is_relayout_boundary` answer to read once nodes have laid out at
    /// least once. Pre-bootstrap, all nodes report `false` and propagation
    /// runs to root — the correct fallback (root is the implicit boundary).
    ///
    /// [`RenderState::<BoxProtocol>::compute_relayout_boundary`]: crate::storage::RenderState::compute_relayout_boundary
    /// [`PipelineOwner::mark_needs_layout`]: crate::pipeline::PipelineOwner::mark_needs_layout
    fn bootstrap_relayout_boundary(
        state: &crate::storage::RenderState<Self>,
        sized_by_parent: bool,
        has_parent: bool,
    ) where
        Self: Sized,
    {
        let _ = (state, sized_by_parent, has_parent);
    }

    /// Debug-only check that a freshly computed layout result is well-formed:
    /// a finite geometry that satisfies the constraints it was laid out under.
    ///
    /// Called at every layout-commit site (leaf and container paths). The
    /// default is a no-op — sliver geometry validation is deferred to Core.2.
    /// `BoxProtocol` overrides it: a node's own size must be finite and
    /// within its constraints. The `debug_assert!` bodies compile out in release builds,
    /// leaving the override an empty call the optimizer can drop.
    fn debug_assert_layout_output(constraints: &Self::Constraints, geometry: &Self::Geometry)
    where
        Self: Sized,
    {
        let _ = (constraints, geometry);
    }

    /// Runtime validation for a freshly computed layout result before it is
    /// committed to [`RenderState`](crate::storage::RenderState).
    ///
    /// The default is a no-op. Protocols with recoverable geometry contracts
    /// override this to surface
    /// [`RenderError::InvalidGeometry`](crate::error::RenderError::InvalidGeometry)
    /// while leaving the previous committed geometry intact.
    fn validate_layout_output(
        render_object: &'static str,
        constraints: &Self::Constraints,
        geometry: &Self::Geometry,
    ) -> crate::error::RenderResult<()>
    where
        Self: Sized,
    {
        let _ = (render_object, constraints, geometry);
        Ok(())
    }

    /// Constructs a leaf-mode (no children, no layout callback) erased
    /// layout context from protocol-typed constraints, then invokes `f`
    /// with a `&mut Self::ErasedLayoutContext<'_>` referencing it.
    ///
    /// Closure shape (`FnOnce`) keeps the typed context's lifetime scoped
    /// to the call — the typed `RawBoxLayoutContext::new(...)` value lives on
    /// the caller's stack inside the trait method, the erased coercion
    /// borrows it, and the borrow expires when `f` returns.
    ///
    /// Used by
    /// [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only) for
    /// the leaf / single-node layout path. The pipeline's
    /// `layout_dirty_root` constructs its own typed context with
    /// children access via disjoint borrows and bypasses this helper.
    ///
    /// `text` is the pipeline's text context the leaf measures with (box
    /// protocol only; slivers do not measure text).
    fn with_leaf_erased_ctx<R>(
        constraints: Self::Constraints,
        text: crate::pipeline::TextSource<'_>,
        f: impl FnOnce(&mut Self::ErasedLayoutContext<'_>) -> R,
    ) -> R
    where
        Self: Sized;
}

// ============================================================================
// USAGE BY PARENT (Compose-inspired 3-state)
// ============================================================================

/// How a parent uses a child's geometry during layout.
///
/// This is a 3-state enum inspired by Jetpack Compose's `UsageByParent`
/// (rather than a boolean `parent_uses_size`):
///
/// - `NotUsed` — parent doesn't depend on child's size at all
/// - `InLayout` — parent reads child's geometry during its own layout
/// - `InPlacement` — parent only positions the child (scroll viewport)
///
/// The relayout boundary formula uses this:
/// ```text
/// is_boundary = matches!(usage, NotUsed | InPlacement)
///     || sized_by_parent
///     || constraints.is_tight()
///     || !has_parent
/// ```
///
/// # Why 3 states instead of a boolean
///
/// A boolean `parent_uses_size = false` makes `!false = true` ⇒ every node
/// is a boundary. `InPlacement` is the middle ground: the parent uses the
/// child's size for positioning (scroll offset) but not for its own size
/// computation. This is the common case for viewport → sliver children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum UsageByParent {
    /// Parent doesn't depend on this child's geometry at all.
    /// Changes to this child's size don't affect the parent's layout.
    NotUsed,

    /// Parent reads this child's geometry during its own layout pass.
    /// Changes to this child's size may require the parent to re-layout.
    #[default]
    InLayout,

    /// Parent only positions this child (e.g., scroll viewport placing
    /// sliver children). The parent's own size doesn't depend on this
    /// child's geometry — only the child's position within the parent.
    InPlacement,
}
