//! A render object's dirty and boundary flags.
//!
//! Each render object tracks, in one `u32` bitset:
//! - what it needs: layout, paint, compositing bits, semantics;
//! - what it is: a relayout boundary, a repaint boundary;
//! - what the pipeline propagates and remembers between passes.
//!
//! [`RenderFlagsCell`] holds the set behind `&self`. It is a `Cell`, not an
//! atomic: the render tree is `!Send + !Sync` and one thread runs every
//! pipeline pass (ADR-0323 §1), so a flag needs interior mutability and no
//! ordering. A mark from another thread reaches the tree through
//! `RenderInvalidationHandle`'s queue, never through these bits.
//!
//! ```rust
//! use flui_rendering::storage::{RenderFlags, RenderFlagsCell};
//!
//! let flags = RenderFlagsCell::empty();
//! flags.mark_needs_layout();
//! assert!(flags.needs_layout());
//! flags.remove(RenderFlags::NEEDS_LAYOUT);
//! assert!(!flags.contains(RenderFlags::NEEDS_LAYOUT));
//! ```

use std::{cell::Cell, fmt};

use bitflags::bitflags;

bitflags! {
    /// Per-render-node state flags stored in a compact bitset.
    ///
    /// These flags track the dirty state of a render object and its properties.
    /// A node holds its set in a [`RenderFlagsCell`].
    ///
    /// # Flag Categories
    ///
    /// ## Dirty Flags (require processing)
    /// - `NEEDS_LAYOUT` - Layout recomputation required
    /// - `NEEDS_PAINT` - Painting pass required
    /// - `NEEDS_COMPOSITING` - Compositing bits update required
    /// - `NEEDS_SEMANTICS` - Semantics (accessibility) update required
    ///
    /// ## Boundary Flags (optimization)
    /// - `IS_RELAYOUT_BOUNDARY` - Layout change isolation boundary
    /// - `IS_REPAINT_BOUNDARY` - Paint change isolation boundary
    ///
    /// # Memory Layout
    ///
    /// Stored as a single `u32` (4 bytes) with bit positions:
    /// ```text
    /// Bit 0: NEEDS_LAYOUT
    /// Bit 1: NEEDS_PAINT
    /// Bit 2: NEEDS_COMPOSITING
    /// Bit 3: IS_RELAYOUT_BOUNDARY
    /// Bit 4: IS_REPAINT_BOUNDARY
    /// Bit 5: NEEDS_SEMANTICS
    /// Bits 6-7: unused
    /// Bit 8: NEEDS_LAYOUT_PROPAGATION
    /// Bit 9: NEEDS_PAINT_PROPAGATION
    /// Bit 10: WAS_REPAINT_BOUNDARY
    /// Bit 11: NEEDS_COMPOSITING_BITS_UPDATE
    /// Bit 12: GEOMETRY_DEGRADED
    /// Bit 13: NEEDS_COMPOSITED_LAYER_UPDATE
    /// Bit 14: LAID_OUT_SINCE_PAINT
    /// Bit 15: PAINT_CHANGED
    /// Bits 16-31: Reserved for future use
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RenderFlags: u32 {
        // ===== Dirty Flags (Processing Required) =====

        /// Layout recomputation required.
        ///
        /// Set when:
        /// - Constraints change
        /// - Children added/removed
        /// - Properties affecting layout change
        const NEEDS_LAYOUT = 1 << 0;

        /// Painting pass required.
        ///
        /// Set when:
        /// - Visual properties change (color, opacity, etc.)
        /// - Layout changes (implies paint)
        /// - Decoration changes
        const NEEDS_PAINT = 1 << 1;

        /// Compositing bits update required.
        ///
        /// Set when:
        /// - Repaint boundary status changes
        /// - Opacity changes
        /// - Transform changes requiring new layer
        const NEEDS_COMPOSITING = 1 << 2;

        /// Semantics (accessibility) update required.
        ///
        /// Set when:
        /// - Semantic properties change
        /// - Structure changes affecting a11y tree
        /// - Label or hint text changes
        const NEEDS_SEMANTICS = 1 << 5;

        // ===== Boundary Flags (Optimization) =====

        /// Layout change isolation boundary.
        ///
        /// When set, layout changes don't propagate to parent.
        /// This creates a relayout boundary that limits the scope
        /// of layout computation.
        const IS_RELAYOUT_BOUNDARY = 1 << 3;

        /// Paint change isolation boundary.
        ///
        /// When set, creates a compositing layer that can be
        /// cached and reused. Paint changes below this boundary
        /// don't require repainting ancestors.
        const IS_REPAINT_BOUNDARY = 1 << 4;

        // ===== Propagation Flags (Internal) =====

        /// Layout needs to propagate to parent.
        ///
        /// Internal flag used during layout phase to track
        /// which nodes need to notify their parents.
        const NEEDS_LAYOUT_PROPAGATION = 1 << 8;

        /// Paint needs to propagate to parent.
        ///
        /// Internal flag used during paint phase to track
        /// which nodes need to notify their parents.
        const NEEDS_PAINT_PROPAGATION = 1 << 9;

        /// Previous frame's `IS_REPAINT_BOUNDARY` value.
        ///
        /// Written by the paint phase (`PipelineOwner::paint_subtree`)
        /// when a node is painted. Read by compositing-bits propagation to
        /// detect a transition into or out of repaint-boundary status.
        ///
        /// It lives here rather than on the `RenderObject<P>` trait surface so
        /// the paint phase can flip it through `&self` without borrowing the
        /// render object mutably.
        const WAS_REPAINT_BOUNDARY = 1 << 10;

        /// Marks a node whose compositing-bits subtree walk must be
        /// re-run on the next `run_compositing` invocation.
        ///
        /// Distinct from `NEEDS_COMPOSITING` (bit 2): this flag is the
        /// *scheduling* signal (the node is queued in
        /// `dirty.needs_compositing` and the walk has work to do here),
        /// whereas `NEEDS_COMPOSITING` is the *computed result* (the
        /// node — or some descendant — needs a compositing layer for
        /// the upcoming paint).
        ///
        /// Set through
        /// [`PipelineOwner::mark_needs_compositing_bits_update`](crate::pipeline::PipelineOwner::mark_needs_compositing_bits_update),
        /// which performs the ancestor walk before it
        /// queues the responsible root. Setting this flag and pushing a raw
        /// queue entry separately is insufficient because it can skip that
        /// walk.
        ///
        /// Cleared by `run_compositing` after the subtree walk finishes
        /// (analogous to how `NEEDS_LAYOUT` clears post-layout).
        const NEEDS_COMPOSITING_BITS_UPDATE = 1 << 11;

        /// The geometry this node last committed came from a pass that read a
        /// stand-in: a descendant's layout failed and its caller continued on
        /// a substitute, or a poisoned descendant served one.
        ///
        /// Sticky across frames, because the geometry is: a later pass that
        /// serves this node's cached geometry (clean, same constraints) never
        /// walks the broken descendant and would otherwise look healthy. It is
        /// cleared the first time the node completes a pass in which nothing
        /// below it degraded.
        const GEOMETRY_DEGRADED = 1 << 12;

        /// This node's own composited-layer properties changed, but nothing it
        /// painted did.
        ///
        /// Set when a property that lands ONLY on a layer this node pushes
        /// changes — an opacity's alpha, a transform's matrix — so the frame
        /// can rebuild that layer and replay the rest of the enclosing
        /// boundary's retained output instead of repainting the subtree.
        ///
        /// Distinct from [`Self::NEEDS_PAINT`] and deliberately weaker: a node
        /// carrying this flag is NOT dirty for paint, and the enclosing
        /// boundary it enqueues is not either. Paint always wins — see
        /// `Scheduler::mark_needs_composited_layer_update`, which refuses to
        /// set this on a node already needing paint.
        const NEEDS_COMPOSITED_LAYER_UPDATE = 1 << 13;

        /// The node laid out since its last paint.
        ///
        /// Layout marks paint on its dirty root and on each repaint boundary
        /// it lays out, not on every node, so a node inside a boundary can
        /// change what it paints with [`Self::NEEDS_PAINT`] clear. Set by the
        /// layout walk on every node it lays out rather than serves from its
        /// cache, and taken by the paint walk on every node it reaches: a
        /// node carrying it records its chunks again (ADR-0304 §2), and so
        /// do its descendants, which it may have moved.
        const LAID_OUT_SINCE_PAINT = 1 << 14;

        /// The node itself was marked for paint since its last paint.
        ///
        /// [`Self::NEEDS_PAINT`] is also set on every ancestor a mark walks
        /// up through; this one only on the node the mark names, whose own
        /// paint changed and with it, perhaps, where it paints its children.
        /// Taken by the paint walk on every node it reaches: the descendants
        /// of a node carrying it record their chunks again (ADR-0304 §2).
        const PAINT_CHANGED = 1 << 15;
    }
}

