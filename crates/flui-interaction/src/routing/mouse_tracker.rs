//! Mouse tracking for hover, enter, and exit events.
//!
//! The tracker owns per-device enter/exit/cursor state, gated to
//! `Mouse | Pen`. `MouseRegion::on_hover` is deliberately
//! **not** part of that device state machine, and has no device-kind gate:
//! it fires for any hover-shaped move reaching a hit-test target.
//! Executable region callbacks do not live in render objects or hit-test
//! entries either way: hit testing contributes a data-only
//! [`MouseRegionTarget`], resolved through the active owner-local
//! [`InteractionLane`](super::InteractionLane).
//!
//! Each device keeps its own hover state. One transition delivers every exit
//! before any enter: exits innermost-first, enters outermost-first, then the
//! cursor change. A region hovered by several devices gets an enter and an
//! exit per device. A stationary device is re-hit-tested after layout
//! ([`MouseTracker::update_all_devices`]), so content moving under a still
//! cursor enters and exits exactly as motion would.
//!
//! [`MouseTracker::dispatch_hover`] is the self-contained, directly testable
//! form of that resolve-and-invoke step and remains public for exactly that.
//! Production `GestureBinding` delivery does NOT call it: a `Listener` and a
//! nested `MouseRegion` must fire in hit-test order relative to each other
//! (one per-entry loop over the leaf-first hit path), so the binding instead
//! resolves and invokes
//! pointer targets and mouse-hover regions together in one per-entry walk
//! (`InteractionDispatchHandle::dispatch_hover_interleaved`,
//! `routing/interaction_lane.rs`) — calling `dispatch_hover` as its own
//! separate full pass would deliver every ordinary pointer target before
//! every region regardless of which is actually the hit path's leaf. Both
//! paths apply the identical gate (a buttons-empty `Move`) and read the same
//! lane state; they exist because Rust's ownership boundary makes "resolve
//! once, deliver in one interleaved order" and "a small function that just
//! does hover" different call shapes, not because the underlying contract
//! differs.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use smallvec::SmallVec;

pub use super::interaction_lane::{
    MouseEnterCallback, MouseExitCallback, MouseHoverCallback, MouseRegionTarget,
};
use super::{HitTestResult, OwnerLatch, RoutePanic, active_dispatch_handle};
use crate::{
    events::{CursorIcon, PointerEvent, PointerEventExt, PointerType},
    ids::RegionId,
    retain::Retain,
    routing::interaction_lane::MouseRegionCell,
};

/// Device ID type (re-exported from events).
pub use crate::events::DeviceId;

/// How a pointer move participates in the mouse-region protocol.
///
/// Both variants refresh enter/exit/cursor state from a fresh hit test.
/// Neither invokes `MouseRegion::on_hover` — see
/// [`MouseTracker::dispatch_hover`] for that, which does not take a `kind`
/// because it derives the hover/contact distinction from the event itself
/// (an empty-buttons `Move`).
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
    /// The region owner's terminal latch, carried into every callback snapshot.
    latch: OwnerLatch,
}

impl ResolvedMouseTrackerAnnotation {
    fn on_enter(&self) -> Option<Latched<MouseEnterCallback>> {
        self.cell
            .snapshot()
            .on_enter
            .map(|callback| (callback, self.latch.clone()))
    }

    fn on_exit(&self) -> Option<Latched<MouseExitCallback>> {
        self.cell
            .snapshot()
            .on_exit
            .map(|callback| (callback, self.latch.clone()))
    }

    fn on_hover(&self) -> Option<Latched<MouseHoverCallback>> {
        self.cell
            .snapshot()
            .on_hover
            .map(|callback| (callback, self.latch.clone()))
    }
}

impl Retain for ResolvedMouseTrackerAnnotation {
    fn retain(self) {
        self.cell.retain();
    }
}

/// A callback snapshot and the terminal latch of the owner that registered it.
type Latched<C> = (C, OwnerLatch);

