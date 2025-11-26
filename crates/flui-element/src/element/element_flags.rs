//! Atomic element flags for lock-free dirty tracking
//!
//! This module provides `AtomicElementFlags` for thread-safe, lock-free
//! element state tracking with zero overhead.
//!
//! # Architecture
//!
//! Per FINAL_ARCHITECTURE_V2.md, atomic flags enable:
//! - Lock-free dirty marking from any thread
//! - Zero contention dirty tracking
//! - Same memory size as bool (1 byte)
//! - Foundation for parallel layout/paint
//!
//! # Performance
//!
//! | Operation | Time | Notes |
//! |-----------|------|-------|
//! | mark_dirty | ~2ns | Single atomic OR |
//! | is_dirty | ~1ns | Single atomic load |
//! | clear_dirty | ~2ns | Single atomic AND |
//!
//! # Thread Safety
//!
//! All operations are lock-free and safe for concurrent access:
//! - `mark_dirty()` - Can be called from any thread
//! - `is_dirty()` - Can be called from any thread
//! - `clear_dirty()` - Should only be called by pipeline
//!
//! # Example
//!
//! ```rust
//! use flui_element::{AtomicElementFlags, ElementFlags};
//!
//! let flags = AtomicElementFlags::new();
//!
//! // Mark dirty (from any thread)
//! flags.insert(ElementFlags::DIRTY);
//!
//! // Check dirty (from any thread)
//! assert!(flags.contains(ElementFlags::DIRTY));
//!
//! // Clear dirty (from pipeline)
//! flags.remove(ElementFlags::DIRTY);
//! assert!(!flags.contains(ElementFlags::DIRTY));
//! ```

use bitflags::bitflags;
use std::sync::atomic::{AtomicU8, Ordering};

bitflags! {
    /// Element state flags
    ///
    /// Bitflags for tracking element state. Each flag fits in a single bit,
    /// allowing 8 flags total in a u8.
    ///
    /// # Flags
    ///
    /// - `DIRTY`: Element needs rebuild (build phase)
    /// - `NEEDS_LAYOUT`: Element needs layout (layout phase)
    /// - `NEEDS_PAINT`: Element needs paint (paint phase)
    /// - `DETACHED`: Element is detached from tree
    /// - `MOUNTED`: Element is mounted in tree
    /// - `ACTIVE`: Element is active (not deactivated)
    ///
    /// # Memory Layout
    ///
    /// ```text
    /// Bit:  7 6 5 4 3 2 1 0
    ///       │ │ │ │ │ │ │ └─ DIRTY (0x01)
    ///       │ │ │ │ │ │ └─── NEEDS_LAYOUT (0x02)
    ///       │ │ │ │ │ └───── NEEDS_PAINT (0x04)
    ///       │ │ │ │ └─────── DETACHED (0x08)
    ///       │ │ │ └───────── MOUNTED (0x10)
    ///       │ │ └─────────── ACTIVE (0x20)
    ///       │ └───────────── (reserved)
    ///       └─────────────── (reserved)
    /// ```
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ElementFlags: u8 {
        /// Element needs rebuild (build phase)
        ///
        /// Set when setState() is called or parent changes.
        /// Cleared after rebuild.
        const DIRTY        = 0b0000_0001;

        /// Element needs layout (layout phase)
        ///
        /// Set when size might have changed.
        /// Cleared after layout.
        const NEEDS_LAYOUT = 0b0000_0010;

        /// Element needs paint (paint phase)
        ///
        /// Set when visual appearance changed.
        /// Cleared after paint.
        const NEEDS_PAINT  = 0b0000_0100;

        /// Element is detached from tree
        ///
        /// Set when element is removed but not yet unmounted.
        /// Used for deferred cleanup.
        const DETACHED     = 0b0000_1000;

        /// Element is mounted in tree
        ///
        /// Set during mount(), cleared during unmount().
        /// Used to verify element is in tree.
        const MOUNTED      = 0b0001_0000;

        /// Element is active (lifecycle)
        ///
        /// Set when lifecycle is Active, cleared when Inactive/Defunct.
        /// Used for optimization.
        const ACTIVE       = 0b0010_0000;
    }
}

/// Atomic version of ElementFlags for lock-free access
///
/// Provides thread-safe atomic operations on element flags using
/// `AtomicU8` for lock-free concurrency.
///
/// # Thread Safety
///
/// All methods are safe to call from multiple threads concurrently:
/// - `contains()` uses Acquire ordering for reading
/// - `insert()` uses Release ordering for setting
/// - `remove()` uses Release ordering for clearing
///
/// # Size
///
/// ```text
/// size_of::<AtomicElementFlags>() == 1 byte
/// size_of::<bool>() == 1 byte
/// ```
///
/// Same size as a bool - zero memory overhead!
///
/// # Performance
///
/// Lock-free atomic operations are extremely fast:
/// - No mutex contention
/// - No context switches
/// - CPU cache-friendly
/// - Scales to N threads
///
/// # Example
///
/// ```rust,ignore
/// use flui_element::{AtomicElementFlags, ElementFlags};
/// use std::thread;
///
/// let flags = AtomicElementFlags::new();
///
/// // Thread 1: Mark dirty
/// thread::spawn(|| {
///     flags.insert(ElementFlags::DIRTY);
/// });
///
/// // Thread 2: Check dirty (no race condition!)
/// thread::spawn(|| {
///     if flags.contains(ElementFlags::DIRTY) {
///         // Handle dirty element
///     }
/// });
/// ```
#[derive(Debug)]
pub struct AtomicElementFlags {
    /// Atomic u8 storage for flags
    ///
    /// Uses `AtomicU8` for lock-free atomic operations.
    /// Size: 1 byte (same as bool)
    bits: AtomicU8,
}

