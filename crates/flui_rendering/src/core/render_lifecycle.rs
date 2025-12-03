//! Render lifecycle - states and flags for render elements.
//!
//! This module implements Flutter's RenderObject lifecycle semantics with proper
//! state transitions, dirty tracking, and boundary management.
//!
//! # Flutter RenderObject Lifecycle
//!
//! Flutter's rendering pipeline has three main phases:
//! 1. **Layout** - Compute sizes and positions
//! 2. **Paint** - Draw to canvas
//! 3. **Compositing** - Layer composition (optional in FLUI)
//!
//! Each phase is managed through dirty flags and owner-managed lists.
//!
//! # Design Philosophy
//!
//! - **Explicit state tracking**: Clear lifecycle states with validation
//! - **Dirty flags**: Separate flags for layout, paint, and semantics
//! - **Boundary support**: Relayout and repaint boundaries for optimization
//! - **Flutter compliance**: Exact semantics from Flutter's RenderObject
//!
//! # State Transitions
//!
//! ```text
//! Detached ←→ Attached → NeedsLayout → LaidOut → NeedsPaint → Painted
//!              ↑            ↓                         ↓           │
//!              │            └─────────────────────────┘           │
//!              └────────────────────────────────────────────────────┘
//! ```
//!
//! # Examples
//!
//! ## Basic Lifecycle
//!
//! ```rust
//! use flui_rendering::core::RenderLifecycle;
//!
//! let mut lifecycle = RenderLifecycle::default(); // Detached
//! assert!(lifecycle.is_detached());
//!
//! // Attach to tree
//! lifecycle.attach();
//! assert!(lifecycle.is_attached());
//! assert!(lifecycle.needs_layout());
//!
//! // Complete layout
//! lifecycle.mark_laid_out();
//! assert!(lifecycle.is_laid_out());
//! assert!(lifecycle.needs_paint());
//!
//! // Complete paint
//! lifecycle.mark_painted();
//! assert!(lifecycle.is_painted());
//! ```
//!
//! ## Invalidation
//!
//! ```rust
//! use flui_rendering::core::RenderLifecycle;
//!
//! let mut lifecycle = RenderLifecycle::Painted;
//!
//! // Invalidate just paint (e.g., color change)
//! lifecycle.mark_needs_paint();
//! assert!(lifecycle.needs_paint());
//! assert!(lifecycle.is_laid_out()); // Layout still valid
//!
//! // Invalidate layout (e.g., size change)
//! lifecycle.mark_needs_layout();
//! assert!(lifecycle.needs_layout());
//! assert!(!lifecycle.is_laid_out()); // Layout invalidated
//! ```
//!
//! ## Boundary Optimization
//!
//! ```rust,ignore
//! // Relayout boundary prevents layout propagation upward
//! if render_object.is_relayout_boundary() {
//!     // Layout stops here, don't mark parent dirty
//! }
//!
//! // Repaint boundary enables layer caching
//! if render_object.is_repaint_boundary() {
//!     // Create new compositing layer
//! }
//! ```

use std::fmt;

// ============================================================================
// RENDER LIFECYCLE STATES
// ============================================================================