impl RenderFlags {
    /// Returns all dirty flags (flags requiring processing).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlags;
    ///
    /// let dirty = RenderFlags::dirty_flags();
    /// assert!(dirty.contains(RenderFlags::NEEDS_LAYOUT));
    /// assert!(dirty.contains(RenderFlags::NEEDS_PAINT));
    /// ```
    pub const fn dirty_flags() -> Self {
        Self::NEEDS_LAYOUT
            .union(Self::NEEDS_PAINT)
            .union(Self::NEEDS_COMPOSITING)
            .union(Self::NEEDS_SEMANTICS)
    }

    /// Returns all boundary flags (optimization flags).
    pub const fn boundary_flags() -> Self {
        Self::IS_RELAYOUT_BOUNDARY.union(Self::IS_REPAINT_BOUNDARY)
    }

    /// Returns whether any dirty flags are set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlags;
    ///
    /// let clean = RenderFlags::empty();
    /// assert!(!clean.is_dirty());
    ///
    /// let dirty = RenderFlags::NEEDS_LAYOUT;
    /// assert!(dirty.is_dirty());
    /// ```
    #[inline]
    pub const fn is_dirty(self) -> bool {
        self.intersects(Self::dirty_flags())
    }

    /// Returns whether the node is clean (no dirty flags).
    #[inline]
    pub const fn is_clean(self) -> bool {
        !self.is_dirty()
    }
}