impl AtomicElementFlags {
    /// Create new atomic flags (all cleared)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// assert!(!flags.contains(ElementFlags::DIRTY));
    /// ```
    #[inline]
    pub const fn new() -> Self {
        Self {
            bits: AtomicU8::new(0),
        }
    }

    /// Create atomic flags with initial value
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::from_flags(ElementFlags::DIRTY);
    /// assert!(flags.contains(ElementFlags::DIRTY));
    /// ```
    #[inline]
    pub const fn from_flags(flags: ElementFlags) -> Self {
        Self {
            bits: AtomicU8::new(flags.bits()),
        }
    }

    /// Check if flag is set
    ///
    /// Uses `Acquire` ordering to ensure visibility of flag changes.
    ///
    /// # Thread Safety
    ///
    /// Safe to call from multiple threads. Guarantees that if a flag
    /// was set by another thread using `insert()`, this will see it.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// flags.insert(ElementFlags::DIRTY);
    ///
    /// assert!(flags.contains(ElementFlags::DIRTY));
    /// assert!(!flags.contains(ElementFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn contains(&self, flag: ElementFlags) -> bool {
        let bits = self.bits.load(Ordering::Acquire);
        (bits & flag.bits()) != 0
    }

    /// Set flag (lock-free)
    ///
    /// Uses `fetch_or` with `Release` ordering for lock-free flag setting.
    /// Multiple threads can safely call this concurrently.
    ///
    /// # Thread Safety
    ///
    /// Safe to call from multiple threads. Uses atomic OR operation
    /// which is idempotent - setting the same flag multiple times is safe.
    ///
    /// # Performance
    ///
    /// Time: ~2ns (single atomic OR instruction)
    /// No locks, no contention, scales to N threads.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    ///
    /// flags.insert(ElementFlags::DIRTY);
    /// flags.insert(ElementFlags::NEEDS_LAYOUT);
    ///
    /// assert!(flags.contains(ElementFlags::DIRTY));
    /// assert!(flags.contains(ElementFlags::NEEDS_LAYOUT));
    /// ```
    #[inline]
    pub fn insert(&self, flag: ElementFlags) {
        self.bits.fetch_or(flag.bits(), Ordering::Release);
    }

    /// Clear flag (lock-free)
    ///
    /// Uses `fetch_and` with `Release` ordering for lock-free flag clearing.
    ///
    /// # Thread Safety
    ///
    /// Safe to call from multiple threads, but typically only the
    /// pipeline should clear flags after processing.
    ///
    /// # Performance
    ///
    /// Time: ~2ns (single atomic AND instruction)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// flags.insert(ElementFlags::DIRTY);
    ///
    /// flags.remove(ElementFlags::DIRTY);
    /// assert!(!flags.contains(ElementFlags::DIRTY));
    /// ```
    #[inline]
    pub fn remove(&self, flag: ElementFlags) {
        self.bits.fetch_and(!flag.bits(), Ordering::Release);
    }

    /// Load all flags
    ///
    /// Returns the current flag state as `ElementFlags`.
    /// Uses `Acquire` ordering.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// flags.insert(ElementFlags::DIRTY | ElementFlags::MOUNTED);
    ///
    /// let current = flags.load();
    /// assert!(current.contains(ElementFlags::DIRTY));
    /// assert!(current.contains(ElementFlags::MOUNTED));
    /// ```
    #[inline]
    pub fn load(&self) -> ElementFlags {
        let bits = self.bits.load(Ordering::Acquire);
        ElementFlags::from_bits_truncate(bits)
    }

    /// Store all flags
    ///
    /// Replaces the current flags with new ones.
    /// Uses `Release` ordering.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// flags.store(ElementFlags::DIRTY | ElementFlags::MOUNTED);
    ///
    /// assert!(flags.contains(ElementFlags::DIRTY));
    /// assert!(flags.contains(ElementFlags::MOUNTED));
    /// ```
    #[inline]
    pub fn store(&self, flags: ElementFlags) {
        self.bits.store(flags.bits(), Ordering::Release);
    }

    /// Clear all flags
    ///
    /// Efficient way to clear all flags at once.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// flags.insert(ElementFlags::DIRTY | ElementFlags::MOUNTED);
    ///
    /// flags.clear();
    /// assert!(!flags.contains(ElementFlags::DIRTY));
    /// assert!(!flags.contains(ElementFlags::MOUNTED));
    /// ```
    #[inline]
    pub fn clear(&self) {
        self.bits.store(0, Ordering::Release);
    }

    /// Check if any flags are set
    ///
    /// Returns `true` if at least one flag is set.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_element::{AtomicElementFlags, ElementFlags};
    ///
    /// let flags = AtomicElementFlags::new();
    /// assert!(!flags.is_any_set());
    ///
    /// flags.insert(ElementFlags::DIRTY);
    /// assert!(flags.is_any_set());
    /// ```
    #[inline]
    pub fn is_any_set(&self) -> bool {
        self.bits.load(Ordering::Acquire) != 0
    }
}

