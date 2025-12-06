//! RenderSliverAnimatedOpacity - Animated opacity transitions for sliver scrollables
//!
//! Applies animated opacity to sliver child, optimized for frequent opacity changes driven by
//! animations. Skips painting when fully transparent (opacity = 0), uses compositing layers for
//! partial transparency, maintains layer during animation to avoid layer creation/destruction
//! overhead. Similar to RenderSliverOpacity but with animation-specific optimizations.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverAnimatedOpacity` | `RenderAnimatedOpacity` adapted for slivers (sliver.dart) |
//! | `opacity` | Animated opacity value (0.0-1.0) |
//! | `animating` | Whether animation is active (for layer optimization) |
//! | `needs_compositing()` | Determines when compositing layer needed |
//! | `should_paint()` | Skip painting optimization when opacity = 0 |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Opacity doesn't affect layout (pure visual effect)
//!    - Child layouts with original constraints
//!
//! 2. **Cache child geometry**
//!    - Return child's SliverGeometry unchanged
//!
//! # Paint Protocol (Intended)
//!
//! 1. **Check opacity** (line 130)
//!    - Skip painting if opacity = 0 (optimization)
//!
//! 2. **Paint child** (line 135-148)
//!    - Get child canvas
//!    - Apply opacity to canvas (NOT IMPLEMENTED - TODO)
//!    - Return canvas
//!
//! 3. **Compositing layer** (line 142-145)
//!    - Use layer when 0 < opacity < 1
//!    - Keep layer during animation (even at opacity = 1)
//!    - Avoids layer creation/destruction overhead
//!
//! # Performance
//!
//! - **Layout**: O(1) + O(child) - pass-through to child
//! - **Paint**: O(child) when visible, O(1) when opacity = 0
//! - **Memory**: 16 bytes (f32 + 2×bool + padding) + 48 bytes (SliverGeometry) = 64 bytes
//! - **Animation**: Triggers repaint only, NOT relayout
//! - **Compositing**: Layer reused during animation for efficiency
//!
//! # Use Cases
//!
//! - **Fade in/out**: Lists/grids appearing/disappearing
//! - **Page transitions**: Opacity-based page navigation
//! - **Loading states**: Fading content during loading
//! - **Interactive feedback**: Dimming on touch
//! - **Animated headers**: AppBar fade based on scroll
//!
//! # Opacity Optimization
//!
//! ```text
//! opacity = 0.0:     [NO PAINT] ← Skip painting (optimization)
//! opacity = 0.5:     [PAINT + LAYER] ← Compositing layer
//! opacity = 1.0:     [PAINT] ← No layer (normal paint)
//! opacity = 1.0 (animating): [PAINT + LAYER] ← Keep layer!
//! ```
//!
//! # ⚠️ IMPLEMENTATION ISSUE
//!
//! This implementation has **ONE INCOMPLETE FEATURE**:
//!
//! 1. **✅ Child IS laid out** (line 115-126)
//!    - Correctly uses layout_sliver_child()
//!    - Child geometry properly cached
//!    - GOOD IMPLEMENTATION!
//!
//! 2. **⚠️ Opacity NOT APPLIED** (line 128-152, TODO at line 140-145)
//!    - Child painted normally without opacity
//!    - TODO comment acknowledges missing opacity layer
//!    - needs_compositing() calculated but not used
//!
//! 3. **⚠️ always_include_semantics NOT USED** (line 42)
//!    - Field exists and can be set
//!    - Never checked in layout/paint
//!    - Dead code - has no effect
//!
//! 4. **✅ should_paint() optimization CORRECT** (line 95-97)
//!    - Skips painting when opacity = 0
//!    - Good performance optimization
//!
//! 5. **✅ needs_compositing() logic CORRECT** (line 103-105)
//!    - Correctly keeps layer during animation
//!    - Avoids layer creation/destruction overhead
//!
//! **This RenderObject is WELL STRUCTURED - only opacity layer missing!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverOpacity**: AnimatedOpacity optimized for animation, Opacity is static
//! - **vs AnimatedOpacity (box)**: SliverAnimatedOpacity for slivers, AnimatedOpacity for boxes
//! - **vs FadeTransition**: AnimatedOpacity is render, FadeTransition is widget
//! - **vs SliverIgnorePointer**: Opacity affects visuals, IgnorePointer affects hit testing
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverAnimatedOpacity;
//!
//! // Fade in animation (0 → 1)
//! let mut opacity = RenderSliverAnimatedOpacity::new(0.0);
//! opacity.set_animating(true);
//! // ... animation loop ...
//! opacity.set_opacity(0.5); // Halfway through fade
//! // WARNING: opacity not applied - child paints normally!
//!
//! // Fade out on scroll
//! let mut opacity = RenderSliverAnimatedOpacity::new(1.0);
//! // ... as user scrolls ...
//! opacity.set_opacity(0.7); // Partially faded
//! opacity.set_opacity(0.0); // Fully transparent (not painted)
//! ```

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::SliverGeometry;

