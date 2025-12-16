//! Render object dirty flags and state tracking.
//!
//! This module provides dirty flag tracking for render objects, matching
//! Flutter's approach of using separate boolean flags for each dirty state.
//!
//! # Flutter Equivalence
//!
//! In Flutter, render objects track dirty state with separate boolean fields:
//! - `_needsLayout`
//! - `_needsPaint`
//! - `_needsCompositingBitsUpdate`
//! - `_needsSemanticsUpdate`
//! - `_needsCompositedLayerUpdate`
//! - `isRepaintBoundary` (getter)
//! - `_wasRepaintBoundary`
//! - `_isRelayoutBoundary` (nullable)
//!
//! We pack these into a single byte using bitflags for memory efficiency.
//!
//! # Memory Layout
//!
//! - `DirtyFlags`: 1 byte (vs 8+ bytes of separate booleans in Flutter)
//!
//! # Example
//!
//! ```
//! use flui_rendering::lifecycle::DirtyFlags;
//!
//! let mut flags = DirtyFlags::empty();
//! flags.insert(DirtyFlags::NEEDS_LAYOUT);
//! assert!(flags.needs_layout());
//!
//! flags.remove(DirtyFlags::NEEDS_LAYOUT);
//! assert!(!flags.needs_layout());
//! ```

use bitflags::bitflags;

// ============================================================================
// DirtyFlags
// ============================================================================

bitflags! {
    /// Dirty flags for render objects.
    ///
    /// These flags track what needs to be updated. This matches Flutter's
    /// approach where dirty flags work independently of attach/detach state.
    ///
    /// # Memory Layout
    ///
    /// Size: 1 byte (`u8`)
    ///
    /// Flutter uses 6-8 separate boolean fields = 6-8 bytes + padding.
    /// We use 1 byte bitflags = **85-90% memory savings per node**.
    ///
    /// # Flutter Equivalence
    ///
    /// | Flutter Field | FLUI Flag |
    /// |---------------|-----------|
    /// | `_needsLayout` | `NEEDS_LAYOUT` |
    /// | `_needsPaint` | `NEEDS_PAINT` |
    /// | `_needsCompositingBitsUpdate` | `NEEDS_COMPOSITING_BITS_UPDATE` |
    /// | `_needsSemanticsUpdate` | `NEEDS_SEMANTICS_UPDATE` |
    /// | `_needsCompositedLayerUpdate` | `NEEDS_COMPOSITED_LAYER_UPDATE` |
    /// | `_needsCompositing` | `NEEDS_COMPOSITING` |
    /// | `isRepaintBoundary` | `IS_REPAINT_BOUNDARY` |
    /// | `_wasRepaintBoundary` | `WAS_REPAINT_BOUNDARY` |
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct DirtyFlags: u8 {
        /// Needs layout.
        ///
        /// Set when something changes that affects the layout of this object.
        /// Initially `true` - new render objects need their first layout.
        ///
        /// # Flutter Equivalence
        /// `bool _needsLayout = true;`
        const NEEDS_LAYOUT = 1 << 0;

        /// Needs paint.
        ///
        /// Set when something changes that affects the visual appearance
        /// of this object but not its layout.
        /// Initially `true` - new render objects need their first paint.
        ///
        /// # Flutter Equivalence
        /// `bool _needsPaint = true;`
        const NEEDS_PAINT = 1 << 1;

        /// Needs compositing bits update.
        ///
        /// Set when a child is added or when something changes that affects
        /// whether this object or its descendants need compositing layers.
        ///
        /// # Flutter Equivalence
        /// `bool _needsCompositingBitsUpdate = false;`
        const NEEDS_COMPOSITING_BITS_UPDATE = 1 << 2;

        /// Needs semantics update.
        ///
        /// Set when something changes that affects the accessibility
        /// tree for this object.
        ///
        /// # Flutter Equivalence
        /// `bool _needsSemanticsUpdate` (managed via semantics system)
        const NEEDS_SEMANTICS_UPDATE = 1 << 3;

        /// Needs composited layer update.
        ///
        /// Set when a property of the composited layer changes but
        /// the children don't need to be repainted.
        ///
        /// # Flutter Equivalence
        /// `bool _needsCompositedLayerUpdate = false;`
        const NEEDS_COMPOSITED_LAYER_UPDATE = 1 << 4;

        /// Needs compositing.
        ///
        /// Set when this object or a descendant requires compositing.
        /// Initialized based on `isRepaintBoundary || alwaysNeedsCompositing`.
        ///
        /// # Flutter Equivalence
        /// `late bool _needsCompositing;`
        const NEEDS_COMPOSITING = 1 << 5;

        /// Is a repaint boundary.
        ///
        /// Set when this object creates its own compositing layer.
        /// This is typically a constant property of a render object class.
        ///
        /// # Flutter Equivalence
        /// `bool get isRepaintBoundary => false;` (overridable getter)
        const IS_REPAINT_BOUNDARY = 1 << 6;

        /// Was a repaint boundary in the previous frame.
        ///
        /// Used to detect when `isRepaintBoundary` changes between frames,
        /// which requires special handling of the layer.
        ///
        /// # Flutter Equivalence
        /// `late bool _wasRepaintBoundary;`
        const WAS_REPAINT_BOUNDARY = 1 << 7;
    }
}

