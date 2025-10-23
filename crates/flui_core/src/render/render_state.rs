//! Shared state structure for all RenderObjects
//!
//! Now stored in ElementTree instead of in each RenderObject, which provides
//! better memory locality and clearer ownership semantics.

use parking_lot::Mutex;
use flui_types::{Size, constraints::BoxConstraints};
use super::RenderFlags;

/// Shared state for RenderObjects
///
/// After architectural refactoring, this state is now stored in `ElementTree` per element
/// rather than inside each RenderObject. This provides:
///
/// 1. **Better ownership model**: State belongs to the tree/element, not the data
/// 2. **Memory savings**: No duplicate state in RenderObject data structures
/// 3. **Clearer lifecycle**: State lifecycle matches element lifecycle
///
/// # Interior Mutability
///
/// Uses `parking_lot::Mutex<T>` for thread-safe interior mutability.
/// This enables `layout(&self)` instead of `layout(&mut self)`, which is required
/// for RenderContext architecture where multiple RenderObjects may need to access
/// the ElementTree simultaneously.
///
/// # Memory Layout
///
/// ```text
/// RenderState (~48 bytes on 64-bit):
/// - size: Mutex<Option<Size>> ~16 bytes
/// - constraints: Mutex<Option<BoxConstraints>> ~16 bytes
/// - flags: Mutex<RenderFlags> ~16 bytes
/// ```
///
/// ParentData is stored separately in ElementNode, not in RenderState,
/// because it's not part of layout/paint state but rather describes
/// the parent-child relationship.
#[derive(Debug)]
pub struct RenderState {
    /// The size determined by the last layout pass
    ///
    /// `None` if layout hasn't been performed yet.
    /// After `layout()` is called, this contains the size chosen by the RenderObject.
    ///
    /// Uses `Mutex` for thread-safe interior mutability.
    pub size: Mutex<Option<Size>>,

    /// The constraints used in the last layout pass
    ///
    /// `None` if layout hasn't been performed yet.
    /// Stored to enable cache invalidation when constraints change.
    ///
    /// Uses `Mutex` for thread-safe interior mutability.
    pub constraints: Mutex<Option<BoxConstraints>>,

    /// Dirty state flags
    ///
    /// Tracks whether this RenderObject needs layout, paint, compositing update, etc.
    /// Uses bitflags for memory efficiency (1 byte vs 4+ bytes for separate bools).
    ///
    /// Uses `Mutex` for thread-safe interior mutability.
    pub flags: Mutex<RenderFlags>,
}

impl RenderState {
    /// Create a new RenderState with default values
    ///
    /// # Default Values
    ///
    /// - `size`: None (not laid out yet)
    /// - `constraints`: None (not laid out yet)
    /// - `flags`: NEEDS_LAYOUT (new objects need layout)
    pub fn new() -> Self {
        Self {
            size: Mutex::new(None),
            constraints: Mutex::new(None),
            flags: Mutex::new(RenderFlags::default()), // NEEDS_LAYOUT by default
        }
    }

    /// Check if layout has been performed
    ///
    /// Returns `true` if this RenderObject has been laid out at least once.
    #[inline]
    pub fn has_size(&self) -> bool {
        self.size.lock().is_some()
    }

    /// Get the size, panicking if not laid out
    ///
    /// # Panics
    ///
    /// Panics if `layout()` hasn't been called yet.
    #[inline]
    pub fn size_unchecked(&self) -> Size {
        self.size.lock().expect("RenderObject not laid out yet")
    }

    /// Mark as needing layout
    ///
    /// This sets the NEEDS_LAYOUT flag, indicating that `layout()` should be called
    /// during the next frame.
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags.lock().insert(RenderFlags::NEEDS_LAYOUT);
    }

    /// Mark as needing paint
    ///
    /// This sets the NEEDS_PAINT flag, indicating that `paint()` should be called
    /// during the next frame.
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.flags.lock().insert(RenderFlags::NEEDS_PAINT);
    }

    /// Check if this RenderObject needs layout
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.lock().contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Check if this RenderObject needs paint
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.lock().contains(RenderFlags::NEEDS_PAINT)
    }

    /// Clear the needs_layout flag
    ///
    /// Called after layout is performed.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.lock().remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clear the needs_paint flag
    ///
    /// Called after painting is performed.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.lock().remove(RenderFlags::NEEDS_PAINT);
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_state_new() {
        let state = RenderState::new();
        assert!(state.size.lock().is_none());
        assert!(state.constraints.lock().is_none());
        assert!(state.needs_layout());
        assert!(!state.needs_paint());
    }

    #[test]
    fn test_render_state_has_size() {
        let state = RenderState::new();
        assert!(!state.has_size());

        *state.size.lock() = Some(Size::new(100.0, 100.0));
        assert!(state.has_size());
    }

    #[test]
    fn test_render_state_flags() {
        let state = RenderState::new();

        state.mark_needs_paint();
        assert!(state.needs_paint());

        state.clear_needs_paint();
        assert!(!state.needs_paint());

        state.clear_needs_layout();
        assert!(!state.needs_layout());
    }

    #[test]
    #[should_panic(expected = "RenderObject not laid out yet")]
    fn test_size_unchecked_panics() {
        let state = RenderState::new();
        let _ = state.size_unchecked();
    }

    #[test]
    fn test_render_state_size() {
        // Verify RenderState is reasonably sized
        let size = std::mem::size_of::<RenderState>();
        println!("RenderState size: {} bytes", size);
        // Should be small enough for efficient stack/heap usage
        assert!(size <= 64, "RenderState is too large: {} bytes", size);
    }
}
