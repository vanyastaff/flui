//! ViewportOffset - Controls which portion of content is visible in a viewport
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/ViewportOffset-class.html>

use std::sync::Arc;

/// Direction of user scroll intent
///
/// Used to determine the direction the user is trying to scroll,
/// which may differ from the direction content is actually moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollDirection {
    /// Not currently scrolling
    #[default]
    Idle,
    /// Scrolling forward (down/right for normal direction)
    Forward,
    /// Scrolling backward (up/left for normal direction)
    Reverse,
}

impl ScrollDirection {
    /// Returns true if the scroll direction is idle
    pub fn is_idle(&self) -> bool {
        matches!(self, ScrollDirection::Idle)
    }

    /// Returns the opposite direction
    pub fn opposite(&self) -> Self {
        match self {
            ScrollDirection::Idle => ScrollDirection::Idle,
            ScrollDirection::Forward => ScrollDirection::Reverse,
            ScrollDirection::Reverse => ScrollDirection::Forward,
        }
    }
}

/// Callback for when the viewport offset changes
pub type ViewportOffsetCallback = Arc<dyn Fn(f32) + Send + Sync>;

/// Controls which portion of content is visible in a viewport
///
/// ViewportOffset is the scroll position controller for viewports. It determines
/// which part of the scrollable content is visible by providing a `pixels` value
/// that represents the scroll offset.
///
/// # Relationship with ScrollPosition
///
/// In Flutter, `ScrollPosition` is the common concrete implementation of
/// `ViewportOffset`. This trait provides the abstract interface that viewports
/// use to interact with scroll positions.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::ViewportOffset;
///
/// // Create a fixed offset at 100 pixels
/// let offset = ViewportOffset::fixed(100.0);
/// assert_eq!(offset.pixels(), 100.0);
///
/// // Jump to a new position
/// offset.jump_to(200.0);
/// assert_eq!(offset.pixels(), 200.0);
/// ```
pub struct ViewportOffset {
    /// Current scroll offset in pixels
    pixels: f32,

    /// Whether the pixels value has been set
    has_pixels: bool,

    /// Whether viewport can adjust pixels implicitly during layout
    allow_implicit_scrolling: bool,

    /// User's intended scroll direction
    user_scroll_direction: ScrollDirection,

    /// Viewport dimension (main axis extent)
    viewport_dimension: Option<f32>,

    /// Minimum scroll extent
    min_scroll_extent: Option<f32>,

    /// Maximum scroll extent
    max_scroll_extent: Option<f32>,

    /// Callback when offset changes
    on_change: Option<ViewportOffsetCallback>,
}

impl ViewportOffset {
    /// Create a new ViewportOffset with zero offset
    pub fn zero() -> Self {
        Self {
            pixels: 0.0,
            has_pixels: true,
            allow_implicit_scrolling: false,
            user_scroll_direction: ScrollDirection::Idle,
            viewport_dimension: None,
            min_scroll_extent: None,
            max_scroll_extent: None,
            on_change: None,
        }
    }

    /// Create a new ViewportOffset with a fixed value
    pub fn fixed(value: f32) -> Self {
        Self {
            pixels: value,
            has_pixels: true,
            allow_implicit_scrolling: false,
            user_scroll_direction: ScrollDirection::Idle,
            viewport_dimension: None,
            min_scroll_extent: None,
            max_scroll_extent: None,
            on_change: None,
        }
    }

    /// Get the current scroll offset in pixels
    ///
    /// The pixels value determines which portion of content is visible.
    /// A value of 0.0 typically means the start of content is visible.
    pub fn pixels(&self) -> f32 {
        self.pixels
    }

    /// Check if the pixels value has been set
    pub fn has_pixels(&self) -> bool {
        self.has_pixels
    }

    /// Check if implicit scrolling is allowed
    ///
    /// When true, the viewport can adjust the scroll position during layout
    /// without explicit user interaction.
    pub fn allow_implicit_scrolling(&self) -> bool {
        self.allow_implicit_scrolling
    }

    /// Set whether implicit scrolling is allowed
    pub fn set_allow_implicit_scrolling(&mut self, allow: bool) {
        self.allow_implicit_scrolling = allow;
    }

    /// Get the user's intended scroll direction
    pub fn user_scroll_direction(&self) -> ScrollDirection {
        self.user_scroll_direction
    }

    /// Set the user's scroll direction
    pub fn set_user_scroll_direction(&mut self, direction: ScrollDirection) {
        self.user_scroll_direction = direction;
    }

