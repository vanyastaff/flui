//! Render object lifecycle management.
//!
//! This module provides [`RenderLifecycle`] enum that replaces multiple boolean
//! flags from Flutter (`_needsLayout`, `_needsPaint`, etc.) with a single
//! type-safe state machine.
//!
//! # Benefits over Flutter's approach
//!
//! - **Memory efficiency**: 1 byte enum vs 3-4 bytes of booleans
//! - **Type safety**: Compile-time state transition validation
//! - **Clarity**: Single source of truth for lifecycle state
//! - **Debug**: Clear state names in error messages
//!
//! # Example
//!
//! ```
//! use flui_rendering::lifecycle::{RenderLifecycle, DirtyFlags};
//!
//! let mut lifecycle = RenderLifecycle::Detached;
//! assert!(lifecycle.can_transition_to(RenderLifecycle::Attached));
//!
//! lifecycle = RenderLifecycle::Attached;
//! assert!(lifecycle.can_transition_to(RenderLifecycle::NeedsLayout));
//! ```

use bitflags::bitflags;

// ============================================================================
// RenderLifecycle Enum
// ============================================================================

/// Lifecycle state of a render object.
///
/// This enum represents the state machine for render object lifecycle,
/// replacing Flutter's multiple boolean flags with a single byte.
///
/// # State Machine
///
/// ```text
///                     ┌──────────────┐
///                     │   Detached   │ (initial)
///                     └──────┬───────┘
///                            │ attach()
///                            ▼
///                     ┌──────────────┐
///                     │   Attached   │
///                     └──────┬───────┘
///                            │ mark_needs_layout()
///                            ▼
///         ┌──────────┬───────────────┐
///         │          │  NeedsLayout  │
///         │          └───────┬───────┘
///         │                  │ perform_layout()
///         │                  ▼
///         │          ┌───────────────┐
///         │          │    LaidOut    │◀─────────┐
///         │          └───────┬───────┘          │
///         │                  │ mark_needs_paint()│
///         │                  ▼                   │
///         │          ┌───────────────┐          │
///         │          │  NeedsPaint   │          │
///         │          └───────┬───────┘          │
///         │                  │ paint()          │
///         │                  ▼                  │
///         │          ┌───────────────┐          │
///         │          │    Painted    │          │
///         │          └───────┬───────┘          │
///         │                  │                  │
///         │                  └──────────────────┘
///         │                     mark_needs_layout()
///         │
///         └─────────────────────────────────────▶ Disposed
///                            (terminal)
/// ```
///
/// # Flutter Equivalence
///
/// | Flutter | FLUI |
/// |---------|------|
/// | `owner == null` | `Detached` |
/// | `owner != null` (initial) | `Attached` |
/// | `_needsLayout == true` | `NeedsLayout` |
/// | `_needsLayout == false` | `LaidOut` |
/// | `_needsPaint == true` | `NeedsPaint` |
/// | `_needsPaint == false` | `Painted` |
/// | `_debugDisposed == true` | `Disposed` |
///
/// # Memory Layout
///
/// Size: 1 byte (`#[repr(u8)]`)
///
/// Flutter uses 3-4 boolean flags = 3-4 bytes + padding = 4-8 bytes.
/// FLUI uses 1 byte enum = **75-87% memory savings per node**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum RenderLifecycle {
    /// Not attached to pipeline.
    ///
    /// This is the initial state when a render object is created.
    /// The object has no owner and cannot participate in layout or paint.
    ///
    /// # Flutter Equivalence
    /// `owner == null`
    #[default]
    Detached = 0,

    /// Attached to pipeline but not yet laid out.
    ///
    /// The object has an owner but needs its first layout pass.
    /// Automatically transitions to `NeedsLayout` when attached.
    ///
    /// # Flutter Equivalence
    /// `owner != null` (initial state after attach)
    Attached = 1,

    /// Needs layout.
    ///
    /// The object's layout information is stale and needs to be recomputed.
    /// This is set by `mark_needs_layout()`.
    ///
    /// # Flutter Equivalence
    /// `_needsLayout == true`
    NeedsLayout = 2,

    /// Layout complete, ready for paint.
    ///
    /// The object has valid layout information but may need painting.
    /// This is set after `perform_layout()` completes.
    ///
    /// # Flutter Equivalence
    /// `_needsLayout == false`
    LaidOut = 3,

    /// Needs paint.
    ///
    /// The object's visual appearance is stale and needs to be repainted.
    /// This is set by `mark_needs_paint()`.
    ///
    /// # Flutter Equivalence
    /// `_needsPaint == true`
    NeedsPaint = 4,

    /// Paint complete.
    ///
    /// The object has been painted and is visually up-to-date.
    /// This is the normal "clean" state for an attached object.
    ///
    /// # Flutter Equivalence
    /// `_needsPaint == false`
    Painted = 5,

    /// Resource has been disposed (terminal state).
    ///
    /// The object has been disposed and cannot be used again.
    /// This is a terminal state - no transitions out of this state are valid.
    ///
    /// # Flutter Equivalence
    /// `dispose()` called, `_debugDisposed = true`
    Disposed = 6,
}

