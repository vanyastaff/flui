//! Mouse tracking for hover, enter, and exit events.
//!
//! The tracker owns per-device hover state. Executable region callbacks do not
//! live in render objects or hit-test entries: hit testing contributes a
//! data-only [`MouseRegionTarget`], and the tracker resolves it through the
//! active owner-local [`InteractionLane`](super::InteractionLane).

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use flui_types::geometry::{Offset, Pixels};
use smallvec::SmallVec;

pub use super::interaction_lane::{
    MouseEnterCallback, MouseExitCallback, MouseHoverCallback, MouseRegionTarget,
};
use super::{HitTestResult, active_dispatch_handle};
use crate::{
    events::{CursorIcon, PointerEvent, PointerEventExt, PointerType},
    ids::RegionId,
    routing::interaction_lane::MouseRegionCell,
};

/// Device ID type (re-exported from events).
pub use crate::events::DeviceId;

/// How a pointer move participates in the mouse-region protocol.
///
/// Both variants refresh enter/exit/cursor state from a fresh hit test.
/// Only `Hover` invokes `MouseRegion::on_hover`; contact motion continues to
/// the gesture route captured at Down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerMotionKind {
    /// Motion without an active Down sequence.
    Hover,
    /// Motion inside an active Down-to-Up sequence.
    Contact,
}

/// Data-plane annotation for a mouse-sensitive render region.
///
/// The annotation is safe to store in hit-test results because it carries only
/// opaque identity. The executable callbacks are retained by the interaction
/// lane and are resolved only while the owner lane is active.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseTrackerAnnotation {
    /// Unique render-region ID for diffing previous and current hover state.
    pub region_id: RegionId,
    /// Owner-local callback target for this region.
    pub target: MouseRegionTarget,
}

impl std::fmt::Debug for MouseTrackerAnnotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseTrackerAnnotation")
            .field("region_id", &self.region_id)
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

impl MouseTrackerAnnotation {
    /// Creates a data-only annotation for `region_id`.
    #[must_use]
    pub const fn new(region_id: RegionId, target: MouseRegionTarget) -> Self {
        Self { region_id, target }
    }
}

#[derive(Clone)]
struct ResolvedMouseTrackerAnnotation {
    cell: Rc<MouseRegionCell>,
}

/// State for a single mouse device.
#[derive(Debug, Clone)]
struct DeviceState {
    /// Device class used by the `mouse_is_connected` query.
    pointer_type: PointerType,
    /// Last known position.
    last_position: Offset<Pixels>,
    /// Set of regions currently under this device.
    active_regions: HashSet<RegionId>,
    /// Hit-test order of regions currently under this device.
    active_order: Vec<RegionId>,
    /// Current mouse cursor for this device.
    current_cursor: CursorIcon,
}

impl DeviceState {
    fn new(pointer_type: PointerType, position: Offset<Pixels>) -> Self {
        Self {
            pointer_type,
            last_position: position,
            active_regions: HashSet::new(),
            active_order: Vec::new(),
            current_cursor: CursorIcon::Default,
        }
    }
}

/// Owner-local mouse tracker.
///
/// The tracker is intentionally `!Send + !Sync` under ADR-0027: it invokes
/// owner-plane callbacks and stores `Rc` handles into the interaction lane.
#[derive(Clone)]
pub struct MouseTracker {
    inner: Rc<RefCell<MouseTrackerInner>>,
}

impl std::fmt::Debug for MouseTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseTracker").finish_non_exhaustive()
    }
}

/// Callback for cursor changes.
pub type CursorChangeCallback = Rc<dyn Fn(DeviceId, CursorIcon) + 'static>;

struct MouseTrackerInner {
    /// State for each mouse device.
    devices: HashMap<DeviceId, DeviceState>,
    /// Last resolved annotations by region.
    ///
    /// Entries stay here until their exit callback has been collected. This is
    /// the FLUI equivalent of Flutter replacing `_MouseState.annotations` only
    /// after the previous map is available to `_handleDeviceUpdateMouseEvents`.
    annotations: HashMap<RegionId, ResolvedMouseTrackerAnnotation>,
    /// Whether any mouse is connected.
    mouse_connected: bool,
    /// Callback for cursor changes.
    cursor_change_callback: Option<CursorChangeCallback>,
}

