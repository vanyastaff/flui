//! RenderState - per-RenderObject state storage
//!
//! Migrated from flui_core_old with performance optimizations

use parking_lot::RwLock;
use flui_types::{Size, Offset};
use flui_types::constraints::BoxConstraints;

use super::render_flags::{RenderFlags, AtomicRenderFlags};

/// State for a RenderObject
///
/// **Performance Critical Design**:
/// - Atomic flags for lock-free checks (10x faster than RwLock)
/// - RwLock for actual data (size, constraints, offset)
/// - Separate locks to minimize contention
///
/// # Memory Layout
///
/// ```text
/// RenderState {
///     flags: 4 bytes (atomic)      ← Lock-free!
///     size: RwLock<Option<Size>>   ← 16 bytes + lock
///     constraints: RwLock<...>     ← 16 bytes + lock
///     offset: RwLock<Offset>       ← 8 bytes + lock
/// }
/// ```
///
/// Total: ~60 bytes per RenderObject (acceptable overhead)
#[derive(Debug)]
pub struct RenderState {
    /// Atomic flags for lock-free state checks
    ///
    /// **Critical for performance**: checking `needs_layout()` happens
    /// thousands of times per frame. Atomic operations are ~10x faster
    /// than RwLock for these hot paths.
    pub flags: AtomicRenderFlags,

    /// Computed size after layout
    ///
    /// `None` if layout hasn't been computed yet.
    /// RwLock allows concurrent reads from multiple threads.
    pub size: RwLock<Option<Size>>,

    /// Constraints used for last layout
    ///
    /// Used for cache validation and relayout decisions.
    pub constraints: RwLock<Option<BoxConstraints>>,

    /// Offset in parent's coordinate space
    ///
    /// Set during parent's layout phase.
    pub offset: RwLock<Offset>,
}

impl RenderState {
    /// Create new RenderState with empty flags
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::empty(),
            size: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
        }
    }

    /// Create RenderState with initial flags
    pub fn with_flags(flags: RenderFlags) -> Self {
        Self {
            flags: AtomicRenderFlags::new(flags),
            size: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
        }
    }

    // ========== Layout State ==========

    /// Check if layout is needed (lock-free!)
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Mark as needing layout
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags.set(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clear needs_layout flag
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Check if this is a relayout boundary
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }

    /// Set relayout boundary flag
    #[inline]
    pub fn set_relayout_boundary(&self, value: bool) {
        if value {
            self.flags.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    // ========== Paint State ==========

    /// Check if paint is needed (lock-free!)
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Mark as needing paint
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.flags.set(RenderFlags::NEEDS_PAINT);
    }

    /// Clear needs_paint flag
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Check if this is a repaint boundary
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Set repaint boundary flag
    #[inline]
    pub fn set_repaint_boundary(&self, value: bool) {
        if value {
            self.flags.set(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    // ========== Compositing State ==========

    /// Check if compositing is needed
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Mark as needing compositing
    #[inline]
    pub fn mark_needs_compositing(&self) {
        self.flags.set(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Clear needs_compositing flag
    #[inline]
    pub fn clear_needs_compositing(&self) {
        self.flags.remove(RenderFlags::NEEDS_COMPOSITING);
    }

    // ========== Size & Constraints ==========

    /// Get computed size
    pub fn get_size(&self) -> Option<Size> {
        *self.size.read()
    }

    /// Set computed size
    pub fn set_size(&self, size: Size) {
        *self.size.write() = Some(size);
        self.flags.set(RenderFlags::HAS_SIZE);
    }

    /// Check if size has been computed
    #[inline]
    pub fn has_size(&self) -> bool {
        self.flags.contains(RenderFlags::HAS_SIZE)
    }

    /// Get constraints
    pub fn get_constraints(&self) -> Option<BoxConstraints> {
        *self.constraints.read()
    }

    /// Set constraints
    pub fn set_constraints(&self, constraints: BoxConstraints) {
        *self.constraints.write() = Some(constraints);
    }

    // ========== Offset ==========

    /// Get offset
    pub fn get_offset(&self) -> Offset {
        *self.offset.read()
    }

    /// Set offset
    pub fn set_offset(&self, offset: Offset) {
        *self.offset.write() = offset;
    }

    // ========== Lifecycle ==========

    /// Check if detached from tree
    #[inline]
    pub fn is_detached(&self) -> bool {
        self.flags.contains(RenderFlags::IS_DETACHED)
    }

    /// Mark as detached
    #[inline]
    pub fn mark_detached(&self) {
        self.flags.set(RenderFlags::IS_DETACHED);
    }

    /// Mark as attached
    #[inline]
    pub fn mark_attached(&self) {
        self.flags.remove(RenderFlags::IS_DETACHED);
    }

    /// Reset all state (for reuse)
    pub fn reset(&self) {
        self.flags.clear();
        *self.size.write() = None;
        *self.constraints.write() = None;
        *self.offset.write() = Offset::ZERO;
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RenderState {
    fn clone(&self) -> Self {
        Self {
            flags: self.flags.clone(),
            size: RwLock::new(*self.size.read()),
            constraints: RwLock::new(*self.constraints.read()),
            offset: RwLock::new(*self.offset.read()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_state_creation() {
        let state = RenderState::new();
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
        assert!(!state.has_size());
    }

    #[test]
    fn test_layout_flags() {
        let state = RenderState::new();

        state.mark_needs_layout();
        assert!(state.needs_layout());

        state.clear_needs_layout();
        assert!(!state.needs_layout());
    }

    #[test]
    fn test_size_management() {
        let state = RenderState::new();
        assert!(!state.has_size());

        state.set_size(Size::new(100.0, 100.0));
        assert!(state.has_size());
        assert_eq!(state.get_size(), Some(Size::new(100.0, 100.0)));
    }

    #[test]
    fn test_relayout_boundary() {
        let state = RenderState::new();
        assert!(!state.is_relayout_boundary());

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
    }

    #[test]
    fn test_reset() {
        let state = RenderState::new();

        state.mark_needs_layout();
        state.set_size(Size::new(50.0, 50.0));

        state.reset();

        assert!(!state.needs_layout());
        assert!(!state.has_size());
        assert_eq!(state.get_size(), None);
    }
}