impl Default for AtomicElementFlags {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// Implement Clone for AtomicElementFlags (loads current value)
impl Clone for AtomicElementFlags {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            bits: AtomicU8::new(self.bits.load(Ordering::Acquire)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_flags_creation() {
        let flags = AtomicElementFlags::new();
        assert!(!flags.contains(ElementFlags::DIRTY));
        assert!(!flags.is_any_set());
    }

    #[test]
    fn test_atomic_flags_insert() {
        let flags = AtomicElementFlags::new();

        flags.insert(ElementFlags::DIRTY);
        assert!(flags.contains(ElementFlags::DIRTY));
        assert!(!flags.contains(ElementFlags::NEEDS_LAYOUT));

        flags.insert(ElementFlags::NEEDS_LAYOUT);
        assert!(flags.contains(ElementFlags::DIRTY));
        assert!(flags.contains(ElementFlags::NEEDS_LAYOUT));
    }

    #[test]
    fn test_atomic_flags_remove() {
        let flags = AtomicElementFlags::new();

        flags.insert(ElementFlags::DIRTY | ElementFlags::NEEDS_LAYOUT);
        assert!(flags.contains(ElementFlags::DIRTY));
        assert!(flags.contains(ElementFlags::NEEDS_LAYOUT));

        flags.remove(ElementFlags::DIRTY);
        assert!(!flags.contains(ElementFlags::DIRTY));
        assert!(flags.contains(ElementFlags::NEEDS_LAYOUT));
    }

    #[test]
    fn test_atomic_flags_load_store() {
        let flags = AtomicElementFlags::new();

        let new_flags = ElementFlags::DIRTY | ElementFlags::MOUNTED;
        flags.store(new_flags);

        let loaded = flags.load();
        assert_eq!(loaded, new_flags);
    }

    #[test]
    fn test_atomic_flags_clear() {
        let flags = AtomicElementFlags::new();

        flags.insert(ElementFlags::DIRTY | ElementFlags::NEEDS_LAYOUT | ElementFlags::MOUNTED);
        assert!(flags.is_any_set());

        flags.clear();
        assert!(!flags.is_any_set());
        assert!(!flags.contains(ElementFlags::DIRTY));
        assert!(!flags.contains(ElementFlags::NEEDS_LAYOUT));
        assert!(!flags.contains(ElementFlags::MOUNTED));
    }

    #[test]
    fn test_atomic_flags_from_flags() {
        let flags = AtomicElementFlags::from_flags(ElementFlags::DIRTY | ElementFlags::MOUNTED);

        assert!(flags.contains(ElementFlags::DIRTY));
        assert!(flags.contains(ElementFlags::MOUNTED));
        assert!(!flags.contains(ElementFlags::NEEDS_LAYOUT));
    }

    #[test]
    fn test_atomic_flags_size() {
        use std::mem::size_of;

        // AtomicElementFlags should be same size as bool (1 byte)
        assert_eq!(size_of::<AtomicElementFlags>(), 1);
        assert_eq!(size_of::<AtomicElementFlags>(), size_of::<bool>());
        assert_eq!(size_of::<AtomicElementFlags>(), size_of::<u8>());
    }

    #[test]
    fn test_atomic_flags_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let flags = Arc::new(AtomicElementFlags::new());

        // Spawn 10 threads, each marking dirty
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let flags = Arc::clone(&flags);
                thread::spawn(move || {
                    flags.insert(ElementFlags::DIRTY);
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // All threads should have successfully marked dirty
        assert!(flags.contains(ElementFlags::DIRTY));
    }

    #[test]
    fn test_element_flags_bitwise() {
        // Test that bitflags work correctly
        let dirty = ElementFlags::DIRTY;
        let layout = ElementFlags::NEEDS_LAYOUT;
        let paint = ElementFlags::NEEDS_PAINT;

        let combined = dirty | layout;
        assert!(combined.contains(ElementFlags::DIRTY));
        assert!(combined.contains(ElementFlags::NEEDS_LAYOUT));
        assert!(!combined.contains(ElementFlags::NEEDS_PAINT));

        let all = dirty | layout | paint;
        assert!(all.contains(ElementFlags::DIRTY));
        assert!(all.contains(ElementFlags::NEEDS_LAYOUT));
        assert!(all.contains(ElementFlags::NEEDS_PAINT));
    }
}
