//! Owner-local interaction callback storage for ADR-0027.
//!
//! Executable interaction handlers remain behind owner-thread `Rc` cells; only
//! opaque identity tickets are `Send + Sync`. Production pointer dispatch
//! resolves hit-test `PointerTarget`s through this lane, invokes the retained
//! owner-local cells synchronously, and releases cached routes at the end of the
//! pointer sequence.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, ThreadId};

use flui_types::geometry::Matrix4;

use super::hit_test::{HitTestEntry, transform_pointer_event};
use crate::events::PointerEvent;

static NEXT_LANE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct LaneId(NonZeroU64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct TargetId(NonZeroU64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct RouteId(NonZeroU64);

#[derive(Clone, Copy, PartialEq, Eq)]
struct LaneTicket {
    lane_id: LaneId,
    owner: ThreadId,
}

/// Why an owner-local interaction operation could not complete.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InteractionDispatchError {
    /// A private monotonic identity source has no unused values remaining.
    #[error("interaction identity space is exhausted")]
    IdentifierExhausted,
    /// The capability was used away from its owner thread.
    #[error("interaction capability used from the wrong thread")]
    WrongThread,
    /// No interaction realm is active on the owner thread.
    #[error("no interaction realm is active")]
    InactiveRealm,
    /// Another realm is active, or the supplied token belongs to another realm.
    #[error("interaction capability or token belongs to another realm")]
    WrongRealm,
    /// The lane that minted the capability has been dropped.
    #[error("interaction owner is gone")]
    OwnerGone,
    /// A target is no longer available for a new route resolution or mutation.
    #[error("interaction target is gone")]
    TargetGone,
    /// A cached route has already been released.
    #[error("resolved interaction route is stale")]
    StaleRoute,
}

/// Opaque data-plane identity for an ordinary pointer event target.
///
/// Framework authors store this value in render objects and hit-test entries
/// instead of storing an executable callback there. It is minted when a
/// pointer handler is registered with the active interaction lane and is valid
/// only with the lane that created it.
///
/// `PointerTarget` is cheap to copy and is `Send + Sync`, so immutable hit-test
/// data may cross framework execution boundaries. The value itself does not
/// keep a handler alive: resolving a route acquires that owner-local lifetime.
/// Its identity is intentionally opaque, with no raw constructor, raw accessor,
/// default, or serialization contract.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for PointerTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerTarget").finish_non_exhaustive()
    }
}

/// Opaque data-plane identity for a framework mouse-region target.
///
/// Framework authors use this type in mouse-region render data and hit-test
/// annotations rather than carrying owner-local callbacks through the render
/// tree. Its separate type prevents pointer and mouse target identities from
/// being mixed accidentally.
///
/// Like [`PointerTarget`], it is a copyable `Send + Sync` identity bound to its
/// originating interaction lane. Registration and resolution remain
/// framework-composition responsibilities; the identity exposes no raw value,
/// constructor, default, or serialization contract.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseRegionTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for MouseRegionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MouseRegionTarget").finish_non_exhaustive()
    }
}

/// Opaque key for an owner-local resolved route.
///
/// It carries its minting lane identity, so realm recreation cannot make an old
/// token address a route in the new owner.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedRouteToken {
    lane_id: LaneId,
    route_id: RouteId,
}

impl fmt::Debug for ResolvedRouteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedRouteToken").finish_non_exhaustive()
    }
}

/// One target omitted while resolving a partial route.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteResolutionMiss {
    /// The same-lane target was no longer registered.
    TargetGone {
        /// Zero-based position in the requested hit path.
        path_index: usize,
    },
}

impl RouteResolutionMiss {
    /// The missing target's position in the requested hit path.
    #[must_use]
    pub const fn path_index(self) -> usize {
        match self {
            Self::TargetGone { path_index } => path_index,
        }
    }
}

/// Result of resolving a hit path, including ordered same-lane misses.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteResolution {
    token: ResolvedRouteToken,
    misses: Vec<RouteResolutionMiss>,
}

impl RouteResolution {
    /// The token naming the resolved live subset.
    #[must_use]
    pub const fn token(&self) -> ResolvedRouteToken {
        self.token
    }

    /// Missing targets in their original path order.
    #[must_use]
    pub fn misses(&self) -> &[RouteResolutionMiss] {
        &self.misses
    }
}

