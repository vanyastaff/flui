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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use flui_types::events::PointerEvent;
use flui_types::geometry::Offset;

use crate::hit_test::{ElementId, HitTestResult};

/// Unique identifier for a mouse device
pub type DeviceId = i32;

/// Unique identifier for a mouse region (Element ID)
pub type RegionId = ElementId;

/// Callback for mouse enter events
pub type MouseEnterCallback = Arc<dyn Fn(DeviceId, Offset) + Send + Sync>;

/// Callback for mouse exit events
pub type MouseExitCallback = Arc<dyn Fn(DeviceId, Offset) + Send + Sync>;

/// Callback for mouse hover events
pub type MouseHoverCallback = Arc<dyn Fn(DeviceId, Offset) + Send + Sync>;

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
        F: Fn(DeviceId, Offset) + Send + Sync + 'static,
    {
        self.on_enter = Some(Arc::new(callback));
        self
    }

    /// Sets the exit callback
    pub fn with_exit<F>(mut self, callback: F) -> Self
    where
        F: Fn(DeviceId, Offset) + Send + Sync + 'static,
    {
        self.on_exit = Some(Arc::new(callback));
        self
    }

    /// Sets the hover callback
    pub fn with_hover<F>(mut self, callback: F) -> Self
    where
        F: Fn(DeviceId, Offset) + Send + Sync + 'static,
    {
        self.on_hover = Some(Arc::new(callback));
        self
    }
}

/// State for a single mouse device
#[derive(Debug, Clone)]
struct DeviceState {
    /// Last known position
    last_position: Offset,
    /// Set of regions currently under this device
    active_regions: HashSet<RegionId>,
}

/// Global mouse tracker
///
/// Tracks all mouse devices and their relationships with UI regions.
/// Generates enter/exit/hover events as the mouse moves.
#[derive(Clone)]
pub struct MouseTracker {
    inner: Arc<Mutex<MouseTrackerInner>>,
}

struct MouseTrackerInner {
    /// State for each mouse device
    devices: HashMap<DeviceId, DeviceState>,
    /// Registered annotations (regions)
    annotations: HashMap<RegionId, MouseTrackerAnnotation>,
    /// Whether any mouse is connected
    mouse_connected: bool,
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