/// Lifecycle states for render elements (Flutter-style).
///
/// Render elements progress through these states during the rendering pipeline.
/// The lifecycle matches Flutter's RenderObject state machine exactly.
///
/// # States
///
/// ## Detached
/// - Not attached to render tree
/// - Initial state or after removal
/// - Cannot participate in layout/paint
/// - Must attach before any operations
///
/// ## Attached
/// - Attached to tree, needs layout
/// - Ready for layout phase
/// - Cannot paint until laid out
/// - May have stale geometry
///
/// ## NeedsLayout
/// - Explicitly marked as needing layout
/// - Will be processed in next layout phase
/// - Typically from invalidation
/// - Equivalent to Attached in terms of operations
///
/// ## LaidOut
/// - Layout complete, geometry computed
/// - Size and position are valid
/// - Ready for paint phase
/// - Needs paint to become visible
///
/// ## NeedsPaint
/// - Layout valid but needs repaint
/// - Geometry unchanged
/// - Will be processed in next paint phase
/// - Common after style changes
///
/// ## Painted
/// - Fully rendered and visible
/// - All phases complete
/// - Can be displayed
/// - Clean state until invalidated
///
/// # Flutter Equivalent
///
/// Flutter doesn't use a single enum but tracks multiple boolean flags:
/// - `_needsLayout` - Similar to Attached/NeedsLayout
/// - `_needsPaint` - Similar to LaidOut/NeedsPaint
/// - `_needsCompositingBitsUpdate` - Not tracked in basic lifecycle
///
/// FLUI combines these into explicit states for clarity.
///
/// # Thread Safety
///
/// RenderLifecycle is Copy and can be read from any thread. However, mutations
/// should be synchronized through render tree access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderLifecycle {
    /// Not attached to tree. Initial state or after removal.
    ///
    /// Operations allowed: attach()
    /// Operations forbidden: layout(), paint(), all others
    Detached = 0,

    /// Attached to tree, needs layout.
    ///
    /// Operations allowed: layout()
    /// Operations forbidden: paint() (until laid out)
    Attached = 1,

    /// Explicitly marked as needing layout.
    ///
    /// Functionally equivalent to Attached, but indicates explicit invalidation.
    /// Flutter doesn't distinguish this in lifecycle state.
    ///
    /// Operations allowed: layout()
    /// Operations forbidden: paint() (until laid out)
    NeedsLayout = 2,

    /// Layout complete, geometry valid.
    ///
    /// Operations allowed: paint(), read geometry
    /// Operations forbidden: none (all operations valid)
    LaidOut = 3,

    /// Needs repaint but layout is valid.
    ///
    /// Operations allowed: paint()
    /// Operations forbidden: none (can skip layout)
    NeedsPaint = 4,

    /// Fully painted and ready.
    ///
    /// Clean state. All operations allowed.
    /// This is the terminal "happy state" until invalidation.
    Painted = 5,
}

impl Default for RenderLifecycle {
    fn default() -> Self {
        Self::Detached
    }
}

// ============================================================================
// LIFECYCLE QUERIES
// ============================================================================

impl RenderLifecycle {
    /// Returns whether element is attached to tree.
    ///
    /// Attached elements can participate in layout and paint. Detached elements
    /// are isolated and cannot be rendered.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// bool get attached => owner != null;
    /// ```
    #[inline]
    pub const fn is_attached(self) -> bool {
        !matches!(self, Self::Detached)
    }

    /// Returns whether element is detached from tree.
    ///
    /// Detached elements are in initial state or have been removed from tree.
    #[inline]
    pub const fn is_detached(self) -> bool {
        matches!(self, Self::Detached)
    }

