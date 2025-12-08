//! PlatformViewLayer - Native view embedding
//!
//! This layer embeds a native platform view (Android View, iOS UIView, etc.)
//! into the FLUI layer tree.

use flui_types::geometry::Rect;

/// Unique identifier for a platform view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformViewId(i64);

impl PlatformViewId {
    /// Creates a new platform view ID.
    #[inline]
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
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
/// use flui_layer::{PlatformViewLayer, PlatformViewId, PlatformViewHitTestBehavior};
/// use flui_types::geometry::Rect;
///
/// // Embed a map view
/// let map_view = PlatformViewLayer::new(
///     PlatformViewId::new(1),
///     Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
/// );
///
/// // Embed a web view with custom hit testing
/// let web_view = PlatformViewLayer::new(
///     PlatformViewId::new(2),
///     Rect::from_xywh(0.0, 0.0, 800.0, 600.0),
/// ).with_hit_test_behavior(PlatformViewHitTestBehavior::Defer);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlatformViewLayer {
    /// Unique identifier for the platform view
    view_id: PlatformViewId,

    /// Rectangle where the platform view is displayed
    rect: Rect,

    /// Hit test behavior
    hit_test_behavior: PlatformViewHitTestBehavior,
}

impl PlatformViewLayer {
    /// Creates a new platform view layer.
    #[inline]
    pub fn new(view_id: PlatformViewId, rect: Rect) -> Self {
        Self {
            view_id,
            rect,
            hit_test_behavior: PlatformViewHitTestBehavior::Opaque,
        }
    }

    /// Sets the hit test behavior.
    #[inline]
    pub fn with_hit_test_behavior(mut self, behavior: PlatformViewHitTestBehavior) -> Self {
        self.hit_test_behavior = behavior;
        self
    }

    /// Returns the platform view ID.
    #[inline]
    pub fn view_id(&self) -> PlatformViewId {
        self.view_id
    }

    /// Returns the display rectangle.
    #[inline]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Returns the bounds (same as rect).
    #[inline]
    pub fn bounds(&self) -> Rect {
        self.rect
    }

    /// Returns the hit test behavior.
    #[inline]
    pub fn hit_test_behavior(&self) -> PlatformViewHitTestBehavior {
        self.hit_test_behavior
    }

    /// Sets the platform view ID.
    #[inline]
    pub fn set_view_id(&mut self, view_id: PlatformViewId) {
        self.view_id = view_id;
    }

    /// Sets the display rectangle.
    #[inline]
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Sets the hit test behavior.
    #[inline]
    pub fn set_hit_test_behavior(&mut self, behavior: PlatformViewHitTestBehavior) {
        self.hit_test_behavior = behavior;
    }

    /// Returns true if the platform view consumes hit tests.
    #[inline]
    pub fn is_hit_test_opaque(&self) -> bool {
        self.hit_test_behavior == PlatformViewHitTestBehavior::Opaque
    }

    /// Returns true if the platform view should be skipped in hit testing.
    #[inline]
    pub fn is_hit_test_transparent(&self) -> bool {
        self.hit_test_behavior == PlatformViewHitTestBehavior::Transparent
    }
}

// Thread safety
unsafe impl Send for PlatformViewLayer {}
unsafe impl Sync for PlatformViewLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_view_id() {
        let id = PlatformViewId::new(42);
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn test_platform_view_layer_new() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let layer = PlatformViewLayer::new(id, rect);

        assert_eq!(layer.view_id(), id);
        assert_eq!(layer.rect(), rect);
        assert_eq!(
            layer.hit_test_behavior(),
            PlatformViewHitTestBehavior::Opaque
        );
    }

    #[test]
    fn test_platform_view_layer_with_hit_test() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
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
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let layer = PlatformViewLayer::new(id, rect);

        assert_eq!(layer.bounds(), rect);
    }

    #[test]
    fn test_platform_view_layer_setters() {
        let mut layer = PlatformViewLayer::new(
            PlatformViewId::new(1),
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        );

        layer.set_view_id(PlatformViewId::new(99));
        layer.set_rect(Rect::from_xywh(5.0, 5.0, 50.0, 50.0));
        layer.set_hit_test_behavior(PlatformViewHitTestBehavior::Transparent);

        assert_eq!(layer.view_id().value(), 99);
        assert_eq!(layer.rect().left(), 5.0);
        assert!(layer.is_hit_test_transparent());
    }

    #[test]
    fn test_platform_view_hit_test_queries() {
        let id = PlatformViewId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let opaque = PlatformViewLayer::new(id, rect)
            .with_hit_test_behavior(PlatformViewHitTestBehavior::Opaque);
        assert!(opaque.is_hit_test_opaque());
        assert!(!opaque.is_hit_test_transparent());

        let transparent = PlatformViewLayer::new(id, rect)
            .with_hit_test_behavior(PlatformViewHitTestBehavior::Transparent);
        assert!(!transparent.is_hit_test_opaque());
        assert!(transparent.is_hit_test_transparent());

        let defer = PlatformViewLayer::new(id, rect)
            .with_hit_test_behavior(PlatformViewHitTestBehavior::Defer);
        assert!(!defer.is_hit_test_opaque());
        assert!(!defer.is_hit_test_transparent());
    }

    #[test]
    fn test_platform_view_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<PlatformViewLayer>();
        assert_sync::<PlatformViewLayer>();
        assert_send::<PlatformViewId>();
        assert_sync::<PlatformViewId>();
    }
}