    /// Get the viewport dimension
    pub fn viewport_dimension(&self) -> Option<f32> {
        self.viewport_dimension
    }

    /// Get the minimum scroll extent
    pub fn min_scroll_extent(&self) -> Option<f32> {
        self.min_scroll_extent
    }

    /// Get the maximum scroll extent
    pub fn max_scroll_extent(&self) -> Option<f32> {
        self.max_scroll_extent
    }

    /// Set the change callback
    pub fn set_on_change(&mut self, callback: ViewportOffsetCallback) {
        self.on_change = Some(callback);
    }

    /// Jump immediately to a new scroll position
    ///
    /// This changes the scroll offset without animation.
    pub fn jump_to(&mut self, value: f32) {
        if self.pixels != value {
            self.pixels = value;
            self.has_pixels = true;
            self.notify_listeners();
        }
    }

    /// Apply a correction to the scroll offset during layout
    ///
    /// This is used to correct the scroll position when content dimensions
    /// change during layout (e.g., when items are inserted or removed).
    pub fn correct_by(&mut self, correction: f32) {
        if correction != 0.0 {
            self.pixels += correction;
            // Don't notify listeners - this is a layout-time correction
        }
    }

    /// Apply viewport dimension from layout
    ///
    /// Called by the viewport during layout to inform the offset of
    /// the viewport's size.
    ///
    /// Returns true if the dimension changed.
    pub fn apply_viewport_dimension(&mut self, dimension: f32) -> bool {
        if self.viewport_dimension != Some(dimension) {
            self.viewport_dimension = Some(dimension);
            true
        } else {
            false
        }
    }

    /// Apply content dimensions from layout
    ///
    /// Called by the viewport during layout to inform the offset of
    /// the content's scroll extent range.
    ///
    /// Returns true if the dimensions changed.
    pub fn apply_content_dimensions(&mut self, min: f32, max: f32) -> bool {
        let changed = self.min_scroll_extent != Some(min) || self.max_scroll_extent != Some(max);

        self.min_scroll_extent = Some(min);
        self.max_scroll_extent = Some(max);

        // Clamp pixels to valid range
        if let (Some(min_extent), Some(max_extent)) =
            (self.min_scroll_extent, self.max_scroll_extent)
        {
            let clamped = self.pixels.clamp(min_extent, max_extent);
            if clamped != self.pixels {
                self.pixels = clamped;
            }
        }

        changed
    }

    /// Notify listeners that the offset changed
    fn notify_listeners(&self) {
        if let Some(ref callback) = self.on_change {
            callback(self.pixels);
        }
    }

    /// Check if the offset is at the start of content
    pub fn at_edge_start(&self) -> bool {
        if let Some(min) = self.min_scroll_extent {
            (self.pixels - min).abs() < 0.001
        } else {
            false
        }
    }

    /// Check if the offset is at the end of content
    pub fn at_edge_end(&self) -> bool {
        if let Some(max) = self.max_scroll_extent {
            (self.pixels - max).abs() < 0.001
        } else {
            false
        }
    }

    /// Check if the offset is outside the valid scroll range
    pub fn out_of_range(&self) -> bool {
        if let (Some(min), Some(max)) = (self.min_scroll_extent, self.max_scroll_extent) {
            self.pixels < min || self.pixels > max
        } else {
            false
        }
    }
}

impl Default for ViewportOffset {
    fn default() -> Self {
        Self::zero()
    }
}