impl RenderLifecycle {
    // ========================================================================
    // State Transition Validation
    // ========================================================================

    /// Check if transition to another state is valid.
    ///
    /// This encodes the state machine rules for render object lifecycle.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::lifecycle::RenderLifecycle;
    ///
    /// let state = RenderLifecycle::Attached;
    /// assert!(state.can_transition_to(RenderLifecycle::NeedsLayout));
    /// assert!(!state.can_transition_to(RenderLifecycle::Painted));
    /// ```
    #[inline]
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use RenderLifecycle::*;
        matches!(
            (self, next),
            // From Detached
            (Detached, Attached)
                | (Detached, Disposed)

                // From Attached
                | (Attached, NeedsLayout)
                | (Attached, Disposed)

                // From NeedsLayout
                | (NeedsLayout, LaidOut)
                | (NeedsLayout, Disposed)

                // From LaidOut
                | (LaidOut, NeedsPaint)
                | (LaidOut, NeedsLayout) // Relayout
                | (LaidOut, Disposed)

                // From NeedsPaint
                | (NeedsPaint, Painted)
                | (NeedsPaint, NeedsLayout) // Relayout during paint
                | (NeedsPaint, Disposed)

                // From Painted
                | (Painted, NeedsLayout) // Relayout
                | (Painted, NeedsPaint) // Repaint
                | (Painted, Disposed)
        )
    }

    /// Transition to a new state, panicking if the transition is invalid.
    ///
    /// In debug builds, this validates the transition. In release builds,
    /// validation is skipped for performance.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if the transition is not valid.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::lifecycle::RenderLifecycle;
    ///
    /// let mut state = RenderLifecycle::Detached;
    /// state = state.transition_to(RenderLifecycle::Attached);
    /// assert_eq!(state, RenderLifecycle::Attached);
    /// ```
    #[inline]
    #[must_use]
    pub fn transition_to(self, next: Self) -> Self {
        debug_assert!(
            self.can_transition_to(next),
            "Invalid lifecycle transition: {:?} -> {:?}",
            self,
            next
        );
        next
    }

    // ========================================================================
    // State Queries
    // ========================================================================

    /// Can perform layout in this state?
    ///
    /// Returns `true` if the object can have `perform_layout` called.
    #[inline]
    #[must_use]
    pub const fn can_layout(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }

    /// Can perform paint in this state?
    ///
    /// Returns `true` if the object can have `paint` called.
    #[inline]
    #[must_use]
    pub const fn can_paint(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint)
    }

    /// Is node attached to pipeline?
    ///
    /// Returns `true` if the object has an owner and is part of the render tree.
    #[inline]
    #[must_use]
    pub const fn is_attached(self) -> bool {
        matches!(
            self,
            Self::Attached | Self::NeedsLayout | Self::LaidOut | Self::NeedsPaint | Self::Painted
        )
    }

    /// Is node usable (not detached or disposed)?
    ///
    /// Returns `true` if the object can participate in the rendering pipeline.
    #[inline]
    #[must_use]
    pub const fn is_usable(self) -> bool {
        !matches!(self, Self::Detached | Self::Disposed)
    }

    /// Needs layout?
    ///
    /// Returns `true` if `mark_needs_layout` has been called and layout
    /// has not yet been performed.
    #[inline]
    #[must_use]
    pub const fn needs_layout(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }

    /// Needs paint?
    ///
    /// Returns `true` if `mark_needs_paint` has been called and paint
    /// has not yet been performed.
    #[inline]
    #[must_use]
    pub const fn needs_paint(self) -> bool {
        matches!(self, Self::NeedsPaint)
    }

    /// Is in clean state (painted)?
    ///
    /// Returns `true` if the object is fully laid out and painted.
    #[inline]
    #[must_use]
    pub const fn is_clean(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Is disposed (terminal state)?
    ///
    /// Returns `true` if the object has been disposed and cannot be used.
    #[inline]
    #[must_use]
    pub const fn is_disposed(self) -> bool {
        matches!(self, Self::Disposed)
    }

    /// Is detached (not in tree)?
    ///
    /// Returns `true` if the object is not attached to a pipeline owner.
    #[inline]
    #[must_use]
    pub const fn is_detached(self) -> bool {
        matches!(self, Self::Detached)
    }

    // ========================================================================
    // Display
    // ========================================================================

    /// Returns a human-readable name for this state.
    #[inline]
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detached => "Detached",
            Self::Attached => "Attached",
            Self::NeedsLayout => "NeedsLayout",
            Self::LaidOut => "LaidOut",
            Self::NeedsPaint => "NeedsPaint",
            Self::Painted => "Painted",
            Self::Disposed => "Disposed",
        }
    }
}

