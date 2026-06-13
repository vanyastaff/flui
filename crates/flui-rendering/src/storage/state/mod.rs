//! Protocol-specific render state storage.
//!
//! This module provides lock-free state storage for render objects:
//! - Atomic flags for lock-free dirty tracking
//! - Mutable geometry and constraints (`Option<T>`, mutated each layout
//!   pass via `&mut self` accessors)
//! - Atomic offset updates for paint positioning
//! - Boundary accessors (relayout/repaint) for pipeline-owner registration
//!
//! **D-block PR-A1 U14 migration (2026-05-23):** geometry and constraints
//! previously used `OnceCell` for write-once semantics; the resulting
//! panic-on-second-set crashed any re-layout. Migrated to `Option<T>` so
//! re-layout overwrites unconditionally, mirroring Flutter
//! `.flutter/.../object.dart:2865` `_size = size` straight assignment.
//! `set_constraints`/`set_geometry`/`set_size`/`set_sliver_geometry` now
//! take `&mut self`; production callers (`RenderEntry::layout` and the
//! RenderBox/RenderSliver helpers) already hold a mut state borrow.
//!
//! Production dirty marking does **not** live here. It is driven by
//! [`PipelineOwner::mark_needs_layout`](crate::pipeline::PipelineOwner::mark_needs_layout)
//! (D-block PR-A1 U15 — Flutter `markNeedsLayout` walk) invoked from
//! `flui-view::element::behavior_commons::mark_render_needs_layout_and_paint`.
//! The boundary-aware propagation methods that previously hung off
//! `RenderState<P>` were removed in U3 of the flui-rendering Phase 1 zombie
//! cleanup as unreachable code; the `RenderDirtyPropagation` trait that
//! PR #81 U3 preserved as a "cost-cheap option" was deleted in cycle 4 R-5
//! because its `ElementId` typing did not match the crate's `RenderId` key —
//! see the `propagation` submodule's module-level docstring for the audit
//! trail (`docs/research/2026-05-22-flui-rendering-engine-audit.md`).
//!
//! # Design Philosophy
//!
//! - **Lock-free when possible**: Atomic operations for hot paths (flags + offset)
//! - **Cache-friendly**: Optimized memory layout for performance
//! - **Zero-cost abstractions**: No overhead for unused features
//!
//! # Architecture
//!
//! ```text
//! RenderState<P>
//!  ├── flags: AtomicRenderFlags (lock-free, &self mutation)
//!  ├── geometry: Option<ProtocolGeometry<P>> (&mut self set/clear; Flutter parity)
//!  ├── constraints: Option<ProtocolConstraints<P>> (&mut self set/clear)
//!  └── offset: AtomicOffset (lock-free atomic updates)
//! ```
//!
//! # Performance Notes
//!
//! ## Lock-Free Dirty Flags
//!
//! Dirty flags use atomic operations for zero-contention access:
//! - Read: Single atomic load (≈1ns)
//! - Write: Single atomic fetch_or/fetch_and (≈1-5ns)
//! - No blocking, no context switches
//!
//! ## Mutable Geometry/Constraints
//!
//! Plain `Option<T>` fields mutated via `&mut self`:
//! - Set: Direct field assignment (≈1ns)
//! - Read: Direct field load (zero-cost via `Copy` types)
//! - Re-layout: Idempotent overwrite — no clear-before-set required
//!
//! Thread-safety on these fields is enforced by Rust's borrow checker:
//! pipeline layout is single-threaded by contract (the contract a render
//! pass holds `&mut RenderEntry<P>` exclusively for its node, so the inner
//! `&mut RenderState<P>` access is statically race-free).
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxRenderState, RenderFlags};
//!
//! let mut state = BoxRenderState::new();
//!
//! // Lock-free flag checks (hot path)
//! if state.needs_layout() {
//!     // ... perform layout ...
//!     state.clear_needs_layout();
//! }
//!
//! // Write geometry idempotently after layout
//! state.set_geometry(computed_size);
//!
//! // Read geometry many times during paint (Copy, zero-cost)
//! let size = state.geometry();
//! ```

use std::marker::PhantomData;

// NOTE: `OnceCell` was previously imported here for `geometry`/`constraints`
// write-once semantics; D-block PR-A1 U14 migrated both fields to `Option<T>`
// so the import is no longer needed.

use crate::protocol::{
    BoxProtocol, Protocol, ProtocolConstraints, ProtocolGeometry, SliverProtocol,
};
use crate::storage::flags::{AtomicRenderFlags, RenderFlags};

mod constraints;
mod flags;
mod geometry;
mod layout_cache;
mod offset;

