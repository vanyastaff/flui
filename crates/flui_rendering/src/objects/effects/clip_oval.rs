//! RenderClipOval - clips child to an oval shape
//!
//! Implements Flutter's oval/circular clipping container that clips child
//! to an ellipse or circle shape.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------||
//! | `RenderClipOval` | `RenderClipOval` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `clip_behavior` | `clipBehavior` property |
//! | `OvalShape` | N/A - internal implementation detail |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Return child size**
//!    - Container size = child size (no size change)
//!    - Cache size for clipping during paint
//!
//! # Paint Protocol
//!
//! 1. **Apply oval clip**
//!    - Create Path with oval shape (fills bounding box)
//!    - Apply clip using Canvas::clip_path()
//!
//! 2. **Paint child**
//!    - Child content clipped to oval
//!    - If bounds are square: creates circle
//!    - If bounds are rectangular: creates ellipse
//!
//! 3. **Hit testing**
//!    - Use ellipse equation to check if point is inside oval
//!    - Blocks hits outside oval region
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(1) - canvas clip operation + child paint
//! - **Hit Test**: O(1) - ellipse equation check
//! - **Memory**: 12 bytes (OvalShape + Clip + cached Size)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderClipOval;
//! use flui_types::painting::Clip;
//!
//! // Anti-aliased clipping (default, smoother edges)
//! let clip = RenderClipOval::anti_alias();
//!
//! // Hard edge clipping (faster, sharper edges)
//! let clip = RenderClipOval::hard_edge();
//!
//! // Custom clip behavior
//! let clip = RenderClipOval::with_clip(Clip::AntiAlias);
//! ```

use flui_painting::Canvas;
use flui_types::{
    painting::{Clip, Path},
    Offset, Rect, Size,
};

use super::clip_base::{ClipShape, RenderClip};

/// Shape implementation for oval clipping
#[derive(Debug, Clone, Copy)]
pub struct OvalShape;

impl ClipShape for OvalShape {
    fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
        // Create oval clip using a path
        let clip_rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
        let mut path = Path::new();
        path.add_oval(clip_rect);
        canvas.clip_path(&path);
    }

    fn contains_point(&self, position: Offset, size: Size) -> bool {
        // Check if point is inside ellipse using the ellipse equation:
        // (x - cx)² / rx² + (y - cy)² / ry² <= 1
        //
        // For an oval filling the bounding box:
        // - Center: (width/2, height/2)
        // - Radius X: width/2
        // - Radius Y: height/2

        let cx = size.width / 2.0;
        let cy = size.height / 2.0;
        let rx = size.width / 2.0;
        let ry = size.height / 2.0;

        // Avoid division by zero for degenerate cases
        if rx < f32::EPSILON || ry < f32::EPSILON {
            return false;
        }

        let dx = position.dx - cx;
        let dy = position.dy - cy;

        // Ellipse equation
        let value = (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry);
        value <= 1.0
    }
}

/// RenderObject that clips its child to an oval shape.
///
/// The oval fills the bounds of this RenderObject.
/// If the bounds are square, this creates a circle.
/// If the bounds are rectangular, this creates an ellipse.
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
/// **Proxy** - Passes constraints unchanged, clips during paint only.
///
/// # Use Cases
///
/// - **Circular avatars**: Clip profile pictures to circles
/// - **Rounded buttons**: Create circular or oval buttons
/// - **Design effects**: Clip images or content to oval shapes
/// - **Masking**: Create oval-shaped masks for content
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderClipOval behavior:
/// - Passes constraints unchanged to child
/// - Clips child during paint to oval/circular bounds
/// - Uses Path-based clipping with Canvas API
/// - Blocks hit testing outside oval region using ellipse equation
/// - Supports both hard-edge and anti-aliased clipping
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipOval;
/// use flui_types::painting::Clip;
///
/// let clip_oval = RenderClipOval::with_clip(Clip::AntiAlias);
/// ```
pub type RenderClipOval = RenderClip<OvalShape>;

impl RenderClipOval {
    /// Create with specified clip behavior
    pub fn with_clip(clip_behavior: Clip) -> Self {
        RenderClip::new(OvalShape, clip_behavior)
    }

    /// Create with hard edge clipping
    pub fn hard_edge() -> Self {
        Self::with_clip(Clip::HardEdge)
    }

    /// Create with anti-aliased clipping (default for ovals)
    pub fn anti_alias() -> Self {
        Self::with_clip(Clip::AntiAlias)
    }
}

impl Default for RenderClipOval {
    fn default() -> Self {
        Self::anti_alias()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_oval_with_clip() {
        let clip = RenderClipOval::with_clip(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_default() {
        let clip = RenderClipOval::default();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_hard_edge() {
        let clip = RenderClipOval::hard_edge();
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_oval_anti_alias() {
        let clip = RenderClipOval::anti_alias();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_set_clip_behavior() {
        let mut clip = RenderClipOval::hard_edge();
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }
}
