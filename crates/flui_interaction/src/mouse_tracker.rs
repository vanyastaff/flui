//! Mouse tracking for hover, enter, and exit events
//!
//! The MouseTracker manages the relationship between mouse devices and UI regions,
//! triggering mouse events as the cursor moves through the widget tree.
//!
//! # Architecture
//!
//! ```text
//! Platform Mouse Move
//!     ↓
//! MouseTracker.update_with_event()
//!     ↓
//! Hit Test (find regions under cursor)
//!     ↓
//! Compare with previous hit list
//!     ↓
//! Generate Enter/Exit/Hover events
//!     ↓
//! Notify MouseRegion widgets
//! ```
//!
//! # Design
//!
//! - **Centralized**: Single global tracker for all mouse devices
//! - **Lazy**: Only updates when mouse actually moves
//! - **Cached**: Stores previous hit list to detect enter/exit
//! - **Thread-safe**: Uses `Arc<Mutex>` for concurrent access
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::mouse_tracker::MouseTracker;
//!
//! let tracker = MouseTracker::global();
//!
//! // Update on mouse move
//! tracker.update_with_event(pointer_event, &hit_test_result);
//!
//! // Check if mouse is connected
//! if tracker.mouse_is_connected() {
//!     println!("Mouse detected!");
//! }
//! ```

use parking_lot::Mutex;
use smallvec::SmallVec;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::events::{CursorIcon, InputEvent, PointerEvent, PointerEventExt};
use flui_types::geometry::{Offset, Pixels};

use crate::ids::RegionId;

/// Device ID type (re-exported from events).
pub use crate::events::DeviceId;
use crate::routing::HitTestResult;

/// Callback for mouse enter events
pub type MouseEnterCallback = Arc<dyn Fn(DeviceId, Offset<Pixels>) + Send + Sync>;

/// Callback for mouse exit events
pub type MouseExitCallback = Arc<dyn Fn(DeviceId, Offset<Pixels>) + Send + Sync>;

/// Callback for mouse hover events
pub type MouseHoverCallback = Arc<dyn Fn(DeviceId, Offset<Pixels>) + Send + Sync>;

/// Annotation for a mouse-sensitive region
///
/// This is typically created by MouseRegion widgets and registered
/// with the MouseTracker.
#[derive(Clone)]
pub struct MouseTrackerAnnotation {
    /// Unique ID for this region
    pub region_id: RegionId,
    /// Called when mouse enters this region
    pub on_enter: Option<MouseEnterCallback>,
    /// Called when mouse exits this region
    pub on_exit: Option<MouseExitCallback>,
    /// Called when mouse hovers over this region
    pub on_hover: Option<MouseHoverCallback>,
}

impl MouseTrackerAnnotation {
    /// Creates a new annotation for a region
    pub fn new(region_id: RegionId) -> Self {
        Self {
            region_id,
            on_enter: None,
            on_exit: None,
            on_hover: None,
        }
    }

    /// Sets the enter callback
    pub fn with_enter<F>(mut self, callback: F) -> Self
    where
        F: Fn(DeviceId, Offset<Pixels>) + Send + Sync + 'static,
    {
        self.on_enter = Some(Arc::new(callback));
        self
    }

    /// Sets the exit callback
    pub fn with_exit<F>(mut self, callback: F) -> Self
    where
        F: Fn(DeviceId, Offset<Pixels>) + Send + Sync + 'static,
    {
        self.on_exit = Some(Arc::new(callback));
        self
    }

    /// Sets the hover callback
    pub fn with_hover<F>(mut self, callback: F) -> Self
    where
        F: Fn(DeviceId, Offset<Pixels>) + Send + Sync + 'static,
    {
        self.on_hover = Some(Arc::new(callback));
        self
    }
}

/// State for a single mouse device
#[derive(Debug, Clone)]
struct DeviceState {
    /// Last known position
    last_position: Offset<Pixels>,
    /// Set of regions currently under this device
    active_regions: HashSet<RegionId>,
    /// Current mouse cursor for this device
    current_cursor: CursorIcon,
}

/// Global mouse tracker
///
/// Tracks all mouse devices and their relationships with UI regions.
/// Generates enter/exit/hover events as the mouse moves.
#[derive(Clone)]
pub struct MouseTracker {
    inner: Arc<Mutex<MouseTrackerInner>>,
}

/// Callback for cursor changes.
pub type CursorChangeCallback = Arc<dyn Fn(DeviceId, CursorIcon) + Send + Sync>;

struct MouseTrackerInner {
    /// State for each mouse device
    devices: HashMap<DeviceId, DeviceState>,
    /// Registered annotations (regions)
    annotations: HashMap<RegionId, MouseTrackerAnnotation>,
    /// Whether any mouse is connected
    mouse_connected: bool,
    /// Callback for cursor changes
    cursor_change_callback: Option<CursorChangeCallback>,
}