impl std::fmt::Debug for ViewportOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportOffset")
            .field("pixels", &self.pixels)
            .field("has_pixels", &self.has_pixels)
            .field("allow_implicit_scrolling", &self.allow_implicit_scrolling)
            .field("user_scroll_direction", &self.user_scroll_direction)
            .field("viewport_dimension", &self.viewport_dimension)
            .field("min_scroll_extent", &self.min_scroll_extent)
            .field("max_scroll_extent", &self.max_scroll_extent)
            .field("on_change", &self.on_change.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

impl Clone for ViewportOffset {
    fn clone(&self) -> Self {
        Self {
            pixels: self.pixels,
            has_pixels: self.has_pixels,
            allow_implicit_scrolling: self.allow_implicit_scrolling,
            user_scroll_direction: self.user_scroll_direction,
            viewport_dimension: self.viewport_dimension,
            min_scroll_extent: self.min_scroll_extent,
            max_scroll_extent: self.max_scroll_extent,
            on_change: self.on_change.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_offset_zero() {
        let offset = ViewportOffset::zero();
        assert_eq!(offset.pixels(), 0.0);
        assert!(offset.has_pixels());
    }

    #[test]
    fn test_viewport_offset_fixed() {
        let offset = ViewportOffset::fixed(100.0);
        assert_eq!(offset.pixels(), 100.0);
        assert!(offset.has_pixels());
    }

    #[test]
    fn test_viewport_offset_default() {
        let offset = ViewportOffset::default();
        assert_eq!(offset.pixels(), 0.0);
    }

    #[test]
    fn test_jump_to() {
        let mut offset = ViewportOffset::zero();
        offset.jump_to(200.0);
        assert_eq!(offset.pixels(), 200.0);
    }

    #[test]
    fn test_correct_by() {
        let mut offset = ViewportOffset::fixed(100.0);
        offset.correct_by(50.0);
        assert_eq!(offset.pixels(), 150.0);

        offset.correct_by(-30.0);
        assert_eq!(offset.pixels(), 120.0);
    }

    #[test]
    fn test_apply_viewport_dimension() {
        let mut offset = ViewportOffset::zero();

        assert!(offset.apply_viewport_dimension(600.0));
        assert_eq!(offset.viewport_dimension(), Some(600.0));

        // Same value should return false
        assert!(!offset.apply_viewport_dimension(600.0));

        // Different value should return true
        assert!(offset.apply_viewport_dimension(800.0));
        assert_eq!(offset.viewport_dimension(), Some(800.0));
    }

    #[test]
    fn test_apply_content_dimensions() {
        let mut offset = ViewportOffset::fixed(500.0);

        assert!(offset.apply_content_dimensions(0.0, 1000.0));
        assert_eq!(offset.min_scroll_extent(), Some(0.0));
        assert_eq!(offset.max_scroll_extent(), Some(1000.0));

        // Pixels should be clamped
        offset.jump_to(1500.0);
        offset.apply_content_dimensions(0.0, 1000.0);
        assert_eq!(offset.pixels(), 1000.0);
    }

    #[test]
    fn test_at_edge_start() {
        let mut offset = ViewportOffset::zero();
        offset.apply_content_dimensions(0.0, 1000.0);

        assert!(offset.at_edge_start());
        assert!(!offset.at_edge_end());

        offset.jump_to(500.0);
        assert!(!offset.at_edge_start());
    }

    #[test]
    fn test_at_edge_end() {
        let mut offset = ViewportOffset::fixed(1000.0);
        offset.apply_content_dimensions(0.0, 1000.0);

        assert!(!offset.at_edge_start());
        assert!(offset.at_edge_end());
    }

    #[test]
    fn test_out_of_range() {
        let mut offset = ViewportOffset::fixed(-50.0);
        offset.min_scroll_extent = Some(0.0);
        offset.max_scroll_extent = Some(1000.0);

        assert!(offset.out_of_range());

        offset.pixels = 500.0;
        assert!(!offset.out_of_range());

        offset.pixels = 1050.0;
        assert!(offset.out_of_range());
    }

    #[test]
    fn test_scroll_direction() {
        assert_eq!(ScrollDirection::default(), ScrollDirection::Idle);
        assert!(ScrollDirection::Idle.is_idle());
        assert!(!ScrollDirection::Forward.is_idle());

        assert_eq!(
            ScrollDirection::Forward.opposite(),
            ScrollDirection::Reverse
        );
        assert_eq!(
            ScrollDirection::Reverse.opposite(),
            ScrollDirection::Forward
        );
        assert_eq!(ScrollDirection::Idle.opposite(), ScrollDirection::Idle);
    }

    #[test]
    fn test_allow_implicit_scrolling() {
        let mut offset = ViewportOffset::zero();
        assert!(!offset.allow_implicit_scrolling());

        offset.set_allow_implicit_scrolling(true);
        assert!(offset.allow_implicit_scrolling());
    }

    #[test]
    fn test_user_scroll_direction() {
        let mut offset = ViewportOffset::zero();
        assert_eq!(offset.user_scroll_direction(), ScrollDirection::Idle);

        offset.set_user_scroll_direction(ScrollDirection::Forward);
        assert_eq!(offset.user_scroll_direction(), ScrollDirection::Forward);
    }

    #[test]
    fn test_on_change_callback() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        let mut offset = ViewportOffset::zero();
        offset.set_on_change(Arc::new(move |_| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        }));

        offset.jump_to(100.0);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Same value shouldn't trigger callback
        offset.jump_to(100.0);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        offset.jump_to(200.0);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
