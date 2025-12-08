//! RenderSliverOpacity - Applies opacity to sliver content
//!
//! Implements Flutter's SliverOpacity that controls the opacity of sliver children. This
//! allows fading in/out entire sliver sections (lists, grids, headers) without affecting
//! their layout or semantics tree.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverOpacity` | `RenderSliverOpacity` from `package:flutter/src/rendering/sliver.dart` |
//! | `opacity` property | `opacity` property (0.0-1.0) |
//! | `always_include_semantics` | `alwaysIncludeSemantics` property |
//! | `should_paint()` | Optimization check (opacity > 0) |
//! | `needs_compositing()` | `alwaysNeedsCompositing` override |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child (if present)**
//!    - Opacity doesn't affect layout - passes constraints unchanged
//!    - Child receives identical constraints (proxy behavior)
//!
//! 2. **Return child geometry**
//!    - Returns child's geometry unchanged
//!    - If no child: returns default (zero) geometry
//!    - Layout is transparent to opacity value
//!
//! # Paint Protocol
//!
//! 1. **Check if should paint**
//!    - If opacity = 0.0 and !always_include_semantics: skip painting (optimization)
//!    - Otherwise proceed to paint
//!
//! 2. **Check child visibility**
//!    - Only paint if child geometry is visible
//!
//! 3. **Apply opacity and paint child**
//!    - If opacity = 1.0: paint child directly (no layer)
//!    - If 0.0 < opacity < 1.0: paint with compositing layer + opacity
//!    - If opacity = 0.0: skip painting
//!
//! # Performance
//!
//! - **Layout**: O(1) + child layout - pass-through proxy
//! - **Paint**: O(1) + child paint - compositing layer overhead when 0 < opacity < 1
//! - **Memory**: 12 bytes (f32 opacity + bool + SliverGeometry cache)
//! - **Optimization**: Skips painting when opacity = 0.0 (unless semantics required)
//! - **Compositing**: Creates layer only when 0 < opacity < 1 (not for 0 or 1)
//!
//! # Use Cases
//!
//! - **Fade animations**: Animate sliver opacity for smooth transitions
//! - **Loading states**: Fade out old content while loading new
//! - **Scroll effects**: Fade headers on scroll
//! - **Modal overlays**: Dim background slivers behind modals
//! - **Disabled states**: Visually indicate disabled sliver sections
//! - **Transition effects**: Cross-fade between different sliver content
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderOpacity**: SliverOpacity is sliver protocol, Opacity is box protocol
//! - **vs SliverAnimatedOpacity**: AnimatedOpacity includes animation controller
//! - **vs SliverIgnorePointer**: IgnorePointer affects hit testing, Opacity affects visuals
//! - **vs SliverOffstage**: Offstage removes from layout, Opacity keeps layout
//! - **vs SliverVisibility**: Visibility combines multiple effects, Opacity is visual only
//!
//! # Implementation Status
//!
//! **IMPORTANT**: The current implementation has a TODO - opacity is not actually applied!
//! The paint method currently renders the child at full opacity regardless of the opacity
//! value. Full implementation requires `Canvas::save_layer_alpha()` which is not yet
//! available in the painting backend.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverOpacity;
//!
//! // Semi-transparent sliver (50%)
//! let faded = RenderSliverOpacity::new(0.5);
//!
//! // Fully opaque (default, no compositing overhead)
//! let opaque = RenderSliverOpacity::new(1.0);
//!
//! // Fully transparent (optimized - skips painting)
//! let transparent = RenderSliverOpacity::new(0.0);
//!
//! // Dynamic opacity updates
//! let mut opacity = RenderSliverOpacity::new(1.0);
//! opacity.set_opacity(0.3); // Fade to 30%
//!
//! // Keep in semantics tree even when invisible
//! let mut opacity = RenderSliverOpacity::new(0.0);
//! opacity.set_always_include_semantics(true);
//! ```

use flui_rendering::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::SliverGeometry;

