//! Protocol-specific render state storage.
//!
//! This module provides lock-free state storage for render objects:
//! - Atomic flags for lock-free dirty tracking
//! - Write-once geometry and constraints using `OnceCell`
//! - Atomic offset updates for paint positioning
//! - Boundary accessors (relayout/repaint) for pipeline-owner registration
//!
//! Production dirty marking does **not** live here. It is driven by
//! `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked
//! from `flui-view` and `flui-hot-reload`. The boundary-aware propagation
//! methods that previously hung off `RenderState<P>` were removed in U3 of
//! the flui-rendering Phase 1 zombie cleanup as unreachable code; the
//! `RenderDirtyPropagation` trait that PR #81 U3 preserved as a "cost-cheap
//! option" was deleted in cycle 4 R-5 because its `ElementId` typing did not
//! match the crate's `RenderId` key — see the `propagation` submodule's
//! module-level docstring for the rationale and the audit trail
//! (`docs/research/2026-05-22-flui-rendering-engine-audit.md`).
//!
//! # Design Philosophy
//!
//! - **Lock-free when possible**: Atomic operations for hot paths
//! - **Cache-friendly**: Optimized memory layout for performance
//! - **Zero-cost abstractions**: No overhead for unused features
//!
//! # Architecture
//!
//! ```text
//! RenderState<P>
//!  ├── flags: AtomicRenderFlags (lock-free, always accessible)
//!  ├── geometry: OnceCell<ProtocolGeometry<P>> (write-once, read-many)
//!  ├── constraints: OnceCell<ProtocolConstraints<P>> (write-once, read-many)
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
//! ## Write-Once Geometry/Constraints
//!
//! Uses `OnceCell` for write-once, read-many pattern:
//! - First layout: One atomic CAS to initialize
//! - Subsequent reads: Zero-cost (just a pointer load)
//! - Relayout: Clear and reinitialize (rare operation)
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxRenderState, RenderFlags};
//!
//! let state = BoxRenderState::new();
//!
//! // Lock-free flag checks (hot path)
//! if state.needs_layout() {
//!     // ... perform layout ...
//!     state.clear_needs_layout();
//! }
//!
//! // Write geometry once after layout
//! state.set_geometry(computed_size);
//!
//! // Read geometry many times during paint (zero-cost)
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
mod offset;
mod propagation;

#[cfg(test)]
mod tests;

// The `propagation` submodule is intentionally body-less after cycle 4 R-5
// (see its module-level docstring). No re-export is needed; the file is kept
// as a placeholder so a future viewport-invalidation hook lands in a known
// location.

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
            _phantom: PhantomData,
        }
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
            _phantom: PhantomData,
        }
    }
}