impl DirtyFlags {
    /// Initial flags for a new render object.
    ///
    /// New objects need layout and paint, matching Flutter's initialization:
    /// - `_needsLayout = true`
    /// - `_needsPaint = true`
    #[inline]
    pub const fn initial() -> Self {
        Self::NEEDS_LAYOUT.union(Self::NEEDS_PAINT)
    }

    // ========================================================================
    // Dirty State Queries
    // ========================================================================

    /// Returns whether layout is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsLayout`
    #[inline]
    pub const fn needs_layout(self) -> bool {
        self.contains(Self::NEEDS_LAYOUT)
    }

    /// Returns whether paint is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsPaint`
    #[inline]
    pub const fn needs_paint(self) -> bool {
        self.contains(Self::NEEDS_PAINT)
    }

    /// Returns whether compositing bits need update.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositingBitsUpdate`
    #[inline]
    pub const fn needs_compositing_bits_update(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITING_BITS_UPDATE)
    }

    /// Returns whether semantics need update.
    ///
    /// # Flutter Equivalence
    /// Part of semantics system
    #[inline]
    pub const fn needs_semantics_update(self) -> bool {
        self.contains(Self::NEEDS_SEMANTICS_UPDATE)
    }

    /// Returns whether the composited layer needs update.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositedLayerUpdate`
    #[inline]
    pub const fn needs_composited_layer_update(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITED_LAYER_UPDATE)
    }

    /// Returns whether compositing is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositing`
    #[inline]
    pub const fn needs_compositing(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITING)
    }

    /// Returns whether this is a repaint boundary.
    ///
    /// # Flutter Equivalence
    /// `isRepaintBoundary`
    #[inline]
    pub const fn is_repaint_boundary(self) -> bool {
        self.contains(Self::IS_REPAINT_BOUNDARY)
    }

    /// Returns whether this was a repaint boundary.
    ///
    /// # Flutter Equivalence
    /// `_wasRepaintBoundary`
    #[inline]
    pub const fn was_repaint_boundary(self) -> bool {
        self.contains(Self::WAS_REPAINT_BOUNDARY)
    }
}

// ============================================================================
// RelayoutBoundary
// ============================================================================

/// Relayout boundary state.
///
/// In Flutter, `_isRelayoutBoundary` is a nullable boolean:
/// - `null`: layout has never been called
/// - `true`: this is a relayout boundary
/// - `false`: this is not a relayout boundary
///
/// We represent this as an enum for type safety.
///
/// # Flutter Equivalence
/// `bool? _isRelayoutBoundary;`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum RelayoutBoundary {
    /// Layout has never been called on this object.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary == null`
    #[default]
    Unknown = 0,

    /// This object is a relayout boundary.
    ///
    /// The parent does not depend on this object's size, so layout
    /// changes do not propagate upward.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary == true`
    Yes = 1,

    /// This object is not a relayout boundary.
    ///
    /// The parent depends on this object's size, so layout changes
    /// propagate upward.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary == false`
    No = 2,
}

