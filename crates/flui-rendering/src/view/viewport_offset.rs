//! Viewport offset for scroll position tracking.

use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

/// The direction of a scroll, relative to the positive scroll offset axis.
///
/// This indicates the direction that the user is scrolling, not the direction
/// that the scroll offset is changing.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ScrollDirection` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ScrollDirection {
    /// No scrolling is underway.
    #[default]
    Idle,

    /// Scrolling is happening in the negative scroll offset direction.
    ///
    /// For a vertical list with `AxisDirection::down`, this means the content
    /// is moving down, exposing earlier content.
    Forward,

    /// Scrolling is happening in the positive scroll offset direction.
    ///
    /// For a vertical list with `AxisDirection::down`, this means the content
    /// is moving up, exposing later content.
    Reverse,
}

impl ScrollDirection {
    /// Returns the opposite scroll direction.
    pub fn flip(self) -> Self {
        match self {
            Self::Idle => Self::Idle,
            Self::Forward => Self::Reverse,
            Self::Reverse => Self::Forward,
        }
    }
}

/// Which part of the content inside the viewport should be visible.
///
/// The `pixels` value determines the scroll offset that the viewport uses to
/// select which part of its content to display. As the user scrolls the
/// viewport, this value changes, which changes the content that is displayed.
///
/// This trait is a [`ChangeNotifier`]-like that notifies its listeners when
/// `pixels` changes.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ViewportOffset` abstract class.
pub trait ViewportOffset: Debug + Send + Sync {
    /// The number of pixels to offset the children in the opposite of the axis direction.
    ///
    /// For example, if the axis direction is down, then the pixel value
    /// represents the number of logical pixels to move the children _up_ the
    /// screen.
    fn pixels(&self) -> f32;

    /// Whether the `pixels` property is available.
    fn has_pixels(&self) -> bool;

    /// Called when the viewport's extents are established.
    ///
    /// The argument is the dimension of the viewport in the main axis
    /// (e.g., the height for a vertical viewport).
    ///
    /// If applying the viewport dimension changes the scroll offset, return
    /// `false`. Otherwise, return `true`.
    fn apply_viewport_dimension(&mut self, viewport_dimension: f32) -> bool;

    /// Called when the viewport's content extents are established.
    ///
    /// The arguments are the minimum and maximum scroll extents respectively.
    ///
    /// If applying the content dimensions changes the scroll offset, return
    /// `false`. Otherwise, return `true`.
    fn apply_content_dimensions(&mut self, min_scroll_extent: f32, max_scroll_extent: f32) -> bool;

    /// Apply a layout-time correction to the scroll offset.
    ///
    /// This method should change the `pixels` value by `correction`, but without
    /// calling the notification callbacks.
    fn correct_by(&mut self, correction: f32);

    /// Jumps `pixels` from its current value to the given value,
    /// without animation.
    fn jump_to(&mut self, pixels: f32);

    /// Animates `pixels` from its current value to the given value.
    ///
    /// For synchronous implementations, this can just call `jump_to`.
    fn animate_to(&mut self, to: f32, duration_ms: u64);

    /// Calls `jump_to` if duration is zero, otherwise `animate_to`.
    fn move_to(&mut self, to: f32, duration_ms: Option<u64>) {
        match duration_ms {
            Some(0) | None => self.jump_to(to),
            Some(ms) => self.animate_to(to, ms),
        }
    }

    /// The direction in which the user is trying to change `pixels`.
    fn user_scroll_direction(&self) -> ScrollDirection;

    /// Whether a viewport is allowed to change `pixels` implicitly to respond to
    /// a call to show a render object on screen.
    fn allow_implicit_scrolling(&self) -> bool;

    /// Adds a listener that will be called when `pixels` changes.
    fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>);

    /// Removes a listener.
    fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>);
}

/// A simple fixed viewport offset that doesn't change.
///
/// The `pixels` value does not change unless the viewport issues a correction.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `_FixedViewportOffset` class.
pub struct FixedViewportOffset {
    pixels: f32,
    listeners: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

impl Debug for FixedViewportOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedViewportOffset")
            .field("pixels", &self.pixels)
            .field("listeners_count", &self.listeners.read().len())
            .finish()
    }
}

