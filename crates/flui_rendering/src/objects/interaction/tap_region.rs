//! RenderTapRegion - detects taps inside or outside its bounds
//!
//! This RenderObject can detect when taps occur both inside and outside
//! its boundaries, which is useful for implementing dismissible overlays,
//! dropdown menus, and similar UI patterns.
//!
//! Flutter reference: https://api.flutter.dev/flutter/widgets/TapRegion-class.html

use crate::core::{BoxProtocol, LayoutContext, PaintContext, FullRenderTree, RenderBox, Single};
use flui_types::Size;
use std::sync::Arc;

/// Callback type for tap region events
pub type TapRegionCallback = Arc<dyn Fn() + Send + Sync>;

/// Callbacks for tap region events
///
/// These callbacks are invoked when taps occur inside or outside the region.
#[derive(Clone, Default)]
pub struct TapRegionCallbacks {
    /// Called when a tap occurs inside this region (or any grouped region)
    pub on_tap_inside: Option<TapRegionCallback>,

    /// Called when a tap occurs outside this region (and its group)
    pub on_tap_outside: Option<TapRegionCallback>,

    /// Called when tap is released inside this region
    pub on_tap_up_inside: Option<TapRegionCallback>,

    /// Called when tap is released outside this region
    pub on_tap_up_outside: Option<TapRegionCallback>,
}

impl TapRegionCallbacks {
    /// Create new empty callbacks
    pub fn new() -> Self {
        Self::default()
    }

    /// Set on_tap_inside callback
    pub fn with_on_tap_inside<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap_inside = Some(Arc::new(callback));
        self
    }

    /// Set on_tap_outside callback
    pub fn with_on_tap_outside<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap_outside = Some(Arc::new(callback));
        self
    }

    /// Set on_tap_up_inside callback
    pub fn with_on_tap_up_inside<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap_up_inside = Some(Arc::new(callback));
        self
    }

    /// Set on_tap_up_outside callback
    pub fn with_on_tap_up_outside<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap_up_outside = Some(Arc::new(callback));
        self
    }
}

impl std::fmt::Debug for TapRegionCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapRegionCallbacks")
            .field("on_tap_inside", &self.on_tap_inside.is_some())
            .field("on_tap_outside", &self.on_tap_outside.is_some())
            .field("on_tap_up_inside", &self.on_tap_up_inside.is_some())
            .field("on_tap_up_outside", &self.on_tap_up_outside.is_some())
            .finish()
    }
}

/// Group ID for linking multiple TapRegion instances
///
/// When multiple TapRegion widgets share the same group ID, they act as one
/// region. A tap inside any member of the group is considered "inside" for
/// all members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TapRegionGroupId(pub u64);

impl TapRegionGroupId {
    /// Create a new group ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// RenderObject that detects taps inside or outside its bounds
///
/// Unlike standard gesture recognizers, TapRegion can detect taps that occur
/// outside its boundaries. This is useful for implementing:
///
/// - Dismissible overlays (tap outside to close)
/// - Dropdown menus (tap outside to close)
/// - Modal dialogs (tap outside to dismiss)
/// - Focus management (detect when user taps elsewhere)
///
/// # Grouping
///
/// Multiple TapRegion widgets can be grouped using `group_id`. When grouped,
/// they act as a single region - a tap inside any member is considered
/// "inside" for all members.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderTapRegion, TapRegionCallbacks};
///
/// let callbacks = TapRegionCallbacks::new()
///     .with_on_tap_outside(|| println!("Tapped outside - closing overlay"));
///
/// let tap_region = RenderTapRegion::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderTapRegion {
    /// Tap callbacks
    pub callbacks: TapRegionCallbacks,

    /// Optional group ID for linking multiple regions
    pub group_id: Option<TapRegionGroupId>,

    /// Whether outside taps should consume the event
    ///
    /// When true, taps outside this region (and its group) will stop
    /// event propagation in the gesture arena.
    pub consume_outside_taps: bool,

    /// Whether this region is enabled
    pub enabled: bool,

    /// Cached size from last layout
    size: Size,
}

impl RenderTapRegion {
    /// Create new RenderTapRegion
    pub fn new(callbacks: TapRegionCallbacks) -> Self {
        Self {
            callbacks,
            group_id: None,
            consume_outside_taps: false,
            enabled: true,
            size: Size::ZERO,
        }
    }