impl Default for RenderFlags {
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Display for RenderFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "RenderFlags(empty)");
        }

        let mut flags = Vec::new();
        if self.contains(Self::NEEDS_LAYOUT) {
            flags.push("NEEDS_LAYOUT");
        }
        if self.contains(Self::NEEDS_COMPOSITED_LAYER_UPDATE) {
            flags.push("NEEDS_COMPOSITED_LAYER_UPDATE");
        }
        if self.contains(Self::NEEDS_PAINT) {
            flags.push("NEEDS_PAINT");
        }
        if self.contains(Self::NEEDS_COMPOSITING) {
            flags.push("NEEDS_COMPOSITING");
        }
        if self.contains(Self::NEEDS_SEMANTICS) {
            flags.push("NEEDS_SEMANTICS");
        }
        if self.contains(Self::IS_RELAYOUT_BOUNDARY) {
            flags.push("IS_RELAYOUT_BOUNDARY");
        }
        if self.contains(Self::IS_REPAINT_BOUNDARY) {
            flags.push("IS_REPAINT_BOUNDARY");
        }
        if self.contains(Self::WAS_REPAINT_BOUNDARY) {
            flags.push("WAS_REPAINT_BOUNDARY");
        }

        write!(f, "RenderFlags({})", flags.join(" | "))
    }
}

// ============================================================================
// RENDER FLAGS CELL
// ============================================================================

/// A [`RenderFlags`] set that a render node changes through `&self`.
///
/// A `Cell`: the tree is owned by one thread, so no operation needs to be
/// atomic (see the module docs).
#[derive(Debug)]
pub struct RenderFlagsCell {
    bits: Cell<RenderFlags>,
}

impl RenderFlagsCell {
    /// Creates a flag set holding `flags`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
    /// assert!(flags.needs_layout());
    /// ```
    pub const fn new(flags: RenderFlags) -> Self {
        Self {
            bits: Cell::new(flags),
        }
    }

    /// Creates an empty flag set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// assert!(flags.is_clean());
    /// ```
    pub const fn empty() -> Self {
        Self::new(RenderFlags::empty())
    }

    // ========================================================================
    // LOW-LEVEL OPERATIONS
    // ========================================================================