struct MonotonicIdSource {
    next: Cell<u64>,
}

impl MonotonicIdSource {
    const fn new() -> Self {
        Self { next: Cell::new(1) }
    }

    #[cfg(test)]
    const fn starting_at(next: u64) -> Self {
        Self {
            next: Cell::new(next),
        }
    }

    fn try_next(&self) -> Result<NonZeroU64, InteractionDispatchError> {
        let current = self.next.get();
        let id = NonZeroU64::new(current).ok_or(InteractionDispatchError::IdentifierExhausted)?;
        self.next.set(current.checked_add(1).unwrap_or(0));
        Ok(id)
    }
}

fn try_mint_lane_id(source: &AtomicU64) -> Result<LaneId, InteractionDispatchError> {
    let current = source
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current != 0).then(|| current.checked_add(1).unwrap_or(0))
        })
        .map_err(|_| InteractionDispatchError::IdentifierExhausted)?;
    let non_zero = NonZeroU64::new(current).ok_or(InteractionDispatchError::IdentifierExhausted)?;
    Ok(LaneId(non_zero))
}

type PointerHandler = Rc<dyn Fn(&PointerEvent) + 'static>;

struct HandlerCell {
    current: RefCell<PointerHandler>,
}

impl HandlerCell {
    fn new(handler: PointerHandler) -> Self {
        Self {
            current: RefCell::new(handler),
        }
    }

    fn snapshot(&self) -> PointerHandler {
        Rc::clone(&self.current.borrow())
    }

    fn replace(&self, handler: PointerHandler) -> PointerHandler {
        std::mem::replace(&mut *self.current.borrow_mut(), handler)
    }
}

/// How a resolved entry maps the dispatched global event into its local space.
enum LocalEventTransform {
    /// The entry captured no transform; it receives the global event.
    Global,
    /// Global-to-local inverse computed once at resolution time.
    Inverse(Matrix4),
    /// The captured transform is singular; the entry is skipped, matching the
    /// pre-route dispatch behavior for non-invertible transforms.
    NonInvertible,
}

impl LocalEventTransform {
    fn capture(transform: Option<Matrix4>) -> Self {
        match transform {
            None => Self::Global,
            Some(transform) => transform
                .try_inverse()
                .map_or(Self::NonInvertible, Self::Inverse),
        }
    }
}

struct ResolvedHitEntry {
    handler_cell: Rc<HandlerCell>,
    local_transform: LocalEventTransform,
}

struct ResolvedHitRoute {
    entries: Vec<ResolvedHitEntry>,
}

impl ResolvedHitRoute {
    /// Deliver `event` to every entry leaf-first, isolating per-target panics.
    ///
    /// The first panic payload is captured and returned so the dispatch owner
    /// can perform mandatory cleanup (arena close/sweep, route release) before
    /// resuming it; later panics are traced without replacing the first,
    /// matching Flutter's per-entry exception isolation in
    /// `GestureBinding.dispatchEvent`.
    fn invoke(&self, event: &PointerEvent) -> Option<RoutePanic> {
        let mut first_panic = None;
        for entry in &self.entries {
            let local_event = match &entry.local_transform {
                LocalEventTransform::Global => None,
                LocalEventTransform::Inverse(inverse) => {
                    Some(transform_pointer_event(event, inverse))
                }
                LocalEventTransform::NonInvertible => continue,
            };
            let handler = entry.handler_cell.snapshot();
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                handler(local_event.as_ref().unwrap_or(event));
            }));
            // The handler Rc must not drop inside the unwind bookkeeping below.
            drop(handler);
            if let Err(payload) = delivered {
                if first_panic.is_none() {
                    first_panic = Some(RoutePanic { payload });
                } else {
                    tracing::error!(
                        "pointer target panicked after an earlier target already panicked; \
                         only the first panic is resumed"
                    );
                }
            }
        }
        first_panic
    }
}

/// The first panic captured while invoking a resolved pointer route.
///
/// The dispatch owner must finish its mandatory cleanup (close the arena on
/// Down, sweep/release on Up/Cancel, release an ephemeral route) and then call
/// [`resume`](Self::resume) so the panic propagates under the repository panic
/// policy.
#[doc(hidden)]
#[must_use = "a captured route panic must be resumed after dispatch cleanup"]
pub struct RoutePanic {
    payload: Box<dyn Any + Send>,
}

