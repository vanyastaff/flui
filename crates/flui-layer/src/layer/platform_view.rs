//! `PlatformViewLayer` — a native view (Android `View`, iOS `UIView`) the embedder
//! composites at a rectangle.

use flui_types::geometry::{Pixels, Rect};

/// Unique identifier for a platform view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformViewId(i64);

impl PlatformViewId {
    /// Wraps the platform's integer view id.
    #[inline]
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// The platform's own integer id.
    #[inline]
    pub const fn value(&self) -> i64 {
        self.0
    }
}

/// Hit test behavior for platform views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlatformViewHitTestBehavior {
    /// The platform view is opaque to hit testing - all hits are consumed.
    #[default]
    Opaque,

    /// The platform view is transparent to hit testing in FLUI,
    /// but the native view still receives touch events.
    Transparent,

    /// Hit testing is deferred to the native platform view.
    Defer,
}

/// Layer that embeds a native platform view.
///
/// Platform views allow embedding native UI components within FLUI:
/// - Android Views (maps, web views, video players)
/// - iOS UIViews
/// - Windows HWND
/// - macOS NSView
///
/// # Architecture
///
/// ```text
/// FLUI Layer Tree
///   │
///   ├── Other layers (rendered by GPU)
///   │
///   └── PlatformViewLayer
///         │
///         │ Composites native view at rect
///         ▼
///       Native View (rendered by platform)
/// ```
///
/// # Compositing Modes
///
/// Platform views can be composited in different ways:
/// - **Texture**: Native view renders to a texture, composited in GPU
/// - **Hybrid**: Mix of texture and platform-specific compositing
/// - **Virtual Display**: Native view on a virtual display (Android)
///
/// # Example
///
/// ```rust
/// use flui_layer::{PlatformViewHitTestBehavior, PlatformViewId, PlatformViewLayer};
/// use flui_types::geometry::{Rect, px};
///
/// // Embed a map view
/// let map_view = PlatformViewLayer::new(
///     PlatformViewId::new(1),
///     Rect::from_xywh(px(0.0), px(0.0), px(400.0), px(300.0)),
/// );
///
/// // Embed a web view with custom hit testing
/// let web_view = PlatformViewLayer::new(
///     PlatformViewId::new(2),
///     Rect::from_xywh(px(0.0), px(0.0), px(800.0), px(600.0)),
/// )
/// .with_hit_test_behavior(PlatformViewHitTestBehavior::Defer);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlatformViewLayer {
    /// Unique identifier for the platform view
    view_id: PlatformViewId,

    /// Rectangle where the platform view is displayed
    rect: Rect<Pixels>,

    /// Hit test behavior
    hit_test_behavior: PlatformViewHitTestBehavior,
}

impl PlatformViewLayer {
    /// Composites the native view `view_id` at `rect`.
    #[inline]
    pub fn new(view_id: PlatformViewId, rect: Rect<Pixels>) -> Self {
        Self {
            view_id,
            rect,
            hit_test_behavior: PlatformViewHitTestBehavior::Opaque,
        }
    }

    /// How pointer events over the view are routed.
    #[inline]
    #[must_use]
    pub fn with_hit_test_behavior(mut self, behavior: PlatformViewHitTestBehavior) -> Self {
        self.hit_test_behavior = behavior;
        self
    }

    /// The platform view to composite.
    #[inline]
    pub fn view_id(&self) -> PlatformViewId {
        self.view_id
    }

    /// The rectangle the view occupies.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.rect
    }

    /// See [`Self::with_hit_test_behavior`].
    #[inline]
    pub fn hit_test_behavior(&self) -> PlatformViewHitTestBehavior {
        self.hit_test_behavior
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_platform_view_id() {
        let id = PlatformViewId::new(42);
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn test_platform_view_layer_new() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = PlatformViewLayer::new(id, rect);

        assert_eq!(layer.view_id(), id);
        assert_eq!(layer.bounds(), rect);
        assert_eq!(
            layer.hit_test_behavior(),
            PlatformViewHitTestBehavior::Opaque
        );
    }

    #[test]
    fn test_platform_view_layer_with_hit_test() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = PlatformViewLayer::new(id, rect)
            .with_hit_test_behavior(PlatformViewHitTestBehavior::Defer);

        assert_eq!(
            layer.hit_test_behavior(),
            PlatformViewHitTestBehavior::Defer
        );
    }

    #[test]
    fn test_platform_view_layer_bounds() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = PlatformViewLayer::new(id, rect);

        assert_eq!(layer.bounds(), rect);
    }
}