/// State for a single mouse device.
#[derive(Debug, Clone)]
struct DeviceState {
    /// Device class used by the `mouse_is_connected` query.
    pointer_type: PointerType,
    /// Last known position.
    last_position: Offset<f64>,
    /// Set of regions currently under this device.
    active_regions: HashSet<RegionId>,
    /// Hit-test order of regions currently under this device.
    active_order: Vec<RegionId>,
    /// Current mouse cursor for this device.
    current_cursor: CursorIcon,
    /// Whether the device's pointer is inside the hosting window. Cleared by
    /// the window-leave sweep, restored by any subsequent motion — while
    /// false, ambient re-hit-testing (`update_all_devices`) must skip the
    /// device, or the frame requested right after a sweep would re-enter the
    /// regions at the stale last-known position and undo the sweep.
    inside_window: bool,
}

impl DeviceState {
    fn new(pointer_type: PointerType, position: Offset<f64>) -> Self {
        Self {
            pointer_type,
            last_position: position,
            active_regions: HashSet::new(),
            active_order: Vec::new(),
            current_cursor: CursorIcon::Default,
            inside_window: true,
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
    closed: bool,
    close_mode: crate::__runtime::CloseTombstone,
    /// State for each mouse device.
    devices: HashMap<DeviceId, DeviceState>,
    /// Last resolved annotations by region.
    ///
    /// Shared by every device: an entry stays while any device hovers its
    /// region, so each device's exit still finds the region's callbacks.
    annotations: HashMap<RegionId, ResolvedMouseTrackerAnnotation>,
    /// Whether any mouse is connected.
    mouse_connected: bool,
    /// Callback for cursor changes.
    cursor_change_callback: Option<CursorChangeCallback>,
}

impl MouseTrackerInner {
    /// Removes the cached annotation of each of `regions` that no device
    /// hovers any more, returning them for release outside the borrow.
    fn release_unhovered(
        &mut self,
        regions: impl Iterator<Item = RegionId>,
    ) -> Vec<ResolvedMouseTrackerAnnotation> {
        regions
            .filter(|id| {
                !self
                    .devices
                    .values()
                    .any(|state| state.active_regions.contains(id))
            })
            .filter_map(|id| self.annotations.remove(&id))
            .collect()
    }
}

/// Keeps the first panic of a batch; a later payload is traced and retained,
/// since its own `Drop` may panic and must neither replace the first failure
/// nor abort (ADR-0104).
fn keep_first_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    payload: Box<dyn std::any::Any + Send>,
    kind: &str,
) {
    if first.is_none() {
        *first = Some(payload);
    } else {
        tracing::error!(
            kind,
            "mouse tracker panicked after an earlier panic in the same batch; \
             only the first panic is resumed"
        );
        flui_foundation::panic::retain_opaque_payload(payload);
    }
}

impl MouseTracker {
    /// Creates a new owner-local mouse tracker.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(MouseTrackerInner {
                closed: false,
                close_mode: crate::__runtime::CloseTombstone::default(),
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
        if self.inner.borrow().closed {
            return;
        }
        if let Some(resolved) = resolve_annotation(annotation) {
            // The replaced entry may own the region's last callbacks; their
            // captures are destroyed only after the tracker borrow ends.
            let replaced = self
                .inner
                .borrow_mut()
                .annotations
                .insert(annotation.region_id, resolved);
            drop(replaced);
        }
    }

    /// Unregisters a mouse region annotation and removes it from active device
    /// state.
    pub fn unregister_annotation(&self, region_id: RegionId) {
        let removed = {
            let mut inner = self.inner.borrow_mut();
            let removed = inner.annotations.remove(&region_id);
            for state in inner.devices.values_mut() {
                state.active_regions.remove(&region_id);
                state.active_order.retain(|id| *id != region_id);
            }
            removed
        };
        drop(removed);
    }