impl RoutePanic {
    /// Continue unwinding with the captured payload.
    pub fn resume(self) -> ! {
        resume_unwind(self.payload)
    }
}

impl fmt::Debug for RoutePanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutePanic").finish_non_exhaustive()
    }
}

struct LocalLaneInner {
    ticket: LaneTicket,
    target_ids: MonotonicIdSource,
    route_ids: MonotonicIdSource,
    targets: RefCell<HashMap<TargetId, Rc<HandlerCell>>>,
    routes: RefCell<HashMap<RouteId, Rc<ResolvedHitRoute>>>,
}

thread_local! {
    static LOCAL_LANES: RefCell<HashMap<LaneId, Weak<LocalLaneInner>>> =
        RefCell::new(HashMap::new());
    static ACTIVE_LANES: RefCell<Vec<LaneTicket>> = const { RefCell::new(Vec::new()) };
}

/// Owner-affine storage for local interaction handlers and resolved routes.
///
/// This runtime-composition type is public only for sibling FLUI crates. It is
/// structurally `!Send + !Sync` through its `Rc` storage and is not exported by
/// the interaction prelude.
#[doc(hidden)]
pub struct InteractionLane {
    inner: Rc<LocalLaneInner>,
}

impl InteractionLane {
    /// Create a lane on the current owner thread.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError::IdentifierExhausted`] if the private
    /// process-wide lane identity source has no unused value remaining.
    pub fn try_new() -> Result<Self, InteractionDispatchError> {
        let lane_id = try_mint_lane_id(&NEXT_LANE_ID)?;
        let inner = Rc::new(LocalLaneInner {
            ticket: LaneTicket {
                lane_id,
                owner: thread::current().id(),
            },
            target_ids: MonotonicIdSource::new(),
            route_ids: MonotonicIdSource::new(),
            targets: RefCell::new(HashMap::new()),
            routes: RefCell::new(HashMap::new()),
        });
        LOCAL_LANES.with(|registry| {
            registry.borrow_mut().insert(lane_id, Rc::downgrade(&inner));
        });
        Ok(Self { inner })
    }

    /// Mint the Send-safe, least-privilege capability for this lane.
    #[must_use]
    pub fn dispatch_handle(&self) -> InteractionDispatchHandle {
        InteractionDispatchHandle {
            ticket: self.inner.ticket,
        }
    }

    /// Activate this lane for the dynamic extent of `callback`.
    pub fn enter<R>(&self, callback: impl FnOnce() -> R) -> R {
        ACTIVE_LANES.with(|active| active.borrow_mut().push(self.inner.ticket));
        let _activation = LaneActivation {
            ticket: self.inner.ticket,
            _lane: self,
        };
        callback()
    }
}

impl fmt::Debug for InteractionLane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InteractionLane").finish_non_exhaustive()
    }
}

impl Drop for InteractionLane {
    fn drop(&mut self) {
        let removed_registry_entry = LOCAL_LANES
            .try_with(|registry| registry.borrow_mut().remove(&self.inner.ticket.lane_id))
            .ok()
            .flatten();
        drop(removed_registry_entry);

        let routes = self.inner.routes.take();
        let targets = self.inner.targets.take();

        let mut routes: Vec<_> = routes.into_iter().collect();
        routes.sort_unstable_by_key(|(id, _)| *id);
        drop(routes);

        let mut targets: Vec<_> = targets.into_iter().collect();
        targets.sort_unstable_by_key(|(id, _)| *id);
        drop(targets);
    }
}

struct LaneActivation<'lane> {
    ticket: LaneTicket,
    _lane: &'lane InteractionLane,
}

impl Drop for LaneActivation<'_> {
    fn drop(&mut self) {
        let _ = ACTIVE_LANES.try_with(|active| {
            let popped = active.borrow_mut().pop();
            if popped != Some(self.ticket) {
                tracing::error!("interaction lane activation stack was not LIFO");
            }
        });
    }
}