impl FixedViewportOffset {
    /// Creates a fixed viewport offset with the given pixels value.
    pub fn new(pixels: f32) -> Self {
        Self {
            pixels,
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Creates a fixed viewport offset at zero.
    pub fn zero() -> Self {
        Self::new(0.0)
    }

    #[allow(dead_code)] // Reserved for future ViewportOffset listener API
    fn notify_listeners(&self) {
        let listeners = self.listeners.read();
        for listener in listeners.iter() {
            listener();
        }
    }
}

impl Default for FixedViewportOffset {
    fn default() -> Self {
        Self::zero()
    }
}

impl ViewportOffset for FixedViewportOffset {
    fn pixels(&self) -> f32 {
        self.pixels
    }

    fn has_pixels(&self) -> bool {
        true
    }

    fn apply_viewport_dimension(&mut self, _viewport_dimension: f32) -> bool {
        true
    }

    fn apply_content_dimensions(
        &mut self,
        _min_scroll_extent: f32,
        _max_scroll_extent: f32,
    ) -> bool {
        true
    }

    fn correct_by(&mut self, correction: f32) {
        self.pixels += correction;
    }

    fn jump_to(&mut self, _pixels: f32) {
        // Fixed viewport offset doesn't change
    }

    fn animate_to(&mut self, _to: f32, _duration_ms: u64) {
        // Fixed viewport offset doesn't animate
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        ScrollDirection::Idle
    }

    fn allow_implicit_scrolling(&self) -> bool {
        false
    }

    fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().push(listener);
    }

    fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>) {
        let mut listeners = self.listeners.write();
        if let Some(pos) = listeners.iter().position(|l| Arc::ptr_eq(l, listener)) {
            listeners.remove(pos);
        }
    }
}

/// A mutable viewport offset that can be scrolled.
///
/// This is a more complete implementation that tracks scroll state.
///
/// # Flutter Equivalence
///
/// Similar to Flutter's `ScrollPosition` but simplified.
pub struct ScrollableViewportOffset {
    /// Current scroll position in pixels.
    pixels: f32,

    /// Whether pixels has been set.
    has_pixels: bool,

    /// Minimum scroll extent.
    min_scroll_extent: f32,

    /// Maximum scroll extent.
    max_scroll_extent: f32,

    /// The viewport dimension.
    viewport_dimension: f32,

    /// Current scroll direction.
    user_scroll_direction: ScrollDirection,

    /// Whether implicit scrolling is allowed.
    allow_implicit_scrolling: bool,

    /// Listeners for change notifications.
    listeners: RwLock<Vec<Arc<dyn Fn() + Send + Sync>>>,

    /// Whether we're currently notifying listeners.
    notifying: AtomicBool,
}

impl Debug for ScrollableViewportOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollableViewportOffset")
            .field("pixels", &self.pixels)
            .field("has_pixels", &self.has_pixels)
            .field("min_scroll_extent", &self.min_scroll_extent)
            .field("max_scroll_extent", &self.max_scroll_extent)
            .field("viewport_dimension", &self.viewport_dimension)
            .field("user_scroll_direction", &self.user_scroll_direction)
            .field("allow_implicit_scrolling", &self.allow_implicit_scrolling)
            .field("listeners_count", &self.listeners.read().len())
            .finish()
    }
}

impl ScrollableViewportOffset {
    /// Creates a new scrollable viewport offset.
    pub fn new(initial_pixels: f32) -> Self {
        Self {
            pixels: initial_pixels,
            has_pixels: true,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            viewport_dimension: 0.0,
            user_scroll_direction: ScrollDirection::Idle,
            allow_implicit_scrolling: true,
            listeners: RwLock::new(Vec::new()),
            notifying: AtomicBool::new(false),
        }
    }

    /// Creates a scrollable viewport offset at zero.
    pub fn zero() -> Self {
        Self::new(0.0)
    }

    /// Returns the minimum scroll extent.
    pub fn min_scroll_extent(&self) -> f32 {
        self.min_scroll_extent
    }

    /// Returns the maximum scroll extent.
    pub fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    /// Returns the viewport dimension.
    pub fn viewport_dimension(&self) -> f32 {
        self.viewport_dimension
    }

    /// Returns whether there's content above the current scroll position.
    pub fn extends_before(&self) -> bool {
        self.pixels > self.min_scroll_extent
    }

