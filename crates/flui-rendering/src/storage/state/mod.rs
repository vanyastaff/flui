//! Protocol-specific render state storage with Flutter-compliant dirty
//! tracking.
//!
//! This module provides lock-free state management for render objects following
//! Flutter's exact dirty propagation semantics:
//! - Atomic flags for lock-free dirty tracking (10x faster than RwLock)
//! - Smart propagation that respects relayout/repaint boundaries
//! - Intrinsic size invalidation with parent notification
//! - Pipeline owner integration for efficient batch processing
//!
//! # Design Philosophy
//!
//! - **Flutter-compatible**: Exact Flutter RenderObject dirty tracking
//!   semantics
//! - **Lock-free when possible**: Atomic operations for hot paths
//! - **Smart propagation**: Boundary-aware upward propagation
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
//! # Flutter Protocol Compliance
//!
//! ## Dirty Propagation Rules
//!
//! 1. **markNeedsLayout()**:
//!    - If already dirty → early return (optimization)
//!    - Mark self dirty
//!    - If NOT relayout boundary → propagate to parent recursively
//!    - If IS relayout boundary → register with pipeline owner
//!
//! 2. **markParentNeedsLayout()**:
//!    - Mark self dirty
//!    - ALWAYS propagate to parent (even if relayout boundary)
//!    - Used when intrinsic size changes
//!
//! 3. **markNeedsPaint()**:
//!    - If already dirty → early return
//!    - Mark self dirty
//!    - If NOT repaint boundary → propagate to parent
//!    - If IS repaint boundary → register with pipeline owner
//!
//! # Performance Optimizations
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
//! ## Smart Boundary Detection
//!
//! Early propagation termination at boundaries:
//! - Relayout boundary: Stop layout propagation
//! - Repaint boundary: Stop paint propagation
//! - Reduces work in large trees (O(log n) instead of O(n))
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
//!
//! ## Flutter-Style Dirty Tracking
//!
//! ```rust,ignore
//! // Mark needs layout with automatic propagation
//! state.mark_needs_layout(element_id, tree);
//! // → Propagates up to first relayout boundary
//! // → Boundary registers with pipeline owner
//!
//! // Mark parent needs layout (for intrinsic changes)
//! state.mark_parent_needs_layout(element_id, tree);
//! // → ALWAYS propagates up (even through boundaries)
//! // → Used when min/max intrinsic size changes
//!
//! // Mark needs paint with boundary awareness
//! state.mark_needs_paint(element_id, tree);
//! // → Propagates up to first repaint boundary
//! // → Boundary registers with pipeline owner
//! ```
//!
//! ## Relayout Boundary Optimization
//!
//! ```rust,ignore
//! // Mark as relayout boundary to prevent propagation
//! state.set_relayout_boundary(true);
//!
//! // Now layout changes stop here
//! state.mark_needs_layout(element_id, tree);
//! // → Does NOT propagate to parent
//! // → Registers this element with pipeline owner
//! // → Parent unaffected (huge performance win!)
//! ```

use std::marker::PhantomData;

use once_cell::sync::OnceCell;

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

// Re-export the dirty-propagation trait at this module's path so the
// in-crate import path (`crate::storage::state::RenderDirtyPropagation`)
// remains the same as before the split. The `#[allow(unused_imports)]`
// silences a benign warning when no module currently consumes the trait
// outside the `propagation` submodule and its tests.
#[allow(unused_imports)]
pub use propagation::RenderDirtyPropagation;

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

/// Protocol-specific render state storage with Flutter-compliant dirty
/// tracking.
///
/// This struct provides efficient storage for render object state with:
/// - Lock-free dirty flags using atomic operations
/// - Smart propagation that respects boundaries
/// - Write-once geometry and constraints using `OnceCell`
/// - Atomic offset updates for paint positioning
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
    /// Write-once per layout pass, read many times during paint.
    geometry: OnceCell<ProtocolGeometry<P>>,

    /// Last constraints used for layout.
    ///
    /// For BoxProtocol: BoxConstraints
    /// For SliverProtocol: SliverConstraints
    ///
    /// Used for cache validation and optimization.
    constraints: OnceCell<ProtocolConstraints<P>>,

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
            geometry: OnceCell::new(),
            constraints: OnceCell::new(),
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
            geometry: OnceCell::new(),
            constraints: OnceCell::new(),
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
            geometry: self
                .geometry
                .get()
                .cloned()
                .map_or_else(OnceCell::new, |g| {
                    let cell = OnceCell::new();
                    let _ = cell.set(g);
                    cell
                }),
            constraints: self
                .constraints
                .get()
                .cloned()
                .map_or_else(OnceCell::new, |c| {
                    let cell = OnceCell::new();
                    let _ = cell.set(c);
                    cell
                }),
            offset: AtomicOffset::new(self.offset.load()),
            _phantom: PhantomData,
        }
    }
}