    /// Updates tracking state based on a pointer event
    ///
    /// This should be called whenever a mouse move/hover event occurs.
    ///
    /// # Arguments
    ///
    /// * `event` - The pointer event (typically Move or Hover)
    /// * `hit_test_result` - Result of hit testing at the event position
    pub fn update_with_event(&self, event: &PointerEvent, hit_test_result: &HitTestResult) {
        let mut inner = self.inner.lock();

        let (device_id, position) = match event {
            PointerEvent::Move(data) | PointerEvent::Hover(data) => (data.device, data.position),
            PointerEvent::Added { device, .. } => {
                inner.mouse_connected = true;
                // Initialize device state
                inner.devices.insert(
                    *device,
                    DeviceState {
                        last_position: Offset::ZERO,
                        active_regions: HashSet::new(),
                    },
                );
                return;
            }
            PointerEvent::Removed { device } => {
                inner.devices.remove(device);
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
            });

        // Build new set of active regions from hit test
        let new_regions: HashSet<RegionId> = hit_test_result
            .entries()
            .iter()
            .map(|entry| entry.element_id)
            .collect();

        // Find regions that were entered (new but not in old)
        let entered: Vec<RegionId> = new_regions
            .difference(&state.active_regions)
            .copied()
            .collect();

        // Find regions that were exited (old but not in new)
        let exited: Vec<RegionId> = state
            .active_regions
            .difference(&new_regions)
            .copied()
            .collect();

        // Find regions that are still active (for hover events)
        let hovering: Vec<RegionId> = new_regions
            .intersection(&state.active_regions)
            .copied()
            .collect();

        // Update state
        state.last_position = position;
        state.active_regions = new_regions;

        // Trigger callbacks (must be done outside the lock to avoid deadlock)
        let enter_callbacks: Vec<_> = entered
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_enter.clone())
            })
            .collect();

        let exit_callbacks: Vec<_> = exited
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_exit.clone())
            })
            .collect();

        let hover_callbacks: Vec<_> = hovering
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(|ann| ann.on_hover.clone())
            })
            .collect();

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
    pub fn mouse_is_connected(&self) -> bool {
        self.inner.lock().mouse_connected
    }

    /// Gets the last known position for a device
    pub fn device_position(&self, device_id: DeviceId) -> Option<Offset> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_test::{HitTestEntry, HitTestResult};

    #[test]
    fn test_mouse_tracker_creation() {
        let tracker = MouseTracker::global();
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_register_annotation() {
        let tracker = MouseTracker::new(); // Create local instance for testing
        let annotation = MouseTrackerAnnotation::new(ElementId::new(1));

        tracker.register_annotation(annotation);

        // Annotation is now registered (verified by not panicking)
    }

    #[test]
    fn test_mouse_added_event() {
        let tracker = MouseTracker::new();
        let event = PointerEvent::Added {
            device: 0,
            device_kind: flui_types::events::PointerDeviceKind::Mouse,
        };
        let hit_result = HitTestResult::new();

        tracker.update_with_event(&event, &hit_result);

        assert!(tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_removed_event() {
        let tracker = MouseTracker::new();

        // Add device
        let add_event = PointerEvent::Added {
            device: 0,
            device_kind: flui_types::events::PointerDeviceKind::Mouse,
        };
        tracker.update_with_event(&add_event, &HitTestResult::new());

        // Remove device
        let remove_event = PointerEvent::Removed { device: 0 };
        tracker.update_with_event(&remove_event, &HitTestResult::new());

        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_position_tracking() {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let tracker = MouseTracker::new();

        // Add device
        tracker.update_with_event(
            &PointerEvent::Added {
                device: 0,
                device_kind: PointerDeviceKind::Mouse,
            },
            &HitTestResult::new(),
        );

        // Move mouse
        let position = Offset::new(100.0, 200.0);
        let mut data = PointerEventData::new(position, PointerDeviceKind::Mouse);
        data.device = 0;
        tracker.update_with_event(&PointerEvent::Hover(data), &HitTestResult::new());

        assert_eq!(tracker.device_position(0), Some(position));
    }

    #[test]
    fn test_enter_exit_callbacks() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let tracker = MouseTracker::new();
        let entered = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));

        let entered_clone = entered.clone();
        let exited_clone = exited.clone();

        let annotation = MouseTrackerAnnotation::new(ElementId::new(1))
            .with_enter(move |_, _| {
                entered_clone.store(true, Ordering::Relaxed);
            })
            .with_exit(move |_, _| {
                exited_clone.store(true, Ordering::Relaxed);
            });

        use flui_types::events::{PointerDeviceKind, PointerEventData};
        use flui_types::geometry::Rect;

        tracker.register_annotation(annotation);

        // Add device
        tracker.update_with_event(
            &PointerEvent::Added {
                device: 0,
                device_kind: PointerDeviceKind::Mouse,
            },
            &HitTestResult::new(),
        );

        // Move into region (simulate hit test finding region 1)
        let mut hit_result = HitTestResult::new();
        hit_result.add(HitTestEntry::new(
            ElementId::new(1), // element_id
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        ));

        let mut data = PointerEventData::new(Offset::new(10.0, 10.0), PointerDeviceKind::Mouse);
        data.device = 0;
        tracker.update_with_event(&PointerEvent::Hover(data.clone()), &hit_result);

        assert!(entered.load(Ordering::Relaxed));

        // Move out of region
        let empty_result = HitTestResult::new();
        let mut data2 =
            PointerEventData::new(Offset::new(1000.0, 1000.0), PointerDeviceKind::Mouse);
        data2.device = 0;
        tracker.update_with_event(&PointerEvent::Hover(data2), &empty_result);

        assert!(exited.load(Ordering::Relaxed));
    }
}