/// Send-safe ticket for owner-local interaction registration and route access.
///
/// It carries identity only. Calls succeed exclusively while its lane is the
/// active top scope on the owner thread.
#[doc(hidden)]
#[derive(Clone)]
pub struct InteractionDispatchHandle {
    ticket: LaneTicket,
}

impl InteractionDispatchHandle {
    fn active_lane(&self) -> Result<Rc<LocalLaneInner>, InteractionDispatchError> {
        if thread::current().id() != self.ticket.owner {
            return Err(InteractionDispatchError::WrongThread);
        }

        let lane = LOCAL_LANES.with(|registry| {
            registry
                .borrow()
                .get(&self.ticket.lane_id)
                .and_then(Weak::upgrade)
        });
        let lane = lane.ok_or(InteractionDispatchError::OwnerGone)?;

        let active = ACTIVE_LANES.with(|active| active.borrow().last().copied());
        match active {
            None => Err(InteractionDispatchError::InactiveRealm),
            Some(ticket) if ticket != self.ticket => Err(InteractionDispatchError::WrongRealm),
            Some(_) => Ok(lane),
        }
    }

    fn validate_lane(&self, lane_id: LaneId) -> Result<(), InteractionDispatchError> {
        if lane_id == self.ticket.lane_id {
            Ok(())
        } else {
            Err(InteractionDispatchError::WrongRealm)
        }
    }

    /// Register an ordinary pointer handler in the active owner lane.
    pub fn register_pointer(
        &self,
        handler: impl Fn(&PointerEvent) + 'static,
    ) -> Result<PointerTarget, InteractionDispatchError> {
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        lane.targets
            .borrow_mut()
            .insert(target_id, Rc::new(HandlerCell::new(Rc::new(handler))));
        Ok(PointerTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a target's current pointer handler without changing its identity.
    pub fn replace_pointer(
        &self,
        target: PointerTarget,
        handler: impl Fn(&PointerEvent) + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_handler = cell.replace(Rc::new(handler));
        drop(old_handler);
        Ok(())
    }

    /// Remove a target from future route resolution.
    pub fn unregister_pointer(
        &self,
        target: PointerTarget,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let removed = lane
            .targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        drop(removed);
        Ok(())
    }

    /// Resolve the target-bearing entries of a hit path into one ordered
    /// owner-local route, capturing each entry's local transform.
    ///
    /// Entries without a pointer target (the majority of render objects) are
    /// skipped silently. A registered-then-removed same-lane target is
    /// reported by its position in `path` and does not suppress live
    /// neighbors. A foreign-lane target rejects the whole request.
    pub fn resolve_pointer_route(
        &self,
        path: &[HitTestEntry],
    ) -> Result<RouteResolution, InteractionDispatchError> {
        let lane = self.active_lane()?;
        for target in path.iter().filter_map(|entry| entry.pointer_target) {
            self.validate_lane(target.lane_id)?;
        }

        let (entries, misses) = {
            let registered = lane.targets.borrow();
            let mut entries = Vec::new();
            let mut misses = Vec::new();
            for (path_index, entry) in path.iter().enumerate() {
                let Some(target) = entry.pointer_target else {
                    continue;
                };
                if let Some(cell) = registered.get(&target.target_id) {
                    entries.push(ResolvedHitEntry {
                        handler_cell: Rc::clone(cell),
                        local_transform: LocalEventTransform::capture(entry.transform),
                    });
                } else {
                    misses.push(RouteResolutionMiss::TargetGone { path_index });
                }
            }
            (entries, misses)
        };

        let route_id = RouteId(lane.route_ids.try_next()?);
        lane.routes
            .borrow_mut()
            .insert(route_id, Rc::new(ResolvedHitRoute { entries }));
        Ok(RouteResolution {
            token: ResolvedRouteToken {
                lane_id: self.ticket.lane_id,
                route_id,
            },
            misses,
        })
    }

    /// Invoke an already-resolved pointer route synchronously, leaf-first.
    ///
    /// Every live entry receives its locally transformed event; a per-target
    /// panic is isolated and delivery continues to later entries. The first
    /// captured panic is returned so the caller can perform its mandatory
    /// cleanup before resuming it.
    pub fn invoke_pointer_route(
        &self,
        token: ResolvedRouteToken,
        event: &PointerEvent,
    ) -> Result<Option<RoutePanic>, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(token.lane_id)?;
        let route = lane
            .routes
            .borrow()
            .get(&token.route_id)
            .cloned()
            .ok_or(InteractionDispatchError::StaleRoute)?;
        Ok(route.invoke(event))
    }

    /// Release a cached route after its pointer sequence completes.
    pub fn release_route(&self, token: ResolvedRouteToken) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(token.lane_id)?;
        let removed = lane
            .routes
            .borrow_mut()
            .remove(&token.route_id)
            .ok_or(InteractionDispatchError::StaleRoute)?;
        drop(removed);
        Ok(())
    }
}

