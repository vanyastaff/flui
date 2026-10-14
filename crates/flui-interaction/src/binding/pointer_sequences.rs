//! Admitted contacts: the route each Down resolved, its capture and resampler,
//! and the refusals and teardown barriers around them.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use flui_foundation::InputSequenceId;
use flui_platform_api::EventTime;
use smallvec::SmallVec;

use crate::{
    events::{DeviceId, PointerInfo},
    ids::{InputSurface, PointerId},
    pointer::{PointerEventResampler, ResolvedRouteToken, pointer_capture::ContactCapture},
};

/// Upper bound on simultaneously-tracked pointers.
///
/// Per-pointer state (hit tests, resamplers, arena entries, recogniser maps)
/// grows with active pointers; an untrusted event source could open unbounded
/// pointers to exhaust memory. Real hardware tops out around 10–16 touches, so
/// this generous cap never rejects a legitimate gesture.
pub(super) const MAX_SIMULTANEOUS_POINTERS: usize = 32;

/// Contact admission identity, independent of a reusable platform pointer ID:
/// the [`InputSequenceId`] the binding minted at Down (ADR-0207 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PointerSequence(pub(super) InputSequenceId);

/// Per-pointer state cached at Down. The owner-local resolved token retains
/// the admitted handlers and transforms that Move reuses and Up/Cancel releases.
/// The original spatial hit snapshot is no longer needed after resolution.
///
/// `token` is `None` when no interaction lane was active at Down (a
/// gesture-only binding without a mounted tree) or when the path carried no
/// pointer targets; the pointer router still routes such events.
#[derive(Clone)]
pub(super) struct CachedPointerRoute {
    pub(super) token: Option<ResolvedRouteToken>,
    pub(super) sequence: PointerSequence,
    /// The surface that admitted the Down. The contact's tail stays with it
    /// wherever the platform reports it (capture, ADR-0207 §5).
    pub(super) surface: InputSurface,
    pub(super) capture: Rc<ContactCapture>,
    pub(super) resampler: PointerEventResampler,
    /// Latest admitted source metadata, so lifecycle cancellation preserves
    /// the tool and role most recently observed on this contact.
    pub(super) pointer: PointerInfo,
    /// Last accepted measured contact timestamp, retained for lifecycle
    /// cancellation that has no new platform timestamp of its own.
    pub(super) time: EventTime,
}

/// What lifecycle cancellation needs of a contact, copied out of the map
/// before any callback runs.
#[derive(Clone, Copy)]
pub(super) struct ContactSnapshot {
    pub(super) pointer: PointerInfo,
    pub(super) surface: InputSurface,
    pub(super) time: EventTime,
    pub(super) sequence: PointerSequence,
}

/// A contact whose capture was released, with whether a delivery round still
/// owns a committed prefix of it.
pub(super) struct ReleaseRequest {
    pub(super) contact: ContactSnapshot,
    pub(super) delivering: bool,
}

pub(super) struct ResamplerSnapshot {
    pub(super) pointer_id: PointerId,
    pub(super) sequence: PointerSequence,
    pub(super) token: Option<ResolvedRouteToken>,
    pub(super) resampler: PointerEventResampler,
    pub(super) capture: Rc<ContactCapture>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefusalPolicy {
    NativeTerminal,
    FreshDown(Option<DeviceId>),
}

#[derive(Clone, Copy)]
struct RefusedContact {
    pointer: PointerId,
    policy: RefusalPolicy,
}

enum RefusedContacts {
    Tracking(Box<[Option<RefusedContact>; MAX_SIMULTANEOUS_POINTERS]>),
    Saturated,
}

impl Default for RefusedContacts {
    fn default() -> Self {
        Self::Tracking(Box::new([None; MAX_SIMULTANEOUS_POINTERS]))
    }
}

impl RefusedContacts {
    fn contains(&self, pointer: PointerId, device: Option<DeviceId>) -> bool {
        match self {
            Self::Tracking(ids) => ids.iter().flatten().any(|entry| {
                entry.pointer == pointer
                    && match entry.policy {
                        RefusalPolicy::NativeTerminal => true,
                        RefusalPolicy::FreshDown(released_device) => released_device == device,
                    }
            }),
            Self::Saturated => true,
        }
    }