impl std::fmt::Display for RenderLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// DirtyFlags Bitflags
// ============================================================================

bitflags! {
    /// Dirty flags for render objects.
    ///
    /// These flags track what needs to be updated, independent of
    /// lifecycle state. This matches Flutter's approach where dirty
    /// flags can be set regardless of attach/detach state.
    ///
    /// # Memory Layout
    ///
    /// Size: 1 byte (`u8`)
    ///
    /// Combined with `RenderLifecycle` (1 byte), total lifecycle state
    /// is 2 bytes vs Flutter's 4-8 bytes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct DirtyFlags: u8 {
        /// Needs layout.
        ///
        /// Set when something changes that affects the layout of this
        /// object. This flag works independently of attach state.
        ///
        /// # Flutter Equivalence
        /// `_needsLayout`
        const NEEDS_LAYOUT = 1 << 0;

        /// Needs paint.
        ///
        /// Set when something changes that affects the visual appearance
        /// of this object but not its layout.
        ///
        /// # Flutter Equivalence
        /// `_needsPaint`
        const NEEDS_PAINT = 1 << 1;

        /// Needs compositing bits update.
        ///
        /// Set when something changes that affects whether this object
        /// or its descendants need compositing layers.
        ///
        /// # Flutter Equivalence
        /// `_needsCompositingBitsUpdate`
        const NEEDS_COMPOSITING_BITS = 1 << 2;

        /// Needs semantics update.
        ///
        /// Set when something changes that affects the accessibility
        /// tree for this object.
        ///
        /// # Flutter Equivalence
        /// `_needsSemanticsUpdate`
        const NEEDS_SEMANTICS = 1 << 3;

        /// Needs composited layer update.
        ///
        /// Set when a property of the composited layer changes but
        /// the children don't need to be repainted.
        ///
        /// # Flutter Equivalence
        /// Part of `_needsCompositedLayerUpdate` logic
        const NEEDS_COMPOSITED_LAYER_UPDATE = 1 << 4;

        /// Is a relayout boundary.
        ///
        /// Set when this object's parent doesn't use its size,
        /// meaning layout changes don't propagate upward.
        ///
        /// # Flutter Equivalence
        /// `_relayoutBoundary == this`
        const IS_RELAYOUT_BOUNDARY = 1 << 5;

        /// Is a repaint boundary.
        ///
        /// Set when this object creates its own compositing layer.
        ///
        /// # Flutter Equivalence
        /// `isRepaintBoundary == true`
        const IS_REPAINT_BOUNDARY = 1 << 6;

        /// Needs compositing.
        ///
        /// Set when this object or a descendant requires compositing.
        ///
        /// # Flutter Equivalence
        /// `_needsCompositing`
        const NEEDS_COMPOSITING = 1 << 7;
    }
}