impl MouseTracker {
    /// Creates a new owner-local mouse tracker.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(MouseTrackerInner {
                devices: HashMap::new(),
                annotations: HashMap::new(),
                mouse_connected: false,
                cursor_change_callback: None,
            })),
        }
    }

    /// Registers a mouse region annotation.
    ///
    /// Production normally resolves annotations from hit-test results. This
    /// method exists for lower-level tests and embedders that manually manage
    /// annotations; it succeeds only while the matching interaction lane is
    /// active.
    pub fn register_annotation(&self, annotation: MouseTrackerAnnotation) {
        if let Some(resolved) = resolve_annotation(annotation) {
            self.inner
                .borrow_mut()
                .annotations
                .insert(annotation.region_id, resolved);
        }
    }

    /// Unregisters a mouse region annotation and removes it from active device
    /// state.
    pub fn unregister_annotation(&self, region_id: RegionId) {
        let mut inner = self.inner.borrow_mut();
        inner.annotations.remove(&region_id);

        for state in inner.devices.values_mut() {
            state.active_regions.remove(&region_id);
            state.active_order.retain(|id| *id != region_id);
        }
    }

    /// Registers a pointing device with an optional initial position.
    pub fn add_device(
        &self,
        device_id: DeviceId,
        pointer_type: PointerType,
        position: Offset<Pixels>,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner
            .devices
            .entry(device_id)
            .or_insert_with(|| DeviceState::new(pointer_type, position));
        inner.mouse_connected = inner
            .devices
            .values()
            .any(|state| state.pointer_type == PointerType::Mouse);
    }

    /// Removes a pointing device and all hover state associated with it.
    pub fn remove_device(&self, device_id: DeviceId) {
        let mut inner = self.inner.borrow_mut();
        inner.devices.remove(&device_id);
        inner.mouse_connected = inner
            .devices
            .values()
            .any(|state| state.pointer_type == PointerType::Mouse);
    }

    /// Updates tracking state from one freshly hit-tested pointer move.
    pub fn update_with_motion(
        &self,
        event: &PointerEvent,
        kind: PointerMotionKind,
        hit_test_result: &HitTestResult,
    ) {
        if !matches!(event, PointerEvent::Move(_)) {
            return;
        }
        let Some(pointer_type) = event.pointer_type() else {
            return;
        };
        if !matches!(pointer_type, PointerType::Mouse | PointerType::Pen) {
            return;
        }
        let device_id = event.device_id();
        let position = event.position();

        let resolved = resolve_hit_test_annotations(hit_test_result);
        let new_regions: HashSet<RegionId> = resolved.order.iter().copied().collect();
        let new_cursor = hit_test_result.resolve_cursor();

        let work = {
            let mut inner = self.inner.borrow_mut();
            for (region_id, annotation) in resolved.annotations {
                inner.annotations.insert(region_id, annotation);
            }
            if pointer_type == PointerType::Mouse {
                inner.mouse_connected = true;
            }

            let state = inner
                .devices
                .entry(device_id)
                .or_insert_with(|| DeviceState::new(pointer_type, position));
            state.pointer_type = pointer_type;

            let entered: SmallVec<[RegionId; 4]> = resolved
                .order
                .iter()
                .rev()
                .filter(|id| !state.active_regions.contains(id))
                .copied()
                .collect();
            let exited: SmallVec<[RegionId; 4]> = state
                .active_order
                .iter()
                .filter(|id| !new_regions.contains(id))
                .copied()
                .collect();

            let cursor_changed = state.current_cursor != new_cursor;
            state.last_position = position;
            state.active_regions = new_regions;
            state.current_cursor = new_cursor;

            let enter_callbacks: SmallVec<[MouseEnterCallback; 4]> = entered
                .iter()
                .filter_map(|id| {
                    inner
                        .annotations
                        .get(id)
                        .and_then(|ann| ann.cell.snapshot().on_enter)
                })
                .collect();
            let exit_callbacks: SmallVec<[MouseExitCallback; 4]> = exited
                .iter()
                .filter_map(|id| {
                    inner
                        .annotations
                        .get(id)
                        .and_then(|ann| ann.cell.snapshot().on_exit)
                })
                .collect();
            let hover_callbacks: SmallVec<[MouseHoverCallback; 4]> =
                if kind == PointerMotionKind::Hover {
                    resolved
                        .order
                        .iter()
                        .filter_map(|id| {
                            inner
                                .annotations
                                .get(id)
                                .and_then(|ann| ann.cell.snapshot().on_hover)
                        })
                        .collect()
                } else {
                    SmallVec::new()
                };
            for id in exited {
                inner.annotations.remove(&id);
            }
            let cursor_callback = cursor_changed
                .then(|| inner.cursor_change_callback.clone())
                .flatten();
            inner
                .devices
                .get_mut(&device_id)
                .expect("BUG: mouse device was inserted earlier in this transaction")
                .active_order = resolved.order;

            DeviceWork {
                device_id,
                position,
                enter_callbacks,
                exit_callbacks,
                hover_callbacks,
                cursor_callback,
                new_cursor,
            }
        };

        work.invoke();
    }

    /// Re-runs hit testing for every tracked mouse device at its last known
    /// position and emits enter / exit / cursor-change callbacks for any
    /// region transitions.
    ///
    /// Hover callbacks are intentionally not emitted here because no pointer
    /// motion occurred; only structural enter/exit/cursor changes are valid.
    pub fn update_all_devices<F>(&self, hit_test_fn: F)
    where
        F: Fn(Offset<Pixels>) -> HitTestResult,
    {
        let device_positions: Vec<(DeviceId, Offset<Pixels>)> = self
            .inner
            .borrow()
            .devices
            .iter()
            .map(|(id, state)| (*id, state.last_position))
            .collect();

        let mut pending = Vec::with_capacity(device_positions.len());
        for (device_id, position) in device_positions {
            let result = hit_test_fn(position);
            let resolved = resolve_hit_test_annotations(&result);
            let new_regions: HashSet<RegionId> = resolved.order.iter().copied().collect();
            let new_cursor = result.resolve_cursor();

            let work = {
                let mut inner = self.inner.borrow_mut();
                for (region_id, annotation) in resolved.annotations {
                    inner.annotations.insert(region_id, annotation);
                }

                let Some(state) = inner.devices.get_mut(&device_id) else {
                    continue;
                };

                let entered: SmallVec<[RegionId; 4]> = resolved
                    .order
                    .iter()
                    .rev()
                    .filter(|id| !state.active_regions.contains(id))
                    .copied()
                    .collect();
                let exited: SmallVec<[RegionId; 4]> = state
                    .active_order
                    .iter()
                    .filter(|id| !new_regions.contains(id))
                    .copied()
                    .collect();

                let cursor_changed = state.current_cursor != new_cursor;
                state.active_regions = new_regions;
                state.active_order = resolved.order;
                state.current_cursor = new_cursor;

                let enter_callbacks: SmallVec<[MouseEnterCallback; 4]> = entered
                    .iter()
                    .filter_map(|id| {
                        inner
                            .annotations
                            .get(id)
                            .and_then(|ann| ann.cell.snapshot().on_enter)
                    })
                    .collect();
                let exit_callbacks: SmallVec<[MouseExitCallback; 4]> = exited
                    .iter()
                    .filter_map(|id| {
                        inner
                            .annotations
                            .get(id)
                            .and_then(|ann| ann.cell.snapshot().on_exit)
                    })
                    .collect();
                for id in exited {
                    inner.annotations.remove(&id);
                }
                let cursor_callback = cursor_changed
                    .then(|| inner.cursor_change_callback.clone())
                    .flatten();

                DeviceWork {
                    device_id,
                    position,
                    enter_callbacks,
                    exit_callbacks,
                    hover_callbacks: SmallVec::new(),
                    cursor_callback,
                    new_cursor,
                }
            };
            pending.push(work);
        }

        for work in pending {
            work.invoke();
        }
    }

    /// Checks if any mouse is currently connected.
    #[inline]
    #[must_use]
    pub fn mouse_is_connected(&self) -> bool {
        self.inner.borrow().mouse_connected
    }

    /// Gets the last known position for a device.
    #[must_use]
    pub fn device_position(&self, device_id: DeviceId) -> Option<Offset<Pixels>> {
        self.inner
            .borrow()
            .devices
            .get(&device_id)
            .map(|state| state.last_position)
    }

    /// Gets the set of active regions for a device.
    #[must_use]
    pub fn device_active_regions(&self, device_id: DeviceId) -> HashSet<RegionId> {
        self.inner
            .borrow()
            .devices
            .get(&device_id)
            .map(|state| state.active_regions.clone())
            .unwrap_or_default()
    }

    /// Gets the current cursor for a device.
    #[must_use]
    pub fn device_cursor(&self, device_id: DeviceId) -> CursorIcon {
        self.inner
            .borrow()
            .devices
            .get(&device_id)
            .map_or(CursorIcon::Default, |state| state.current_cursor)
    }

    /// Sets the callback for cursor changes.
    pub fn set_cursor_change_callback(&self, callback: CursorChangeCallback) {
        self.inner.borrow_mut().cursor_change_callback = Some(callback);
    }

    /// Clears the cursor change callback.
    pub fn clear_cursor_change_callback(&self) {
        self.inner.borrow_mut().cursor_change_callback = None;
    }

    /// Gets the current cursor for the primary mouse device (device 0).
    #[inline]
    #[must_use]
    pub fn current_cursor(&self) -> CursorIcon {
        self.device_cursor(0)
    }
}

