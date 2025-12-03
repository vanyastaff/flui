//! Render lifecycle - high-level state machine for render elements.
//!
//! This module provides a high-level lifecycle state machine that works
//! WITH `render_flags.rs` (not duplicating it). Think of it as:
//!
//! - **RenderFlags** (render_flags.rs) - Low-level atomic flags for dirty tracking
//! - **RenderLifecycle** (this file) - High-level state machine for lifecycle phases
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │     RenderLifecycle (enum)          │  High-level state machine
//! │  Detached → Attached → ... Painted  │  (Semantic meaning)
//! └─────────────────────────────────────┘
//!              ↕ (uses)
//! ┌─────────────────────────────────────┐
//! │   AtomicRenderFlags (bitflags)      │  Low-level atomic flags
//! │  NEEDS_LAYOUT | NEEDS_PAINT | ...   │  (Performance)
//! └─────────────────────────────────────┘
//! ```
//!
//! # Why Two Systems?
//!
//! ## AtomicRenderFlags (render_flags.rs)
//! - ✅ Lock-free atomic operations (1-5ns)
//! - ✅ Individual flag manipulation
//! - ✅ Bitmask optimizations
//! - ❌ No semantic meaning (just bits)
//! - ❌ No state transition validation
//!
//! ## RenderLifecycle (this file)
//! - ✅ Clear semantic meaning (what phase are we in?)
//! - ✅ State transition validation (debug)
//! - ✅ Explicit lifecycle documentation
//! - ❌ Slower (enum, not atomic)
//! - ❌ Can't check individual concerns
//!
//! # When to Use Which?
//!
//! **Use AtomicRenderFlags for:**
//! - Hot path dirty checks (`needs_layout()`, `needs_paint()`)
//! - Setting individual dirty flags
//! - Boundary checks (`is_relayout_boundary()`)
//! - Performance-critical code
//!
//! **Use RenderLifecycle for:**
//! - Understanding current lifecycle phase
//! - Debug assertions about valid operations
//! - Documentation and error messages
//! - State machine validation
//!
//! # Examples
//!
//! ## Using Both Together
//!
//! ```rust,ignore
//! // Lifecycle tracks high-level phase
//! let mut lifecycle = RenderLifecycle::Attached;
//!
//! // Flags track specific dirty state
//! let flags = AtomicRenderFlags::empty();
//! flags.mark_needs_layout();
//!
//! // Lifecycle follows flags
//! assert_eq!(lifecycle, RenderLifecycle::Attached);
//! assert!(flags.needs_layout());
//!
//! // After layout completes
//! flags.clear_needs_layout();
//! lifecycle.mark_laid_out();
//!
//! assert_eq!(lifecycle, RenderLifecycle::LaidOut);
//! assert!(!flags.needs_layout());
//! ```

use std::fmt;

use super::render_flags::{AtomicRenderFlags, RenderFlags};

// ============================================================================
// RENDER LIFECYCLE STATE MACHINE
// ============================================================================

/// High-level lifecycle state for render elements.
///
/// This enum tracks which phase of the rendering pipeline an element is in.
/// It works alongside `AtomicRenderFlags` which tracks specific dirty flags.
///
/// # States
///
/// ```text
/// Detached ←→ Attached → NeedsLayout → LaidOut → NeedsPaint → Painted
///              ↑            ↓                         ↓           │
///              │            └─────────────────────────┘           │
///              └────────────────────────────────────────────────────┘
/// ```
///
/// ## Detached
/// Not in tree, cannot render. Initial state.
///
/// ## Attached
/// In tree, needs initial layout.
///
/// ## NeedsLayout
/// Layout invalidated, needs relayout.
///
/// ## LaidOut
/// Layout complete, needs paint.
///
/// ## NeedsPaint
/// Paint invalidated (layout still valid).
///
/// ## Painted
/// Fully rendered, clean state.
///
/// # Relationship with RenderFlags
///
/// | Lifecycle | Typical Flags |
/// |-----------|---------------|
/// | Detached | (none - not in tree) |
/// | Attached | NEEDS_LAYOUT \| NEEDS_PAINT |
/// | NeedsLayout | NEEDS_LAYOUT \| NEEDS_PAINT |
/// | LaidOut | NEEDS_PAINT |
/// | NeedsPaint | NEEDS_PAINT |
/// | Painted | (none - clean) |
///
/// Note: Flags are authoritative for dirty state. Lifecycle is for phase tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderLifecycle {
    Detached = 0,
    Attached = 1,
    NeedsLayout = 2,
    LaidOut = 3,
    NeedsPaint = 4,
    Painted = 5,
}

impl Default for RenderLifecycle {
    fn default() -> Self {
        Self::Detached
    }
}

// ============================================================================
// LIFECYCLE QUERIES (Semantic)
// ============================================================================