pub use layout_cache::{BoxLayoutCache, IntrinsicDimension, ProtocolLayoutCache};

#[cfg(test)]
mod tests;

use offset::AtomicOffset;

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Render state for Box protocol (uses Size and BoxConstraints).
pub type BoxRenderState = RenderState<BoxProtocol>;

/// Render state for Sliver protocol (uses SliverGeometry and
/// SliverConstraints).
pub type SliverRenderState = RenderState<SliverProtocol>;

// ============================================================================
// RENDER STATE
// ============================================================================

/// Protocol-specific render state storage.
///
/// This struct provides efficient storage for render object state with:
/// - Lock-free dirty flags using atomic operations
/// - Boundary accessors (relayout/repaint) for pipeline-owner registration
/// - Write-once geometry and constraints using `OnceCell`
/// - Atomic offset updates for paint positioning
///
/// Boundary-aware dirty propagation is **not** performed here. Production
/// dirty marking goes through `PipelineOwner::add_node_needing_layout /
/// add_node_needing_paint` invoked from `flui-view` and `flui-hot-reload`.
///
/// # Memory Layout
///
/// ```text
/// [AtomicRenderFlags: 4 bytes] - Hot path, always accessible
/// [OnceCell<Geometry>: 16-24 bytes] - Write-once, read-many
/// [OnceCell<Constraints>: 16-24 bytes] - Write-once, read-many
/// [AtomicOffset: 8 bytes] - Lock-free f32 pair
/// ```
///
/// Total: ≈44-60 bytes depending on protocol geometry size
///
/// # Thread Safety
///
/// All methods are thread-safe:
/// - Atomic operations use appropriate memory ordering
/// - OnceCell provides interior mutability safely
/// - No data races possible
///
/// # Performance Characteristics
///
/// - Flag checks: O(1), ≈1ns, lock-free
/// - Flag updates: O(1), ≈1-5ns, lock-free
/// - Geometry reads: O(1), ≈1ns after first set
/// - Geometry writes: O(1), ≈10ns first time, then cached
/// - Offset reads: O(1), ≈1ns, lock-free
/// - Offset writes: O(1), ≈5ns, lock-free
#[derive(Debug)]
pub struct RenderState<P: Protocol> {
    /// Atomic flags for lock-free dirty state checks.
    ///
    /// Hot path operations (needs_layout, needs_paint) go here.
    /// Uses single atomic operations for best performance.
    flags: AtomicRenderFlags,

    /// Cached layout result (protocol-specific geometry).
    ///
    /// For BoxProtocol: Size
    /// For SliverProtocol: SliverGeometry
    ///
    /// Mutated each layout pass via [`set_geometry`](Self::set_geometry).
    /// Migrated from `OnceCell` to `Option` in D-block PR-A1 U14 — re-layout
    /// must be idempotent (frame-2 panic fix per memo D2; Flutter `_size`
    /// is straight-assigned each layout at `.flutter/.../object.dart:2865`).
    geometry: Option<ProtocolGeometry<P>>,

    /// Last constraints used for layout.
    ///
    /// For BoxProtocol: BoxConstraints
    /// For SliverProtocol: SliverConstraints
    ///
    /// Used for cache validation and the relayout-boundary short-circuit.
    /// Migrated from `OnceCell` to `Option` in D-block PR-A1 U14 alongside
    /// `geometry`; see field doc above for rationale.
    constraints: Option<ProtocolConstraints<P>>,

    /// Offset relative to parent (atomic for lock-free updates).
    ///
    /// Set by parent during layout, read during paint and hit testing.
    offset: AtomicOffset,

    /// Per-node layout calculation cache (Flutter `_LayoutCacheStorage`):
    /// memoized intrinsic dimensions / dry layout / dry baselines.
    /// Cleared by `mark_needs_layout`; a non-empty clear escalates the
    /// invalidation past relayout boundaries (box.dart:2840).
    layout_cache: P::LayoutCache,

    /// Persistent parent data for this node, set by the parent during
    /// layout. Survives across frames — the parent writes it once, and
    /// subsequent reads return the cached value.
    ///
    /// This replaces the transient `Option<Box<dyn ParentData>>` in
    /// `ErasedChildState` which was rebuilt every frame with `None`.
    ///
    /// # Type erasure
    ///
    /// The parent data type is determined by the PARENT's
    /// `RenderBox::ParentData` associated type, not this node's protocol.
    /// We store it as `Option<Box<dyn ParentData>>` (type-erased) because
    /// `RenderState<P>` cannot name the parent's concrete type.
    /// The parent downcasts via `downcast_ref::<T>()` when reading.
    parent_data: Option<Box<dyn crate::parent_data::ParentData>>,