impl RelayoutBoundary {
    /// Returns whether this is a known relayout boundary.
    ///
    /// Returns `true` only if explicitly set to `Yes`.
    #[inline]
    pub const fn is_boundary(self) -> bool {
        matches!(self, Self::Yes)
    }

    /// Returns whether layout has been called at least once.
    #[inline]
    pub const fn is_known(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Converts from Option<bool> for Flutter compatibility.
    #[inline]
    pub const fn from_option(value: Option<bool>) -> Self {
        match value {
            None => Self::Unknown,
            Some(true) => Self::Yes,
            Some(false) => Self::No,
        }
    }

    /// Converts to Option<bool> for Flutter compatibility.
    #[inline]
    pub const fn to_option(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Yes => Some(true),
            Self::No => Some(false),
        }
    }
}

// ============================================================================
// RenderObjectFlags
// ============================================================================

/// Combined render object flags (dirty flags + relayout boundary).
///
/// This struct packs all render object flags into 2 bytes:
/// - `dirty`: DirtyFlags (1 byte)
/// - `relayout_boundary`: RelayoutBoundary (1 byte)
///
/// # Memory Layout
///
/// Total: 2 bytes
///
/// Compare to Flutter's approach with separate fields:
/// - 6-8 boolean fields = 6-8 bytes
/// - `_isRelayoutBoundary` nullable bool = 2 bytes (Dart nullable)
/// - **Total: 8-10 bytes**
///
/// We use **2 bytes** = **75-80% memory savings**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderObjectFlags {
    /// Dirty flags.
    dirty: DirtyFlags,

    /// Relayout boundary state.
    relayout_boundary: RelayoutBoundary,
}

impl RenderObjectFlags {
    /// Creates new flags with initial dirty state.
    ///
    /// Matches Flutter's initialization:
    /// - `_needsLayout = true`
    /// - `_needsPaint = true`
    /// - `_isRelayoutBoundary = null`
    #[inline]
    pub const fn new() -> Self {
        Self {
            dirty: DirtyFlags::initial(),
            relayout_boundary: RelayoutBoundary::Unknown,
        }
    }

    /// Creates flags with specific dirty flags.
    #[inline]
    pub const fn with_dirty(dirty: DirtyFlags) -> Self {
        Self {
            dirty,
            relayout_boundary: RelayoutBoundary::Unknown,
        }
    }

    // ========================================================================
    // Dirty Flags Access
    // ========================================================================

    /// Returns the dirty flags.
    #[inline]
    pub const fn dirty(&self) -> DirtyFlags {
        self.dirty
    }

    /// Returns mutable access to dirty flags.
    #[inline]
    pub fn dirty_mut(&mut self) -> &mut DirtyFlags {
        &mut self.dirty
    }

    // ========================================================================
    // Dirty State Queries (delegated)
    // ========================================================================

    /// Returns whether layout is needed.
    #[inline]
    pub const fn needs_layout(&self) -> bool {
        self.dirty.needs_layout()
    }

    /// Returns whether paint is needed.
    #[inline]
    pub const fn needs_paint(&self) -> bool {
        self.dirty.needs_paint()
    }

    /// Returns whether compositing bits need update.
    #[inline]
    pub const fn needs_compositing_bits_update(&self) -> bool {
        self.dirty.needs_compositing_bits_update()
    }

    /// Returns whether semantics need update.
    #[inline]
    pub const fn needs_semantics_update(&self) -> bool {
        self.dirty.needs_semantics_update()
    }

    /// Returns whether compositing is needed.
    #[inline]
    pub const fn needs_compositing(&self) -> bool {
        self.dirty.needs_compositing()
    }

    /// Returns whether this is a repaint boundary.
    #[inline]
    pub const fn is_repaint_boundary(&self) -> bool {
        self.dirty.is_repaint_boundary()
    }

    /// Returns whether this was a repaint boundary.
    #[inline]
    pub const fn was_repaint_boundary(&self) -> bool {
        self.dirty.was_repaint_boundary()
    }

    // ========================================================================
    // Dirty State Marking
    // ========================================================================

