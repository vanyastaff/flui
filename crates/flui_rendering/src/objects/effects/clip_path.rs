//! RenderClipPath - clips child to an arbitrary path

use flui_types::{Size, painting::{Clip, path::Path}};
use flui_engine::BoxedLayer;

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
    fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
        use flui_engine::ClipPathLayer;

        // Get clip path from clipper if available
        if let Some(clipper) = &self.clipper {
            let clip_path = clipper.get_clip(size);
            let mut clip_layer = ClipPathLayer::new(clip_path);
            clip_layer.add_child(child_layer);
            Box::new(clip_layer)
        } else {
            // No clipper set - just return child layer without clipping
            child_layer
        }
    }
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
