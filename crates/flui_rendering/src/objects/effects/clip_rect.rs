//! RenderClipRect - clips child to a rectangle
//!
//! Implements Flutter's rectangular clipping container that clips child
//! to its bounding box.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderClipRect` | `RenderClipRect` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `clip_behavior` | `clipBehavior` property |
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
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(1) - canvas clip operation + child paint
//! - **Hit Test**: O(1) - bounds check + child hit test
//! - **Memory**: 12 bytes (RectShape + Clip + cached Size)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderClipRect;
//! use flui_types::painting::Clip;
//!
//! // Hard edge clipping (default, faster)
//! let clip = RenderClipRect::hard_edge();
//!
//! // Anti-aliased clipping (slower but smoother)
//! let clip = RenderClipRect::anti_alias();
//!
//! // Custom clip behavior
//! let clip = RenderClipRect::with_clip(Clip::AntiAlias);
//! ```

use flui_painting::Canvas;
use flui_types::{painting::Clip, Rect, Size};

use super::clip_base::{ClipShape, RenderClip};

/// Shape implementation for rectangular clipping
#[derive(Debug, Clone, Copy)]
pub struct RectShape;

impl ClipShape for RectShape {
    fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
        let clip_rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
        canvas.clip_rect(clip_rect);
    }
}

/// RenderObject that clips its child to a rectangle.
///
/// Clips child content to rectangular bounds using Canvas clip API.
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
/// # Flutter Compliance
///
/// Matches Flutter's RenderClipRect behavior:
/// - Passes constraints unchanged to child
/// - Clips child during paint to rectangular bounds
/// - Blocks hit testing outside clip region
pub type RenderClipRect = RenderClip<RectShape>;

impl RenderClipRect {
    /// Create with specified clip behavior
    pub fn with_clip(clip_behavior: Clip) -> Self {
        RenderClip::new(RectShape, clip_behavior)
    }

    /// Create with hard edge clipping (default)
    pub fn hard_edge() -> Self {
        Self::with_clip(Clip::HardEdge)
    }

    /// Create with anti-aliased clipping
    pub fn anti_alias() -> Self {
        Self::with_clip(Clip::AntiAlias)
    }
}

impl Default for RenderClipRect {
    fn default() -> Self {
        Self::hard_edge()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rect_with_clip() {
        let clip = RenderClipRect::with_clip(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rect_default() {
        let clip = RenderClipRect::default();
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_rect_hard_edge() {
        let clip = RenderClipRect::hard_edge();
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_rect_anti_alias() {
        let clip = RenderClipRect::anti_alias();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rect_set_clip_behavior() {
        let mut clip = RenderClipRect::hard_edge();
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }
}