impl Default for MouseTracker {
    fn default() -> Self {
        Self::new()
    }
}

struct ResolvedHitAnnotations {
    order: Vec<RegionId>,
    annotations: HashMap<RegionId, ResolvedMouseTrackerAnnotation>,
}

fn resolve_hit_test_annotations(result: &HitTestResult) -> ResolvedHitAnnotations {
    let mut order = Vec::new();
    let mut annotations = HashMap::new();
    let Some(handle) = active_dispatch_handle().ok() else {
        return ResolvedHitAnnotations { order, annotations };
    };

    for entry in result.iter() {
        let Some(annotation) = entry.mouse_annotation else {
            continue;
        };
        let Some(resolved) = resolve_annotation_with_handle(&handle, annotation) else {
            continue;
        };
        if annotations.insert(annotation.region_id, resolved).is_none() {
            order.push(annotation.region_id);
        }
    }

    ResolvedHitAnnotations { order, annotations }
}

fn resolve_annotation(
    annotation: MouseTrackerAnnotation,
) -> Option<ResolvedMouseTrackerAnnotation> {
    let handle = active_dispatch_handle().ok()?;
    resolve_annotation_with_handle(&handle, annotation)
}

fn resolve_annotation_with_handle(
    handle: &super::InteractionDispatchHandle,
    annotation: MouseTrackerAnnotation,
) -> Option<ResolvedMouseTrackerAnnotation> {
    match handle.resolve_mouse_region(annotation.target) {
        Ok(cell) => Some(ResolvedMouseTrackerAnnotation { cell }),
        Err(error) => {
            tracing::debug!(
                ?error,
                "mouse tracker skipped an annotation whose owner-local target could not be resolved"
            );
            None
        }
    }
}

