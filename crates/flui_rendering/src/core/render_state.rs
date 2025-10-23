//! Shared state structure for all RenderObjects

use std::any::Any;
use flui_types::{Size, constraints::BoxConstraints};
use super::RenderFlags;

/// Shared state for all RenderObjects
///
/// This structure contains the common state fields that all RenderObjects need.
/// By extracting this into a shared type, we:
///
/// 1. Reduce code duplication across 81 RenderObject types
/// 2. Ensure consistent state management
/// 3. Make the generic architecture possible
///
/// # Memory Layout
///
/// ```text
/// RenderState (24 bytes total on 64-bit):
/// - size: Option<Size> = 12 bytes (Option<(f32, f32)>)
/// - constraints: Option<BoxConstraints> = 12 bytes
/// - flags: RenderFlags = 1 byte
/// - padding: 7 bytes (alignment)
/// ```
///
/// # Design Rationale
///
/// All RenderObjects share these fundamental properties:
/// - **size**: The size determined by layout
/// - **constraints**: The constraints used in layout
/// - **flags**: Dirty state (needs layout, needs paint, etc.)
///
/// By using `RenderState`, we get consistent behavior across all RenderObject types
/// without repeating field declarations 81 times.
#[derive(Debug)]
pub struct RenderState {
    /// The size determined by the last layout pass
    ///
    /// `None` if layout hasn't been performed yet.
    /// After `layout()` is called, this contains the size chosen by the RenderObject.
    pub size: Option<Size>,

    /// The constraints used in the last layout pass
    ///
    /// `None` if layout hasn't been performed yet.
    /// Stored to enable cache invalidation when constraints change.
    pub constraints: Option<BoxConstraints>,

    /// Dirty state flags
    ///
    /// Tracks whether this RenderObject needs layout, paint, compositing update, etc.
    /// Uses bitflags for memory efficiency (1 byte vs 4+ bytes for separate bools).
    pub flags: RenderFlags,

    /// Parent data (type-erased)
    ///
    /// This is data that the parent RenderObject attaches to this child.
    /// For example, Stack attaches StackParentData to position children,
    /// Flex attaches FlexParentData for flex factors.
    ///
    /// Stored as `Box<dyn Any + Send + Sync>` for type erasure, downcasted when needed.
    pub parent_data: Option<Box<dyn Any + Send + Sync>>,
}

impl RenderState {
    /// Create a new RenderState with default values
    ///
    /// # Default Values
    ///
    /// - `size`: None (not laid out yet)
    /// - `constraints`: None (not laid out yet)
    /// - `flags`: NEEDS_LAYOUT (new objects need layout)
    /// - `parent_data`: None
    pub fn new() -> Self {
        Self {
            size: None,
            constraints: None,
            flags: RenderFlags::default(), // NEEDS_LAYOUT by default
            parent_data: None,
        }
    }

    /// Check if layout has been performed
    ///
    /// Returns `true` if this RenderObject has been laid out at least once.
    #[inline]
    pub fn has_size(&self) -> bool {
        self.size.is_some()
    }

    /// Get the size, panicking if not laid out
    ///
    /// # Panics
    ///
    /// Panics if `layout()` hasn't been called yet.
    #[inline]
    pub fn size_unchecked(&self) -> Size {
        self.size.expect("RenderObject not laid out yet")
    }

    /// Mark as needing layout
    ///
    /// This sets the NEEDS_LAYOUT flag, indicating that `layout()` should be called
    /// during the next frame.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.flags.insert(RenderFlags::NEEDS_LAYOUT);
    }

    /// Mark as needing paint
    ///
    /// This sets the NEEDS_PAINT flag, indicating that `paint()` should be called
    /// during the next frame.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.flags.insert(RenderFlags::NEEDS_PAINT);
    }

    /// Check if this RenderObject needs layout
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Check if this RenderObject needs paint
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Clear the needs_layout flag
    ///
    /// Called after layout is performed.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clear the needs_paint flag
    ///
    /// Called after painting is performed.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
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
        assert!(state.size.is_none());
        assert!(state.constraints.is_none());
        assert!(state.needs_layout());
        assert!(!state.needs_paint());
    }

    #[test]
    fn test_render_state_has_size() {
        let mut state = RenderState::new();
        assert!(!state.has_size());

        state.size = Some(Size::new(100.0, 100.0));
        assert!(state.has_size());
    }

    #[test]
    fn test_render_state_flags() {
        let mut state = RenderState::new();

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
        // On 64-bit: Option<Size> = 12 bytes, Option<BoxConstraints> = 12 bytes, RenderFlags = 1 byte
        // Plus alignment = ~24-32 bytes
        let size = std::mem::size_of::<RenderState>();
        println!("RenderState size: {} bytes", size);
        // Should be small enough for efficient stack/heap usage
        assert!(size <= 64, "RenderState is too large: {} bytes", size);
    }
}