    /// The current flags.
    ///
    /// Uses `Acquire` ordering to observe prior mutations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT);
    /// let current = flags.load();
    /// assert!(current.contains(RenderFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn load(&self) -> RenderFlags {
        self.bits.get()
    }

    /// Replaces the whole set.
    ///
    /// Uses `Release` ordering to publish the new state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.store(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
    /// assert!(flags.needs_layout());
    /// assert!(flags.needs_paint());
    /// ```
    #[inline]
    pub fn store(&self, flags: RenderFlags) {
        self.bits.set(flags);
    }

    /// Clears `flag` and answers whether it was set.
    #[inline]
    fn take(&self, flag: RenderFlags) -> bool {
        let bits = self.bits.get();
        self.bits.set(bits.difference(flag));
        bits.contains(flag)
    }

    /// Checks if the specified flag is set.
    ///
    /// # Performance
    ///
    /// This is an extremely fast operation (~1ns) suitable for hot paths.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT);
    /// assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
    /// assert!(!flags.contains(RenderFlags::NEEDS_PAINT));
    /// ```
    #[inline]
    pub fn contains(&self, flag: RenderFlags) -> bool {
        self.load().contains(flag)
    }

    /// Sets a single flag.
    ///
    /// Uses `AcqRel` ordering for read-modify-write correctness.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.set(RenderFlags::NEEDS_LAYOUT);
    /// assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn set(&self, flag: RenderFlags) {
        self.bits.set(self.bits.get() | flag);
    }

    /// Removes a single flag.
    ///
    /// Uses `AcqRel` ordering for read-modify-write correctness.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT);
    /// flags.remove(RenderFlags::NEEDS_LAYOUT);
    /// assert!(!flags.contains(RenderFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn remove(&self, flag: RenderFlags) {
        self.bits.set(self.bits.get().difference(flag));
    }

    /// Toggles a flag.
    ///
    /// Uses `AcqRel` ordering for read-modify-write correctness.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.toggle(RenderFlags::NEEDS_LAYOUT);
    /// assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
    /// flags.toggle(RenderFlags::NEEDS_LAYOUT);
    /// assert!(!flags.contains(RenderFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn toggle(&self, flag: RenderFlags) {
        self.bits.set(self.bits.get() ^ flag);
    }

    /// Inserts several flags.
    ///
    /// Uses `AcqRel` ordering for read-modify-write correctness.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.insert(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
    /// assert!(flags.needs_layout());
    /// assert!(flags.needs_paint());
    /// ```
    #[inline]
    pub fn insert(&self, flags: RenderFlags) {
        self.bits.set(self.bits.get() | flags);
    }

    /// Clears all flags.
    ///
    /// Uses `Release` ordering to publish the cleared state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::{RenderFlagsCell, RenderFlags};
    ///
    /// let flags = RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
    /// flags.clear();
    /// assert!(flags.is_clean());
    /// ```
    #[inline]
    pub fn clear(&self) {
        self.bits.set(RenderFlags::empty());
    }

    // ========================================================================
    // HIGH-LEVEL CONVENIENCE API
    // ========================================================================

    /// Marks the render object as needing layout.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.mark_needs_layout();
    /// assert!(flags.needs_layout());
    /// ```
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.set(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clears the needs-layout flag.
    ///
    /// Called after successful layout completion.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.mark_needs_layout();
    /// // ... perform layout ...
    /// flags.clear_needs_layout();
    /// assert!(!flags.needs_layout());
    /// ```
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Checks if the render object needs layout.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// assert!(!flags.needs_layout());
    /// flags.mark_needs_layout();
    /// assert!(flags.needs_layout());
    /// ```
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Marks the render object as needing paint.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.mark_needs_paint();
    /// assert!(flags.needs_paint());
    /// ```
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.set(RenderFlags::NEEDS_PAINT);
    }

    /// Clears the needs-paint flag.
    ///
    /// Called after successful paint completion.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Checks if the render object needs paint.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Marks the render object as needing a composited-layer update.
    ///
    /// Only sets the flag. The eligibility decision and the
    /// enqueue live in `Scheduler::mark_needs_composited_layer_update`.
    #[inline]
    pub fn mark_needs_composited_layer_update(&self) {
        self.set(RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    /// Clears the needs-composited-layer-update flag.
    ///
    /// Called wherever [`Self::clear_needs_paint`] is, because a repaint
    /// subsumes a layer-property update.
    #[inline]
    pub fn clear_needs_composited_layer_update(&self) {
        self.remove(RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    /// Checks if the render object needs a composited-layer update.
    #[inline]
    pub fn needs_composited_layer_update(&self) -> bool {
        self.contains(RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE)
    }

    /// Records that the render object laid out since its last paint.
    #[inline]
    pub fn mark_laid_out_since_paint(&self) {
        self.set(RenderFlags::LAID_OUT_SINCE_PAINT);
    }

    /// Clears the laid-out-since-paint flag and answers whether it was set.
    #[inline]
    pub fn take_laid_out_since_paint(&self) -> bool {
        self.take(RenderFlags::LAID_OUT_SINCE_PAINT)
    }

    /// Records that the render object itself was marked for paint.
    #[inline]
    pub fn mark_paint_changed(&self) {
        self.set(RenderFlags::PAINT_CHANGED);
    }

    /// Clears the paint-changed flag and answers whether it was set.
    #[inline]
    pub fn take_paint_changed(&self) -> bool {
        self.take(RenderFlags::PAINT_CHANGED)
    }

    /// Marks the render object as needing compositing update.
    #[inline]
    pub fn mark_needs_compositing(&self) {
        self.set(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Clears the needs-compositing flag.
    #[inline]
    pub fn clear_needs_compositing(&self) {
        self.remove(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Checks if the render object needs compositing update.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Marks the render object as needing a compositing-bits subtree walk
    /// on the next `run_compositing` invocation.
    ///
    /// Distinct from `mark_needs_compositing` — this is the *scheduling*
    /// signal used by
    /// [`PipelineOwner::mark_needs_compositing_bits_update`](crate::pipeline::PipelineOwner::mark_needs_compositing_bits_update),
    /// whereas `NEEDS_COMPOSITING` is the *computed result* of the
    /// walk.
    #[inline]
    pub fn mark_needs_compositing_bits_update(&self) {
        self.set(RenderFlags::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Clears the needs-compositing-bits-update flag.
    ///
    /// Called by `run_compositing` after the subtree walk visits this
    /// node and updates its `NEEDS_COMPOSITING` result bit.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&self) {
        self.remove(RenderFlags::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Checks if the render object's compositing-bits walk is queued.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.contains(RenderFlags::NEEDS_COMPOSITING_BITS_UPDATE)
    }

    /// Marks the render object as needing semantics update.
    #[inline]
    pub fn mark_needs_semantics(&self) {
        self.set(RenderFlags::NEEDS_SEMANTICS);
    }

    /// Clears the needs-semantics flag.
    #[inline]
    pub fn clear_needs_semantics(&self) {
        self.remove(RenderFlags::NEEDS_SEMANTICS);
    }

    /// Checks if the render object needs semantics update.
    #[inline]
    pub fn needs_semantics(&self) -> bool {
        self.contains(RenderFlags::NEEDS_SEMANTICS)
    }

    /// Sets whether this is a relayout boundary.
    ///
    /// Relayout boundaries prevent layout changes from propagating upward.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.set_relayout_boundary(true);
    /// assert!(flags.is_relayout_boundary());
    /// ```
    #[inline]
    pub fn set_relayout_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Checks if this is a relayout boundary.
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }

    /// Sets whether this is a repaint boundary.
    ///
    /// Repaint boundaries create compositing layers for caching.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// flags.set_repaint_boundary(true);
    /// assert!(flags.is_repaint_boundary());
    /// ```
    #[inline]
    pub fn set_repaint_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.set(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Checks if this is a repaint boundary.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Sets the previous-frame repaint-boundary value.
    ///
    /// Written by the paint phase after a node is painted. Reads happen
    /// in compositing-bits propagation to detect repaint-boundary state
    /// transitions.
    #[inline]
    pub fn set_was_repaint_boundary(&self, was_boundary: bool) {
        if was_boundary {
            self.set(RenderFlags::WAS_REPAINT_BOUNDARY);
        } else {
            self.remove(RenderFlags::WAS_REPAINT_BOUNDARY);
        }
    }

    /// Returns the previous-frame repaint-boundary value.
    #[inline]
    pub fn was_repaint_boundary(&self) -> bool {
        self.contains(RenderFlags::WAS_REPAINT_BOUNDARY)
    }

    /// Whether the last committed geometry came from a degraded pass
    /// ([`RenderFlags::GEOMETRY_DEGRADED`]).
    #[inline]
    pub fn geometry_degraded(&self) -> bool {
        self.contains(RenderFlags::GEOMETRY_DEGRADED)
    }

    /// Records whether the pass that just committed this node's geometry read
    /// a stand-in ([`RenderFlags::GEOMETRY_DEGRADED`]).
    #[inline]
    pub fn set_geometry_degraded(&self, degraded: bool) {
        if degraded {
            self.insert(RenderFlags::GEOMETRY_DEGRADED);
        } else {
            self.remove(RenderFlags::GEOMETRY_DEGRADED);
        }
    }

    /// Checks if the render object is dirty (needs any processing).
    ///
    /// Returns true if any dirty flags are set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::storage::RenderFlagsCell;
    ///
    /// let flags = RenderFlagsCell::empty();
    /// assert!(flags.is_clean());
    ///
    /// flags.mark_needs_layout();
    /// assert!(flags.is_dirty());
    /// ```
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.load().is_dirty()
    }

    /// Checks if the render object is clean (no processing needed).
    #[inline]
    pub fn is_clean(&self) -> bool {
        !self.is_dirty()
    }
}

impl Default for RenderFlagsCell {
    fn default() -> Self {
        Self::empty()
    }
}

impl Clone for RenderFlagsCell {
    fn clone(&self) -> Self {
        Self::new(self.load())
    }
}