    fn insert(&mut self, pointer: PointerId, policy: RefusalPolicy) {
        if let Self::Tracking(ids) = self {
            if ids.iter().flatten().any(|entry| entry.pointer == pointer) {
                return;
            }
            if let Some(slot) = ids.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(RefusedContact { pointer, policy });
            } else {
                *self = Self::Saturated;
            }
        }
    }

    fn finish(&mut self, pointer: PointerId) {
        if let Self::Tracking(ids) = self
            && let Some(slot) = ids
                .iter_mut()
                .find(|slot| slot.is_some_and(|entry| entry.pointer == pointer))
        {
            *slot = None;
        }
    }

    fn refuses_down(&self, pointer: PointerId) -> bool {
        matches!(self, Self::Tracking(ids) if ids.iter().flatten().any(|entry| entry.pointer == pointer && entry.policy == RefusalPolicy::NativeTerminal))
    }
}

/// Re-entrancy barrier for abnormal pointer-sequence teardown.
pub(super) struct PointerTeardownGuard<'a> {
    active: &'a RefCell<HashSet<PointerId>>,
    pointer: PointerId,
}

impl Drop for PointerTeardownGuard<'_> {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.pointer);
    }
}

pub(super) struct AllPointerTeardownGuard<'a> {
    active: &'a Cell<bool>,
}

impl Drop for AllPointerTeardownGuard<'_> {
    fn drop(&mut self) {
        self.active.set(false);
    }
}

/// Admitted contact sequences of one binding.
///
/// A contact is one physical pointer, so it is keyed by its [`PointerId`] and
/// records the [`InputSurface`] that admitted it: a Down on another surface
/// supersedes it, and its Move/Up/Cancel are delivered on the admitting
/// surface's route wherever they are reported. A refused Down admits no route;
/// its tail stays refused until that contact terminates. Refusals occupy a
/// fixed array bounded by the admission limit; once it fills, every untracked
/// Move/Up/Cancel is suppressed until a lifecycle reset, so a hostile stream
/// of distinct refused identities cannot grow owner state.
#[derive(Default)]
pub(super) struct PointerSequences {
    routes: RefCell<HashMap<PointerId, CachedPointerRoute>>,
    refused: RefCell<RefusedContacts>,
    /// Prevent route/member destructors from starting a replacement pointer
    /// transaction while abnormal teardown is still draining its snapshot.
    retiring: RefCell<HashSet<PointerId>>,
    retiring_all: Cell<bool>,
}

impl PointerSequences {
    pub(super) fn len(&self) -> usize {
        self.routes.borrow().len()
    }

    pub(super) fn contains(&self, pointer: PointerId) -> bool {
        self.routes.borrow().contains_key(&pointer)
    }

    /// The sequence the contact admitted as `pointer` belongs to.
    pub(super) fn sequence_of(&self, pointer: PointerId) -> Option<InputSequenceId> {
        self.routes
            .borrow()
            .get(&pointer)
            .map(|route| route.sequence.0)
    }

    pub(super) fn is_current(&self, pointer: PointerId, sequence: PointerSequence) -> bool {
        self.routes
            .borrow()
            .get(&pointer)
            .is_some_and(|cached| cached.sequence == sequence)
    }