impl fmt::Debug for InteractionDispatchHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InteractionDispatchHandle")
            .finish_non_exhaustive()
    }
}

/// A dispatch handle for the lane currently active on this thread.
///
/// This is the seam production (`GestureBinding`) and direct
/// (`HitTestResult::dispatch`) pointer delivery share: the event is already
/// executing on the owner thread inside a lane scope, so the active lane *is*
/// the dispatch authority — no capability is stored in `Send` render state.
///
/// # Errors
///
/// Returns [`InteractionDispatchError::InactiveRealm`] when no lane scope is
/// active on the current thread.
pub(crate) fn active_dispatch_handle() -> Result<InteractionDispatchHandle, InteractionDispatchError>
{
    let ticket = ACTIVE_LANES.with(|active| active.borrow().last().copied());
    ticket
        .map(|ticket| InteractionDispatchHandle { ticket })
        .ok_or(InteractionDispatchError::InactiveRealm)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::atomic::AtomicU64;
    use std::thread::ThreadId;

    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::events::{PointerType, make_down_event};
    use flui_types::Offset;

    assert_not_impl_any!(HandlerCell: Send, Sync);
    assert_not_impl_any!(ResolvedHitEntry: Send, Sync);
    assert_not_impl_any!(ResolvedHitRoute: Send, Sync);

    fn event() -> PointerEvent {
        make_down_event(Offset::ZERO, PointerType::Touch)
    }

    /// A transform-less hit entry addressing `target`, for resolver tests.
    fn hit_entry(target: PointerTarget) -> HitTestEntry {
        HitTestEntry::new(flui_foundation::RenderId::new(1)).pointer_target(target)
    }

    #[test]
    fn private_counters_fail_typed_without_wrapping() {
        let source = MonotonicIdSource::starting_at(u64::MAX);
        assert_eq!(source.try_next().map(NonZeroU64::get), Ok(u64::MAX));
        assert_eq!(
            source.try_next(),
            Err(InteractionDispatchError::IdentifierExhausted)
        );

        let atomic = AtomicU64::new(u64::MAX);
        assert!(try_mint_lane_id(&atomic).is_ok());
        assert!(matches!(
            try_mint_lane_id(&atomic),
            Err(InteractionDispatchError::IdentifierExhausted)
        ));
    }

    #[test]
    fn nested_activation_restores_outer_lane_after_unwind() {
        let outer = InteractionLane::try_new().expect("outer lane");
        let inner = InteractionLane::try_new().expect("inner lane");
        let outer_handle = outer.dispatch_handle();

        outer.enter(|| {
            assert!(outer_handle.register_pointer(|_| {}).is_ok());
            let panic = catch_unwind(AssertUnwindSafe(|| {
                inner.enter(|| panic!("nested probe"));
            }));
            assert!(panic.is_err());
            assert!(outer_handle.register_pointer(|_| {}).is_ok());
        });
    }

    #[test]
    fn route_keeps_cell_alive_and_observes_replacement_after_unregister() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let count = Rc::new(Cell::new(0));
        lane.enter(|| {
            let first = Rc::clone(&count);
            let target = handle
                .register_pointer(move |_| first.set(1))
                .expect("register");
            let route = handle
                .resolve_pointer_route(&[hit_entry(target)])
                .expect("resolve")
                .token();
            let replacement = Rc::clone(&count);
            handle
                .replace_pointer(target, move |_| replacement.set(2))
                .expect("replace");
            handle.unregister_pointer(target).expect("unregister");
            assert_eq!(
                handle
                    .resolve_pointer_route(&[hit_entry(target)])
                    .map(|r| r.misses),
                Ok(vec![RouteResolutionMiss::TargetGone { path_index: 0 }])
            );
            assert!(
                handle
                    .invoke_pointer_route(route, &event())
                    .expect("strong route remains live")
                    .is_none()
            );
            assert_eq!(count.get(), 2);
        });
    }

    #[test]
    fn invoking_snapshots_route_and_handler_before_reentrant_mutation() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let token_slot = Rc::new(Cell::new(None));
            let callback_handle = handle.clone();
            let callback_slot = Rc::clone(&token_slot);
            let target = handle
                .register_pointer(move |_| {
                    if let Some(token) = callback_slot.get() {
                        callback_handle
                            .release_route(token)
                            .expect("route map borrow ended before callback");
                    }
                })
                .expect("register");
            let route = handle
                .resolve_pointer_route(&[hit_entry(target)])
                .expect("resolve")
                .token();
            token_slot.set(Some(route));
            assert!(
                handle
                    .invoke_pointer_route(route, &event())
                    .expect("invocation owns one route Rc")
                    .is_none()
            );
            assert!(matches!(
                handle.invoke_pointer_route(route, &event()),
                Err(InteractionDispatchError::StaleRoute)
            ));
        });
    }

    #[test]
    fn invoke_isolates_per_target_panics_and_returns_the_first_payload() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let order = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let first_order = Rc::clone(&order);
            let first = handle
                .register_pointer(move |_| first_order.borrow_mut().push(1))
                .expect("register first");
            let panicking = handle
                .register_pointer(|_| panic!("first target panic"))
                .expect("register panicking");
            let also_panicking = handle
                .register_pointer(|_| panic!("second target panic"))
                .expect("register second panicking");
            let last_order = Rc::clone(&order);
            let last = handle
                .register_pointer(move |_| last_order.borrow_mut().push(2))
                .expect("register last");

            let route = handle
                .resolve_pointer_route(&[
                    hit_entry(first),
                    hit_entry(panicking),
                    hit_entry(also_panicking),
                    hit_entry(last),
                ])
                .expect("resolve")
                .token();
            let captured = handle
                .invoke_pointer_route(route, &event())
                .expect("route is live")
                .expect("first panic must be captured");

            // Delivery continued past both panicking targets.
            assert_eq!(&*order.borrow(), &[1, 2]);

            // The captured payload is the FIRST panic, resumable by the owner.
            let resumed = catch_unwind(AssertUnwindSafe(|| captured.resume()))
                .expect_err("resume must propagate the panic");
            let message = resumed
                .downcast_ref::<&str>()
                .copied()
                .expect("panic payload is the original &str");
            assert_eq!(message, "first target panic");
        });
    }

    struct DropProbe {
        label: usize,
        owner: ThreadId,
        lane: Weak<LocalLaneInner>,
        cell: Rc<RefCell<Option<Weak<HandlerCell>>>>,
        log: Rc<RefCell<Vec<usize>>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            assert_eq!(
                thread::current().id(),
                self.owner,
                "owner-local capture dropped off its owner thread"
            );
            if let Some(lane) = self.lane.upgrade() {
                assert!(
                    lane.targets.try_borrow().is_ok(),
                    "capture dropped while target map was borrowed"
                );
                assert!(
                    lane.routes.try_borrow().is_ok(),
                    "capture dropped while route map was borrowed"
                );
            }
            let cell = self.cell.borrow().clone();
            if let Some(cell) = cell.and_then(|cell| cell.upgrade()) {
                assert!(
                    cell.current.try_borrow().is_ok(),
                    "capture dropped while handler cell was borrowed"
                );
            }
            self.log.borrow_mut().push(self.label);
        }
    }

    fn register_drop_probe(
        handle: &InteractionDispatchHandle,
        lane: &InteractionLane,
        label: usize,
        log: &Rc<RefCell<Vec<usize>>>,
    ) -> PointerTarget {
        let cell = Rc::new(RefCell::new(None));
        let probe = DropProbe {
            label,
            owner: thread::current().id(),
            lane: Rc::downgrade(&lane.inner),
            cell: Rc::clone(&cell),
            log: Rc::clone(log),
        };
        let target = handle
            .register_pointer(move |_| {
                let _keep_capture_alive = &probe;
            })
            .expect("probe registration");
        let handler_cell = lane
            .inner
            .targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .expect("registered probe cell");
        *cell.borrow_mut() = Some(Rc::downgrade(&handler_cell));
        target
    }

    #[test]
    fn replacement_drops_old_handler_after_internal_borrows_end() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let log = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let target = register_drop_probe(&handle, &lane, 1, &log);
            handle.replace_pointer(target, |_| {}).expect("replacement");
            assert_eq!(&*log.borrow(), &[1]);
        });
    }

    #[test]
    fn last_owner_unregister_drops_handler_after_target_borrow_ends() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let log = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let target = register_drop_probe(&handle, &lane, 1, &log);
            handle
                .unregister_pointer(target)
                .expect("unregister last owner");
            assert_eq!(&*log.borrow(), &[1]);
        });
    }

    #[test]
    fn release_route_drops_last_handler_owner_after_route_borrow_ends() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let log = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let target = register_drop_probe(&handle, &lane, 1, &log);
            let route = handle
                .resolve_pointer_route(&[hit_entry(target)])
                .expect("route")
                .token();
            handle
                .unregister_pointer(target)
                .expect("route becomes the last handler owner");
            assert!(log.borrow().is_empty());

            handle
                .release_route(route)
                .expect("release last handler owner");
            assert_eq!(&*log.borrow(), &[1]);
        });
    }

    struct ReentrantReplacementDropProbe {
        handle: InteractionDispatchHandle,
        completed: Rc<Cell<bool>>,
    }

    impl Drop for ReentrantReplacementDropProbe {
        fn drop(&mut self) {
            let nested_target = self
                .handle
                .register_pointer(|_| {})
                .expect("replacement drop may register through the public handle");
            self.handle
                .unregister_pointer(nested_target)
                .expect("replacement drop may unregister through the public handle");
            self.completed.set(true);
        }
    }

    #[test]
    fn replacement_drop_can_reenter_public_registration_api() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let completed = Rc::new(Cell::new(false));
        lane.enter(|| {
            let probe = ReentrantReplacementDropProbe {
                handle: handle.clone(),
                completed: Rc::clone(&completed),
            };
            let target = handle
                .register_pointer(move |_| {
                    let _keep_probe_alive = &probe;
                })
                .expect("register probe");

            handle
                .replace_pointer(target, |_| {})
                .expect("replace probe handler");
            assert!(completed.get());
        });
    }

    struct TeardownOwnerGoneDropProbe {
        old_handle: InteractionDispatchHandle,
        observed: Rc<Cell<Option<InteractionDispatchError>>>,
    }

    impl Drop for TeardownOwnerGoneDropProbe {
        fn drop(&mut self) {
            let result = self.old_handle.register_pointer(|_| {});
            self.observed.set(result.err());
        }
    }

    #[test]
    fn lane_teardown_removes_registry_before_dropping_handlers() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let observed = Rc::new(Cell::new(None));
        lane.enter(|| {
            let probe = TeardownOwnerGoneDropProbe {
                old_handle: handle.clone(),
                observed: Rc::clone(&observed),
            };
            handle
                .register_pointer(move |_| {
                    let _keep_probe_alive = &probe;
                })
                .expect("register teardown probe");
        });

        drop(lane);

        assert_eq!(observed.get(), Some(InteractionDispatchError::OwnerGone));
    }

    #[test]
    fn teardown_drops_sorted_routes_before_sorted_targets_outside_borrows() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let log = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let route_first = register_drop_probe(&handle, &lane, 1, &log);
            let first_token = handle
                .resolve_pointer_route(&[hit_entry(route_first)])
                .expect("first route")
                .token();
            let route_second = register_drop_probe(&handle, &lane, 2, &log);
            let second_token = handle
                .resolve_pointer_route(&[hit_entry(route_second)])
                .expect("second route")
                .token();
            assert_ne!(first_token, second_token);
            handle
                .unregister_pointer(route_first)
                .expect("route owns first cell now");
            handle
                .unregister_pointer(route_second)
                .expect("route owns second cell now");

            let _target_first = register_drop_probe(&handle, &lane, 3, &log);
            let _target_second = register_drop_probe(&handle, &lane, 4, &log);
        });
        drop(lane);
        assert_eq!(&*log.borrow(), &[1, 2, 3, 4]);
    }
}
