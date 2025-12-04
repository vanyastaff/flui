//! Atomic view flags for lock-free dirty tracking.
//!
//! This module provides `AtomicViewFlags` for thread-safe, lock-free
//! view element state tracking.
//!
//! # Difference from RenderFlags
//!
//! ViewFlags are simpler than RenderFlags because views don't participate
//! in layout/paint - they only handle build phase:
//!
//! - `DIRTY` - Needs rebuild
//! - `MOUNTED` - In tree
//! - `ACTIVE` - Lifecycle is Active
//!
//! No NEEDS_LAYOUT or NEEDS_PAINT since views delegate that to RenderElements.

use bitflags::bitflags;
use std::sync::atomic::{AtomicU8, Ordering};

bitflags! {
    /// View element state flags.
    ///
    /// Simpler than ElementFlags - only tracks build-related state.
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ViewFlags: u8 {
        /// View needs rebuild (build phase).
        const DIRTY   = 0b0000_0001;

        /// View is mounted in tree.
        const MOUNTED = 0b0000_0010;

        /// View is active (lifecycle).
        const ACTIVE  = 0b0000_0100;
    }
}

/// Atomic version of ViewFlags for lock-free access.
///
/// Provides thread-safe atomic operations on view flags.
#[derive(Debug)]
pub struct AtomicViewFlags {
    bits: AtomicU8,
}

impl AtomicViewFlags {
    /// Create new atomic flags (all cleared).
    #[inline]
    pub const fn new() -> Self {
        Self {
            bits: AtomicU8::new(0),
        }
    }

    /// Create atomic flags with initial value.
    #[inline]
    pub const fn from_flags(flags: ViewFlags) -> Self {
        Self {
            bits: AtomicU8::new(flags.bits()),
        }
    }

    /// Check if flag is set.
    #[inline]
    pub fn contains(&self, flag: ViewFlags) -> bool {
        let bits = self.bits.load(Ordering::Acquire);
        (bits & flag.bits()) != 0
    }

    /// Set flag (lock-free).
    #[inline]
    pub fn insert(&self, flag: ViewFlags) {
        self.bits.fetch_or(flag.bits(), Ordering::Release);
    }

    /// Clear flag (lock-free).
    #[inline]
    pub fn remove(&self, flag: ViewFlags) {
        self.bits.fetch_and(!flag.bits(), Ordering::Release);
    }

    /// Load all flags.
    #[inline]
    pub fn load(&self) -> ViewFlags {
        let bits = self.bits.load(Ordering::Acquire);
        ViewFlags::from_bits_truncate(bits)
    }

    /// Store all flags.
    #[inline]
    pub fn store(&self, flags: ViewFlags) {
        self.bits.store(flags.bits(), Ordering::Release);
    }

    /// Clear all flags.
    #[inline]
    pub fn clear(&self) {
        self.bits.store(0, Ordering::Release);
    }

    // ========== Convenience Methods ==========

    /// Check if needs rebuild.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.contains(ViewFlags::DIRTY)
    }

    /// Mark as needing rebuild.
    #[inline]
    pub fn mark_dirty(&self) {
        self.insert(ViewFlags::DIRTY);
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&self) {
        self.remove(ViewFlags::DIRTY);
    }

    /// Check if mounted.
    #[inline]
    pub fn is_mounted(&self) -> bool {
        self.contains(ViewFlags::MOUNTED)
    }

    /// Check if active.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.contains(ViewFlags::ACTIVE)
    }
}

impl Default for AtomicViewFlags {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AtomicViewFlags {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            bits: AtomicU8::new(self.bits.load(Ordering::Acquire)),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_flags_creation() {
        let flags = AtomicViewFlags::new();
        assert!(!flags.is_dirty());
        assert!(!flags.is_mounted());
        assert!(!flags.is_active());
    }

    #[test]
    fn test_atomic_flags_dirty() {
        let flags = AtomicViewFlags::new();

        flags.mark_dirty();
        assert!(flags.is_dirty());

        flags.clear_dirty();
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_flags_insert_remove() {
        let flags = AtomicViewFlags::new();

        flags.insert(ViewFlags::DIRTY | ViewFlags::MOUNTED);
        assert!(flags.is_dirty());
        assert!(flags.is_mounted());

        flags.remove(ViewFlags::DIRTY);
        assert!(!flags.is_dirty());
        assert!(flags.is_mounted());
    }

    #[test]
    fn test_atomic_flags_load_store() {
        let flags = AtomicViewFlags::new();

        let new_flags = ViewFlags::DIRTY | ViewFlags::ACTIVE;
        flags.store(new_flags);

        let loaded = flags.load();
        assert_eq!(loaded, new_flags);
    }

    #[test]
    fn test_atomic_flags_size() {
        use std::mem::size_of;
        assert_eq!(size_of::<AtomicViewFlags>(), 1);
    }

    #[test]
    fn test_atomic_flags_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let flags = Arc::new(AtomicViewFlags::new());

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let flags = Arc::clone(&flags);
                thread::spawn(move || {
                    flags.mark_dirty();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert!(flags.is_dirty());
    }
}
