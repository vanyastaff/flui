//! Protocol capability traits.
//!
//! This module defines the capability traits that protocols compose:
//! - [`LayoutCapability`]: Layout input/output types
//! - [`HitTestCapability`]: Hit test input/output types

use std::{fmt::Debug, hash::Hash};

use flui_tree::Arity;
use flui_types::geometry::Offset;

use crate::parent_data::ParentData;

// ============================================================================
// LAYOUT CAPABILITY
// ============================================================================

/// Capability trait for layout operations.
///
/// Groups all types related to layout: constraints from parent,
/// geometry output, cache key, and the layout context GAT.
///
/// # Cache Key Design
///
/// Constraints contain floats which don't implement `Hash + Eq` reliably.
/// Instead of forcing awkward float hashing, we separate concerns:
/// - `Constraints`: semantic layout input (no Hash/Eq required)
/// - `CacheKey`: hashable fingerprint for caching (derived from constraints)
///
/// This allows proper caching while keeping constraints ergonomic.
pub trait LayoutCapability: Send + Sync + 'static {
    /// Layout constraints from parent.
    ///
    /// No `Hash + Eq` required - use `CacheKey` for caching.
    type Constraints: Clone + Debug + Send + Sync + 'static;

    /// Layout geometry output (returned to parent after layout).
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;

    /// Hashable cache key derived from constraints.
    ///
    /// Used for layout caching. Implementations should handle float
    /// comparison carefully (e.g., using integer bits).
    type CacheKey: Clone + Debug + Hash + Eq + Send + Sync + 'static;

    /// Layout context with Generic Associated Type.
    type Context<'ctx, A: Arity, P: ParentData + Default>: LayoutContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;

    /// Returns the default geometry for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Validates constraints before layout.
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
}

/// Outcome of an on-demand child build-and-layout request (the re-entrant build
/// contract — see `build_and_layout_box_child` on the sliver layout context).
///
/// **Mid-pass-shaped**: the states let the *same* contract be served by either
/// backend without a breaking change — the whole point of locking the shape now.
/// `Ready` carries a **handle** (identity *and* geometry), not bare geometry, so a
/// consumer can position, re-measure, reconcile, or dispose the child it built; a
/// `Ready(geometry-only)` would have forced a breaking signature change the moment
/// a true mid-pass backend or dispose-on-scroll-off needed the child's identity.
///
/// - [`Ready`](ChildLayout::Ready) — the child exists and was laid out **in this
///   pass**; the handle `G` carries its identity + geometry. A true mid-pass
///   backend (build → measure → decide-next, Compose `SubcomposeLayout`-style)
///   returns this; the consumer feeds the real extent back (e.g. to
///   `Virtualizer::set_measured`) and records the handle for later disposal.
/// - [`Scheduled`](ChildLayout::Scheduled) — the child did not exist and was
///   **queued to be built before a later pass** (the v1 next-frame backend: the
///   layout walk's borrows are frozen mid-pass, so a freshly-built child cannot be
///   inserted synchronously and its identity is unknown until the insert applies).
///   The consumer uses the item's *estimate* this pass; the handle + real geometry
///   arrive when a later pass returns `Ready`.
/// - [`NoChild`](ChildLayout::NoChild) — the builder declined (no item at this
///   index): the end of an unknown-length data source. The consumer stops.
/// - [`Unwired`](ChildLayout::Unwired) — this context has no build backend (a
///   leaf/test context, or a not-yet-wired path). **Distinct from `NoChild` on
///   purpose**: a production consumer treats `Unwired` as a bug, because folding
///   it into "end of data" is exactly how a wiring regression silently renders an
///   empty list.
///
/// A consumer that handles `Ready` and `Scheduled` identically-modulo-estimate is
/// **forward-compatible**: swapping the v1 next-frame backend for a future true
/// mid-pass backend only changes which arm fires, never the call site.
///
/// `#[non_exhaustive]` — the backend taxonomy may yet gain a state (e.g. a
/// partial/retry result); a contract whose reason to exist is "do not ossify"
/// must not foreclose that with an exhaustive enum.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ChildLayout<G> {
    /// Built and laid out this pass; the handle `G` carries identity + geometry.
    Ready(G),
    /// Queued to be built before a later pass (v1 next-frame backend).
    Scheduled,
    /// The builder declined — no item at this index (end of the data source).
    NoChild,
    /// No build backend is wired in this context (a bug if reached in production).
    Unwired,
}

