//! RenderTapRegion - Detects taps inside or outside widget bounds
//!
//! Implements Flutter's TapRegion that can detect when taps occur both inside
//! and outside its boundaries, useful for implementing dismissible overlays,
//! dropdown menus, and focus management.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderTapRegion` | `RenderTapRegion` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `TapRegionCallbacks` | TapRegion callbacks |
//! | `on_tap_inside` | `onTapInside` callback |
//! | `on_tap_outside` | `onTapOutside` callback |
//! | `on_tap_up_inside` | `onTapUpInside` callback (tap release inside) |
//! | `on_tap_up_outside` | `onTapUpOutside` callback (tap release outside) |
//! | `group_id` | `groupId` property (TapRegionGroup) |
//! | `consume_outside_taps` | `consumeOutsideTaps` property |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Cache size**
//!    - Store child size for hit region bounds calculation
//!
//! 3. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Paint child normally**
//!    - Child painted at widget offset
//!    - No visual changes from tap detection
//!
//! 2. **Register tap region** (framework integration)
//!    - TapRegionSurface ancestor manages global tap detection
//!    - Hit testing provides region bounds for inside/outside detection
//!    - Event dispatcher invokes callbacks based on tap location
//!
//! # Event Handling Protocol
//!
//! 1. **Tap inside**
//!    - Triggered when tap occurs within bounds (or any grouped region)
//!    - Calls `on_tap_inside` callback if provided
//!
//! 2. **Tap outside**
//!    - Triggered when tap occurs outside bounds (and its group)
//!    - Calls `on_tap_outside` callback if provided
//!    - Can consume event if `consume_outside_taps = true`
//!
//! 3. **Tap up inside**
//!    - Triggered when tap is released inside bounds
//!    - Calls `on_tap_up_inside` callback if provided
//!
//! 4. **Tap up outside**
//!    - Triggered when tap is released outside bounds
//!    - Calls `on_tap_up_outside` callback if provided
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child + size cache
//! - **Paint**: O(1) - pass-through to child
//! - **Event handling**: O(1) - callback invocation per event
//! - **Memory**: ~64 bytes (4 Arc callbacks + group ID + flags + size)
//!
//! # Use Cases
//!
//! - **Dismissible overlays**: Tap outside to close modal dialogs
//! - **Dropdown menus**: Close menu when clicking outside
//! - **Focus management**: Detect when user clicks elsewhere
//! - **Tooltip dismissal**: Hide tooltip on outside click
//! - **Context menus**: Close menu on outside interaction
//! - **Autocomplete**: Dismiss suggestions on outside click
//!
//! # Grouping
//!
//! Multiple TapRegion widgets can be grouped using `group_id`. When grouped,
//! they act as a single region - a tap inside any member is considered
//! "inside" for all members. This is useful for:
//!
//! - **Split UI elements**: Button + dropdown menu as one region
//! - **Toolbar groups**: Multiple buttons as single tap region
//! - **Form sections**: Related inputs as single focus region
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderTapRegion, TapRegionCallbacks};
//!
//! // Dismissible overlay - tap outside to close
//! let callbacks = TapRegionCallbacks::new()
//!     .with_on_tap_outside(|| println!("Closing overlay"));
//! let overlay = RenderTapRegion::new(callbacks);
//!
//! // Grouped regions - button + dropdown act as one
//! let group_id = TapRegionGroupId::new(1);
//! let button_region = RenderTapRegion::with_group(callbacks.clone(), group_id);
//! let dropdown_region = RenderTapRegion::with_group(callbacks, group_id);
//! // Tapping dropdown won't trigger on_tap_outside for button
//!
//! // Consume outside taps (prevent propagation)
//! let mut modal = RenderTapRegion::new(callbacks);
//! modal.set_consume_outside_taps(true);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
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

/// RenderObject that detects taps inside or outside its bounds.
///
/// Unlike standard gesture recognizers, TapRegion can detect taps that occur
/// outside its boundaries, enabling interaction patterns where outside clicks
/// trigger actions (dismissal, focus loss, etc.).
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
/// **Proxy** - Passes constraints unchanged, only adds tap detection.
///
/// # Use Cases
///
/// - **Dismissible overlays**: Modal dialogs that close on outside tap
/// - **Dropdown menus**: Menus that close when clicking outside
/// - **Focus management**: Detect clicks outside focused element
/// - **Tooltip dismissal**: Hide tooltips on outside interaction
/// - **Context menus**: Close menus on outside click
/// - **Autocomplete**: Dismiss suggestions when clicking away
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderTapRegion behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Detects taps both inside and outside bounds
/// - Supports region grouping via `group_id`
/// - Can consume outside taps with `consume_outside_taps`
/// - Provides callbacks for tap/tap up inside/outside
/// - Requires TapRegionSurface ancestor for tap coordination
///
/// # Grouping
///
/// Multiple TapRegion widgets can be grouped using `group_id`. When grouped,
/// they act as a single region - a tap inside any member is considered
/// "inside" for all members. Useful for split UI elements that should act
/// as one tap region (e.g., button + attached dropdown).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderTapRegion, TapRegionCallbacks};
///
/// // Dismissible overlay
/// let callbacks = TapRegionCallbacks::new()
///     .with_on_tap_outside(|| println!("Closing overlay"));
/// let tap_region = RenderTapRegion::new(callbacks);
///
/// // Grouped regions (button + dropdown = one region)
/// let group_id = TapRegionGroupId::new(1);
/// let button = RenderTapRegion::with_group(callbacks.clone(), group_id);
/// let dropdown = RenderTapRegion::with_group(callbacks, group_id);
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

impl RenderObject for RenderTapRegion {}

impl RenderBox<Single> for RenderTapRegion {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        let size = ctx.layout_child(child_id, ctx.constraints, true)?;

        // Cache size for hit region bounds calculation
        self.size = size;

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: paint child at widget offset
        // Tap detection doesn't affect visual rendering
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