    /// Registers a pointing device with an optional initial position.
    pub fn add_device(
        &self,
        device_id: DeviceId,
        pointer_type: PointerType,
        position: Offset<f64>,
    ) {
        let mut inner = self.inner.borrow_mut();
        if inner.closed {
            return;
        }
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
    ///
    /// No exit callback runs. A region the device hovered stays cached while
    /// another device still hovers it; otherwise its cached callbacks are
    /// released after the tracker's own state is committed.
    pub fn remove_device(&self, device_id: DeviceId) {
        let retired = {
            let mut inner = self.inner.borrow_mut();
            let inner = &mut *inner;
            let removed = inner.devices.remove(&device_id);
            inner.mouse_connected = inner
                .devices
                .values()
                .any(|state| state.pointer_type == PointerType::Mouse);
            removed.map_or_else(Vec::new, |state| {
                inner.release_unhovered(state.active_order.iter().copied())
            })
        };
        drop(retired);
    }

    /// Fires exit callbacks for every region every device currently hovers,
    /// clears that hover state, and resets the cursor — the cursor has left
    /// the window, so nothing is hovered any more.
    ///
    /// The winit wire has no synthetic remove event, so the window-leave
    /// signal calls this directly. Without it, a widget hovered at the moment the cursor
    /// crosses the window edge keeps its hover visuals forever and
    /// `MouseRegion::on_exit` never fires.
    ///
    /// Device registrations survive (the pointer will come back); only the
    /// hover/cursor state is swept. Calling with nothing hovered is a no-op.
    /// Per-callback panics are isolated exactly like a motion update's: the
    /// first is resumed after every callback ran.
    pub fn dispatch_window_left(&self) {
        if self.inner.borrow().closed {
            return;
        }
        let mut swept_regions: Vec<RegionId> = Vec::new();
        let mut sweeps: Vec<DeviceWork> = {
            let mut inner = self.inner.borrow_mut();
            let inner = &mut *inner;
            inner
                .devices
                .iter_mut()
                .filter_map(|(&device_id, state)| {
                    if state.active_order.is_empty() && state.current_cursor == CursorIcon::Default
                    {
                        return None;
                    }
                    let exit_callbacks: SmallVec<[Latched<MouseExitCallback>; 4]> = state
                        .active_order
                        .iter()
                        .filter_map(|id| {
                            inner
                                .annotations
                                .get(id)
                                .and_then(ResolvedMouseTrackerAnnotation::on_exit)
                        })
                        .collect();
                    let cursor_callback = (state.current_cursor != CursorIcon::Default)
                        .then(|| inner.cursor_change_callback.clone())
                        .flatten();
                    let position = state.last_position;
                    swept_regions.append(&mut state.active_order);
                    state.active_regions.clear();
                    state.current_cursor = CursorIcon::Default;
                    state.inside_window = false;
                    Some(DeviceWork {
                        tracker: Rc::clone(&self.inner),
                        device_id,
                        position,
                        enter_callbacks: SmallVec::new(),
                        exit_callbacks,
                        cursor_callback,
                        new_cursor: CursorIcon::Default,
                        retired: Vec::new(),
                    })
                })
                .collect()
        };
        // Nothing is hovered any more, so the swept regions' cached
        // callbacks retire with the last device's batch.
        if let Some(last) = sweeps.last_mut() {
            last.retired = self
                .inner
                .borrow_mut()
                .release_unhovered(swept_regions.into_iter());
        }
        // Every device is swept even if an earlier device's callback
        // panics — the first panic resumes only after the loop, the same
        // all-callbacks-run-first posture `DeviceWork::invoke` has within
        // one device.
        let mut first_panic = None;
        for work in sweeps {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| work.invoke())) {
                keep_first_panic(&mut first_panic, payload, "window-leave sweep");
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Updates tracking state from one freshly hit-tested pointer move.
    pub fn update_with_motion(
        &self,
        event: &PointerEvent,
        kind: PointerMotionKind,
        hit_test_result: &HitTestResult,
    ) {
        if self.inner.borrow().closed {
            return;
        }
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
        tracing::trace!(
            device_id,
            ?kind,
            "mouse tracker updating enter/exit/cursor state"
        );

        self.commit_device(device_id, position, Some(pointer_type), hit_test_result)
            .invoke();
    }

    /// Invokes `MouseRegion::on_hover` for every region under a hover-shaped
    /// pointer move, with no device-kind gate.
    ///
    /// `on_hover` fires for any hover-shaped move reaching a hit-test target,
    /// regardless of device kind, whereas enter/exit are gated to
    /// mouse/stylus only. [`update_with_motion`](Self::update_with_motion)
    /// keeps that gate for enter/exit; this method carries the ungated hover
    /// half of the contract, called from the ordinary coalesced-move
    /// dispatch path rather than the enter/exit device-state machine.
    ///
    /// A non-`Move` event, or a `Move` with any button held (a contact drag,
    /// not a hover), is a no-op.
    ///
    /// Per-target panics are isolated the same way the render tree isolates
    /// resolved pointer-route dispatch: the first is returned for the caller
    /// to resume after its own mandatory cleanup, later ones are traced.
    #[must_use = "a captured hover-callback panic must be resumed after dispatch cleanup"]
    pub fn dispatch_hover(
        &self,
        event: &PointerEvent,
        hit_test_result: &HitTestResult,
    ) -> Option<RoutePanic> {
        let PointerEvent::Move(update) = event else {
            return None;
        };
        if !update.current.buttons.is_empty() {
            return None;
        }
        let device_id = event.device_id();
        let position = event.position();

        let resolved = resolve_hit_test_annotations(hit_test_result);
        let hover_callbacks: SmallVec<[Latched<MouseHoverCallback>; 4]> = resolved
            .order
            .iter()
            .filter_map(|id| {
                resolved
                    .annotations
                    .get(id)
                    .and_then(ResolvedMouseTrackerAnnotation::on_hover)
            })
            .collect();

        let mut first_panic = None;
        for (callback, latch) in hover_callbacks {
            // An earlier callback may have closed this region's owner.
            if !latch.is_closed() {
                let delivered = RoutePanic::capture(|| callback(device_id, position));
                RoutePanic::preserve_first(&mut first_panic, delivered, "mouse hover callback");
            }
            let cleanup = RoutePanic::capture(|| latch.release(callback));
            RoutePanic::preserve_first(&mut first_panic, cleanup, "mouse hover snapshot cleanup");
        }
        first_panic
    }

    /// Re-runs hit testing for every tracked mouse device at its last known
    /// position and emits enter / exit / cursor-change callbacks for any
    /// region transitions.
    ///
    /// Hover callbacks are intentionally not emitted here because no pointer
    /// motion occurred; only structural enter/exit/cursor changes are valid.
    ///
    /// Every device is processed even when another device's hit test or
    /// callback panics: a device whose hit test panics keeps its previous
    /// state (the next refresh retries it), every committed device still
    /// receives its callbacks, and the first panic resumes after all devices
    /// ran. Later panics are traced and their payloads retained.
    ///
    /// # Panics
    ///
    /// Resumes the first panic raised by `hit_test_fn` or by a region or
    /// cursor callback.
    pub fn update_all_devices<F>(&self, hit_test_fn: F)
    where
        F: Fn(Offset<f64>) -> HitTestResult,
    {
        let device_positions: Vec<(DeviceId, Offset<f64>)> = self
            .inner
            .borrow()
            .devices
            .iter()
            // A device swept by a window-leave is not re-hit-tested at its
            // stale in-window position — that would re-enter the regions the
            // sweep just exited. Its next real motion re-primes it.
            .filter(|(_, state)| state.inside_window)
            .map(|(id, state)| (*id, state.last_position))
            .collect();

        // Commit every device before any callback runs, so a callback that
        // reenters the tracker observes the whole refresh.
        let mut first_panic = None;
        let mut pending = Vec::with_capacity(device_positions.len());
        for (device_id, position) in device_positions {
            match catch_unwind(AssertUnwindSafe(|| {
                let result = hit_test_fn(position);
                self.commit_device(device_id, position, None, &result)
            })) {
                Ok(work) => pending.push(work),
                Err(payload) => keep_first_panic(&mut first_panic, payload, "ambient hit test"),
            }
        }
        for work in pending {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| work.invoke())) {
                keep_first_panic(&mut first_panic, payload, "ambient mouse refresh");
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Diffs one device's fresh hit test against its committed state, commits
    /// the new state and returns the callbacks the transition owes.
    ///
    /// `motion` carries the pointer type of a real move, which registers the
    /// device if needed and refreshes its position. Without it (an ambient
    /// refresh) a device removed in the meantime produces an empty batch.
    ///
    /// The cached annotation of an exited region is released only when no
    /// other device still hovers it: the cache is shared, and each device's
    /// later exit must still find the region's callbacks. Replaced and
    /// released annotations travel in the returned batch, so their captures
    /// are destroyed after the tracker borrow ends.
    fn commit_device(
        &self,
        device_id: DeviceId,
        position: Offset<f64>,
        motion: Option<PointerType>,
        hit_test_result: &HitTestResult,
    ) -> DeviceWork {
        let resolved = resolve_hit_test_annotations(hit_test_result);
        let new_cursor = hit_test_result.resolve_cursor();
        let mut work = DeviceWork {
            tracker: Rc::clone(&self.inner),
            device_id,
            position,
            enter_callbacks: SmallVec::new(),
            exit_callbacks: SmallVec::new(),
            cursor_callback: None,
            new_cursor,
            retired: Vec::new(),
        };

        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let state = match motion {
            Some(pointer_type) => {
                if pointer_type == PointerType::Mouse {
                    inner.mouse_connected = true;
                }
                let state = inner
                    .devices
                    .entry(device_id)
                    .or_insert_with(|| DeviceState::new(pointer_type, position));
                state.pointer_type = pointer_type;
                state.inside_window = true;
                state.last_position = position;
                state
            }
            None => {
                let Some(state) = inner.devices.get_mut(&device_id) else {
                    work.retired.extend(resolved.annotations.into_values());
                    return work;
                };
                state
            }
        };

        let new_regions: HashSet<RegionId> = resolved.order.iter().copied().collect();
        // Exits run innermost-first (hit order), enters outermost-first.
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

        for (region_id, annotation) in resolved.annotations {
            if let Some(replaced) = inner.annotations.insert(region_id, annotation) {
                work.retired.push(replaced);
            }
        }
        work.enter_callbacks = entered
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(ResolvedMouseTrackerAnnotation::on_enter)
            })
            .collect();
        work.exit_callbacks = exited
            .iter()
            .filter_map(|id| {
                inner
                    .annotations
                    .get(id)
                    .and_then(ResolvedMouseTrackerAnnotation::on_exit)
            })
            .collect();
        let released = inner.release_unhovered(exited.into_iter());
        work.retired.extend(released);
        if cursor_changed {
            work.cursor_callback = inner.cursor_change_callback.clone();
        }
        work
    }

    /// Checks if any mouse is currently connected.
    #[inline]
    #[must_use]
    pub fn mouse_is_connected(&self) -> bool {
        self.inner.borrow().mouse_connected
    }

    /// Gets the last known position for a device.
    #[must_use]
    pub fn device_position(&self, device_id: DeviceId) -> Option<Offset<f64>> {
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
        let (outgoing, mode) = {
            let mut inner = self.inner.borrow_mut();
            let mode = inner.close_mode.mode();
            let outgoing = if inner.closed {
                Some(callback)
            } else {
                inner.cursor_change_callback.replace(callback)
            };
            (outgoing, mode)
        };
        let mut failure = crate::__runtime::ClosePanic::for_rejection(mode);
        failure.retire(outgoing);
        failure.finish();
    }

    /// Clears the cursor change callback.
    pub fn clear_cursor_change_callback(&self) {
        let (outgoing, mode) = {
            let mut inner = self.inner.borrow_mut();
            (inner.cursor_change_callback.take(), inner.close_mode.mode())
        };
        let mut failure = crate::__runtime::ClosePanic::for_rejection(mode);
        failure.retire(outgoing);
        failure.finish();
    }

    pub(crate) fn close_tombstone(&self) -> crate::__runtime::CloseTombstone {
        self.inner.borrow().close_mode.clone()
    }

    pub(crate) fn close_with_mode(&self, mode: crate::__runtime::CloseMode) {
        let (callback, annotations, terminal) = {
            let mut inner = self.inner.borrow_mut();
            let terminal = inner.close_mode.clone();
            // Commit the terminal mode before any callback ownership retires.
            if mode == crate::__runtime::CloseMode::PreservingFailure || std::thread::panicking() {
                terminal.preserve();
            }
            inner.closed = true;
            inner.devices.clear();
            inner.mouse_connected = false;
            (
                inner.cursor_change_callback.take(),
                std::mem::take(&mut inner.annotations),
                terminal,
            )
        };
        let mut failure = crate::__runtime::ClosePanic::for_close(mode, terminal);
        failure.retire(callback);
        for annotation in annotations.into_values() {
            failure.retire(annotation);
        }
        failure.finish();
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
        Ok((cell, latch)) => Some(ResolvedMouseTrackerAnnotation { cell, latch }),
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
    /// The tracker whose cursor callback this batch snapshotted.
    tracker: Rc<RefCell<MouseTrackerInner>>,
    device_id: DeviceId,
    position: Offset<f64>,
    enter_callbacks: SmallVec<[Latched<MouseEnterCallback>; 4]>,
    exit_callbacks: SmallVec<[Latched<MouseExitCallback>; 4]>,
    cursor_callback: Option<CursorChangeCallback>,
    new_cursor: CursorIcon,
    /// Cached annotations this transition replaced or released. They may own
    /// a region's last callbacks, so they retire here, outside the tracker
    /// borrow, where a capture destructor may reenter the tracker.
    retired: Vec<ResolvedMouseTrackerAnnotation>,
}

impl DeviceWork {
    /// Run the batch: exits (innermost first), enters (outermost first), the
    /// cursor change, then the retirement of released annotations.
    ///
    /// A callback may close its presentation reentrantly, so
    /// each snapshot is rechecked against its owner's latch (and the cursor
    /// callback against this tracker) right before it runs, and released
    /// under that close's retention policy afterwards (ADR-0127).
    fn invoke(self) {
        let mut first_panic = None;
        let mut record = |payload, kind: &str| keep_first_panic(&mut first_panic, payload, kind);
        for (callback, latch) in self.exit_callbacks {
            if !latch.is_closed()
                && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                    callback(self.device_id, self.position);
                }))
            {
                record(payload, "exit");
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| latch.release(callback))) {
                record(payload, "exit snapshot cleanup");
            }
        }
        for (callback, latch) in self.enter_callbacks {
            if !latch.is_closed()
                && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                    callback(self.device_id, self.position);
                }))
            {
                record(payload, "enter");
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| latch.release(callback))) {
                record(payload, "enter snapshot cleanup");
            }
        }
        if let Some(callback) = self.cursor_callback {
            let closed = self.tracker.borrow().closed;
            if !closed
                && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                    callback(self.device_id, self.new_cursor);
                }))
            {
                record(payload, "cursor");
            }
            let preserved = {
                let inner = self.tracker.borrow();
                inner.closed && inner.close_mode.preserved()
            };
            if preserved {
                callback.retain();
            } else if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(callback))) {
                record(payload, "cursor snapshot cleanup");
            }
        }
        for annotation in self.retired {
            let latch = annotation.latch.clone();
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| latch.release(annotation))) {
                record(payload, "region retirement");
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
    use flui_foundation::geometry::Offset;

    use super::*;
    use crate::{
        events::{PointerType, make_move_event},
        routing::{HitTestEntry, HitTestResult, InteractionLane, MouseRegionCallbacks},
    };

    struct DropPanickingPayload;

    impl Drop for DropPanickingPayload {
        fn drop(&mut self) {
            panic!("later payload drop");
        }
    }

    fn add_primary_mouse(tracker: &MouseTracker) {
        tracker.add_device(0, PointerType::Mouse, Offset::ZERO);
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
                        // A second failure whose payload panics when dropped.
                        panic::panic_any(DropPanickingPayload);
                    })),
                    ..MouseRegionCallbacks::default()
                })
                .expect("register later region");
            (panicking_target, later_target)
        });

        add_primary_mouse(&tracker);

        let position = Offset::new(10.0, 10.0);
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

        let payload = panic.expect_err("the first mouse callback panic must resume");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"first exit panic"),
            "a later payload cannot replace the first failure"
        );
        assert_eq!(
            later_exit.get(),
            1,
            "a later mouse callback must still run before the first panic resumes"
        );
    }
}
