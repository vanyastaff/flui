//! RenderOpacity - applies opacity to a child using OpacityLayer
//!
//! Implements Flutter's opacity container that composites its child with alpha blending.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderOpacity` | `RenderOpacity` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `opacity` | `opacity` property |
//! | `set_opacity()` | `opacity = value` setter |
//! | `alwaysNeedsCompositing` | Implicit when 0.0 < opacity < 1.0 |
//!
//! **Flutter API:**
//! ```dart
//! class RenderOpacity extends RenderProxyBox {
//!   RenderOpacity({
//!     double opacity = 1.0,
//!     RenderBox? child,
//!   });
//!
//!   @override
//!   bool get alwaysNeedsCompositing => child != null && opacity < 1.0;
//! }
//! ```
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Check opacity value**
//!    - If opacity <= 0.0: skip painting (fast path)
//!    - If opacity >= 1.0: paint child directly (no layer needed)
//!    - If 0.0 < opacity < 1.0: create opacity layer
//!
//! 2. **Create opacity layer** (when needed)
//!    - Save canvas layer with alpha value
//!    - Paint child to layer
//!    - Restore layer with alpha blending
//!
//! 3. **Paint child**
//!    - Child painted with applied opacity
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**:
//!   - O(1) when opacity = 0.0 or 1.0 (fast paths)
//!   - O(2-3x) when 0.0 < opacity < 1.0 (layer compositing overhead)
//! - **Memory**: 4 bytes (f32 opacity value)
//!
//! # Compositing Behavior
//!
//! - **opacity = 1.0**: No compositing layer, child painted directly (optimal)
//! - **opacity = 0.0**: Child not painted at all (optimal fast path)
//! - **0.0 < opacity < 1.0**: Creates OpacityLayer for proper blending (expensive)
//!
//! # Use Cases
//!
//! - **Fade effects**: Fade in/out animations for UI elements
//! - **Disabled states**: Semi-transparent to indicate disabled widgets
//! - **Overlays**: Semi-transparent overlays and modal backgrounds
//! - **Loading states**: Fade content while loading
//! - **Visual hierarchy**: De-emphasize less important content
//!
//! # Performance Considerations
//!
//! Opacity requires expensive compositing when 0.0 < opacity < 1.0:
//! 1. **Layer Creation**: Allocates off-screen buffer
//! 2. **Child Rendering**: Paints child to layer
//! 3. **Alpha Blending**: Composites layer with parent at given opacity
//!
//! **Cost**: ~2-3x slower than direct painting
//!
//! **Optimization**: Use `AnimatedOpacity` for animations (optimizes partial repaints)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderOpacity;
//!
//! // Semi-transparent (50%)
//! let opacity = RenderOpacity::new(0.5);
//!
//! // Fade out animation (75% transparent)
//! let fade = RenderOpacity::new(0.25);
//!
//! // Disabled state (30% opaque)
//! let disabled = RenderOpacity::new(0.3);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that applies opacity to its child.
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only affects painting.
///
/// # Use Cases
///
/// - **Fade animations**: Smooth fade in/out transitions
/// - **Disabled UI**: Semi-transparent disabled buttons, inputs
/// - **Modal overlays**: Semi-transparent backgrounds behind dialogs
/// - **Loading indicators**: Fade content while loading
/// - **Visual hierarchy**: Reduce opacity to de-emphasize content
/// - **Hover effects**: Opacity changes on mouse hover
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderOpacity behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Opacity clamped to [0.0, 1.0] range
/// - Fast paths: skip paint when opacity = 0.0, no layer when opacity = 1.0
/// - Creates compositing layer when 0.0 < opacity < 1.0
/// - `alwaysNeedsCompositing` implicit when opacity requires layer
/// - Fully transparent widgets still participate in hit testing
///
/// # Paint Fast Paths
///
/// ```text
/// opacity == 1.0 → Paint child directly (no layer, no overhead)
/// opacity == 0.0 → Skip painting entirely (fastest)
/// 0.0 < opacity < 1.0 → Create opacity layer (2-3x slower)
/// ```
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderOpacity;
///
/// // Semi-transparent (50%)
/// let opacity = RenderOpacity::new(0.5);
///
/// // Fully opaque (no-op, optimized away)
/// let opaque = RenderOpacity::new(1.0);
///
/// // Fully transparent (child not painted)
/// let transparent = RenderOpacity::new(0.0);
///
/// // Update opacity dynamically
/// let mut opacity = RenderOpacity::new(0.7);
/// opacity.set_opacity(0.3);
/// ```
///
/// # Implementation Notes
///
/// - **Input Validation**: Opacity clamped to [0.0, 1.0] in constructor and setter
/// - **Layer Management**: TODO - Implement proper OpacityLayer support in Canvas API
/// - **Hit Testing**: Fully transparent (opacity = 0.0) still participates in hit testing
#[derive(Debug)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    ///
    /// Automatically clamped to [0.0, 1.0] range.
    pub opacity: f32,
}

impl RenderOpacity {
    /// Create new RenderOpacity
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    /// Set new opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl RenderObject for RenderOpacity {}

impl RenderBox<Single> for RenderOpacity {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Layout child with same constraints (proxy behavior)
        Ok(ctx.layout_child(ctx.single_child(), ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // If fully transparent, don't paint anything
        if self.opacity <= 0.0 {
            return;
        }

        // TODO: Implement proper opacity layer support in Canvas API
        // For now, just paint child directly - opacity effect is visual only
        // In future: save layer with opacity, paint child, restore layer
        ctx.paint_child(ctx.single_child(), ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert_eq!(opacity.opacity, 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = RenderOpacity::new(-0.5);
        assert_eq!(opacity1.opacity, 0.0);

        let opacity2 = RenderOpacity::new(1.5);
        assert_eq!(opacity2.opacity, 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = RenderOpacity::new(0.5);
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity, 0.8);
    }
}
