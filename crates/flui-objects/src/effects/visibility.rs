//! RenderVisibility - Advanced visibility control with fine-grained options
//!
//! Implements Flutter's Visibility widget with granular control over what aspects
//! of the child are preserved when hidden (size, state, animations, interactivity, semantics).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderVisibility` | `RenderVisibility` from `package:flutter/src/widgets/visibility.dart` |
//! | `visible` | `visible` property (bool) |
//! | `maintain_size` | `maintainSize` property (bool) |
//! | `maintain_state` | `maintainState` property (bool) |
//! | `maintain_animation` | `maintainAnimation` property (bool) |
//! | `maintain_interactivity` | `maintainInteractivity` property (bool) |
//! | `maintain_semantics` | `maintainSemantics` property (bool) |
//!
//! # Layout Protocol
//!
//! 1. **Determine if child should be laid out**
//!    - If `visible = true`: always layout
//!    - If `visible = false`: layout if `maintain_state = true` OR `maintain_size = true`
//!    - Otherwise: skip layout (child completely removed)
//!
//! 2. **Return size based on visibility and flags**
//!    - If `visible = true` OR `maintain_size = true`: return child size
//!    - Otherwise: return `Size::ZERO`
//!
//! # Paint Protocol
//!
//! 1. **Check visible flag**
//!    - If `visible = true`: paint child normally
//!    - If `visible = false`: skip painting (no visual output)
//!
//! 2. **Interactivity and Semantics** (TODO)
//!    - `maintain_interactivity`: Controls hit testing when invisible
//!    - `maintain_semantics`: Controls accessibility tree when invisible
//!
//! # Performance
//!
//! - **Layout**:
//!   - O(1) when child completely removed (visible=false, maintain_state=false, maintain_size=false)
//!   - O(child) when visible or maintaining state/size
//! - **Paint**:
//!   - O(1) when visible = false (skip painting)
//!   - O(child) when visible = true (normal painting)
//! - **Memory**: 6 bytes (6 bool flags)
//!
//! # Use Cases
//!
//! - **Fade animations**: Hide with `maintain_size=true` for smooth opacity transitions
//! - **Placeholder loading**: Reserve space with `maintain_size=true` while content loads
//! - **Complex state preservation**: Keep widget state alive with `maintain_state=true`
//! - **Conditional rendering**: Remove widget entirely with all flags false
//! - **Animation continuity**: Keep animations running with `maintain_animation=true`
//! - **Accessibility**: Control screen reader visibility with `maintain_semantics`
//!
//! # Visibility Mode Combinations
//!
//! **Fully visible:**
//! ```text
//! visible=true → Normal rendering, all features active
//! ```
//!
//! **Invisible placeholder (for fade animations):**
//! ```text
//! visible=false, maintain_size=true → Space reserved, no visual output
//! ```
//!
//! **Hidden but stateful (tab views):**
//! ```text
//! visible=false, maintain_state=true → State preserved, no space taken
//! ```
//!
//! **Completely removed:**
//! ```text
//! visible=false, all maintain_* flags=false → No layout, no paint, no space
//! ```
//!
//! # Comparison with Other Visibility Approaches
//!
//! **RenderVisibility (this):**
//! - 6 different control flags
//! - Fine-grained control over what's preserved
//! - Most flexible but most complex
//!
//! **RenderOffstage:**
//! - Single offstage flag
//! - Always layouts child (state preserved)
//! - Simpler API, less flexibility
//!
//! **RenderOpacity(0.0):**
//! - Always takes up space
//! - Always laid out
//! - Paint executed (compositing overhead)
//!
//! **Conditional widget (if/else):**
//! - Child completely removed
//! - State lost
//! - Rebuild required when showing
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderVisibility;
//!
//! // Fade animation placeholder (reserve space, no visual)
//! let placeholder = RenderVisibility::new(false, true, true, false, false, false);
//!
//! // Hidden tab (preserve state, no space)
//! let hidden_tab = RenderVisibility::new(false, false, true, false, false, false);
//!
//! // Fully visible
//! let visible = RenderVisibility::new(true, false, false, false, false, false);
//!
//! // Completely removed
//! let removed = RenderVisibility::new(false, false, false, false, false, false);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that provides fine-grained control over child visibility.
///
/// More advanced than RenderOffstage with separate flags for controlling size,
/// state, animations, interactivity, and semantics when child is hidden.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Fade animations**: Hide with `maintain_size=true` for smooth transitions
/// - **Loading placeholders**: Reserve space while content loads
/// - **Tab views**: Hide tabs with `maintain_state=true` to preserve state
/// - **Complex state**: Keep widget alive but invisible
/// - **Accessibility control**: Fine-tune screen reader visibility
/// - **Animation continuity**: Keep animations running when invisible
///
/// # Flutter Compliance
///
/// Matches Flutter's Visibility widget behavior:
/// - 6 independent control flags (visible, maintain_size, maintain_state, maintain_animation, maintain_interactivity, maintain_semantics)
/// - Child layout depends on visibility and maintain flags
/// - Size reported based on visibility and maintain_size
/// - Paint skipped when not visible
/// - State preservation controlled by maintain_state
///
/// # Flag Combinations
///
/// - `visible=true`: Full visibility and interaction
/// - `visible=false, maintain_size=true`: Hidden but space reserved (for fade animations)
/// - `visible=false, maintain_state=true`: Hidden, no space, state preserved (for tab views)
/// - `visible=false, all maintain=false`: Completely removed (like conditional rendering)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderVisibility;
///
/// // Placeholder for fade animation
/// let fade_placeholder = RenderVisibility::new(false, true, true, false, false, false);
///
/// // Hidden tab with preserved state
/// let hidden_tab = RenderVisibility::new(false, false, true, false, false, false);
/// ```
#[derive(Debug)]
pub struct RenderVisibility {
    /// Whether the child is visible
    pub visible: bool,

