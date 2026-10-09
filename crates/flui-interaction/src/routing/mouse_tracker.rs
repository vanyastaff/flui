//! Mouse tracking for hover, enter, and exit events.
//!
//! Hardware identity keys state when reported; otherwise the contact identity
//! keys a separate fallback namespace. Callbacks receive the complete source
//! [`PointerInfo`], preserving that distinction without a sentinel device.
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
    collections::{BTreeMap, HashSet},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use smallvec::SmallVec;

pub use super::interaction_lane::{
    MouseEnterCallback, MouseExitCallback, MouseHoverCallback, MouseRegionTarget,
};
use super::{HitTestResult, OwnerLatch, RoutePanic, active_dispatch_handle};
use crate::{
    events::{CursorIcon, PointerEvent, PointerInfo, PointerKind},
    ids::RegionId,
    retain::Retain,
    routing::interaction_lane::MouseRegionCell,
};

/// Device ID type (re-exported from events).
pub use crate::events::DeviceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SourceKey {
    KnownDevice(DeviceId),
    FallbackContact(crate::ids::PointerId),
}

impl SourceKey {
    fn from_pointer(pointer: &PointerInfo) -> Self {
        pointer
            .device
            .map_or(Self::FallbackContact(pointer.id), Self::KnownDevice)
    }
}

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
    /// Authority retained by ambient probes across their reentrant hit-test call.
    observation: Rc<()>,
    /// Latest source metadata for callbacks and the `mouse_is_connected` query.
    pointer: PointerInfo,
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
    fn new(pointer: PointerInfo, position: Offset<f64>) -> Self {
        Self {
            observation: Rc::new(()),
            pointer,
            last_position: position,
            active_regions: HashSet::new(),
            active_order: Vec::new(),
            current_cursor: CursorIcon::Default,
            inside_window: true,
        }
    }

    fn advance_observation(&mut self) {
        // A retained probe must remain distinguishable from a newer admission.
        // With no overlapping probe, the unique token can be reused.
        if Rc::strong_count(&self.observation) > 1 {
            self.observation = Rc::new(());
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

/// Callback for cursor changes with the source's actual contact and optional hardware identity.
pub type CursorChangeCallback = Rc<dyn Fn(PointerInfo, CursorIcon) + 'static>;

#[derive(Default)]
struct CursorPublication {
    observation: Rc<()>,
    published: CursorIcon,
    pending: bool,
    in_flight: SmallVec<[CursorFlight; 1]>,
}

struct CursorFlight {
    observation: Rc<()>,
    callback: std::rc::Weak<dyn Fn(PointerInfo, CursorIcon)>,
    cursor: CursorIcon,
}

impl CursorPublication {
    fn observe(&mut self, cursor: CursorIcon) -> Rc<()> {
        // Only overlapping work needs another allocation; ordinary sequential
        // mouse packets reuse the uniquely held observation identity.
        if Rc::strong_count(&self.observation) > 1 {
            self.observation = Rc::new(());
        }
        self.pending |= self.published != cursor;
        Rc::clone(&self.observation)
    }
}

#[derive(Default)]
enum CursorOwner {
    #[default]
    Unobserved,
    Physical(PointerInfo),
    Removed(PointerInfo),
}

impl CursorOwner {
    fn physical_source(&self) -> Option<SourceKey> {
        match self {
            Self::Physical(pointer) => Some(SourceKey::from_pointer(pointer)),
            _ => None,
        }
    }
}

struct MouseTrackerInner {
    closed: bool,
    close_mode: crate::__runtime::CloseTombstone,
    /// State for each reported hardware device or fallback contact.
    devices: BTreeMap<SourceKey, DeviceState>,
    /// Last resolved annotations by region.
    ///
    /// Entries stay here until their exit callback has been collected: the
    /// previous map is replaced only after it has been diffed against the
    /// fresh one.
    annotations: BTreeMap<RegionId, ResolvedMouseTrackerAnnotation>,
    /// Whether any mouse is connected.
    mouse_connected: bool,
    /// Callback for cursor changes.
    cursor_change_callback: Option<CursorChangeCallback>,
    cursor_publication: CursorPublication,
    cursor_owner: CursorOwner,
}

impl MouseTracker {
    /// Creates a new owner-local mouse tracker.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(MouseTrackerInner {
                closed: false,
                close_mode: crate::__runtime::CloseTombstone::default(),
                devices: BTreeMap::new(),
                annotations: BTreeMap::new(),
                mouse_connected: false,
                cursor_change_callback: None,
                cursor_publication: CursorPublication::default(),
                cursor_owner: CursorOwner::Unobserved,
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
            let outgoing = self
                .inner
                .borrow_mut()
                .annotations
                .insert(annotation.region_id, resolved);
            let mut failure = crate::__runtime::ClosePanic::new();
            failure.retire(outgoing);
            failure.finish();
        }
    }

    /// Unregisters a mouse region annotation and removes it from active device
    /// state.
    pub fn unregister_annotation(&self, region_id: RegionId) {
        let outgoing = {
            let mut inner = self.inner.borrow_mut();
            let outgoing = inner.annotations.remove(&region_id);
            for state in inner.devices.values_mut() {
                state.active_regions.remove(&region_id);
                state.active_order.retain(|id| *id != region_id);
            }
            outgoing
        };
        let mut failure = crate::__runtime::ClosePanic::new();
        failure.retire(outgoing);
        failure.finish();
    }

    /// Removes a pointing device and all hover state associated with it.
    pub fn remove_device(&self, device_id: DeviceId) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.devices.remove(&SourceKey::KnownDevice(device_id));
            if inner.cursor_owner.physical_source() == Some(SourceKey::KnownDevice(device_id))
                && let CursorOwner::Physical(pointer) = inner.cursor_owner
            {
                inner.cursor_owner = CursorOwner::Removed(pointer);
                inner.cursor_publication.pending |=
                    inner.cursor_publication.published != CursorIcon::Default;
            }
            inner.mouse_connected = inner
                .devices
                .values()
                .any(|state| state.pointer.kind == PointerKind::Mouse);
        }
        let mut failure = crate::__runtime::ClosePanic::new();
        if let Some(work) = self.fallback_cursor_work() {
            work.invoke(&mut failure);
        }
        failure.finish();
    }

    fn fallback_cursor_work(&self) -> Option<DeviceWork> {
        let mut inner = self.inner.borrow_mut();
        let CursorOwner::Removed(pointer) = inner.cursor_owner else {
            return None;
        };
        if inner.closed || !inner.cursor_publication.pending {
            return None;
        }
        Some(DeviceWork {
            tracker: Rc::clone(&self.inner),
            pointer,
            position: Offset::ZERO,
            enter_callbacks: SmallVec::new(),
            exit_callbacks: SmallVec::new(),
            cursor_observation: Some(inner.cursor_publication.observe(CursorIcon::Default)),
            new_cursor: CursorIcon::Default,
            retired_annotations: Vec::new(),
        })
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
        let sweeps: Vec<DeviceWork> = {
            let mut inner = self.inner.borrow_mut();
            let inner = &mut *inner;
            if let CursorOwner::Physical(pointer) = inner.cursor_owner {
                inner.cursor_owner = CursorOwner::Removed(pointer);
                inner.cursor_publication.pending |=
                    inner.cursor_publication.published != CursorIcon::Default;
            }
            inner
                .devices
                .iter_mut()
                .filter_map(|(_, state)| {
                    state.advance_observation();
                    state.inside_window = false;
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
                    let position = state.last_position;
                    state.active_regions.clear();
                    state.active_order.clear();
                    state.current_cursor = CursorIcon::Default;
                    state.inside_window = false;
                    Some(DeviceWork {
                        tracker: Rc::clone(&self.inner),
                        pointer: state.pointer,
                        position,
                        enter_callbacks: SmallVec::new(),
                        exit_callbacks,
                        cursor_observation: None,
                        new_cursor: CursorIcon::Default,
                        retired_annotations: Vec::new(),
                    })
                })
                .collect()
        };
        // Every device is swept even if an earlier device's callback
        // panics — the first panic resumes only after the loop, the same
        // all-callbacks-run-first posture `DeviceWork::invoke` has within
        // one device.
        let mut failure = crate::__runtime::ClosePanic::new();
        for work in sweeps {
            work.invoke(&mut failure);
        }
        if let Some(work) = self.fallback_cursor_work() {
            work.invoke(&mut failure);
        }
        failure.finish();
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
        let PointerEvent::Move(update) = event else {
            return;
        };
        let pointer = update.pointer;
        if !matches!(pointer.kind, PointerKind::Mouse | PointerKind::Pen { .. }) {
            return;
        }
        let device_id = SourceKey::from_pointer(&pointer);
        let point = update.current().position.get();
        let position = Offset::new(point.x, point.y);
        tracing::trace!(
            ?device_id,
            ?kind,
            "mouse tracker updating enter/exit/cursor state"
        );

        let resolved = resolve_hit_test_annotations(hit_test_result);
        let new_regions: HashSet<RegionId> = resolved.order.iter().copied().collect();
        let new_cursor = hit_test_result.resolve_cursor();

        let work = {
            let mut inner = self.inner.borrow_mut();
            let mut retired_annotations = Vec::new();
            for (region_id, annotation) in resolved.annotations {
                if let Some(previous) = inner.annotations.insert(region_id, annotation) {
                    retired_annotations.push(previous);
                }
            }
            if pointer.kind == PointerKind::Mouse {
                inner.mouse_connected = true;
            }

            let state = inner
                .devices
                .entry(device_id)
                .or_insert_with(|| DeviceState::new(pointer, position));
            state.advance_observation();
            state.pointer = pointer;
            state.inside_window = true;

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

            state.last_position = position;
            state.active_regions = new_regions;
            state.active_order = resolved.order;
            state.current_cursor = new_cursor;
            inner.cursor_owner = CursorOwner::Physical(pointer);

            let enter_callbacks: SmallVec<[Latched<MouseEnterCallback>; 4]> = entered
                .iter()
                .filter_map(|id| {
                    inner
                        .annotations
                        .get(id)
                        .and_then(ResolvedMouseTrackerAnnotation::on_enter)
                })
                .collect();
            let exit_callbacks: SmallVec<[Latched<MouseExitCallback>; 4]> = exited
                .iter()
                .filter_map(|id| {
                    inner
                        .annotations
                        .get(id)
                        .and_then(ResolvedMouseTrackerAnnotation::on_exit)
                })
                .collect();
            for id in exited {
                if !inner
                    .devices
                    .values()
                    .any(|state| state.active_regions.contains(&id))
                    && let Some(annotation) = inner.annotations.remove(&id)
                {
                    retired_annotations.push(annotation);
                }
            }
            DeviceWork {
                tracker: Rc::clone(&self.inner),
                pointer,
                position,
                enter_callbacks,
                exit_callbacks,
                cursor_observation: Some(inner.cursor_publication.observe(new_cursor)),
                new_cursor,
                retired_annotations,
            }
        };

        let mut failure = crate::__runtime::ClosePanic::new();
        work.invoke(&mut failure);
        failure.finish();
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
        if !update.buttons.is_empty() {
            return None;
        }
        let pointer = update.pointer;
        let point = update.current().position.get();
        let position = Offset::new(point.x, point.y);

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
                let delivered = RoutePanic::capture(|| callback(pointer, position));
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
    pub fn update_all_devices<F>(&self, hit_test_fn: F)
    where
        F: Fn(Offset<f64>) -> HitTestResult,
    {
        let device_positions: Vec<(SourceKey, PointerInfo, Offset<f64>, Rc<()>)> = self
            .inner
            .borrow()
            .devices
            .iter()
            // A device swept by a window-leave is not re-hit-tested at its
            // stale in-window position — that would re-enter the regions the
            // sweep just exited. Its next real motion re-primes it.
            .filter(|(_, state)| state.inside_window)
            .map(|(id, state)| {
                (*id, state.pointer, state.last_position, Rc::clone(&state.observation))
            })
            .collect();

        let mut failure = crate::__runtime::ClosePanic::new();
        let mut pending = Vec::with_capacity(device_positions.len());
        for (device_id, pointer, position, observation) in device_positions {
            let Some((resolved, new_cursor)) = failure.invoke(|| {
                let result = hit_test_fn(position);
                (
                    resolve_hit_test_annotations(&result),
                    result.resolve_cursor(),
                )
            }) else {
                continue;
            };
            let new_regions: HashSet<RegionId> = resolved.order.iter().copied().collect();

            // User hit testing may admit newer work, even for the same source
            // at the same position. Only the captured admission can commit.
            let current = {
                let inner = self.inner.borrow();
                !inner.closed
                    && inner.devices.get(&device_id).is_some_and(|state| {
                        state.inside_window && Rc::ptr_eq(&state.observation, &observation)
                    })
            };
            if !current {
                for annotation in resolved.annotations.into_values() {
                    failure.retire(annotation);
                }
                continue;
            }

            let work = {
                let mut inner = self.inner.borrow_mut();
                let mut retired_annotations = Vec::new();
                for (region_id, annotation) in resolved.annotations {
                    if let Some(previous) = inner.annotations.insert(region_id, annotation) {
                        retired_annotations.push(previous);
                    }
                }

                let state = inner
                    .devices
                    .get_mut(&device_id)
                    .expect("BUG: refresh checked the device before borrowing the tracker");
                state.advance_observation();

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

                state.active_regions = new_regions;
                state.active_order = resolved.order;
                state.current_cursor = new_cursor;

                let enter_callbacks: SmallVec<[Latched<MouseEnterCallback>; 4]> = entered
                    .iter()
                    .filter_map(|id| {
                        inner
                            .annotations
                            .get(id)
                            .and_then(ResolvedMouseTrackerAnnotation::on_enter)
                    })
                    .collect();
                let exit_callbacks: SmallVec<[Latched<MouseExitCallback>; 4]> = exited
                    .iter()
                    .filter_map(|id| {
                        inner
                            .annotations
                            .get(id)
                            .and_then(ResolvedMouseTrackerAnnotation::on_exit)
                    })
                    .collect();
                for id in exited {
                    if !inner
                        .devices
                        .values()
                        .any(|state| state.active_regions.contains(&id))
                        && let Some(annotation) = inner.annotations.remove(&id)
                    {
                        retired_annotations.push(annotation);
                    }
                }

                DeviceWork {
                    tracker: Rc::clone(&self.inner),
                    pointer,
                    position,
                    enter_callbacks,
                    exit_callbacks,
                    cursor_observation: if inner.cursor_owner.physical_source() == Some(device_id) {
                        Some(inner.cursor_publication.observe(new_cursor))
                    } else {
                        None
                    },
                    new_cursor,
                    retired_annotations,
                }
            };
            pending.push(work);
        }

        for work in pending {
            work.invoke(&mut failure);
        }
        if let Some(work) = self.fallback_cursor_work() {
            work.invoke(&mut failure);
        }
        failure.finish();
    }

    /// Checks if any mouse is currently connected.
    #[inline]
    #[must_use]
    pub fn mouse_is_connected(&self) -> bool {
        self.inner.borrow().mouse_connected
    }

    /// Gets the last known position for reported hardware, excluding fallback contacts.
    #[must_use]
    pub fn device_position(&self, device_id: DeviceId) -> Option<Offset<f64>> {
        self.inner
            .borrow()
            .devices
            .get(&SourceKey::KnownDevice(device_id))
            .map(|state| state.last_position)
    }

    /// Gets the last known position for this hardware or fallback contact.
    #[must_use]
    pub fn source_position(&self, pointer: &PointerInfo) -> Option<Offset<f64>> {
        self.inner
            .borrow()
            .devices
            .get(&SourceKey::from_pointer(pointer))
            .map(|state| state.last_position)
    }

    /// Gets the set of active regions for a device.
    #[must_use]
    pub fn device_active_regions(&self, device_id: DeviceId) -> HashSet<RegionId> {
        self.inner
            .borrow()
            .devices
            .get(&SourceKey::KnownDevice(device_id))
            .map(|state| state.active_regions.clone())
            .unwrap_or_default()
    }

    /// Gets the current cursor for a device.
    #[must_use]
    pub fn device_cursor(&self, device_id: DeviceId) -> CursorIcon {
        self.inner
            .borrow()
            .devices
            .get(&SourceKey::KnownDevice(device_id))
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
                inner.cursor_publication.pending = true;
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
}

impl Default for MouseTracker {
    fn default() -> Self {
        Self::new()
    }
}

struct ResolvedHitAnnotations {
    order: Vec<RegionId>,
    annotations: BTreeMap<RegionId, ResolvedMouseTrackerAnnotation>,
}

fn resolve_hit_test_annotations(result: &HitTestResult) -> ResolvedHitAnnotations {
    let mut order = Vec::new();
    let mut annotations = BTreeMap::new();
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
    pointer: PointerInfo,
    position: Offset<f64>,
    enter_callbacks: SmallVec<[Latched<MouseEnterCallback>; 4]>,
    exit_callbacks: SmallVec<[Latched<MouseExitCallback>; 4]>,
    cursor_observation: Option<Rc<()>>,
    new_cursor: CursorIcon,
    /// Replaced and departed annotations whose captures retire after dispatch.
    retired_annotations: Vec<ResolvedMouseTrackerAnnotation>,
}

impl DeviceWork {
    /// Run the batch. A callback may close its presentation reentrantly, so
    /// each snapshot is rechecked against its owner's latch (and the cursor
    /// callback against this tracker) right before it runs, and released
    /// under that close's retention policy afterwards (ADR-0127).
    fn invoke(self, failure: &mut crate::__runtime::ClosePanic) {
        for (callback, latch) in self.exit_callbacks {
            if !latch.is_closed() {
                failure.invoke(|| {
                    callback(self.pointer, self.position);
                });
            }
            if failure.preserving() {
                callback.retain();
            } else {
                failure.invoke(|| latch.release(callback));
            }
        }
        for (callback, latch) in self.enter_callbacks {
            if !latch.is_closed() {
                failure.invoke(|| {
                    callback(self.pointer, self.position);
                });
            }
            if failure.preserving() {
                callback.retain();
            } else {
                failure.invoke(|| latch.release(callback));
            }
        }
        if let Some(observation) = self.cursor_observation {
            let callback = {
                let mut inner = self.tracker.borrow_mut();
                if !inner.closed
                    && inner.cursor_publication.pending
                    && Rc::ptr_eq(&inner.cursor_publication.observation, &observation)
                {
                    let callback = inner.cursor_change_callback.clone();
                    if let Some(callback) = &callback {
                        let identity = Rc::downgrade(callback);
                        if inner.cursor_publication.in_flight.iter().any(|flight| {
                            flight.cursor == self.new_cursor
                                && std::rc::Weak::ptr_eq(&flight.callback, &identity)
                        }) {
                            // Same-hook, same-state recursion cannot complete the
                            // in-flight publication. Its latest debt stays pending.
                            None
                        } else {
                            inner.cursor_publication.in_flight.push(CursorFlight {
                                observation: Rc::clone(&observation),
                                callback: identity,
                                cursor: self.new_cursor,
                            });
                            Some(Rc::clone(callback))
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(callback) = callback {
                let delivered = failure.invoke(|| callback(self.pointer, self.new_cursor));
                {
                    let mut inner = self.tracker.borrow_mut();
                    if let Some(index) = inner
                        .cursor_publication
                        .in_flight
                        .iter()
                        .position(|flight| Rc::ptr_eq(&flight.observation, &observation))
                    {
                        inner.cursor_publication.in_flight.remove(index);
                    }
                    if Rc::ptr_eq(&inner.cursor_publication.observation, &observation) {
                        if delivered.is_some() {
                            inner.cursor_publication.published = self.new_cursor;
                            if inner
                                .cursor_change_callback
                                .as_ref()
                                .is_some_and(|current| Rc::ptr_eq(current, &callback))
                            {
                                inner.cursor_publication.pending = false;
                            }
                        } else {
                            inner.cursor_publication.pending = true;
                        }
                    }
                }
                let preserved = {
                    let inner = self.tracker.borrow();
                    inner.closed && inner.close_mode.preserved()
                };
                if preserved {
                    callback.retain();
                } else {
                    failure.retire(callback);
                }
            }
        }
        for annotation in self.retired_annotations {
            failure.retire(annotation);
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
        events::{PointerKind, make_move_event},
        routing::{HitTestEntry, HitTestResult, InteractionLane, MouseRegionCallbacks},
    };

    struct DropPanickingPayload;

    impl Drop for DropPanickingPayload {
        fn drop(&mut self) {
            panic!("later payload drop");
        }
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

        let position = Offset::new(10.0, 10.0);
        let event = make_move_event(position, PointerKind::Mouse).expect("finite fixture position");
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