    /// Returns whether element has valid layout.
    ///
    /// Elements with valid layout have computed geometry (size, position) that
    /// can be used for painting and hit testing.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// bool get debugNeedsLayout => _needsLayout;
    /// // is_laid_out() ≈ !debugNeedsLayout
    /// ```
    #[inline]
    pub const fn is_laid_out(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint | Self::Painted)
    }

    /// Returns whether element has been painted.
    ///
    /// Painted elements are fully rendered and visible.
    #[inline]
    pub const fn is_painted(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Returns whether element needs layout.
    ///
    /// Elements needing layout should be processed in the next layout phase.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// bool _needsLayout = true;
    /// ```
    #[inline]
    pub const fn needs_layout(self) -> bool {
        matches!(self, Self::Attached | Self::NeedsLayout)
    }

    /// Returns whether element needs paint.
    ///
    /// Elements needing paint should be processed in the next paint phase.
    /// Layout may or may not be valid.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// bool _needsPaint = false;
    /// ```
    #[inline]
    pub const fn needs_paint(self) -> bool {
        matches!(self, Self::LaidOut | Self::NeedsPaint) || self.needs_layout() // If needs layout, will need paint after
    }

    /// Returns whether layout is clean (valid and no pending layout).
    ///
    /// Clean layout means geometry can be trusted and element doesn't need
    /// relayout. This is the opposite of needs_layout().
    #[inline]
    pub const fn has_clean_layout(self) -> bool {
        !self.needs_layout()
    }

    /// Returns whether paint is clean (valid and no pending paint).
    ///
    /// Clean paint means the element's visual representation is up-to-date.
    #[inline]
    pub const fn has_clean_paint(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Returns whether element is fully clean (both layout and paint).
    ///
    /// Fully clean elements don't need any work in rendering pipeline.
    #[inline]
    pub const fn is_clean(self) -> bool {
        matches!(self, Self::Painted)
    }

    /// Returns whether element is dirty (needs any work).
    ///
    /// Dirty elements need processing in layout and/or paint phases.
    #[inline]
    pub const fn is_dirty(self) -> bool {
        !self.is_clean()
    }
}

// ============================================================================
// LIFECYCLE TRANSITIONS
// ============================================================================

impl RenderLifecycle {
    /// Transitions from Detached to Attached state.
    ///
    /// This is called when a render element is inserted into the tree.
    /// After attaching, the element needs layout before it can be painted.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// @override
    /// void attach(PipelineOwner owner) {
    ///   super.attach(owner);
    ///   _needsLayout = true;
    ///   _needsPaint = true;
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics in debug mode if already attached.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Detached;
    /// lifecycle.attach();
    /// assert!(lifecycle.is_attached());
    /// assert!(lifecycle.needs_layout());
    /// ```
    #[inline]
    pub fn attach(&mut self) {
        debug_assert!(
            *self == Self::Detached,
            "Cannot attach: already attached (current state: {:?})",
            self
        );
        *self = Self::Attached;
    }

    /// Transitions to Detached state.
    ///
    /// This is called when a render element is removed from the tree.
    /// Detached elements cannot participate in rendering until reattached.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// @override
    /// void detach() {
    ///   super.detach();
    ///   // owner = null (implicitly)
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Painted;
    /// lifecycle.detach();
    /// assert!(lifecycle.is_detached());
    /// ```
    #[inline]
    pub fn detach(&mut self) {
        *self = Self::Detached;
    }

    /// Marks element as needing layout.
    ///
    /// This is called when something changes that requires relayout:
    /// - Size constraints changed
    /// - Children added/removed
    /// - Properties that affect layout changed
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// void markNeedsLayout() {
    ///   if (_needsLayout) return;
    ///   if (_relayoutBoundary == null) {
    ///     _needsLayout = true;
    ///     if (parent != null) parent.markNeedsLayout();
    ///     return;
    ///   }
    ///   if (_relayoutBoundary != this) {
    ///     _relayoutBoundary.markNeedsLayout();
    ///   } else {
    ///     _needsLayout = true;
    ///     owner._nodesNeedingLayout.add(this);
    ///   }
    /// }
    /// ```
    ///
    /// # Performance Note
    ///
    /// This transitions to NeedsLayout state even if already needs layout.
    /// The caller should check needs_layout() first to avoid unnecessary work.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Painted;
    /// lifecycle.mark_needs_layout();
    /// assert!(lifecycle.needs_layout());
    /// assert!(!lifecycle.is_laid_out()); // Layout invalidated
    /// ```
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        if self.is_attached() {
            *self = Self::NeedsLayout;
        }
    }

    /// Marks element as needing paint (layout still valid).
    ///
    /// This is called when something changes that only requires repaint:
    /// - Color changed
    /// - Opacity changed
    /// - Decoration changed (but not size)
    ///
    /// Layout remains valid, only paint is dirty.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// void markNeedsPaint() {
    ///   if (_needsPaint) return;
    ///   _needsPaint = true;
    ///   if (isRepaintBoundary) {
    ///     owner._nodesNeedingPaint.add(this);
    ///   } else if (parent != null) {
    ///     parent.markNeedsPaint();
    ///   }
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Painted;
    /// lifecycle.mark_needs_paint();
    /// assert!(lifecycle.needs_paint());
    /// assert!(lifecycle.is_laid_out()); // Layout still valid!
    /// ```
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        if self.is_laid_out() {
            *self = Self::NeedsPaint;
        }
    }

    /// Marks layout as complete.
    ///
    /// This is called after successfully computing geometry. The element
    /// transitions to LaidOut state and becomes ready for paint.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// void layout(Constraints constraints, {bool parentUsesSize = false}) {
    ///   // ... perform layout ...
    ///   _needsLayout = false;
    ///   // But might still need paint
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not attached.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Attached;
    /// lifecycle.mark_laid_out();
    /// assert!(lifecycle.is_laid_out());
    /// assert!(lifecycle.needs_paint()); // Now needs paint
    /// ```
    #[inline]
    pub fn mark_laid_out(&mut self) {
        debug_assert!(
            self.is_attached(),
            "Cannot mark laid out: not attached (current state: {:?})",
            self
        );
        *self = Self::LaidOut;
    }

    /// Marks paint as complete.
    ///
    /// This is called after successfully painting to canvas. The element
    /// transitions to Painted state, which is the terminal clean state.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// void paint(PaintingContext context, Offset offset) {
    ///   // ... perform paint ...
    ///   _needsPaint = false;
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics in debug mode if not laid out.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::LaidOut;
    /// lifecycle.mark_painted();
    /// assert!(lifecycle.is_painted());
    /// assert!(lifecycle.is_clean());
    /// ```
    #[inline]
    pub fn mark_painted(&mut self) {
        debug_assert!(
            self.is_laid_out(),
            "Cannot mark painted: not laid out (current state: {:?})",
            self
        );
        *self = Self::Painted;
    }

    /// Invalidates layout (transitions back to NeedsLayout).
    ///
    /// This is an alias for mark_needs_layout() for backward compatibility.
    /// Prefer mark_needs_layout() in new code.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Painted;
    /// lifecycle.invalidate_layout();
    /// assert!(lifecycle.needs_layout());
    /// ```
    #[inline]
    pub fn invalidate_layout(&mut self) {
        self.mark_needs_layout();
    }

    /// Invalidates paint (transitions back to NeedsPaint).
    ///
    /// This is an alias for mark_needs_paint() for backward compatibility.
    /// Prefer mark_needs_paint() in new code.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let mut lifecycle = RenderLifecycle::Painted;
    /// lifecycle.invalidate_paint();
    /// assert!(lifecycle.needs_paint());
    /// assert!(lifecycle.is_laid_out()); // Layout still valid
    /// ```
    #[inline]
    pub fn invalidate_paint(&mut self) {
        self.mark_needs_paint();
    }
}