    /// Create with group ID
    pub fn with_group(callbacks: TapRegionCallbacks, group_id: TapRegionGroupId) -> Self {
        Self {
            callbacks,
            group_id: Some(group_id),
            consume_outside_taps: false,
            enabled: true,
            size: Size::ZERO,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &TapRegionCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: TapRegionCallbacks) {
        self.callbacks = callbacks;
    }

    /// Get the group ID
    pub fn group_id(&self) -> Option<TapRegionGroupId> {
        self.group_id
    }

    /// Set the group ID
    pub fn set_group_id(&mut self, group_id: Option<TapRegionGroupId>) {
        self.group_id = group_id;
    }

    /// Check if outside taps are consumed
    pub fn consume_outside_taps(&self) -> bool {
        self.consume_outside_taps
    }

    /// Set whether to consume outside taps
    pub fn set_consume_outside_taps(&mut self, consume: bool) {
        self.consume_outside_taps = consume;
    }

    /// Check if this region is enabled
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Set whether this region is enabled
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Handle tap inside event
    pub fn handle_tap_inside(&self) {
        if self.enabled {
            if let Some(callback) = &self.callbacks.on_tap_inside {
                callback();
            }
        }
    }

    /// Handle tap outside event
    pub fn handle_tap_outside(&self) {
        if self.enabled {
            if let Some(callback) = &self.callbacks.on_tap_outside {
                callback();
            }
        }
    }

    /// Handle tap up inside event
    pub fn handle_tap_up_inside(&self) {
        if self.enabled {
            if let Some(callback) = &self.callbacks.on_tap_up_inside {
                callback();
            }
        }
    }

    /// Handle tap up outside event
    pub fn handle_tap_up_outside(&self) {
        if self.enabled {
            if let Some(callback) = &self.callbacks.on_tap_up_outside {
                callback();
            }
        }
    }
}

impl Default for RenderTapRegion {
    fn default() -> Self {
        Self::new(TapRegionCallbacks::default())
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderTapRegion {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child with same constraints
        let size = ctx.layout_child(child_id, ctx.constraints);

        // Cache size
        self.size = size;

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Paint child normally
        ctx.paint_child(child_id, ctx.offset);

        // Note: Tap detection requires integration with the event system:
        // 1. TapRegionSurface (ancestor widget) manages global tap detection
        // 2. Hit testing provides the region bounds for inside/outside detection
        // 3. Event dispatcher invokes callbacks based on tap location
        // This render object provides the structure; tap handling is done by the framework
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_tap_region_new() {
        let callbacks = TapRegionCallbacks::new();
        let region = RenderTapRegion::new(callbacks);

        assert!(region.enabled());
        assert!(!region.consume_outside_taps());
        assert!(region.group_id().is_none());
    }

    #[test]
    fn test_tap_region_with_group() {
        let callbacks = TapRegionCallbacks::new();
        let group_id = TapRegionGroupId::new(42);
        let region = RenderTapRegion::with_group(callbacks, group_id);

        assert_eq!(region.group_id(), Some(TapRegionGroupId::new(42)));
    }

    #[test]
    fn test_tap_region_callbacks_builder() {
        let outside_tapped = Arc::new(AtomicBool::new(false));
        let outside_tapped_clone = outside_tapped.clone();

        let callbacks = TapRegionCallbacks::new()
            .with_on_tap_inside(|| {})
            .with_on_tap_outside(move || outside_tapped_clone.store(true, Ordering::SeqCst));

        let region = RenderTapRegion::new(callbacks);

        assert!(region.callbacks().on_tap_inside.is_some());
        assert!(region.callbacks().on_tap_outside.is_some());
        assert!(region.callbacks().on_tap_up_inside.is_none());

        // Test callback execution
        region.handle_tap_outside();
        assert!(outside_tapped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_tap_region_disabled() {
        let tapped = Arc::new(AtomicBool::new(false));
        let tapped_clone = tapped.clone();

        let callbacks = TapRegionCallbacks::new()
            .with_on_tap_inside(move || tapped_clone.store(true, Ordering::SeqCst));

        let mut region = RenderTapRegion::new(callbacks);
        region.set_enabled(false);

        // Should not trigger callback when disabled
        region.handle_tap_inside();
        assert!(!tapped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_tap_region_callbacks_debug() {
        let callbacks = TapRegionCallbacks::new()
            .with_on_tap_inside(|| {})
            .with_on_tap_outside(|| {});

        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("TapRegionCallbacks"));
        assert!(debug_str.contains("on_tap_inside"));
    }

    #[test]
    fn test_tap_region_group_id() {
        let group1 = TapRegionGroupId::new(1);
        let group2 = TapRegionGroupId::new(1);
        let group3 = TapRegionGroupId::new(2);

        assert_eq!(group1, group2);
        assert_ne!(group1, group3);
    }
}