impl DirtyFlags {
    /// Returns whether layout is needed.
    #[inline]
    #[must_use]
    pub const fn needs_layout(self) -> bool {
        self.contains(Self::NEEDS_LAYOUT)
    }

    /// Returns whether paint is needed.
    #[inline]
    #[must_use]
    pub const fn needs_paint(self) -> bool {
        self.contains(Self::NEEDS_PAINT)
    }

    /// Returns whether compositing bits need to be updated.
    #[inline]
    #[must_use]
    pub const fn needs_compositing_bits_update(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITING_BITS)
    }

    /// Returns whether semantics need to be updated.
    #[inline]
    #[must_use]
    pub const fn needs_semantics_update(self) -> bool {
        self.contains(Self::NEEDS_SEMANTICS)
    }

    /// Returns whether the composited layer needs to be updated.
    #[inline]
    #[must_use]
    pub const fn needs_composited_layer_update(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITED_LAYER_UPDATE)
    }

    /// Returns whether this is a relayout boundary.
    #[inline]
    #[must_use]
    pub const fn is_relayout_boundary(self) -> bool {
        self.contains(Self::IS_RELAYOUT_BOUNDARY)
    }

    /// Returns whether this is a repaint boundary.
    #[inline]
    #[must_use]
    pub const fn is_repaint_boundary(self) -> bool {
        self.contains(Self::IS_REPAINT_BOUNDARY)
    }

    /// Returns whether compositing is needed.
    #[inline]
    #[must_use]
    pub const fn needs_compositing(self) -> bool {
        self.contains(Self::NEEDS_COMPOSITING)
    }
}

// ============================================================================
// RenderState - Combined Lifecycle + Flags
// ============================================================================

/// Combined render object state (lifecycle + dirty flags).
///
/// This struct combines `RenderLifecycle` and `DirtyFlags` into a single
/// 2-byte state representation, providing a complete picture of render
/// object state with minimal memory usage.
///
/// # Memory Layout
///
/// - `lifecycle`: 1 byte
/// - `flags`: 1 byte
/// - **Total: 2 bytes**
///
/// Compare to Flutter's approach:
/// - `_needsLayout`: 1 byte
/// - `_needsPaint`: 1 byte
/// - `_needsCompositingBitsUpdate`: 1 byte
/// - `_needsSemanticsUpdate`: 1 byte
/// - `_relayoutBoundary`: pointer (8 bytes on 64-bit)
/// - Other flags...
/// - **Total: 8+ bytes**
///
/// # Example
///
/// ```
/// use flui_rendering::lifecycle::{RenderState, RenderLifecycle, DirtyFlags};
///
/// let mut state = RenderState::new();
/// assert!(state.lifecycle().is_detached());
///
/// state.set_lifecycle(RenderLifecycle::Attached);
/// state.mark_needs_semantics();
/// assert!(state.flags().needs_semantics_update());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderState {
    lifecycle: RenderLifecycle,
    flags: DirtyFlags,
}