// Global singleton
static GLOBAL_TRACKER: once_cell::sync::Lazy<MouseTracker> =
    once_cell::sync::Lazy::new(MouseTracker::new);

impl MouseTracker {
    /// Creates a new mouse tracker
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MouseTrackerInner {
                devices: HashMap::new(),
                annotations: HashMap::new(),
                mouse_connected: false,
                cursor_change_callback: None,
            })),
        }
    }

    /// Returns the global MouseTracker instance
    pub fn global() -> &'static Self {
        &GLOBAL_TRACKER
    }

    /// Registers a mouse region annotation
    ///
    /// This should be called when a MouseRegion widget is mounted.
    pub fn register_annotation(&self, annotation: MouseTrackerAnnotation) {
        let mut inner = self.inner.lock();
        inner.annotations.insert(annotation.region_id, annotation);
    }

    /// Unregisters a mouse region annotation
    ///
    /// This should be called when a MouseRegion widget is unmounted.
    pub fn unregister_annotation(&self, region_id: RegionId) {
        let mut inner = self.inner.lock();
        inner.annotations.remove(&region_id);

        // Remove from all device active regions
        for state in inner.devices.values_mut() {
            state.active_regions.remove(&region_id);
        }
    }

    /// Updates tracking state based on an input event.
    ///
    /// This should be called whenever a mouse move event occurs.
    ///
    /// # Arguments
    ///
    /// * `event` - The input event (typically Pointer Move)
    /// * `hit_test_result` - Result of hit testing at the event position
    pub fn update_with_event(&self, event: &InputEvent, hit_test_result: &HitTestResult) {
        let mut inner = self.inner.lock();

        let (device_id, position) = match event {
            InputEvent::Pointer(pointer_event) => {
                match pointer_event {
                    PointerEvent::Move(_) => {
                        let pos = pointer_event.position();
                        let id = event.device_id().unwrap_or(0);
                        (id, pos)
                    }
                    PointerEvent::Enter(_) | PointerEvent::Leave(_) => {
                        // Enter/Leave handled separately
                        return;
                    }
                    _ => return, // Not a mouse tracking event
                }
            }
            InputEvent::DeviceAdded {
                device_id,
                pointer_type: _,
            } => {
                inner.mouse_connected = true;
                // Initialize device state
                inner.devices.insert(
                    *device_id,
                    DeviceState {
                        last_position: Offset::new(Pixels::ZERO, Pixels::ZERO),
                        active_regions: HashSet::new(),
                        current_cursor: CursorIcon::Default,
                    },
                );
                return;
            }
            InputEvent::DeviceRemoved { device_id } => {
                inner.devices.remove(device_id);
                inner.mouse_connected = !inner.devices.is_empty();
                return;
            }
            _ => return, // Not a mouse tracking event
        };

        // Get or create device state
        let state = inner
            .devices
            .entry(device_id)
            .or_insert_with(|| DeviceState {
                last_position: position,
                active_regions: HashSet::new(),
                current_cursor: CursorIcon::Default,
            });

        // Build new set of active regions from hit test
        let new_regions: HashSet<RegionId> =
            hit_test_result.iter().map(|entry| entry.target).collect();

        // Find regions that were entered (new but not in old)
        // SmallVec avoids heap allocation for typical mouse moves (1-4 regions)
        let entered: SmallVec<[RegionId; 4]> = new_regions
            .difference(&state.active_regions)
            .copied()
            .collect();

        // Find regions that were exited (old but not in new)
        let exited: SmallVec<[RegionId; 4]> = state
            .active_regions
            .difference(&new_regions)
            .copied()
            .collect();

        // Find regions that are still active (for hover events)
        let hovering: SmallVec<[RegionId; 4]> = new_regions
            .intersection(&state.active_regions)
            .copied()
            .collect();

        // Resolve cursor from hit test result
        let new_cursor = hit_test_result.resolve_cursor();
        let cursor_changed = state.current_cursor != new_cursor;

        // Update state
        state.last_position = position;
        state.active_regions = new_regions;
        state.current_cursor = new_cursor;

        // Collect callbacks (must be invoked outside the lock to avoid deadlock)
        let enter_callbacks: SmallVec<[MouseEnterCallback; 4]> = entered
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_enter.clone())
            })
            .collect();

        let exit_callbacks: SmallVec<[MouseExitCallback; 4]> = exited
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_exit.clone())
            })
            .collect();

        let hover_callbacks: SmallVec<[MouseHoverCallback; 4]> = hovering
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_hover.clone())
            })
            .collect();

        // Get cursor change callback if cursor changed
        let cursor_callback = if cursor_changed {
            inner.cursor_change_callback.clone()
        } else {
            None
        };

        // Release lock before calling callbacks
        drop(inner);

        // Invoke callbacks
        for callback in enter_callbacks {
            callback(device_id, position);
        }

        for callback in exit_callbacks {
            callback(device_id, position);
        }

        for callback in hover_callbacks {
            callback(device_id, position);
        }

        // Notify cursor change
        if let Some(callback) = cursor_callback {
            callback(device_id, new_cursor);
        }
    }

    /// Updates all mouse devices
    ///
    /// This can be used to refresh hover state when the UI tree changes.
    pub fn update_all_devices(&self) {
        // In a full implementation, this would re-run hit tests for all devices
        // For now, this is a placeholder
        tracing::trace!("update_all_devices called");
    }

    /// Checks if any mouse is currently connected
    #[inline]
    pub fn mouse_is_connected(&self) -> bool {
        self.inner.lock().mouse_connected
    }

    /// Gets the last known position for a device
    pub fn device_position(&self, device_id: DeviceId) -> Option<Offset<Pixels>> {
        self.inner
            .lock()
            .devices
            .get(&device_id)
            .map(|state| state.last_position)
    }

    /// Gets the set of active regions for a device
    pub fn device_active_regions(&self, device_id: DeviceId) -> HashSet<RegionId> {
        self.inner
            .lock()
            .devices
            .get(&device_id)
            .map(|state| state.active_regions.clone())
            .unwrap_or_default()
    }

    /// Gets the current cursor for a device.
    ///
    /// Returns `CursorIcon::Default` if the device is not tracked.
    pub fn device_cursor(&self, device_id: DeviceId) -> CursorIcon {
        self.inner
            .lock()
            .devices
            .get(&device_id)
            .map(|state| state.current_cursor)
            .unwrap_or(CursorIcon::Default)
    }

    /// Sets the callback for cursor changes.
    ///
    /// The callback is invoked whenever the active cursor changes for any device.
    /// Use this to update the platform cursor.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tracker.set_cursor_change_callback(Arc::new(|device_id, cursor| {
    ///     // Update platform cursor
    ///     window.set_cursor(cursor.into());
    /// }));
    /// ```
    pub fn set_cursor_change_callback(&self, callback: CursorChangeCallback) {
        self.inner.lock().cursor_change_callback = Some(callback);
    }

    /// Clears the cursor change callback.
    pub fn clear_cursor_change_callback(&self) {
        self.inner.lock().cursor_change_callback = None;
    }

    /// Gets the current cursor for the primary mouse device (device 0).
    ///
    /// This is a convenience method for single-mouse scenarios.
    #[inline]
    pub fn current_cursor(&self) -> CursorIcon {
        self.device_cursor(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::PointerType;
    use crate::routing::HitTestResult;
    use flui_foundation::RenderId;

    #[test]
    fn test_mouse_tracker_creation() {
        let tracker = MouseTracker::global();
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_register_annotation() {
        let tracker = MouseTracker::new(); // Create local instance for testing
        let annotation = MouseTrackerAnnotation::new(RenderId::new(1));

        tracker.register_annotation(annotation);

        // Annotation is now registered (verified by not panicking)
    }

    #[test]
    fn test_mouse_added_event() {
        let tracker = MouseTracker::new();
        let event = InputEvent::DeviceAdded {
            device_id: 0,
            pointer_type: PointerType::Mouse,
        };
        let hit_result = HitTestResult::new();

        tracker.update_with_event(&event, &hit_result);

        assert!(tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_removed_event() {
        let tracker = MouseTracker::new();

        // Add device
        let add_event = InputEvent::DeviceAdded {
            device_id: 0,
            pointer_type: PointerType::Mouse,
        };
        tracker.update_with_event(&add_event, &HitTestResult::new());

        // Remove device
        let remove_event = InputEvent::DeviceRemoved { device_id: 0 };
        tracker.update_with_event(&remove_event, &HitTestResult::new());

        assert!(!tracker.mouse_is_connected());
    }

    // Note: test_mouse_position_tracking and test_enter_exit_callbacks
    // require creating ui_events::PointerEvent which needs more setup.
    // These tests are simplified for now.

    #[test]
    fn test_device_cursor() {
        let tracker = MouseTracker::new();

        // Add device
        let add_event = InputEvent::DeviceAdded {
            device_id: 0,
            pointer_type: PointerType::Mouse,
        };
        tracker.update_with_event(&add_event, &HitTestResult::new());

        // Default cursor should be Default
        assert_eq!(tracker.device_cursor(0), CursorIcon::Default);
    }

    #[test]
    fn test_current_cursor() {
        let tracker = MouseTracker::new();

        // Add primary device
        let add_event = InputEvent::DeviceAdded {
            device_id: 0,
            pointer_type: PointerType::Mouse,
        };
        tracker.update_with_event(&add_event, &HitTestResult::new());

        // Current cursor (device 0) should be Default
        assert_eq!(tracker.current_cursor(), CursorIcon::Default);
    }
}