    /// Protocol marker (zero-sized).
    _phantom: PhantomData<P>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Creates a new render state with default dirty flags.
    ///
    /// Initial state:
    /// - NEEDS_LAYOUT flag set (requires initial layout)
    /// - NEEDS_PAINT flag set (requires initial paint)
    /// - No geometry or constraints (will be set during first layout)
    /// - Offset at origin (will be set by parent during layout)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let state = BoxRenderState::new();
    /// assert!(state.needs_layout());
    /// assert!(state.needs_paint());
    /// ```
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT),
            geometry: None,
            constraints: None,
            offset: AtomicOffset::new(flui_types::Offset::ZERO),
            layout_cache: P::LayoutCache::default(),
            parent_data: None,
            _phantom: PhantomData,
        }
    }

    /// Creates a render state with custom initial flags.
    ///
    /// Use this when you need specific initial flags (e.g., for testing
    /// or special initialization scenarios).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create state that doesn't need initial layout
    /// let state = BoxRenderState::with_flags(RenderFlags::empty());
    /// assert!(!state.needs_layout());
    /// ```
    pub fn with_flags(flags: RenderFlags) -> Self {
        Self {
            flags: AtomicRenderFlags::new(flags),
            geometry: None,
            constraints: None,
            offset: AtomicOffset::new(flui_types::Offset::ZERO),
            layout_cache: P::LayoutCache::default(),
            parent_data: None,
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// PARENT DATA ACCESS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Returns a reference to the persistent parent data, if set.
    ///
    /// Parent data is set by the parent during layout and survives
    /// across frames. Returns `None` if no parent data has been set yet.
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn crate::parent_data::ParentData> {
        self.parent_data.as_deref()
    }

    /// Returns a mutable reference to the persistent parent data, if set.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn crate::parent_data::ParentData> {
        self.parent_data.as_deref_mut()
    }

    /// Sets (or replaces) the persistent parent data.
    ///
    /// Called by the parent during layout to store metadata about this
    /// child (flex factor, stack position, etc.). The data persists
    /// across frames until the parent replaces it.
    #[inline]
    pub fn set_parent_data(&mut self, data: Box<dyn crate::parent_data::ParentData>) {
        self.parent_data = Some(data);
    }

    /// Returns a downcasted reference to the parent data, if the type matches.
    ///
    /// This is the typed read path — the parent calls this with its
    /// concrete `ParentData` type to recover the typed data without
    /// `dyn` dispatch on subsequent reads.
    #[inline]
    pub fn parent_data_as<T: crate::parent_data::ParentData>(&self) -> Option<&T> {
        self.parent_data
            .as_ref()
            .and_then(|d| d.downcast_ref::<T>())
    }

    /// Returns a downcasted mutable reference to the parent data, if the type matches.
    #[inline]
    pub fn parent_data_as_mut<T: crate::parent_data::ParentData>(&mut self) -> Option<&mut T> {
        self.parent_data
            .as_mut()
            .and_then(|d| d.downcast_mut::<T>())
    }
}

// ============================================================================
// LAYOUT CACHE ACCESS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Read access to the layout calculation cache (peek path of the
    /// pipeline's memoizing walks).
    pub fn layout_cache(&self) -> &P::LayoutCache {
        &self.layout_cache
    }

    /// Mutable access to the layout calculation cache. The pipeline
    /// memoizes intrinsic/dry-layout queries here; render objects never
    /// touch it.
    pub fn layout_cache_mut(&mut self) -> &mut P::LayoutCache {
        &mut self.layout_cache
    }

    /// Clears the layout cache, returning whether anything WAS cached.
    ///
    /// `true` means an ancestor's layout consumed this node's
    /// intrinsics/baseline, so the caller must escalate the invalidation
    /// to the parent even across a relayout boundary
    /// (Flutter `RenderBox.markNeedsLayout`, box.dart:2840).
    pub fn clear_layout_cache(&mut self) -> bool {
        self.layout_cache.clear()
    }
}

impl<P: Protocol> Default for RenderState<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Clone for RenderState<P>
where
    ProtocolGeometry<P>: Clone,
    ProtocolConstraints<P>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            flags: AtomicRenderFlags::new(self.flags.load()),
            geometry: self.geometry.clone(),
            constraints: self.constraints.clone(),
            offset: AtomicOffset::new(self.offset.load()),
            // Memoized results are node-local; a cloned state starts cold.
            layout_cache: P::LayoutCache::default(),
            parent_data: self.parent_data.clone(),
            _phantom: PhantomData,
        }
    }
}