struct DeviceWork {
    device_id: DeviceId,
    position: Offset<Pixels>,
    enter_callbacks: SmallVec<[MouseEnterCallback; 4]>,
    exit_callbacks: SmallVec<[MouseExitCallback; 4]>,
    hover_callbacks: SmallVec<[MouseHoverCallback; 4]>,
    cursor_callback: Option<CursorChangeCallback>,
    new_cursor: CursorIcon,
}

impl DeviceWork {
    fn invoke(self) {
        let mut first_panic = None;
        for callback in self.exit_callbacks {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                callback(self.device_id, self.position);
            }));
            if let Err(payload) = delivered {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    tracing::error!(
                        "mouse exit callback panicked after an earlier mouse callback already \
                         panicked; only the first panic is resumed"
                    );
                }
            }
        }
        for callback in self.enter_callbacks {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                callback(self.device_id, self.position);
            }));
            if let Err(payload) = delivered {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    tracing::error!(
                        "mouse enter callback panicked after an earlier mouse callback already \
                         panicked; only the first panic is resumed"
                    );
                }
            }
        }
        if let Some(callback) = self.cursor_callback {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                callback(self.device_id, self.new_cursor);
            }));
            if let Err(payload) = delivered {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    tracing::error!(
                        "mouse cursor callback panicked after an earlier mouse callback already \
                         panicked; only the first panic is resumed"
                    );
                }
            }
        }
        for callback in self.hover_callbacks {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                callback(self.device_id, self.position);
            }));
            if let Err(payload) = delivered {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                } else {
                    tracing::error!(
                        "mouse hover callback panicked after an earlier mouse callback already \
                         panicked; only the first panic is resumed"
                    );
                }
            }
        }

        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic;

    use flui_foundation::RenderId;
    use flui_types::Offset;

    use super::*;
    use crate::{
        events::{PointerType, make_move_event},
        routing::{HitTestEntry, HitTestResult, InteractionLane, MouseRegionCallbacks},
    };

    fn add_primary_mouse(tracker: &MouseTracker) {
        tracker.add_device(0, PointerType::Mouse, Offset::ZERO);
    }

    #[test]
    fn mouse_tracker_creation() {
        let tracker = MouseTracker::new();
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn mouse_added_event() {
        let tracker = MouseTracker::new();
        add_primary_mouse(&tracker);
        assert!(tracker.mouse_is_connected());
    }

    #[test]
    fn mouse_removed_event() {
        let tracker = MouseTracker::new();
        add_primary_mouse(&tracker);
        tracker.remove_device(0);

        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn device_cursor_defaults_to_default() {
        let tracker = MouseTracker::new();
        add_primary_mouse(&tracker);

        assert_eq!(tracker.device_cursor(0), CursorIcon::Default);
        assert_eq!(tracker.current_cursor(), CursorIcon::Default);
    }

    #[test]
    fn enter_exit_callbacks_are_owner_local_and_exit_uses_previous_annotation() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let tracker = MouseTracker::new();
        let enters = Rc::new(Cell::new(0));
        let exits = Rc::new(Cell::new(0));

        let target = lane.enter(|| {
            let enter_count = Rc::clone(&enters);
            let exit_count = Rc::clone(&exits);
            handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_enter: Some(Rc::new(move |_device, _position| {
                        enter_count.set(enter_count.get() + 1);
                    })),
                    on_exit: Some(Rc::new(move |_device, _position| {
                        exit_count.set(exit_count.get() + 1);
                    })),
                    on_hover: None,
                })
                .expect("register mouse region")
        });

        let region_id = RenderId::new(1);
        let inside_position = Offset::new(Pixels(10.0), Pixels(10.0));
        let inside_event = make_move_event(inside_position, PointerType::Mouse);
        let mut inside = HitTestResult::new();
        inside.add(
            HitTestEntry::new(region_id)
                .mouse_annotation(MouseTrackerAnnotation::new(region_id, target)),
        );

        add_primary_mouse(&tracker);
        lane.enter(|| {
            tracker.update_with_motion(&inside_event, PointerMotionKind::Hover, &inside);
        });
        assert_eq!(enters.get(), 1);
        assert_eq!(exits.get(), 0);

        lane.enter(|| {
            handle
                .unregister_mouse_region(target)
                .expect("unregister target");
        });
        let outside_position = Offset::new(Pixels(80.0), Pixels(10.0));
        let outside_event = make_move_event(outside_position, PointerType::Mouse);
        lane.enter(|| {
            tracker.update_with_motion(
                &outside_event,
                PointerMotionKind::Hover,
                &HitTestResult::new(),
            );
        });

        assert_eq!(enters.get(), 1);
        assert_eq!(
            exits.get(),
            1,
            "the tracker must retain the resolved previous annotation long enough to emit exit"
        );
    }

    #[test]
    fn active_regions_are_derived_only_from_mouse_annotations() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let tracker = MouseTracker::new();
        let target = lane.enter(|| {
            handle
                .register_mouse_region(MouseRegionCallbacks::default())
                .expect("register mouse region")
        });
        let region_id = RenderId::new(1);
        let ordinary_id = RenderId::new(2);
        let position = Offset::new(Pixels(10.0), Pixels(10.0));
        let event = make_move_event(position, PointerType::Mouse);
        let mut result = HitTestResult::new();
        result.add(
            HitTestEntry::new(region_id)
                .mouse_annotation(MouseTrackerAnnotation::new(region_id, target)),
        );
        result.add(HitTestEntry::new(ordinary_id));

        add_primary_mouse(&tracker);
        lane.enter(|| {
            tracker.update_with_motion(&event, PointerMotionKind::Hover, &result);
        });

        let active = tracker.device_active_regions(0);
        assert!(active.contains(&region_id));
        assert!(!active.contains(&ordinary_id));
    }

    #[test]
    fn hover_and_contact_share_tracking_but_only_hover_invokes_on_hover() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let tracker = MouseTracker::new();
        let hovers = Rc::new(Cell::new(0));
        let callback_hovers = Rc::clone(&hovers);
        let target = lane.enter(|| {
            handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_hover: Some(Rc::new(move |_device, _position| {
                        callback_hovers.set(callback_hovers.get() + 1);
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("register mouse region")
        });
        let region_id = RenderId::new(1);
        let position = Offset::new(Pixels(10.0), Pixels(10.0));
        let event = make_move_event(position, PointerType::Mouse);
        let mut result = HitTestResult::new();
        result.add(
            HitTestEntry::new(region_id)
                .mouse_annotation(MouseTrackerAnnotation::new(region_id, target)),
        );

        lane.enter(|| {
            tracker.update_with_motion(&event, PointerMotionKind::Hover, &result);
            tracker.update_with_motion(&event, PointerMotionKind::Contact, &result);
        });

        assert_eq!(hovers.get(), 1);
        assert!(tracker.device_active_regions(0).contains(&region_id));
    }

    #[test]
    fn mouse_callback_panic_continues_later_callbacks_then_resumes_first_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let tracker = MouseTracker::new();
        let later_exit = Rc::new(Cell::new(0));

        let (panicking_target, later_target) = lane.enter(|| {
            let panicking_target = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_exit: Some(Rc::new(|_device, _position| panic!("first exit panic"))),
                    ..MouseRegionCallbacks::default()
                })
                .expect("register panicking region");
            let later_counter = Rc::clone(&later_exit);
            let later_target = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_exit: Some(Rc::new(move |_device, _position| {
                        later_counter.set(later_counter.get() + 1);
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("register later region");
            (panicking_target, later_target)
        });

        add_primary_mouse(&tracker);

        let position = Offset::new(Pixels(10.0), Pixels(10.0));
        let event = make_move_event(position, PointerType::Mouse);
        let first_id = RenderId::new(1);
        let second_id = RenderId::new(2);
        let mut inside = HitTestResult::new();
        inside.add(
            HitTestEntry::new(first_id)
                .mouse_annotation(MouseTrackerAnnotation::new(first_id, panicking_target)),
        );
        inside.add(
            HitTestEntry::new(second_id)
                .mouse_annotation(MouseTrackerAnnotation::new(second_id, later_target)),
        );
        lane.enter(|| {
            tracker.update_with_motion(&event, PointerMotionKind::Hover, &inside);
        });

        let outside = HitTestResult::new();
        let panic = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            lane.enter(|| {
                tracker.update_with_motion(&event, PointerMotionKind::Hover, &outside);
            });
        }));

        assert!(panic.is_err(), "the first mouse callback panic must resume");
        assert_eq!(
            later_exit.get(),
            1,
            "a later mouse callback must still run before the first panic resumes"
        );
    }
}
