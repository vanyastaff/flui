//! RenderClipPath - clips child to an arbitrary path
//!
//! Implements Flutter's custom path clipping using the PathClipper trait
//! for complex, non-standard shapes.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderClipPath` | `RenderClipPath` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `PathClipper` trait | `CustomClipper<Path>` abstract class |
//! | `clip_behavior` | `clipBehavior` property |
//! | `get_clip()` | `getClip()` method |
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
//! 1. **Get custom clip path**
//!    - Call `PathClipper::get_clip(size)` to generate path
//!    - Path is size-dependent and can be any shape
//!
//! 2. **Apply path clip**
//!    - Apply clip using Canvas::clip_path()
//!    - Supports complex shapes (stars, polygons, bezier curves)
//!
//! 3. **Paint child**
//!    - Child content clipped to custom path
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(n) - path complexity + clip operation + child paint
//! - **Hit Test**: O(1) - uses default rectangular hit testing (override for custom)
//! - **Memory**: Variable - depends on PathClipper implementation
//!
//! # Use Cases
//!
//! - **Custom shapes**: Stars, hexagons, triangles, irregular polygons
//! - **Complex masks**: Wave patterns, cutouts, decorative borders
//! - **Design effects**: Unique clip shapes matching brand identity
//! - **Animations**: Dynamic clipping paths that change over time
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderClipPath, PathClipper};
//! use flui_types::painting::{Clip, Path};
//! use flui_types::Size;
//!
//! // Create a custom star clipper
//! #[derive(Debug)]
//! struct StarClipper;
//!
//! impl PathClipper for StarClipper {
//!     fn get_clip(&self, size: Size) -> Path {
//!         let mut path = Path::new();
//!         // Draw star shape using path commands
//!         // ... (star drawing logic)
//!         path
//!     }
//! }
//!
//! // Use the clipper
//! let clip = RenderClipPath::with_clipper(Box::new(StarClipper));
//! ```

use flui_painting::Canvas;
use flui_types::{
    painting::{path::Path, Clip},
    Size,
};

use super::clip_base::{ClipShape, RenderClip};

/// Path clipper trait
///
/// Implement this trait to define custom clip paths.
/// The path should be relative to the widget's bounds.
pub trait PathClipper: std::fmt::Debug + Send + Sync {
    /// Get the clip path for the given size
    fn get_clip(&self, size: Size) -> Path;
}

/// Shape implementation for path clipping
#[derive(Debug)]
pub struct PathShape {
    /// Custom clipper
    clipper: Option<Box<dyn PathClipper>>,
}

impl PathShape {
    /// Create new PathShape with a clipper
    pub fn new(clipper: Box<dyn PathClipper>) -> Self {
        Self {
            clipper: Some(clipper),
        }
    }

    /// Create without a clipper
    pub fn empty() -> Self {
        Self { clipper: None }
    }

    /// Set clipper
    pub fn set_clipper(&mut self, clipper: Box<dyn PathClipper>) {
        self.clipper = Some(clipper);
    }

    /// Remove clipper
    pub fn clear_clipper(&mut self) {
        self.clipper = None;
    }

    /// Check if clipper is set
    pub fn has_clipper(&self) -> bool {
        self.clipper.is_some()
    }
}

impl ClipShape for PathShape {
    fn apply_clip(&self, canvas: &mut Canvas, size: Size) {
        // Apply clip path from clipper if available
        if let Some(clipper) = &self.clipper {
            let clip_path = clipper.get_clip(size);
            canvas.clip_path(&clip_path);
        }
        // If no clipper set, no clipping is applied
    }
}

/// RenderObject that clips its child to an arbitrary path.
///
/// Unlike RenderClipRect/RenderClipOval which clip to simple shapes,
/// RenderClipPath can clip to any arbitrary path defined by a PathClipper.
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
/// - **Custom shapes**: Stars, hearts, badges with custom outlines
/// - **Complex UI**: Non-standard shapes for unique designs
/// - **Animated clipping**: Dynamic paths that change frame-by-frame
/// - **Cutouts**: Negative space effects, punch-hole designs
/// - **Brand identity**: Custom clip shapes matching logo/branding
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderClipPath behavior:
/// - Passes constraints unchanged to child
/// - Clips child during paint to custom path
/// - Uses PathClipper trait (Flutter: CustomClipper<Path>)
/// - Path is regenerated per paint if size changes
/// - Supports both hard-edge and anti-aliased clipping
/// - Uses Canvas::clip_path() API
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderClipPath, PathClipper};
/// use flui_types::painting::{Clip, Path};
/// use flui_types::Size;
///
/// // Create a custom clipper
/// #[derive(Debug)]
/// struct MyClipper;
///
/// impl PathClipper for MyClipper {
///     fn get_clip(&self, size: Size) -> Path {
///         // Define custom path
///         Path::new()
///     }
/// }
///
/// let clip_path = RenderClipPath::with_clipper(Box::new(MyClipper));
/// ```
pub type RenderClipPath = RenderClip<PathShape>;

impl RenderClipPath {
    /// Create with specified clip behavior and clipper
    pub fn with_clip_and_clipper(clip_behavior: Clip, clipper: Box<dyn PathClipper>) -> Self {
        RenderClip::new(PathShape::new(clipper), clip_behavior)
    }

    /// Create with anti-aliased clipping and a custom clipper
    pub fn with_clipper(clipper: Box<dyn PathClipper>) -> Self {
        Self::with_clip_and_clipper(Clip::AntiAlias, clipper)
    }

    /// Create with anti-aliased clipping (no clipper set)
    pub fn anti_alias() -> Self {
        RenderClip::new(PathShape::empty(), Clip::AntiAlias)
    }

    /// Set clipper
    pub fn set_clipper(&mut self, clipper: Box<dyn PathClipper>) {
        self.shape_mut().set_clipper(clipper);
    }

    /// Remove clipper
    pub fn clear_clipper(&mut self) {
        self.shape_mut().clear_clipper();
    }

    /// Check if clipper is set
    pub fn has_clipper(&self) -> bool {
        self.shape().has_clipper()
    }
}

impl Default for RenderClipPath {
    fn default() -> Self {
        Self::anti_alias()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestClipper;

    impl PathClipper for TestClipper {
        fn get_clip(&self, _size: Size) -> Path {
            Path::new()
        }
    }

    #[test]
    fn test_render_clip_path_with_clip_and_clipper() {
        let clip = RenderClipPath::with_clip_and_clipper(Clip::AntiAlias, Box::new(TestClipper));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(clip.has_clipper());
    }

    #[test]
    fn test_render_clip_path_with_clipper() {
        let clip = RenderClipPath::with_clipper(Box::new(TestClipper));
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(clip.has_clipper());
    }

    #[test]
    fn test_render_clip_path_anti_alias() {
        let clip = RenderClipPath::anti_alias();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(!clip.has_clipper());
    }

    #[test]
    fn test_render_clip_path_default() {
        let clip = RenderClipPath::default();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
        assert!(!clip.has_clipper());
    }

    #[test]
    fn test_render_clip_path_set_clip_behavior() {
        let mut clip = RenderClipPath::anti_alias();
        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_path_set_clipper() {
        let mut clip = RenderClipPath::anti_alias();
        assert!(!clip.has_clipper());

        clip.set_clipper(Box::new(TestClipper));
        assert!(clip.has_clipper());
    }

    #[test]
    fn test_render_clip_path_clear_clipper() {
        let mut clip = RenderClipPath::with_clipper(Box::new(TestClipper));
        assert!(clip.has_clipper());

        clip.clear_clipper();
        assert!(!clip.has_clipper());
    }
}