    /// Returns whether there's content below the current scroll position.
    pub fn extends_after(&self) -> bool {
        self.pixels < self.max_scroll_extent
    }

    /// Returns whether there's content to scroll in either direction.
    pub fn out_of_range(&self) -> bool {
        self.pixels < self.min_scroll_extent || self.pixels > self.max_scroll_extent
    }

    /// Returns the current scroll position as a ratio (0.0 to 1.0).
    pub fn scroll_ratio(&self) -> f32 {
        let range = self.max_scroll_extent - self.min_scroll_extent;
        if range <= 0.0 {
            0.0
        } else {
            ((self.pixels - self.min_scroll_extent) / range).clamp(0.0, 1.0)
        }
    }

    /// Sets the pixels value and notifies listeners.
    pub fn set_pixels(&mut self, value: f32) {
        if (self.pixels - value).abs() > f32::EPSILON {
            self.pixels = value;
            self.notify_listeners();
        }
    }

    /// Sets the scroll direction.
    pub fn set_user_scroll_direction(&mut self, direction: ScrollDirection) {
        self.user_scroll_direction = direction;
    }

    /// Sets whether implicit scrolling is allowed.
    pub fn set_allow_implicit_scrolling(&mut self, allow: bool) {
        self.allow_implicit_scrolling = allow;
    }

    fn notify_listeners(&self) {
        // Prevent recursive notification
        if self.notifying.swap(true, Ordering::SeqCst) {
            return;
        }

        let listeners = self.listeners.read();
        for listener in listeners.iter() {
            listener();
        }

        self.notifying.store(false, Ordering::SeqCst);
    }
}

impl Default for ScrollableViewportOffset {
    fn default() -> Self {
        Self::zero()
    }
}

impl ViewportOffset for ScrollableViewportOffset {
    fn pixels(&self) -> f32 {
        self.pixels
    }

    fn has_pixels(&self) -> bool {
        self.has_pixels
    }

    fn apply_viewport_dimension(&mut self, viewport_dimension: f32) -> bool {
        if (self.viewport_dimension - viewport_dimension).abs() < f32::EPSILON {
            return true;
        }
        self.viewport_dimension = viewport_dimension;
        true
    }

    fn apply_content_dimensions(&mut self, min_scroll_extent: f32, max_scroll_extent: f32) -> bool {
        if (self.min_scroll_extent - min_scroll_extent).abs() < f32::EPSILON
            && (self.max_scroll_extent - max_scroll_extent).abs() < f32::EPSILON
        {
            return true;
        }

        self.min_scroll_extent = min_scroll_extent;
        self.max_scroll_extent = max_scroll_extent;

        // Clamp pixels to valid range
        let clamped = self.pixels.clamp(min_scroll_extent, max_scroll_extent);
        if (self.pixels - clamped).abs() > f32::EPSILON {
            self.pixels = clamped;
            return false; // Need relayout
        }

        true
    }

    fn correct_by(&mut self, correction: f32) {
        self.pixels += correction;
    }

    fn jump_to(&mut self, pixels: f32) {
        if (self.pixels - pixels).abs() > f32::EPSILON {
            self.pixels = pixels;
            self.notify_listeners();
        }
    }

    fn animate_to(&mut self, to: f32, _duration_ms: u64) {
        // For now, just jump (no animation support yet)
        self.jump_to(to);
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        self.user_scroll_direction
    }

    fn allow_implicit_scrolling(&self) -> bool {
        self.allow_implicit_scrolling
    }

    fn add_listener(&self, listener: Arc<dyn Fn() + Send + Sync>) {
        self.listeners.write().push(listener);
    }