impl RenderLifecycle {
    /// Returns whether element is attached to tree.
    #[inline]
    pub const fn is_attached(self) -> bool {
        !matches!(self, Self::Detached)
    }

    /// Returns whether element is detached.
    #[inline]
    pub const fn is_detached(self) -> bool {
        matches!(self, Self::Detached)
    }

    /// Returns whether element has completed layout phase.
    #[inline]
    pub const fn is_laid_out(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint | Self::Painted)
    }

    /// Returns whether element has completed paint phase.
    #[inline]
    pub const fn is_painted(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Returns whether in layout-needing phase (semantic).
    ///
    /// Note: For actual dirty state, check `AtomicRenderFlags::needs_layout()`.
    #[inline]
    pub const fn in_needs_layout_phase(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }

    /// Returns whether in paint-needing phase (semantic).
    ///
    /// Note: For actual dirty state, check `AtomicRenderFlags::needs_paint()`.
    #[inline]
    pub const fn in_needs_paint_phase(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint) || self.in_needs_layout_phase()
    }

    /// Returns whether in clean state (all phases complete).
    #[inline]
    pub const fn is_clean(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Returns whether in dirty state (needs processing).
    #[inline]
    pub const fn is_dirty(self) -> bool {
        !self.is_clean()
    }
}

// ============================================================================
// LIFECYCLE TRANSITIONS
// ============================================================================

impl RenderLifecycle {
    /// Transitions to Attached state.
    ///
    /// Called when element is inserted into tree.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if already attached.
    #[inline]
    pub fn attach(&mut self) {
        debug_assert!(
            *self == Self::Detached,
            "Cannot attach: already attached (state: {:?})",
            self
        );
        *self = Self::Attached;
    }

    /// Transitions to Detached state.
    ///
    /// Called when element is removed from tree.
    #[inline]
    pub fn detach(&mut self) {
        *self = Self::Detached;
    }

    /// Marks element as needing layout.
    ///
    /// Note: Also set `AtomicRenderFlags::mark_needs_layout()`.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        if self.is_attached() {
            *self = Self::NeedsLayout;
        }
    }

    /// Marks element as laid out.
    ///
    /// Note: Also clear `AtomicRenderFlags::clear_needs_layout()`.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not attached.
    #[inline]
    pub fn mark_laid_out(&mut self) {
        debug_assert!(
            self.is_attached(),
            "Cannot mark laid out: not attached (state: {:?})",
            self
        );
        *self = Self::LaidOut;
    }

    /// Marks element as needing paint (layout still valid).
    ///
    /// Note: Also set `AtomicRenderFlags::mark_needs_paint()`.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        if self.is_laid_out() {
            *self = Self::NeedsPaint;
        }
    }

    /// Marks element as painted.
    ///
    /// Note: Also clear `AtomicRenderFlags::clear_needs_paint()`.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not laid out.
    #[inline]
    pub fn mark_painted(&mut self) {
        debug_assert!(
            self.is_laid_out(),
            "Cannot mark painted: not laid out (state: {:?})",
            self
        );
        *self = Self::Painted;
    }

    // Aliases
    #[inline]
    pub fn invalidate_layout(&mut self) {
        self.mark_needs_layout();
    }

    #[inline]
    pub fn invalidate_paint(&mut self) {
        self.mark_needs_paint();
    }
}

// ============================================================================
// INTEGRATION WITH RENDER FLAGS
// ============================================================================

impl RenderLifecycle {
    /// Syncs lifecycle state from atomic flags.
    ///
    /// This is useful for ensuring lifecycle matches actual dirty state.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let flags = AtomicRenderFlags::empty();
    /// flags.mark_needs_layout();
    ///
    /// let lifecycle = RenderLifecycle::from_flags(&flags);
    /// assert_eq!(lifecycle, RenderLifecycle::NeedsLayout);
    /// ```
    pub fn from_flags(flags: &AtomicRenderFlags) -> Self {
        // If not attached, we can't determine lifecycle from flags alone
        // (this would require additional state tracking)

        if flags.needs_layout() {
            Self::NeedsLayout
        } else if flags.needs_paint() {
            Self::NeedsPaint
        } else if flags.has_geometry() {
            Self::Painted
        } else {
            // No geometry yet, assume needs initial layout
            Self::Attached
        }
    }