/// RenderObject that applies opacity to a sliver child.
///
/// Controls the opacity of an entire sliver subtree without affecting layout or semantics.
/// Optimizes painting by skipping invisible children (opacity = 0) and avoiding compositing
/// layers for fully opaque children (opacity = 1).
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child sliver (optional in implementation).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Visual Effect Proxy** - Passes constraints/geometry unchanged, only affects paint
/// rendering by applying opacity. Layout-transparent visual modifier.
///
/// # Use Cases
///
/// - **Fade transitions**: Smooth opacity animations between states
/// - **Loading overlays**: Fade out content during loading
/// - **Scroll effects**: Parallax fade effects on headers
/// - **Disabled UI**: Dim disabled sliver sections
/// - **Modal backgrounds**: Reduce opacity behind modals
/// - **Visibility transitions**: Gradual show/hide animations
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverOpacity behavior:
/// - Passes constraints unchanged to child (layout-transparent)
/// - Returns child geometry unchanged
/// - Applies opacity during paint phase only
/// - Skips painting when opacity = 0.0 (optimization)
/// - Creates compositing layer when 0 < opacity < 1
/// - Respects `alwaysIncludeSemantics` flag
///
/// # Performance Characteristics
///
/// | Opacity | Paint Behavior | Compositing Layer |
/// |---------|----------------|-------------------|
/// | 0.0 | Skipped (unless semantics required) | No |
/// | 0.0 < x < 1.0 | With alpha blending | Yes |
/// | 1.0 | Direct (no alpha) | No |
///
/// # Implementation Limitation
///
/// **IMPORTANT**: Currently opacity is not applied during painting! The implementation
/// needs `Canvas::save_layer_alpha()` which is pending. Child is painted at full opacity.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverOpacity;
///
/// // Fade sliver to 30% opacity
/// let faded = RenderSliverOpacity::new(0.3);
///
/// // Animate opacity for smooth transitions
/// let mut opacity = RenderSliverOpacity::new(1.0);
/// // In animation loop:
/// opacity.set_opacity(0.5);
/// ```
#[derive(Debug)]
pub struct RenderSliverOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
    /// Whether to always include the child in the tree even when invisible
    pub always_include_semantics: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverOpacity {
    /// Create new sliver opacity
    ///
    /// # Arguments
    /// * `opacity` - Opacity value between 0.0 and 1.0
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Set always include semantics
    pub fn set_always_include_semantics(&mut self, always: bool) {
        self.always_include_semantics = always;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Check if child should be painted
    pub fn should_paint(&self) -> bool {
        self.opacity > 0.0
    }

    /// Check if needs compositing layer
    pub fn needs_compositing(&self) -> bool {
        self.opacity > 0.0 && self.opacity < 1.0
    }
}

impl Default for RenderSliverOpacity {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl RenderObject for RenderSliverOpacity {}

impl RenderSliver<Single> for RenderSliverOpacity {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();

        // Pass through to child
        self.sliver_geometry = ctx.tree_mut().perform_sliver_layout(child_id, ctx.constraints)?;

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // If fully transparent, skip painting (unless semantics required)
        if !self.should_paint() && !self.always_include_semantics {
            return;
        }

        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();

            // TODO: Apply opacity using save_layer_alpha when implemented
            // For now, just paint child directly
            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
                *ctx.canvas = child_canvas;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_opacity_new() {
        let opacity = RenderSliverOpacity::new(0.5);

        assert_eq!(opacity.opacity, 0.5);
        assert!(!opacity.always_include_semantics);
    }

    #[test]
    fn test_render_sliver_opacity_clamps() {
        let opacity_low = RenderSliverOpacity::new(-0.5);
        let opacity_high = RenderSliverOpacity::new(1.5);

        assert_eq!(opacity_low.opacity, 0.0);
        assert_eq!(opacity_high.opacity, 1.0);
    }

    #[test]
    fn test_set_opacity() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_opacity(0.8);

        assert_eq!(opacity.opacity, 0.8);
    }

    #[test]
    fn test_set_opacity_clamps() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_opacity(2.0);

        assert_eq!(opacity.opacity, 1.0);

        opacity.set_opacity(-1.0);
        assert_eq!(opacity.opacity, 0.0);
    }

    #[test]
    fn test_set_always_include_semantics() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        opacity.set_always_include_semantics(true);

        assert!(opacity.always_include_semantics);
    }

    #[test]
    fn test_should_paint() {
        let opacity_visible = RenderSliverOpacity::new(0.5);
        let opacity_invisible = RenderSliverOpacity::new(0.0);

        assert!(opacity_visible.should_paint());
        assert!(!opacity_invisible.should_paint());
    }

    #[test]
    fn test_needs_compositing() {
        let opacity_full = RenderSliverOpacity::new(1.0);
        let opacity_partial = RenderSliverOpacity::new(0.5);
        let opacity_zero = RenderSliverOpacity::new(0.0);

        assert!(!opacity_full.needs_compositing());
        assert!(opacity_partial.needs_compositing());
        assert!(!opacity_zero.needs_compositing());
    }

    #[test]
    fn test_default_is_opaque() {
        let opacity = RenderSliverOpacity::default();

        assert_eq!(opacity.opacity, 1.0);
    }

    #[test]
    fn test_opacity_range() {
        // Test edge cases
        let opacity_min = RenderSliverOpacity::new(0.0);
        let opacity_max = RenderSliverOpacity::new(1.0);
        let opacity_mid = RenderSliverOpacity::new(0.5);

        assert_eq!(opacity_min.opacity, 0.0);
        assert_eq!(opacity_max.opacity, 1.0);
        assert_eq!(opacity_mid.opacity, 0.5);
    }
}