    fn remove_listener(&self, listener: &Arc<dyn Fn() + Send + Sync>) {
        let mut listeners = self.listeners.write();
        if let Some(pos) = listeners.iter().position(|l| Arc::ptr_eq(l, listener)) {
            listeners.remove(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_direction_flip() {
        assert_eq!(ScrollDirection::Idle.flip(), ScrollDirection::Idle);
        assert_eq!(ScrollDirection::Forward.flip(), ScrollDirection::Reverse);
        assert_eq!(ScrollDirection::Reverse.flip(), ScrollDirection::Forward);
    }

    #[test]
    fn test_fixed_viewport_offset_new() {
        let offset = FixedViewportOffset::new(100.0);
        assert_eq!(offset.pixels(), 100.0);
        assert!(offset.has_pixels());
    }

    #[test]
    fn test_fixed_viewport_offset_zero() {
        let offset = FixedViewportOffset::zero();
        assert_eq!(offset.pixels(), 0.0);
    }

    #[test]
    fn test_fixed_viewport_offset_correct_by() {
        let mut offset = FixedViewportOffset::new(100.0);
        offset.correct_by(50.0);
        assert_eq!(offset.pixels(), 150.0);
    }

    #[test]
    fn test_fixed_viewport_offset_jump_to_does_nothing() {
        let mut offset = FixedViewportOffset::new(100.0);
        offset.jump_to(200.0);
        assert_eq!(offset.pixels(), 100.0); // Should not change
    }

    #[test]
    fn test_fixed_viewport_offset_defaults() {
        let offset = FixedViewportOffset::zero();
        assert_eq!(offset.user_scroll_direction(), ScrollDirection::Idle);
        assert!(!offset.allow_implicit_scrolling());
    }

    #[test]
    fn test_scrollable_viewport_offset_new() {
        let offset = ScrollableViewportOffset::new(100.0);
        assert_eq!(offset.pixels(), 100.0);
        assert!(offset.has_pixels());
    }

    #[test]
    fn test_scrollable_viewport_offset_set_pixels() {
        let mut offset = ScrollableViewportOffset::zero();
        offset.set_pixels(50.0);
        assert_eq!(offset.pixels(), 50.0);
    }

    #[test]
    fn test_scrollable_viewport_offset_jump_to() {
        let mut offset = ScrollableViewportOffset::zero();
        offset.jump_to(100.0);
        assert_eq!(offset.pixels(), 100.0);
    }

    #[test]
    fn test_scrollable_viewport_offset_apply_dimensions() {
        let mut offset = ScrollableViewportOffset::zero();

        let result = offset.apply_viewport_dimension(500.0);
        assert!(result);
        assert_eq!(offset.viewport_dimension(), 500.0);

        let result = offset.apply_content_dimensions(0.0, 1000.0);
        assert!(result);
        assert_eq!(offset.min_scroll_extent(), 0.0);
        assert_eq!(offset.max_scroll_extent(), 1000.0);
    }

    #[test]
    fn test_scrollable_viewport_offset_clamps_on_dimensions() {
        let mut offset = ScrollableViewportOffset::new(500.0);

        // Set content dimensions that make current position out of range
        let result = offset.apply_content_dimensions(0.0, 100.0);
        assert!(!result); // Should need relayout
        assert_eq!(offset.pixels(), 100.0); // Clamped to max
    }

    #[test]
    fn test_scrollable_viewport_offset_extends_before_after() {
        let mut offset = ScrollableViewportOffset::new(50.0);
        offset.apply_content_dimensions(0.0, 100.0);

        assert!(offset.extends_before()); // 50 > 0
        assert!(offset.extends_after()); // 50 < 100
    }

    #[test]
    fn test_scrollable_viewport_offset_scroll_ratio() {
        let mut offset = ScrollableViewportOffset::new(50.0);
        offset.apply_content_dimensions(0.0, 100.0);

        assert!((offset.scroll_ratio() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_scrollable_viewport_offset_scroll_direction() {
        let mut offset = ScrollableViewportOffset::zero();
        assert_eq!(offset.user_scroll_direction(), ScrollDirection::Idle);

        offset.set_user_scroll_direction(ScrollDirection::Forward);
        assert_eq!(offset.user_scroll_direction(), ScrollDirection::Forward);
    }

    #[test]
    fn test_scrollable_viewport_offset_listeners() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let offset = ScrollableViewportOffset::zero();
        let counter = Arc::new(AtomicU32::new(0));

        let counter_clone = counter.clone();
        let listener: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        offset.add_listener(listener.clone());
        offset.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        offset.remove_listener(&listener);
        offset.notify_listeners();
        assert_eq!(counter.load(Ordering::SeqCst), 1); // No change after removal
    }

    #[test]
    fn test_scrollable_viewport_offset_correct_by() {
        let mut offset = ScrollableViewportOffset::new(100.0);
        offset.correct_by(25.0);
        assert_eq!(offset.pixels(), 125.0);
    }
}