// ============================================================================
// DEBUG HELPERS
// ============================================================================

impl RenderLifecycle {
    /// Returns a human-readable description of the current state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::RenderLifecycle;
    ///
    /// let lifecycle = RenderLifecycle::NeedsLayout;
    /// println!("{}", lifecycle.description());
    /// // Output: "NeedsLayout (attached, needs layout, layout dirty)"
    /// ```
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
        } else {
            flags.push("detached");
        }
        if self.needs_layout() {
            flags.push("needs layout");
        }
        if self.needs_paint() {
            flags.push("needs paint");
        }
        if self.is_laid_out() {
            flags.push("layout valid");
        }
        if self.is_painted() {
            flags.push("paint valid");
        }
        if self.is_clean() {
            flags.push("clean");
        }
        if self.is_dirty() {
            flags.push("dirty");
        }

        format!("{} ({})", state, flags.join(", "))
    }

    /// Returns whether the state transition is valid.
    ///
    /// Used for validation in debug builds.
    ///
    /// # Valid Transitions
    ///
    /// ```text
    /// Detached → Attached (attach)
    /// Attached → NeedsLayout (mark dirty)
    /// Attached → LaidOut (complete layout)
    /// NeedsLayout → LaidOut (complete layout)
    /// LaidOut → NeedsPaint (mark paint dirty)
    /// LaidOut → Painted (complete paint)
    /// NeedsPaint → Painted (complete paint)
    /// * → Detached (detach)
    /// * → NeedsLayout (invalidate)
    /// ```
    pub fn can_transition_to(&self, next: RenderLifecycle) -> bool {
        use RenderLifecycle::*;

        match (*self, next) {
            // Attach
            (Detached, Attached) => true,

            // Detach (always valid)
            (_, Detached) => true,

            // Layout completion
            (Attached | NeedsLayout, LaidOut) => true,

            // Paint completion
            (LaidOut | NeedsPaint, Painted) => true,

            // Invalidations (when attached)
            (Attached | LaidOut | NeedsPaint | Painted, NeedsLayout) => true,
            (LaidOut | Painted, NeedsPaint) => true,

            // Already in target state (no-op, allowed)
            (state, next) if state == next => true,

            // Invalid transitions
            _ => false,
        }
    }

    /// Validates a state transition, panicking if invalid (debug only).
    ///
    /// This is used internally by transition methods in debug builds.
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