/// RenderObject that applies animated opacity to sliver child.
///
/// Optimized for frequent opacity changes driven by animations. Skips painting when fully
/// transparent (opacity = 0), maintains compositing layer during animation to avoid layer
/// creation/destruction overhead. Child layout is pass-through (opacity is pure visual effect).
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 sliver child.
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Animated Visual Effect Proxy** - Pass-through layout, animated opacity application
/// (intended), compositing layer optimization during animation, skip-paint optimization
/// when opacity = 0.
///
/// # Use Cases
///
/// - **Fade transitions**: Lists/grids appearing/disappearing
/// - **Page navigation**: Opacity-based page transitions
/// - **Loading states**: Fading content during loading
/// - **Interactive feedback**: Dimming on touch/hover
/// - **Scroll effects**: AppBar fade based on scroll position
///
/// # Flutter Compliance
///
/// **WELL STRUCTURED** (opacity layer missing):
/// - ✅ Child layout correct (uses layout_sliver_child)
/// - ✅ should_paint() optimization correct
/// - ✅ needs_compositing() logic correct
/// - ⚠️ Opacity NOT applied (TODO at line 140-145)
/// - ⚠️ always_include_semantics unused (dead code)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverAnimatedOpacity;
///
/// // Fade in from transparent to opaque
/// let mut opacity = RenderSliverAnimatedOpacity::new(0.0);
/// opacity.set_animating(true);
/// opacity.set_opacity(0.5); // Halfway
/// // WARNING: opacity not applied - child paints normally!
/// ```
#[derive(Debug)]
pub struct RenderSliverAnimatedOpacity {
    /// Current opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
    /// Whether to always include the child in the tree even when invisible
    pub always_include_semantics: bool,
    /// Whether the animation is currently running
    pub animating: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverAnimatedOpacity {
    /// Create new animated sliver opacity
    ///
    /// # Arguments
    /// * `opacity` - Initial opacity value between 0.0 and 1.0
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
            animating: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set opacity value
    ///
    /// This method should be called when the animation value changes.
    /// It marks the render object for repaint but not relayout.
    pub fn set_opacity(&mut self, opacity: f32) {
        let new_opacity = opacity.clamp(0.0, 1.0);
        if (self.opacity - new_opacity).abs() > f32::EPSILON {
            self.opacity = new_opacity;
            // In a full implementation, this would call mark_needs_paint()
        }
    }

    /// Set always include semantics
    pub fn set_always_include_semantics(&mut self, always: bool) {
        self.always_include_semantics = always;
    }

    /// Set whether animation is currently running
    ///
    /// This can be used for optimization - the render object may handle
    /// compositing differently when actively animating.
    pub fn set_animating(&mut self, animating: bool) {
        self.animating = animating;
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
    ///
    /// Returns true if opacity is between 0 and 1, or if actively animating
    /// (even at opacity 1.0, to avoid layer creation/destruction during animation).
    pub fn needs_compositing(&self) -> bool {
        (self.opacity > 0.0 && self.opacity < 1.0) || (self.animating && self.opacity > 0.0)
    }
}

impl Default for RenderSliverAnimatedOpacity {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl LegacySliverRender for RenderSliverAnimatedOpacity {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        // Pass constraints through to child
        let child_geometry = if let Some(child_id) = ctx.children.try_single() {
            ctx.tree.layout_sliver_child(child_id, ctx.constraints)
        } else {
            SliverGeometry::default()
        };

        // Cache geometry (opacity doesn't affect layout)
        self.sliver_geometry = child_geometry;
        self.sliver_geometry
    }

    fn paint(&self, ctx: &Sliver) -> Canvas {
        // Don't paint if completely transparent
        if !self.should_paint() {
            return Canvas::new();
        }

        // Paint child if present
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                let child_canvas = ctx.tree.paint_child(child_id, ctx.offset);

                // Apply opacity to canvas
                // TODO: When opacity layer support is available, apply it here
                // For now, we just paint normally (opacity would be applied by compositor)
                if self.needs_compositing() {
                    // Mark that this needs a compositing layer with opacity
                    // In full implementation, this would wrap in an OpacityLayer
                }

                return child_canvas;
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_animated_opacity_new() {
        let opacity = RenderSliverAnimatedOpacity::new(0.7);

        assert_eq!(opacity.opacity, 0.7);
        assert!(!opacity.always_include_semantics);
        assert!(!opacity.animating);
    }

    #[test]
    fn test_render_sliver_animated_opacity_default() {
        let opacity = RenderSliverAnimatedOpacity::default();

        assert_eq!(opacity.opacity, 1.0);
    }

    #[test]
    fn test_set_opacity() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_opacity(0.8);

        assert_eq!(opacity.opacity, 0.8);
    }

    #[test]
    fn test_set_opacity_clamped() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);

        opacity.set_opacity(1.5);
        assert_eq!(opacity.opacity, 1.0);

        opacity.set_opacity(-0.3);
        assert_eq!(opacity.opacity, 0.0);
    }

    #[test]
    fn test_set_always_include_semantics() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_always_include_semantics(true);

        assert!(opacity.always_include_semantics);
    }

    #[test]
    fn test_set_animating() {
        let mut opacity = RenderSliverAnimatedOpacity::new(0.5);
        opacity.set_animating(true);

        assert!(opacity.animating);
    }

    #[test]
    fn test_should_paint_zero_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.0);

        assert!(!opacity.should_paint());
    }

    #[test]
    fn test_should_paint_full_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(1.0);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_should_paint_partial_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);

        assert!(opacity.should_paint());
    }

    #[test]
    fn test_needs_compositing_partial_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);

        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_full_opacity_not_animating() {
        let opacity = RenderSliverAnimatedOpacity::new(1.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_full_opacity_while_animating() {
        let mut opacity = RenderSliverAnimatedOpacity::new(1.0);
        opacity.set_animating(true);

        // Should still composite while animating to avoid layer creation/destruction
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn test_needs_compositing_zero_opacity() {
        let opacity = RenderSliverAnimatedOpacity::new(0.0);

        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_arity_is_single_child() {
        let opacity = RenderSliverAnimatedOpacity::new(0.5);
        assert_eq!(opacity.arity(), RuntimeArity::Exact(1));
    }
}
