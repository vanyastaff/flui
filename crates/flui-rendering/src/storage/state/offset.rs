//! Atomic offset storage and `RenderState<P>` offset accessors.
//!
//! This file contains the private `AtomicOffset` helper (lock-free f32 pair
//! packed into an `AtomicU64`) and the `offset()` / `set_offset()` methods
//! on `RenderState<P>`.

use std::sync::atomic::{AtomicU64, Ordering};

use flui_types::Offset;

use super::RenderState;
use crate::protocol::Protocol;

// ============================================================================
// ATOMIC OFFSET
// ============================================================================

/// Thread-safe offset storage using atomic operations.
///
/// Stores two f32 values in a single AtomicU64 for lock-free updates.
/// This is safe because we treat the bits as opaque data and use atomic
/// operations to ensure consistency.
#[derive(Debug)]
pub(super) struct AtomicOffset {
    bits: AtomicU64,
}

impl AtomicOffset {
    /// Creates a new atomic offset with the given initial value.
    #[inline]
    pub(super) const fn new(offset: Offset) -> Self {
        // Pack two f32s into a u64
        // Use .0.to_bits() instead of .to_bits() because Pixels::to_bits()
        // is not available in const context.
        let dx_bits = offset.dx.0.to_bits() as u64;
        let dy_bits = offset.dy.0.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        Self {
            bits: AtomicU64::new(packed),
        }
    }

    /// Loads the current offset atomically.
    #[inline]
    pub(super) fn load(&self) -> Offset {
        let packed = self.bits.load(Ordering::Acquire);
        let dx_bits = (packed & 0xFFFF_FFFF) as u32;
        let dy_bits = (packed >> 32) as u32;

        Offset {
            dx: flui_types::Pixels(f32::from_bits(dx_bits)),
            dy: flui_types::Pixels(f32::from_bits(dy_bits)),
        }
    }

    /// Stores a new offset atomically.
    #[inline]
    pub(super) fn store(&self, offset: Offset) {
        let dx_bits = offset.dx.0.to_bits() as u64;
        let dy_bits = offset.dy.0.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        self.bits.store(packed, Ordering::Release);
    }
}

// ============================================================================
// OFFSET (ATOMIC, LOCK-FREE)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the offset relative to parent (atomic, lock-free).
    ///
    /// This is set by the parent during layout and read during paint
    /// and hit testing.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single atomic load
    /// - No allocation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let screen_position = parent_offset + state.offset();
    /// ```
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset.load()
    }

    /// Sets the offset relative to parent (atomic, lock-free).
    ///
    /// This is called by the parent during layout to position this
    /// render object. Uses atomic operations for lock-free updates.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single atomic store
    /// - No allocation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Parent positioning child during layout
    /// child_state.set_offset(Offset::new(10.0, 20.0));
    /// ```
    #[inline]
    pub fn set_offset(&self, offset: Offset) {
        self.offset.store(offset);
    }
}