/// API for layout context operations.
pub trait LayoutContextApi<'ctx, L: LayoutCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the layout constraints from parent.
    fn constraints(&self) -> &L::Constraints;

    /// Gets the number of children.
    fn child_count(&self) -> usize;

    /// Layouts a child with given constraints.
    fn layout_child(&mut self, index: usize, constraints: L::Constraints) -> L::Geometry;

    /// Positions a child at the given offset.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Gets a child's current geometry (after layout).
    ///
    /// # Direct vs Proxy semantic
    ///
    /// For `BoxLayoutCtx<A, P>` in **Direct** storage mode (constructed
    /// via `new` / `with_children` / `with_layout_callback`), this
    /// returns the pre-populated `children[i].size` — writes from
    /// sibling `layout_child` calls (or pipeline pre-layout) are
    /// visible.
    ///
    /// In **Proxy** storage mode (constructed via
    /// `BoxLayoutCtx::from_erased` inside the
    /// `RenderObject<BoxProtocol>` blanket impl), this returns from a
    /// **local cache populated only by THIS Proxy's `layout_child`
    /// calls** — sibling writes through the underlying Direct ctx are
    /// NOT visible.
    ///
    /// Today this divergence is moot: Proxy storage is entered only
    /// from the blanket bridge, which is called with a fresh Direct
    /// ctx that has no sibling pre-layout. Once the disjoint-borrow
    /// walk is wired, this becomes load-bearing — callers must
    /// invoke `layout_child(i)` to populate the cache rather than rely
    /// on sibling pre-population.
    fn child_geometry(&self, index: usize) -> Option<&L::Geometry>;

    /// Gets a child's parent data.
    fn child_parent_data(&self, index: usize) -> Option<&P>;

    /// Gets mutable reference to child's parent data.
    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P>;
}

// ============================================================================
// HIT TEST CAPABILITY
// ============================================================================

/// Capability trait for hit testing operations.
///
/// Groups all types related to hit testing: position input,
/// result accumulator, entry type, and the hit test context GAT.
pub trait HitTestCapability: Send + Sync + 'static {
    /// Hit test position type (usually Offset for 2D).
    type Position: Clone + Debug + Default + Send + Sync + 'static;

    /// Hit test result accumulator.
    type Result: Default + Send + Sync + 'static;

    /// Individual hit test entry.
    type Entry: Clone + Debug + Send + Sync + 'static;

    /// Hit test context with Generic Associated Type.
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}

/// API for hit test context operations.
pub trait HitTestContextApi<'ctx, H: HitTestCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the hit test position in local coordinates.
    fn position(&self) -> &H::Position;

    /// Gets the hit test result accumulator.
    fn result(&self) -> &H::Result;

    /// Gets mutable reference to hit test result.
    fn result_mut(&mut self) -> &mut H::Result;

    /// Adds a hit entry to the result.
    fn add_hit(&mut self, entry: H::Entry);

    /// Checks if position is inside the given bounds.
    fn is_hit(&self, bounds: flui_types::Rect) -> bool;

    /// Tests a child for hits with position transformation.
    fn hit_test_child(&mut self, index: usize, position: H::Position) -> bool;

    /// Tests a child at its laid-out position (`RenderState.offset`),
    /// resolved by the pipeline driver — the parent supplies no offset.
    ///
    /// Default `false` for contexts without driver plumbing (leaf test
    /// fixtures, the sliver surface until its hit-test walk lands).
    fn hit_test_child_at_layout_offset(&mut self, _index: usize) -> bool {
        false
    }

    /// Adds a transform to the hit test path.
    fn push_transform(&mut self, transform: flui_types::Matrix4);

    /// Removes the most recent transform.
    fn pop_transform(&mut self);

    /// Adds an offset transform (convenience method).
    fn push_offset(&mut self, offset: Offset) {
        self.push_transform(flui_types::Matrix4::translation(
            offset.dx.get(),
            offset.dy.get(),
            0.0,
        ));
    }
}

// ============================================================================
// TYPE ALIASES (for Protocol usage)
// ============================================================================

use super::protocol::Protocol;

/// Constraints type for a protocol.
pub type ProtocolConstraints<P> = <<P as Protocol>::Layout as LayoutCapability>::Constraints;

/// Geometry type for a protocol.
pub type ProtocolGeometry<P> = <<P as Protocol>::Layout as LayoutCapability>::Geometry;

/// Hit test position type for a protocol.
pub type ProtocolPosition<P> = <<P as Protocol>::HitTest as HitTestCapability>::Position;

/// Hit test result type for a protocol.
pub type ProtocolHitResult<P> = <<P as Protocol>::HitTest as HitTestCapability>::Result;

// `ProtocolLayoutCtx` / `ProtocolHitTestCtx` aliases were removed with the
// dead `ProtocolRenderObject` trait (their only user). The live layout /
// hit-test path uses the erased `LayoutCtxErased` GAT and the ergonomic
// `BoxLayoutContext` / `SliverLayoutContext` wrappers, not these raw aliases.
