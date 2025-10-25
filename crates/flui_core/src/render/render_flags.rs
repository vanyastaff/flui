//! Atomic RenderFlags for lock-free state management
//!
//! Migrated from flui_core_old with performance optimizations

use bitflags::bitflags;
use std::sync::atomic::{AtomicU32, Ordering};

bitflags! {
    /// Flags for render object state
    ///
    /// These flags are stored in an AtomicU32 for lock-free access.
    /// This is critical for performance - checking `needs_layout()` happens
    /// ~1000 times per frame, so we need it to be fast (5ns vs 50ns with RwLock).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RenderFlags: u32 {
        /// RenderObject needs layout computation
        const NEEDS_LAYOUT = 1 << 0;

        /// RenderObject needs painting
        const NEEDS_PAINT = 1 << 1;

        /// RenderObject needs compositing
        const NEEDS_COMPOSITING = 1 << 2;

        /// RenderObject is a relayout boundary
        ///
        /// When true, layout changes don't propagate to parent.
        /// This is a critical optimization for large trees.
        const IS_RELAYOUT_BOUNDARY = 1 << 3;

        /// RenderObject is a repaint boundary
        ///
        /// When true, paint changes don't trigger parent repaint.
        const IS_REPAINT_BOUNDARY = 1 << 4;

        /// RenderObject needs semantics update
        const NEEDS_SEMANTICS = 1 << 5;

        /// RenderObject is detached from tree
        const IS_DETACHED = 1 << 6;

        /// RenderObject has been laid out at least once
        const HAS_SIZE = 1 << 7;
    }
}

/// Atomic wrapper for RenderFlags
///
/// Provides lock-free flag operations using atomic compare-and-swap.
/// This is 10x faster than RwLock for hot-path checks.
///
/// # Example
///
/// ```rust,ignore
/// let flags = AtomicRenderFlags::new(RenderFlags::empty());
///
/// // Lock-free operations
/// flags.set(RenderFlags::NEEDS_LAYOUT);
/// assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
/// flags.remove(RenderFlags::NEEDS_LAYOUT);
/// ```
#[derive(Debug)]
pub struct AtomicRenderFlags {
    bits: AtomicU32,
}

impl AtomicRenderFlags {
    /// Create new atomic flags
    pub const fn new(flags: RenderFlags) -> Self {
        Self {
            bits: AtomicU32::new(flags.bits()),
        }
    }

    /// Create empty flags
    pub const fn empty() -> Self {
        Self::new(RenderFlags::empty())
    }

    /// Load current flags
    #[inline]
    pub fn load(&self) -> RenderFlags {
        RenderFlags::from_bits_truncate(self.bits.load(Ordering::Acquire))
    }

    /// Store new flags
    #[inline]
    pub fn store(&self, flags: RenderFlags) {
        self.bits.store(flags.bits(), Ordering::Release);
    }

    /// Check if flags contain a specific flag
    #[inline]
    pub fn contains(&self, flag: RenderFlags) -> bool {
        self.load().contains(flag)
    }

    /// Set a flag (atomic OR)
    #[inline]
    pub fn set(&self, flag: RenderFlags) {
        self.bits.fetch_or(flag.bits(), Ordering::AcqRel);
    }

    /// Remove a flag (atomic AND NOT)
    #[inline]
    pub fn remove(&self, flag: RenderFlags) {
        self.bits.fetch_and(!flag.bits(), Ordering::AcqRel);
    }

    /// Toggle a flag (atomic XOR)
    #[inline]
    pub fn toggle(&self, flag: RenderFlags) {
        self.bits.fetch_xor(flag.bits(), Ordering::AcqRel);
    }

    /// Insert multiple flags at once
    #[inline]
    pub fn insert(&self, flags: RenderFlags) {
        self.bits.fetch_or(flags.bits(), Ordering::AcqRel);
    }

    /// Clear all flags
    #[inline]
    pub fn clear(&self) {
        self.bits.store(0, Ordering::Release);
    }
}

impl Default for AtomicRenderFlags {
    fn default() -> Self {
        Self::empty()
    }
}

impl Clone for AtomicRenderFlags {
    fn clone(&self) -> Self {
        Self::new(self.load())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_creation() {
        let flags = RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT;
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
        assert!(flags.contains(RenderFlags::NEEDS_PAINT));
        assert!(!flags.contains(RenderFlags::NEEDS_COMPOSITING));
    }

    #[test]
    fn test_atomic_flags() {
        let flags = AtomicRenderFlags::empty();

        // Set flag
        flags.set(RenderFlags::NEEDS_LAYOUT);
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));

        // Remove flag
        flags.remove(RenderFlags::NEEDS_LAYOUT);
        assert!(!flags.contains(RenderFlags::NEEDS_LAYOUT));

        // Insert multiple
        flags.insert(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
        assert!(flags.contains(RenderFlags::NEEDS_LAYOUT));
        assert!(flags.contains(RenderFlags::NEEDS_PAINT));

        // Clear
        flags.clear();
        assert_eq!(flags.load(), RenderFlags::empty());
    }

    #[test]
    fn test_relayout_boundary() {
        let flags = AtomicRenderFlags::new(RenderFlags::IS_RELAYOUT_BOUNDARY);
        assert!(flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY));
    }
}