    #[expect(
        clippy::option_option,
        reason = "outer: whether the pointer has a contact; inner: whether its Down resolved a lane route"
    )]
    pub(super) fn token(&self, pointer: PointerId) -> Option<Option<ResolvedRouteToken>> {
        self.routes
            .borrow()
            .get(&pointer)
            .map(|cached| cached.token)
    }

    /// The capture of the contact `token` was resolved for.
    pub(super) fn capture_for(
        &self,
        pointer: PointerId,
        token: Option<ResolvedRouteToken>,
    ) -> Option<Rc<ContactCapture>> {
        self.routes
            .borrow()
            .get(&pointer)
            .filter(|cached| cached.token == token)
            .map(|cached| Rc::clone(&cached.capture))
    }

    pub(super) fn capture_of(
        &self,
        pointer: PointerId,
        sequence: PointerSequence,
    ) -> Option<Rc<ContactCapture>> {
        self.routes
            .borrow()
            .get(&pointer)
            .filter(|cached| cached.sequence == sequence)
            .map(|cached| Rc::clone(&cached.capture))
    }

    pub(super) fn release_requested(&self, pointer: PointerId) -> bool {
        self.routes
            .borrow()
            .get(&pointer)
            .is_some_and(|cached| cached.capture.release_requested())
    }

    /// Contacts whose capture was released, excluding those already retiring.
    pub(super) fn release_requests(&self) -> SmallVec<[ReleaseRequest; 4]> {
        let routes = self.routes.borrow();
        let retiring = self.retiring.borrow();
        routes
            .values()
            .filter(|cached| {
                cached.capture.release_requested() && !retiring.contains(&cached.pointer.id)
            })
            .map(|cached| ReleaseRequest {
                contact: Self::snapshot(cached),
                delivering: cached.capture.is_delivering(),
            })
            .collect()
    }

    /// Every contact, or only those on hardware `device`, in pointer order.
    pub(super) fn contacts(&self, device: Option<DeviceId>) -> Vec<ContactSnapshot> {
        let mut contacts: Vec<_> = self
            .routes
            .borrow()
            .values()
            .filter(|cached| device.is_none_or(|device| cached.pointer.device == Some(device)))
            .map(Self::snapshot)
            .collect();
        contacts.sort_unstable_by_key(|contact| contact.pointer.id);
        contacts
    }

    fn snapshot(cached: &CachedPointerRoute) -> ContactSnapshot {
        ContactSnapshot {
            pointer: cached.pointer,
            surface: cached.surface,
            time: cached.time,
            sequence: cached.sequence,
        }
    }

    pub(super) fn active_resampler_count(&self) -> usize {
        self.routes
            .borrow()
            .values()
            .filter(|cached| cached.resampler.is_tracked())
            .count()
    }

    /// Whether accepted contact samples or a capture release still need a frame.
    pub(super) fn has_pending_samples(&self) -> bool {
        self.routes.borrow().values().any(|cached| {
            cached.capture.release_requested() || cached.resampler.has_pending_events()
        })
    }

    pub(super) fn pending_sample_count(&self) -> usize {
        self.routes
            .borrow()
            .values()
            .map(|route| route.resampler.pending_event_count())
            .sum()
    }

    /// Snapshot each resampled contact's route capabilities, in pointer order,
    /// so no map borrow crosses the callbacks that deliver its samples.
    pub(super) fn resampler_snapshots(&self) -> SmallVec<[ResamplerSnapshot; 4]> {
        let mut snapshots: SmallVec<[ResamplerSnapshot; 4]> = self
            .routes
            .borrow()
            .iter()
            .filter(|(_, cached)| cached.resampler.is_tracked())
            .map(|(&pointer_id, cached)| ResamplerSnapshot {
                pointer_id,
                sequence: cached.sequence,
                token: cached.token,
                resampler: cached.resampler.clone(),
                capture: Rc::clone(&cached.capture),
            })
            .collect();
        snapshots.sort_unstable_by_key(|snapshot| snapshot.pointer_id);
        snapshots
    }

    /// Record the latest source metadata and timestamp of an admitted contact.
    /// Refuses (returns `false`) a packet from another device: the map key
    /// already matches `PointerId`, and device identity completes the source,
    /// while kind/tool and primary role may change during contact.
    pub(super) fn observe(&self, pointer: &PointerInfo, time: Option<EventTime>) -> bool {
        let mut routes = self.routes.borrow_mut();
        let Some(cached) = routes.get_mut(&pointer.id) else {
            return true;
        };
        if cached.pointer.device != pointer.device {
            return false;
        }
        cached.pointer = *pointer;
        if let Some(time) = time {
            cached.time = time;
        }
        true
    }

    /// Stamp a measured Move on its contact, returning what queues it.
    pub(super) fn advance(
        &self,
        pointer: PointerId,
        time: EventTime,
    ) -> Option<(PointerSequence, InputSurface, PointerEventResampler)> {
        self.routes.borrow_mut().get_mut(&pointer).map(|cached| {
            cached.time = time;
            (cached.sequence, cached.surface, cached.resampler.clone())
        })
    }

    pub(super) fn route_of(
        &self,
        pointer: PointerId,
    ) -> Option<(
        PointerSequence,
        InputSurface,
        Option<ResolvedRouteToken>,
        PointerEventResampler,
    )> {
        self.routes.borrow().get(&pointer).map(|cached| {
            (
                cached.sequence,
                cached.surface,
                cached.token,
                cached.resampler.clone(),
            )
        })
    }

    pub(super) fn admit(&self, pointer: PointerId, route: CachedPointerRoute) {
        let replaced = self.routes.borrow_mut().insert(pointer, route);
        drop(replaced);
    }

    /// Remove a contact, ending its capture before any callback can observe it.
    pub(super) fn detach(&self, pointer: PointerId) -> Option<CachedPointerRoute> {
        let cached = self.routes.borrow_mut().remove(&pointer);
        if let Some(cached) = cached.as_ref() {
            cached.capture.end();
        }
        cached
    }

    /// Yield every admitted contact, in pointer order, and forget refusals.
    /// The maps are empty before the caller runs any callback or destructor.
    pub(super) fn drain(&self) -> Vec<(PointerId, CachedPointerRoute)> {
        self.refused.take();
        let mut routes: Vec<_> = self.routes.take().into_iter().collect();
        routes.sort_unstable_by_key(|(pointer, _)| *pointer);
        routes
    }

    pub(super) fn refuses_tail(&self, pointer: PointerId, device: Option<DeviceId>) -> bool {
        !self.contains(pointer) && self.refused.borrow().contains(pointer, device)
    }

    pub(super) fn refuses_down(&self, pointer: PointerId) -> bool {
        self.refused.borrow().refuses_down(pointer)
    }

    pub(super) fn finish_refusal(&self, pointer: PointerId) {
        self.refused.borrow_mut().finish(pointer);
    }

    /// A Down over the admission limit: its native tail stays refused.
    pub(super) fn refuse(&self, pointer: PointerId) {
        self.refused
            .borrow_mut()
            .insert(pointer, RefusalPolicy::NativeTerminal);
    }

    /// A released capture: the rest of this contact is refused until a fresh
    /// Down from the same device.
    pub(super) fn refuse_released(&self, pointer: PointerInfo) {
        self.refused
            .borrow_mut()
            .insert(pointer.id, RefusalPolicy::FreshDown(pointer.device));
    }

    pub(super) fn reset_refusals(&self) {
        self.refused.take();
    }

    pub(super) fn try_retire(&self, pointer: PointerId) -> Option<PointerTeardownGuard<'_>> {
        self.retiring
            .borrow_mut()
            .insert(pointer)
            .then(|| PointerTeardownGuard {
                active: &self.retiring,
                pointer,
            })
    }

    pub(super) fn try_retire_all(&self) -> Option<AllPointerTeardownGuard<'_>> {
        (!self.retiring_all.replace(true)).then(|| AllPointerTeardownGuard {
            active: &self.retiring_all,
        })
    }

    pub(super) fn is_retiring_all(&self) -> bool {
        self.retiring_all.get()
    }

    pub(super) fn is_retiring(&self, pointer: PointerId) -> bool {
        self.retiring.borrow().contains(&pointer)
    }

    pub(super) fn is_retiring_any(&self) -> bool {
        self.retiring_all.get() || !self.retiring.borrow().is_empty()
    }
}
