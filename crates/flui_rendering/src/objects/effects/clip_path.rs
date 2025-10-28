//! RenderClipPath - clips child to an arbitrary path

use flui_types::{Size, painting::Clip};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::BoxedLayer;

/// Path clipper trait
///
/// Implement this trait to define custom clip paths.
/// The path should be relative to the widget's bounds.
pub trait PathClipper: std::fmt::Debug + Send + Sync {
    /// Get the clip path for the given size
    fn get_clip(&self, size: Size) -> flui_types::painting::path::Path;
}

/// RenderObject that clips its child to an arbitrary path
///
/// Unlike RenderClipRect/RenderClipOval which clip to simple shapes,
/// RenderClipPath can clip to any arbitrary path defined by a PathClipper.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipPath;
/// use flui_types::painting::Clip;
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
/// let clip_path = RenderClipPath::new(Clip::AntiAlias, Box::new(MyClipper));
/// ```
#[derive(Debug)]
pub struct RenderClipPath {
    /// The clipping behavior (None, HardEdge, AntiAlias, etc.)
    pub clip_behavior: Clip,
    /// Custom clipper (optional)
    pub clipper: Option<Box<dyn PathClipper>>,
}

impl RenderClipPath {
    /// Create new RenderClipPath with specified clip behavior and clipper
    pub fn new(clip_behavior: Clip, clipper: Box<dyn PathClipper>) -> Self {
        Self {
            clip_behavior,
            clipper: Some(clipper),
        }
    }

    /// Create with anti-aliased clipping and a custom clipper
    pub fn with_clipper(clipper: Box<dyn PathClipper>) -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
            clipper: Some(clipper),
        }
    }

    /// Create with anti-aliased clipping (no clipper set)
    pub fn anti_alias() -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
            clipper: None,
        }
    }

    /// Set new clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }

    /// Set clipper
    pub fn set_clipper(&mut self, clipper: Box<dyn PathClipper>) {
        self.clipper = Some(clipper);
    }

    /// Remove clipper
    pub fn clear_clipper(&mut self) {
        self.clipper = None;
    }
}

impl Default for RenderClipPath {
    fn default() -> Self {
        Self::anti_alias()
    }
}

impl RenderObject for RenderClipPath {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // If no clipping needed, just return child layer
        if !self.clip_behavior.clips() {
            let child = cx.child();
            return cx.capture_child_layer(child);
        }

        // Get child layer
        let child = cx.child();
        

        // TODO: Implement ClipPathLayer when path clipping is supported
        // For now, just return the child layer without clipping
        // In a real implementation, we would:
        // 1. Get the clip path from the clipper
        // 2. Create a ClipPathLayer with the path
        // 3. Add the child layer to it
        // 4. Return the ClipPathLayer
        //
        // Alternative approaches:
        // - Render to offscreen buffer and mask it
        // - Use backend-specific path clipping
        // - Convert path to polygon and use stencil buffer

        (cx.capture_child_layer(child)) as _
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::painting::path::Path;

    #[derive(Debug)]
    struct TestClipper;

    impl PathClipper for TestClipper {
        fn get_clip(&self, _size: Size) -> Path {
            Path::new()
        }
    }

    #[test]
    fn test_render_clip_path_new() {
        let clip = RenderClipPath::new(Clip::AntiAlias, Box::new(TestClipper));
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
        assert!(clip.clipper.is_some());
    }

    #[test]
    fn test_render_clip_path_with_clipper() {
        let clip = RenderClipPath::with_clipper(Box::new(TestClipper));
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
        assert!(clip.clipper.is_some());
    }

    #[test]
    fn test_render_clip_path_anti_alias() {
        let clip = RenderClipPath::anti_alias();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
        assert!(clip.clipper.is_none());
    }

    #[test]
    fn test_render_clip_path_default() {
        let clip = RenderClipPath::default();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
        assert!(clip.clipper.is_none());
    }

    #[test]
    fn test_render_clip_path_set_clip_behavior() {
        let mut clip = RenderClipPath::anti_alias();
        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_path_set_clipper() {
        let mut clip = RenderClipPath::anti_alias();
        assert!(clip.clipper.is_none());

        clip.set_clipper(Box::new(TestClipper));
        assert!(clip.clipper.is_some());
    }

    #[test]
    fn test_render_clip_path_clear_clipper() {
        let mut clip = RenderClipPath::with_clipper(Box::new(TestClipper));
        assert!(clip.clipper.is_some());

        clip.clear_clipper();
        assert!(clip.clipper.is_none());
    }
}