    /// Whether to maintain the space occupied by the child when not visible
    pub maintain_size: bool,

    /// Whether to maintain the state of the child when not visible
    pub maintain_state: bool,

    /// Whether to maintain animations when not visible
    pub maintain_animation: bool,

    /// Whether to maintain interactivity when not visible
    pub maintain_interactivity: bool,

    /// Whether to maintain semantics when not visible
    pub maintain_semantics: bool,
}

impl RenderVisibility {
    /// Create new RenderVisibility
    pub fn new(
        visible: bool,
        maintain_size: bool,
        maintain_state: bool,
        maintain_animation: bool,
        maintain_interactivity: bool,
        maintain_semantics: bool,
    ) -> Self {
        Self {
            visible,
            maintain_size,
            maintain_state,
            maintain_animation,
            maintain_interactivity,
            maintain_semantics,
        }
    }

    /// Set whether child is visible
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Set whether to maintain size
    pub fn set_maintain_size(&mut self, maintain_size: bool) {
        self.maintain_size = maintain_size;
    }

    /// Set whether to maintain state
    pub fn set_maintain_state(&mut self, maintain_state: bool) {
        self.maintain_state = maintain_state;
    }
}

impl Default for RenderVisibility {
    fn default() -> Self {
        Self::new(true, false, false, false, false, false)
    }
}

impl RenderObject for RenderVisibility {}

impl RenderBox<Single> for RenderVisibility {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Determine if child should be laid out
        // Layout child if:
        // - visible = true (always layout when visible)
        // - maintain_state = true (preserve widget state, animations, controllers)
        // - maintain_size = true (reserve space for child)
        let should_layout = self.visible || self.maintain_state || self.maintain_size;

        if should_layout {
            // Layout child with same constraints
            let child_size = ctx.layout_child(child_id, ctx.constraints, true)?;

            // Determine size to report
            // Return child size if visible OR maintaining size (reserve space)
            if self.visible || self.maintain_size {
                if child_size != Size::ZERO {
                    Ok(child_size)
                } else {
                    // Child has zero size, use smallest constraint
                    Ok(ctx.constraints.smallest())
                }
            } else {
                // Not visible, not maintaining size: report zero (no space taken)
                Ok(Size::ZERO)
            }
        } else {
            // Child completely removed: no layout, no space taken
            Ok(Size::ZERO)
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Only paint child when visible
        if self.visible {
            // Single arity: use ctx.single_child() which returns ElementId directly
            let child_id = ctx.single_child();
            ctx.paint_child(child_id, ctx.offset);
        }
        // When visible = false, skip painting entirely (no visual output)
    }

    // TODO: In a full implementation, also override:
    // - hit_test() - to control interactivity based on maintain_interactivity flag
    //   When maintain_interactivity=false and visible=false, skip hit testing
    // - visit_children_for_semantics() - to control accessibility based on maintain_semantics flag
    //   When maintain_semantics=false and visible=false, exclude from semantics tree
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_visibility_new() {
        let visibility = RenderVisibility::new(true, false, false, false, false, false);
        assert!(visibility.visible);
        assert!(!visibility.maintain_size);
        assert!(!visibility.maintain_state);
    }

    #[test]
    fn test_render_visibility_default() {
        let visibility = RenderVisibility::default();
        assert!(visibility.visible);
        assert!(!visibility.maintain_size);
    }

    #[test]
    fn test_render_visibility_set_visible() {
        let mut visibility = RenderVisibility::default();
        visibility.set_visible(false);
        assert!(!visibility.visible);
    }

    #[test]
    fn test_render_visibility_maintain_size() {
        let visibility = RenderVisibility::new(false, true, true, false, false, false);
        assert!(!visibility.visible);
        assert!(visibility.maintain_size);
        assert!(visibility.maintain_state);
    }
}