// ============================================================================
// DISPLAY
// ============================================================================

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
    fn test_default() {
        assert_eq!(RenderLifecycle::default(), RenderLifecycle::Detached);
    }

    #[test]
    fn test_is_attached() {
        assert!(!RenderLifecycle::Detached.is_attached());
        assert!(RenderLifecycle::Attached.is_attached());
        assert!(RenderLifecycle::NeedsLayout.is_attached());
        assert!(RenderLifecycle::LaidOut.is_attached());
        assert!(RenderLifecycle::NeedsPaint.is_attached());
        assert!(RenderLifecycle::Painted.is_attached());
    }

    #[test]
    fn test_is_detached() {
        assert!(RenderLifecycle::Detached.is_detached());
        assert!(!RenderLifecycle::Attached.is_detached());
        assert!(!RenderLifecycle::Painted.is_detached());
    }

    #[test]
    fn test_is_laid_out() {
        assert!(!RenderLifecycle::Detached.is_laid_out());
        assert!(!RenderLifecycle::Attached.is_laid_out());
        assert!(!RenderLifecycle::NeedsLayout.is_laid_out());
        assert!(RenderLifecycle::LaidOut.is_laid_out());
        assert!(RenderLifecycle::NeedsPaint.is_laid_out());
        assert!(RenderLifecycle::Painted.is_laid_out());
    }

    #[test]
    fn test_is_painted() {
        assert!(!RenderLifecycle::Detached.is_painted());
        assert!(!RenderLifecycle::Attached.is_painted());
        assert!(!RenderLifecycle::NeedsLayout.is_painted());
        assert!(!RenderLifecycle::LaidOut.is_painted());
        assert!(!RenderLifecycle::NeedsPaint.is_painted());
        assert!(RenderLifecycle::Painted.is_painted());
    }

    #[test]
    fn test_needs_layout() {
        assert!(!RenderLifecycle::Detached.needs_layout());
        assert!(RenderLifecycle::Attached.needs_layout());
        assert!(RenderLifecycle::NeedsLayout.needs_layout());
        assert!(!RenderLifecycle::LaidOut.needs_layout());
        assert!(!RenderLifecycle::NeedsPaint.needs_layout());
        assert!(!RenderLifecycle::Painted.needs_layout());
    }

    #[test]
    fn test_needs_paint() {
        // Detached/Attached/NeedsLayout need paint *after* layout
        assert!(RenderLifecycle::Attached.needs_paint());
        assert!(RenderLifecycle::NeedsLayout.needs_paint());
        // These states explicitly need paint
        assert!(RenderLifecycle::LaidOut.needs_paint());
        assert!(RenderLifecycle::NeedsPaint.needs_paint());
        // Painted doesn't need paint
        assert!(!RenderLifecycle::Painted.needs_paint());
    }

    #[test]
    fn test_is_clean() {
        assert!(!RenderLifecycle::Detached.is_clean());
        assert!(!RenderLifecycle::Attached.is_clean());
        assert!(!RenderLifecycle::NeedsLayout.is_clean());
        assert!(!RenderLifecycle::LaidOut.is_clean());
        assert!(!RenderLifecycle::NeedsPaint.is_clean());
        assert!(RenderLifecycle::Painted.is_clean());
    }

    #[test]
    fn test_is_dirty() {
        assert!(RenderLifecycle::Detached.is_dirty());
        assert!(RenderLifecycle::Attached.is_dirty());
        assert!(RenderLifecycle::NeedsLayout.is_dirty());
        assert!(RenderLifecycle::LaidOut.is_dirty());
        assert!(RenderLifecycle::NeedsPaint.is_dirty());
        assert!(!RenderLifecycle::Painted.is_dirty());
    }

    #[test]
    fn test_full_lifecycle() {
        let mut lifecycle = RenderLifecycle::Detached;

        // Attach
        lifecycle.attach();
        assert_eq!(lifecycle, RenderLifecycle::Attached);
        assert!(lifecycle.needs_layout());

        // Complete layout
        lifecycle.mark_laid_out();
        assert_eq!(lifecycle, RenderLifecycle::LaidOut);
        assert!(lifecycle.is_laid_out());
        assert!(lifecycle.needs_paint());

        // Complete paint
        lifecycle.mark_painted();
        assert_eq!(lifecycle, RenderLifecycle::Painted);
        assert!(lifecycle.is_clean());

        // Detach
        lifecycle.detach();
        assert_eq!(lifecycle, RenderLifecycle::Detached);
    }

    #[test]
    fn test_paint_only_invalidation() {
        let mut lifecycle = RenderLifecycle::Painted;

        // Invalidate just paint
        lifecycle.mark_needs_paint();
        assert_eq!(lifecycle, RenderLifecycle::NeedsPaint);
        assert!(lifecycle.is_laid_out()); // Layout still valid!
        assert!(lifecycle.needs_paint());

        // Complete paint
        lifecycle.mark_painted();
        assert_eq!(lifecycle, RenderLifecycle::Painted);
    }

    #[test]
    fn test_layout_invalidation() {
        let mut lifecycle = RenderLifecycle::Painted;

        // Invalidate layout (also invalidates paint)
        lifecycle.mark_needs_layout();
        assert_eq!(lifecycle, RenderLifecycle::NeedsLayout);
        assert!(!lifecycle.is_laid_out());
        assert!(lifecycle.needs_layout());
        assert!(lifecycle.needs_paint()); // Will need paint after layout

        // Complete layout
        lifecycle.mark_laid_out();
        assert_eq!(lifecycle, RenderLifecycle::LaidOut);
        assert!(lifecycle.is_laid_out());
        assert!(lifecycle.needs_paint());

        // Complete paint
        lifecycle.mark_painted();
        assert_eq!(lifecycle, RenderLifecycle::Painted);
        assert!(lifecycle.is_clean());
    }

    #[test]
    fn test_aliases() {
        let mut lifecycle = RenderLifecycle::Painted;

        lifecycle.invalidate_paint();
        assert!(lifecycle.needs_paint());

        lifecycle.mark_painted();
        lifecycle.invalidate_layout();
        assert!(lifecycle.needs_layout());
    }

    #[test]
    fn test_display() {
        assert_eq!(RenderLifecycle::Detached.to_string(), "Detached");
        assert_eq!(RenderLifecycle::Attached.to_string(), "Attached");
        assert_eq!(RenderLifecycle::NeedsLayout.to_string(), "NeedsLayout");
        assert_eq!(RenderLifecycle::LaidOut.to_string(), "LaidOut");
        assert_eq!(RenderLifecycle::NeedsPaint.to_string(), "NeedsPaint");
        assert_eq!(RenderLifecycle::Painted.to_string(), "Painted");
    }

    #[test]
    fn test_description() {
        let desc = RenderLifecycle::Painted.description();
        assert!(desc.contains("Painted"));
        assert!(desc.contains("clean"));

        let desc = RenderLifecycle::NeedsLayout.description();
        assert!(desc.contains("NeedsLayout"));
        assert!(desc.contains("needs layout"));
        assert!(desc.contains("dirty"));
    }

    #[test]
    fn test_valid_transitions() {
        use RenderLifecycle::*;

        // Attach
        assert!(Detached.can_transition_to(Attached));

        // Detach (always valid)
        assert!(Painted.can_transition_to(Detached));
        assert!(Attached.can_transition_to(Detached));

        // Layout completion
        assert!(Attached.can_transition_to(LaidOut));
        assert!(NeedsLayout.can_transition_to(LaidOut));

        // Paint completion
        assert!(LaidOut.can_transition_to(Painted));
        assert!(NeedsPaint.can_transition_to(Painted));

        // Invalidations
        assert!(Painted.can_transition_to(NeedsLayout));
        assert!(Painted.can_transition_to(NeedsPaint));
        assert!(LaidOut.can_transition_to(NeedsPaint));

        // Invalid transitions
        assert!(!Detached.can_transition_to(LaidOut)); // Can't layout when detached
        assert!(!Detached.can_transition_to(Painted)); // Can't paint when detached
        assert!(!Attached.can_transition_to(Painted)); // Can't skip layout
    }

    #[test]
    fn test_copy() {
        let lifecycle1 = RenderLifecycle::Painted;
        let lifecycle2 = lifecycle1; // Copy
        let _lifecycle3 = lifecycle1; // Can still use lifecycle1
        assert_eq!(lifecycle1, lifecycle2);
    }

    #[test]
    fn test_size() {
        use std::mem::size_of;
        // Should be just 1 byte (u8)
        assert_eq!(size_of::<RenderLifecycle>(), 1);
    }

    #[test]
    #[should_panic(expected = "Cannot attach: already attached")]
    #[cfg(debug_assertions)]
    fn test_double_attach_panics() {
        let mut lifecycle = RenderLifecycle::Attached;
        lifecycle.attach(); // Should panic
    }

    #[test]
    #[should_panic(expected = "Cannot mark painted: not laid out")]
    #[cfg(debug_assertions)]
    fn test_paint_without_layout_panics() {
        let mut lifecycle = RenderLifecycle::Attached;
        lifecycle.mark_painted(); // Should panic - not laid out
    }
}