impl RenderState {
    /// Creates a new render state in detached state with no flags set.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            lifecycle: RenderLifecycle::Detached,
            flags: DirtyFlags::empty(),
        }
    }

    /// Creates a new render state with the given lifecycle and flags.
    #[inline]
    #[must_use]
    pub const fn with_lifecycle_and_flags(lifecycle: RenderLifecycle, flags: DirtyFlags) -> Self {
        Self { lifecycle, flags }
    }

    // ========================================================================
    // Lifecycle Access
    // ========================================================================

    /// Returns the current lifecycle state.
    #[inline]
    #[must_use]
    pub const fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Sets the lifecycle state.
    ///
    /// In debug builds, validates that the transition is legal.
    #[inline]
    pub fn set_lifecycle(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = self.lifecycle.transition_to(lifecycle);
    }

    /// Sets the lifecycle state without validation.
    ///
    /// # Safety
    ///
    /// Caller must ensure the transition is valid.
    #[inline]
    pub fn set_lifecycle_unchecked(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = lifecycle;
    }

    // ========================================================================
    // Flags Access
    // ========================================================================

    /// Returns the current dirty flags.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> DirtyFlags {
        self.flags
    }

    /// Returns mutable reference to dirty flags.
    #[inline]
    pub fn flags_mut(&mut self) -> &mut DirtyFlags {
        &mut self.flags
    }

    /// Sets the dirty flags.
    #[inline]
    pub fn set_flags(&mut self, flags: DirtyFlags) {
        self.flags = flags;
    }

    // ========================================================================
    // Lifecycle Shortcuts
    // ========================================================================

    /// Returns whether the object is attached.
    #[inline]
    #[must_use]
    pub const fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    /// Returns whether the object is disposed.
    #[inline]
    #[must_use]
    pub const fn is_disposed(&self) -> bool {
        self.lifecycle.is_disposed()
    }

    // ========================================================================
    // Flag Shortcuts (Dirty State)
    // ========================================================================

    /// Returns whether the object needs layout.
    ///
    /// This is a flag-based check that works regardless of attach state.
    #[inline]
    #[must_use]
    pub const fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }

    /// Returns whether the object needs paint.
    ///
    /// This is a flag-based check that works regardless of attach state.
    #[inline]
    #[must_use]
    pub const fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }

    /// Returns whether compositing bits need update.
    #[inline]
    #[must_use]
    pub const fn needs_compositing_bits_update(&self) -> bool {
        self.flags.needs_compositing_bits_update()
    }

    /// Returns whether semantics need update.
    #[inline]
    #[must_use]
    pub const fn needs_semantics_update(&self) -> bool {
        self.flags.needs_semantics_update()
    }

    /// Returns whether this is a relayout boundary.
    #[inline]
    #[must_use]
    pub const fn is_relayout_boundary(&self) -> bool {
        self.flags.is_relayout_boundary()
    }

    /// Returns whether this is a repaint boundary.
    #[inline]
    #[must_use]
    pub const fn is_repaint_boundary(&self) -> bool {
        self.flags.is_repaint_boundary()
    }

    /// Returns whether compositing is needed.
    #[inline]
    #[must_use]
    pub const fn needs_compositing(&self) -> bool {
        self.flags.needs_compositing()
    }

    // ========================================================================
    // Marking Methods
    // ========================================================================

    /// Marks the object as needing layout.
    ///
    /// This sets the NEEDS_LAYOUT flag regardless of attach state.
    /// The flag will be used by the pipeline owner when attached.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.flags.insert(DirtyFlags::NEEDS_LAYOUT);
    }

    /// Marks the object as needing paint.
    ///
    /// This sets the NEEDS_PAINT flag regardless of attach state.
    /// The flag will be used by the pipeline owner when attached.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.flags.insert(DirtyFlags::NEEDS_PAINT);
    }

    /// Marks compositing bits as needing update.
    #[inline]
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.flags.insert(DirtyFlags::NEEDS_COMPOSITING_BITS);
    }

    /// Marks semantics as needing update.
    #[inline]
    pub fn mark_needs_semantics(&mut self) {
        self.flags.insert(DirtyFlags::NEEDS_SEMANTICS);
    }

    /// Marks composited layer as needing update.
    #[inline]
    pub fn mark_needs_composited_layer_update(&mut self) {
        self.flags.insert(DirtyFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    // ========================================================================
    // Clearing Methods
    // ========================================================================

    /// Clears the needs_layout flag after layout completes.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.remove(DirtyFlags::NEEDS_LAYOUT);
    }

    /// Clears the needs_paint flag after paint completes.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.remove(DirtyFlags::NEEDS_PAINT);
    }

    /// Clears the needs_compositing_bits_update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.flags.remove(DirtyFlags::NEEDS_COMPOSITING_BITS);
    }

    /// Clears the needs_semantics_update flag.
    #[inline]
    pub fn clear_needs_semantics(&mut self) {
        self.flags.remove(DirtyFlags::NEEDS_SEMANTICS);
    }

    /// Clears the needs_composited_layer_update flag.
    #[inline]
    pub fn clear_needs_composited_layer_update(&mut self) {
        self.flags.remove(DirtyFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
    }

    // ========================================================================
    // Boundary Methods
    // ========================================================================

    /// Sets whether this is a relayout boundary.
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        if is_boundary {
            self.flags.insert(DirtyFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(DirtyFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Sets whether this is a repaint boundary.
    #[inline]
    pub fn set_repaint_boundary(&mut self, is_boundary: bool) {
        if is_boundary {
            self.flags.insert(DirtyFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.flags.remove(DirtyFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Sets whether compositing is needed.
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        if needs {
            self.flags.insert(DirtyFlags::NEEDS_COMPOSITING);
        } else {
            self.flags.remove(DirtyFlags::NEEDS_COMPOSITING);
        }
    }

    // ========================================================================
    // Lifecycle Transitions
    // ========================================================================

    /// Attaches the object to a pipeline owner.
    ///
    /// Transitions from `Detached` to `Attached` and marks needs_layout.
    #[inline]
    pub fn attach(&mut self) {
        debug_assert!(
            self.lifecycle == RenderLifecycle::Detached,
            "Can only attach detached objects, current state: {:?}",
            self.lifecycle
        );
        self.lifecycle = RenderLifecycle::Attached;
        // Mark needs layout on attach (Flutter behavior)
        self.flags.insert(DirtyFlags::NEEDS_LAYOUT);
    }

    /// Detaches the object from its pipeline owner.
    ///
    /// Transitions to `Detached` state. Dirty flags are preserved.
    #[inline]
    pub fn detach(&mut self) {
        debug_assert!(
            self.lifecycle.is_attached(),
            "Can only detach attached objects, current state: {:?}",
            self.lifecycle
        );
        self.lifecycle = RenderLifecycle::Detached;
    }

    /// Disposes the object.
    ///
    /// Transitions to terminal `Disposed` state and clears all flags.
    #[inline]
    pub fn dispose(&mut self) {
        debug_assert!(
            self.lifecycle != RenderLifecycle::Disposed,
            "Object already disposed"
        );
        self.lifecycle = RenderLifecycle::Disposed;
        self.flags = DirtyFlags::empty();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // RenderLifecycle Tests
    // ========================================================================

    #[test]
    fn test_lifecycle_size() {
        assert_eq!(std::mem::size_of::<RenderLifecycle>(), 1);
    }

    #[test]
    fn test_lifecycle_default() {
        let lifecycle = RenderLifecycle::default();
        assert_eq!(lifecycle, RenderLifecycle::Detached);
    }

    #[test]
    fn test_lifecycle_transitions_from_detached() {
        let state = RenderLifecycle::Detached;
        assert!(state.can_transition_to(RenderLifecycle::Attached));
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::NeedsLayout));
        assert!(!state.can_transition_to(RenderLifecycle::LaidOut));
        assert!(!state.can_transition_to(RenderLifecycle::Painted));
    }

    #[test]
    fn test_lifecycle_transitions_from_attached() {
        let state = RenderLifecycle::Attached;
        assert!(state.can_transition_to(RenderLifecycle::NeedsLayout));
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::Detached));
        assert!(!state.can_transition_to(RenderLifecycle::Painted));
    }

    #[test]
    fn test_lifecycle_transitions_from_needs_layout() {
        let state = RenderLifecycle::NeedsLayout;
        assert!(state.can_transition_to(RenderLifecycle::LaidOut));
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::Attached));
        assert!(!state.can_transition_to(RenderLifecycle::Painted));
    }

    #[test]
    fn test_lifecycle_transitions_from_laid_out() {
        let state = RenderLifecycle::LaidOut;
        assert!(state.can_transition_to(RenderLifecycle::NeedsPaint));
        assert!(state.can_transition_to(RenderLifecycle::NeedsLayout)); // Relayout
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::Attached));
    }

    #[test]
    fn test_lifecycle_transitions_from_needs_paint() {
        let state = RenderLifecycle::NeedsPaint;
        assert!(state.can_transition_to(RenderLifecycle::Painted));
        assert!(state.can_transition_to(RenderLifecycle::NeedsLayout)); // Relayout during paint
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::Attached));
    }

    #[test]
    fn test_lifecycle_transitions_from_painted() {
        let state = RenderLifecycle::Painted;
        assert!(state.can_transition_to(RenderLifecycle::NeedsLayout)); // Relayout
        assert!(state.can_transition_to(RenderLifecycle::NeedsPaint)); // Repaint
        assert!(state.can_transition_to(RenderLifecycle::Disposed));
        assert!(!state.can_transition_to(RenderLifecycle::Attached));
    }

    #[test]
    fn test_lifecycle_disposed_is_terminal() {
        let state = RenderLifecycle::Disposed;
        assert!(!state.can_transition_to(RenderLifecycle::Detached));
        assert!(!state.can_transition_to(RenderLifecycle::Attached));
        assert!(!state.can_transition_to(RenderLifecycle::NeedsLayout));
        assert!(!state.can_transition_to(RenderLifecycle::Disposed));
    }

    #[test]
    fn test_lifecycle_queries() {
        assert!(RenderLifecycle::Attached.can_layout());
        assert!(RenderLifecycle::NeedsLayout.can_layout());
        assert!(!RenderLifecycle::LaidOut.can_layout());

        assert!(RenderLifecycle::LaidOut.can_paint());
        assert!(RenderLifecycle::NeedsPaint.can_paint());
        assert!(!RenderLifecycle::NeedsLayout.can_paint());

        assert!(RenderLifecycle::Attached.is_attached());
        assert!(RenderLifecycle::Painted.is_attached());
        assert!(!RenderLifecycle::Detached.is_attached());
        assert!(!RenderLifecycle::Disposed.is_attached());

        assert!(RenderLifecycle::Attached.is_usable());
        assert!(!RenderLifecycle::Detached.is_usable());
        assert!(!RenderLifecycle::Disposed.is_usable());
    }

    #[test]
    fn test_lifecycle_display() {
        assert_eq!(RenderLifecycle::Detached.to_string(), "Detached");
        assert_eq!(RenderLifecycle::NeedsLayout.to_string(), "NeedsLayout");
        assert_eq!(RenderLifecycle::Painted.to_string(), "Painted");
    }

    // ========================================================================
    // DirtyFlags Tests
    // ========================================================================

    #[test]
    fn test_dirty_flags_size() {
        assert_eq!(std::mem::size_of::<DirtyFlags>(), 1);
    }

    #[test]
    fn test_dirty_flags_default() {
        let flags = DirtyFlags::default();
        assert!(flags.is_empty());
    }

    #[test]
    fn test_dirty_flags_operations() {
        let mut flags = DirtyFlags::empty();

        flags.insert(DirtyFlags::NEEDS_COMPOSITING_BITS);
        assert!(flags.needs_compositing_bits_update());
        assert!(!flags.needs_semantics_update());

        flags.insert(DirtyFlags::NEEDS_SEMANTICS);
        assert!(flags.needs_semantics_update());

        flags.remove(DirtyFlags::NEEDS_COMPOSITING_BITS);
        assert!(!flags.needs_compositing_bits_update());
        assert!(flags.needs_semantics_update());
    }

    #[test]
    fn test_dirty_flags_boundaries() {
        let mut flags = DirtyFlags::empty();

        flags.insert(DirtyFlags::IS_RELAYOUT_BOUNDARY);
        assert!(flags.is_relayout_boundary());

        flags.insert(DirtyFlags::IS_REPAINT_BOUNDARY);
        assert!(flags.is_repaint_boundary());
    }

    // ========================================================================
    // RenderState Tests
    // ========================================================================

    #[test]
    fn test_render_state_size() {
        assert_eq!(std::mem::size_of::<RenderState>(), 2);
    }

    #[test]
    fn test_render_state_default() {
        let state = RenderState::new();
        assert_eq!(state.lifecycle(), RenderLifecycle::Detached);
        assert!(state.flags().is_empty());
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
    }

    #[test]
    fn test_render_state_attach_flow() {
        let mut state = RenderState::new();

        // Attach - lifecycle becomes Attached, needs_layout flag is set
        state.attach();
        assert_eq!(state.lifecycle(), RenderLifecycle::Attached);
        assert!(state.needs_layout());

        // Layout complete - clear needs_layout flag
        state.clear_needs_layout();
        assert!(!state.needs_layout());

        // Mark needs paint
        state.mark_needs_paint();
        assert!(state.needs_paint());

        // Paint complete - clear needs_paint flag
        state.clear_needs_paint();
        assert!(!state.needs_paint());
    }

    #[test]
    fn test_render_state_dirty_flags_independent_of_attach() {
        let mut state = RenderState::new();

        // Dirty flags work even when detached (unlike lifecycle-based approach)
        assert!(!state.needs_layout());
        state.mark_needs_layout();
        assert!(state.needs_layout());

        state.mark_needs_paint();
        assert!(state.needs_paint());

        // Clear and verify
        state.clear_needs_layout();
        assert!(!state.needs_layout());
        assert!(state.needs_paint()); // paint flag still set
    }

    #[test]
    fn test_render_state_compositing_flags() {
        let mut state = RenderState::new();

        state.mark_needs_compositing_bits_update();
        assert!(state.needs_compositing_bits_update());

        state.mark_needs_semantics();
        assert!(state.needs_semantics_update());

        state.clear_needs_compositing_bits_update();
        assert!(!state.needs_compositing_bits_update());
        assert!(state.needs_semantics_update());
    }

    #[test]
    fn test_render_state_boundaries() {
        let mut state = RenderState::new();

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_repaint_boundary(true);
        assert!(state.is_repaint_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
        assert!(state.is_repaint_boundary());
    }

    #[test]
    fn test_render_state_dispose() {
        let mut state = RenderState::new();
        state.attach();
        state.mark_needs_semantics();
        state.mark_needs_layout();

        state.dispose();
        assert!(state.is_disposed());
        assert!(state.flags().is_empty()); // All flags cleared
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
    }

    #[test]
    fn test_render_state_detach_preserves_flags() {
        let mut state = RenderState::new();
        state.attach();
        state.mark_needs_paint();

        // Detach preserves dirty flags (object may be reattached)
        state.detach();
        assert!(state.lifecycle().is_detached());
        assert!(state.needs_layout()); // Set on attach
        assert!(state.needs_paint()); // Explicitly set
    }

    // ========================================================================
    // Send + Sync Tests
    // ========================================================================

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RenderLifecycle>();
        assert_send_sync::<DirtyFlags>();
        assert_send_sync::<RenderState>();
    }
}
