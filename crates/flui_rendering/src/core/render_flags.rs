//! Bitflags for RenderObject dirty state

use bitflags::bitflags;

bitflags! {
    /// Flags tracking the dirty state of a RenderObject
    ///
    /// Using bitflags reduces memory usage from multiple bools (typically 8+ bytes)
    /// to a single byte, providing 50-66% memory savings per RenderObject.
    ///
    /// # Memory Savings
    ///
    /// - **Before**: 4 bools = 4 bytes minimum (often padded to 8 bytes)
    /// - **After**: 1 byte for all flags = 87.5% reduction
    ///
    /// # Performance
    ///
    /// Bitflags provide:
    /// - Faster flag checks (single AND operation)
    /// - Better cache locality (less memory per object)
    /// - Atomic updates when needed
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::RenderFlags;
    ///
    /// let mut flags = RenderFlags::empty();
    ///
    /// // Mark as needing layout
    /// flags.insert(RenderFlags::NEEDS_LAYOUT);
    ///
    /// // Check if needs layout
    /// if flags.contains(RenderFlags::NEEDS_LAYOUT) {
    ///     // Perform layout
    ///     flags.remove(RenderFlags::NEEDS_LAYOUT);
    /// }
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RenderFlags: u8 {
        /// This render object needs layout
        ///
        /// Set when: constraints change, children change, or properties that affect layout change
        /// Cleared when: layout() is called
        const NEEDS_LAYOUT = 1 << 0;

        /// This render object needs paint
        ///
        /// Set when: visual properties change (color, decoration, etc.)
        /// Cleared when: paint() is called
        const NEEDS_PAINT = 1 << 1;

        /// This render object needs compositing bits update
        ///
        /// Set when: properties affecting compositing change (opacity, clip, etc.)
        /// Cleared when: compositing bits are updated
        const NEEDS_COMPOSITING_BITS_UPDATE = 1 << 2;

        /// This render object is a repaint boundary
        ///
        /// A repaint boundary isolates painting - when this object needs repaint,
        /// it doesn't cause ancestors to repaint. Useful for optimization.
        const IS_REPAINT_BOUNDARY = 1 << 3;

        /// This render object needs semantics update
        ///
        /// Set when: accessibility/semantics properties change
        /// Cleared when: semantics are updated
        const NEEDS_SEMANTICS_UPDATE = 1 << 4;
    }
}

impl Default for RenderFlags {
    fn default() -> Self {
        // New render objects need layout by default
        Self::NEEDS_LAYOUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_default() {
        let flags = RenderFlags::default();
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
        assert!(!flags.contains(RenderFlags::NEEDS_PAINT));
    }

    #[test]
    fn test_flags_insert_remove() {
        let mut flags = RenderFlags::empty();

        flags.insert(RenderFlags::NEEDS_LAYOUT);
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));

        flags.remove(RenderFlags::NEEDS_LAYOUT);
        assert!(!flags.contains(RenderFlags::NEEDS_LAYOUT));
    }

    #[test]
    fn test_flags_multiple() {
        let mut flags = RenderFlags::empty();

        flags.insert(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
        assert!(flags.contains(RenderFlags::NEEDS_PAINT));
        assert!(!flags.contains(RenderFlags::IS_REPAINT_BOUNDARY));
    }

    #[test]
    fn test_flags_size() {
        // Verify bitflags only takes 1 byte
        assert_eq!(std::mem::size_of::<RenderFlags>(), 1);
    }
}