    /// Marks as needing layout.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.dirty.insert(DirtyFlags::NEEDS_LAYOUT);
    }

    /// Marks as needing paint.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.dirty.insert(DirtyFlags::NEEDS_PAINT);
    }

    /// Marks as needing compositing bits update.
    #[inline]
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.dirty.insert(DirtyFlags::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Marks as needing semantics update.
    #[inline]
    pub fn mark_needs_semantics_update(&mut self) {
        self.dirty.insert(DirtyFlags::NEEDS_SEMANTICS_UPDATE);
    }

    /// Marks as needing composited layer update.
    #[inline]
    pub fn mark_needs_composited_layer_update(&mut self) {
        self.dirty.insert(DirtyFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    // ========================================================================
    // Dirty State Clearing
    // ========================================================================

    /// Clears the needs_layout flag.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.dirty.remove(DirtyFlags::NEEDS_LAYOUT);
    }

    /// Clears the needs_paint flag.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.dirty.remove(DirtyFlags::NEEDS_PAINT);
    }

    /// Clears the needs_compositing_bits_update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.dirty.remove(DirtyFlags::NEEDS_COMPOSITING_BITS_UPDATE);
    }

    /// Clears the needs_semantics_update flag.
    #[inline]
    pub fn clear_needs_semantics_update(&mut self) {
        self.dirty.remove(DirtyFlags::NEEDS_SEMANTICS_UPDATE);
    }

    /// Clears the needs_composited_layer_update flag.
    #[inline]
    pub fn clear_needs_composited_layer_update(&mut self) {
        self.dirty.remove(DirtyFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Returns the relayout boundary state.
    #[inline]
    pub const fn relayout_boundary(&self) -> RelayoutBoundary {
        self.relayout_boundary
    }

    /// Returns whether this is a relayout boundary.
    #[inline]
    pub const fn is_relayout_boundary(&self) -> bool {
        self.relayout_boundary.is_boundary()
    }

    /// Sets the relayout boundary state.
    #[inline]
    pub fn set_relayout_boundary(&mut self, boundary: RelayoutBoundary) {
        self.relayout_boundary = boundary;
    }

    /// Clears the relayout boundary (sets to Unknown).
    ///
    /// Called when a child is dropped from its parent.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary = null` in `dropChild`
    #[inline]
    pub fn clear_relayout_boundary(&mut self) {
        self.relayout_boundary = RelayoutBoundary::Unknown;
    }

    /// Sets whether this is a repaint boundary.
    #[inline]
    pub fn set_repaint_boundary(&mut self, is_boundary: bool) {
        if is_boundary {
            self.dirty.insert(DirtyFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.dirty.remove(DirtyFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Updates was_repaint_boundary to match current repaint_boundary.
    ///
    /// Called at the end of paint.
    ///
    /// # Flutter Equivalence
    /// `_wasRepaintBoundary = isRepaintBoundary;` at end of `_paintWithContext`
    #[inline]
    pub fn sync_was_repaint_boundary(&mut self) {
        if self.dirty.is_repaint_boundary() {
            self.dirty.insert(DirtyFlags::WAS_REPAINT_BOUNDARY);
        } else {
            self.dirty.remove(DirtyFlags::WAS_REPAINT_BOUNDARY);
        }
    }

    /// Sets whether compositing is needed.
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        if needs {
            self.dirty.insert(DirtyFlags::NEEDS_COMPOSITING);
        } else {
            self.dirty.remove(DirtyFlags::NEEDS_COMPOSITING);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // DirtyFlags Tests
    // ========================================================================

    #[test]
    fn test_dirty_flags_size() {
        assert_eq!(std::mem::size_of::<DirtyFlags>(), 1);
    }

    #[test]
    fn test_dirty_flags_initial() {
        let flags = DirtyFlags::initial();
        assert!(flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(!flags.needs_compositing_bits_update());
        assert!(!flags.needs_semantics_update());
    }

    #[test]
    fn test_dirty_flags_operations() {
        let mut flags = DirtyFlags::empty();

        flags.insert(DirtyFlags::NEEDS_LAYOUT);
        assert!(flags.needs_layout());
        assert!(!flags.needs_paint());

        flags.insert(DirtyFlags::NEEDS_PAINT);
        assert!(flags.needs_paint());

        flags.remove(DirtyFlags::NEEDS_LAYOUT);
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());
    }

    #[test]
    fn test_dirty_flags_repaint_boundary() {
        let mut flags = DirtyFlags::empty();

        flags.insert(DirtyFlags::IS_REPAINT_BOUNDARY);
        assert!(flags.is_repaint_boundary());
        assert!(!flags.was_repaint_boundary());

        flags.insert(DirtyFlags::WAS_REPAINT_BOUNDARY);
        assert!(flags.was_repaint_boundary());
    }

    // ========================================================================
    // RelayoutBoundary Tests
    // ========================================================================

    #[test]
    fn test_relayout_boundary_size() {
        assert_eq!(std::mem::size_of::<RelayoutBoundary>(), 1);
    }

    #[test]
    fn test_relayout_boundary_default() {
        let boundary = RelayoutBoundary::default();
        assert_eq!(boundary, RelayoutBoundary::Unknown);
        assert!(!boundary.is_boundary());
        assert!(!boundary.is_known());
    }

    #[test]
    fn test_relayout_boundary_states() {
        assert!(RelayoutBoundary::Yes.is_boundary());
        assert!(RelayoutBoundary::Yes.is_known());

        assert!(!RelayoutBoundary::No.is_boundary());
        assert!(RelayoutBoundary::No.is_known());

        assert!(!RelayoutBoundary::Unknown.is_boundary());
        assert!(!RelayoutBoundary::Unknown.is_known());
    }

    #[test]
    fn test_relayout_boundary_option_conversion() {
        assert_eq!(
            RelayoutBoundary::from_option(None),
            RelayoutBoundary::Unknown
        );
        assert_eq!(
            RelayoutBoundary::from_option(Some(true)),
            RelayoutBoundary::Yes
        );
        assert_eq!(
            RelayoutBoundary::from_option(Some(false)),
            RelayoutBoundary::No
        );

        assert_eq!(RelayoutBoundary::Unknown.to_option(), None);
        assert_eq!(RelayoutBoundary::Yes.to_option(), Some(true));
        assert_eq!(RelayoutBoundary::No.to_option(), Some(false));
    }

    // ========================================================================
    // RenderObjectFlags Tests
    // ========================================================================

    #[test]
    fn test_render_object_flags_size() {
        assert_eq!(std::mem::size_of::<RenderObjectFlags>(), 2);
    }

    #[test]
    fn test_render_object_flags_new() {
        let flags = RenderObjectFlags::new();
        assert!(flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(!flags.needs_compositing_bits_update());
        assert_eq!(flags.relayout_boundary(), RelayoutBoundary::Unknown);
    }

    #[test]
    fn test_render_object_flags_marking() {
        let mut flags = RenderObjectFlags::new();

        flags.clear_needs_layout();
        assert!(!flags.needs_layout());

        flags.mark_needs_layout();
        assert!(flags.needs_layout());

        flags.clear_needs_paint();
        assert!(!flags.needs_paint());

        flags.mark_needs_paint();
        assert!(flags.needs_paint());
    }

    #[test]
    fn test_render_object_flags_relayout_boundary() {
        let mut flags = RenderObjectFlags::new();

        flags.set_relayout_boundary(RelayoutBoundary::Yes);
        assert!(flags.is_relayout_boundary());

        flags.set_relayout_boundary(RelayoutBoundary::No);
        assert!(!flags.is_relayout_boundary());

        flags.clear_relayout_boundary();
        assert_eq!(flags.relayout_boundary(), RelayoutBoundary::Unknown);
    }

    #[test]
    fn test_render_object_flags_repaint_boundary() {
        let mut flags = RenderObjectFlags::new();

        flags.set_repaint_boundary(true);
        assert!(flags.is_repaint_boundary());
        assert!(!flags.was_repaint_boundary());

        flags.sync_was_repaint_boundary();
        assert!(flags.was_repaint_boundary());

        flags.set_repaint_boundary(false);
        assert!(!flags.is_repaint_boundary());
        assert!(flags.was_repaint_boundary()); // Still true until sync
    }

    // ========================================================================
    // Send + Sync Tests
    // ========================================================================

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DirtyFlags>();
        assert_send_sync::<RelayoutBoundary>();
        assert_send_sync::<RenderObjectFlags>();
    }
}