    /// Updates flags to match lifecycle state.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut lifecycle = RenderLifecycle::NeedsLayout;
    /// let flags = AtomicRenderFlags::empty();
    ///
    /// lifecycle.sync_to_flags(&flags);
    /// assert!(flags.needs_layout());
    /// ```
    pub fn sync_to_flags(&self, flags: &AtomicRenderFlags) {
        match self {
            Self::Detached => {
                // Detached elements shouldn't have flags set
                flags.clear();
            }
            Self::Attached | Self::NeedsLayout => {
                flags.mark_needs_layout();
                flags.mark_needs_paint();
            }
            Self::LaidOut => {
                flags.clear_needs_layout();
                flags.mark_needs_paint();
                flags.mark_has_geometry();
            }
            Self::NeedsPaint => {
                flags.clear_needs_layout();
                flags.mark_needs_paint();
            }
            Self::Painted => {
                flags.clear_needs_layout();
                flags.clear_needs_paint();
                flags.mark_has_geometry();
            }
        }
    }
}

// ============================================================================
// DIAGNOSTICS
// ============================================================================

impl RenderLifecycle {
    /// Returns detailed description of lifecycle state.
    pub fn description(&self) -> String {
        let state = match self {
            Self::Detached => "Detached",
            Self::Attached => "Attached",
            Self::NeedsLayout => "NeedsLayout",
            Self::LaidOut => "LaidOut",
            Self::NeedsPaint => "NeedsPaint",
            Self::Painted => "Painted",
        };

        let mut flags = Vec::new();
        if self.is_attached() {
            flags.push("attached");
        }
        if self.in_needs_layout_phase() {
            flags.push("needs layout");
        }
        if self.in_needs_paint_phase() {
            flags.push("needs paint");
        }
        if self.is_laid_out() {
            flags.push("has geometry");
        }
        if self.is_clean() {
            flags.push("clean");
        }

        format!("{} ({})", state, flags.join(", "))
    }

    /// Checks if transition is valid.
    pub fn can_transition_to(&self, next: RenderLifecycle) -> bool {
        use RenderLifecycle::*;

        match (*self, next) {
            (Detached, Attached) => true,
            (_, Detached) => true,
            (Attached | NeedsLayout, LaidOut) => true,
            (LaidOut | NeedsPaint, Painted) => true,
            (Attached | LaidOut | NeedsPaint | Painted, NeedsLayout) => true,
            (LaidOut | Painted, NeedsPaint) => true,
            (state, next) if state == next => true,
            _ => false,
        }
    }

    /// Validates transition (debug only).
    #[cfg(debug_assertions)]
    pub fn assert_valid_transition(&self, next: RenderLifecycle, operation: &str) {
        debug_assert!(
            self.can_transition_to(next),
            "Invalid lifecycle transition during {}: {:?} → {:?}",
            operation,
            self,
            next
        );
    }
}

impl fmt::Display for RenderLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Detached => write!(f, "Detached"),
            Self::Attached => write!(f, "Attached"),
            Self::NeedsLayout => write!(f, "NeedsLayout"),
            Self::LaidOut => write!(f, "LaidOut"),
            Self::NeedsPaint => write!(f, "NeedsPaint"),
            Self::Painted => write!(f, "Painted"),
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
    fn test_lifecycle_states() {
        let mut lifecycle = RenderLifecycle::Detached;
        assert!(lifecycle.is_detached());

        lifecycle.attach();
        assert_eq!(lifecycle, RenderLifecycle::Attached);
        assert!(lifecycle.is_attached());

        lifecycle.mark_laid_out();
        assert!(lifecycle.is_laid_out());

        lifecycle.mark_painted();
        assert!(lifecycle.is_painted());
        assert!(lifecycle.is_clean());
    }

    #[test]
    fn test_integration_with_flags() {
        let flags = AtomicRenderFlags::empty();

        // Start with needs layout
        flags.mark_needs_layout();
        let lifecycle = RenderLifecycle::from_flags(&flags);
        assert_eq!(lifecycle, RenderLifecycle::NeedsLayout);

        // Clear layout, set paint
        flags.clear_needs_layout();
        flags.mark_needs_paint();
        flags.mark_has_geometry();
        let lifecycle = RenderLifecycle::from_flags(&flags);
        assert_eq!(lifecycle, RenderLifecycle::NeedsPaint);

        // Clear paint
        flags.clear_needs_paint();
        let lifecycle = RenderLifecycle::from_flags(&flags);
        assert_eq!(lifecycle, RenderLifecycle::Painted);
    }

    #[test]
    fn test_sync_to_flags() {
        let flags = AtomicRenderFlags::empty();

        let lifecycle = RenderLifecycle::NeedsLayout;
        lifecycle.sync_to_flags(&flags);
        assert!(flags.needs_layout());
        assert!(flags.needs_paint());

        let lifecycle = RenderLifecycle::LaidOut;
        lifecycle.sync_to_flags(&flags);
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(flags.has_geometry());
    }

    #[test]
    fn test_paint_only_invalidation() {
        let mut lifecycle = RenderLifecycle::Painted;

        lifecycle.mark_needs_paint();
        assert_eq!(lifecycle, RenderLifecycle::NeedsPaint);
        assert!(lifecycle.is_laid_out());
    }
}
