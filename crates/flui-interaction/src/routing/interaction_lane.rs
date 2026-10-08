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

use flui_foundation::geometry::Matrix4;
use flui_foundation::geometry::{Offset, Rect, Size};
use flui_painting::paint::{Path, Shader};

use super::hit_test::{EventPropagation, HitTestEntry, HitTestResult, transform_pointer_event};
use super::pointer_capture::{CaptureRequest, ContactCapture, PointerCapture, PointerCaptureError};
use crate::events::{PanZoomEvent, PointerEvent, PointerEventExt, PointerInfo, ScrollEvent};
use crate::retain::Retain;

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
    /// The render tree is checked out by a frame phase and cannot be read.
    #[error("the render tree is busy and cannot answer a hit test right now")]
    TreeBusy,
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

/// Opaque data-plane identity for an owner-local scroll/pointer-signal target.
///
/// Hit-test entries store this value instead of storing executable scroll
/// callbacks in the data plane. The callback itself remains in the active
/// owner lane and is invoked synchronously during scroll dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for ScrollTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollTarget").finish_non_exhaustive()
    }
}

/// Opaque data-plane identity for an owner-local trackpad pan-zoom target.
///
/// The counterpart of [`ScrollTarget`] for the pan-zoom lane: hit-test
/// entries carry this identity, and the claiming handler itself stays in the
/// active owner lane, invoked synchronously during pan-zoom dispatch. A
/// separate type keeps a wheel claim and a pinch claim from addressing each
/// other's handler.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanZoomTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for PanZoomTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PanZoomTarget").finish_non_exhaustive()
    }
}

/// Opaque data-plane identity for an owner-local path clipper.
///
/// Render objects store this value instead of storing a `Fn(Size) -> Path`
/// callback. The executable clipper remains in the owner lane and is resolved
/// synchronously when the render object needs the path for paint or hit-test.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathClipTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for PathClipTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PathClipTarget").finish_non_exhaustive()
    }
}

/// Opaque data-plane identity for an owner-local shader-mask factory.
///
/// Render objects store this value instead of storing a `Fn(Rect) -> Shader`
/// callback. The executable factory remains in the owner lane and is resolved
/// synchronously when the render object paints with its local bounds.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderMaskTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for ShaderMaskTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShaderMaskTarget").finish_non_exhaustive()
    }
}

/// Opaque data-plane identity for an owner-local payload of any type.
///
/// The untyped counterpart of the targets above, for a widget whose
/// executable state is richer than one callback shape: a drag target's slot,
/// a semantics node's action table. The payload stays in the owner lane as
/// an `Rc<dyn Any>`; render objects and hit-test metadata carry only this
/// ticket, and the owner resolves it back — synchronously, on the owner
/// thread, inside the active lane — when it dispatches to the payload.
///
/// Like every other target it is a copyable `Send + Sync` identity bound to
/// its originating lane, with no raw constructor, accessor, default, or
/// serialization contract.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalPayloadTarget {
    lane_id: LaneId,
    target_id: TargetId,
}

impl fmt::Debug for LocalPayloadTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPayloadTarget").finish_non_exhaustive()
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

/// One pointer event as delivered to one hit-test target, in both of the
/// coordinate spaces a handler can legitimately need.
///
/// Dispatch localises the owned event's measured and predicted positions into
/// the receiving entry's space. The original root-space event travels beside
/// it, including events whose platform supplied no position.
///
/// Both fields borrow values the dispatch already owns, so building one costs
/// no clone and no matrix work beyond the localisation dispatch performs
/// anyway.
///
#[derive(Clone, Copy, Debug)]
pub struct PointerDispatch<'a> {
    /// The event rewritten into the receiving target's own space — the value
    /// a handler wants for anything measured against its own box.
    pub local: &'a PointerEvent,
    /// The event exactly as the platform delivered it, in the root's space.
    ///
    /// Never re-derived from a transform, so a frame that moves the receiving
    /// target between one event and the next cannot shift it.
    pub global: &'a PointerEvent,
    capture: Option<CaptureRequest<'a>>,
}

impl<'a> PointerDispatch<'a> {
    /// A dispatch to a target sitting in the root's own space, where the two
    /// spaces coincide.
    ///
    /// This is the honest spelling for a caller that genuinely has one
    /// untransformed event — a synthetic event in a test, or an entry whose
    /// hit-test path composed no transform at all. It is never the right way
    /// to pass an already-localised event: that would claim a local point is
    /// a global one, which is the defect this type exists to prevent.
    #[must_use]
    pub fn at_root(event: &'a PointerEvent) -> Self {
        Self {
            local: event,
            global: event,
            capture: None,
        }
    }

    /// Borrow a local and root-space event without admitted capture authority.
    #[must_use]
    pub const fn new(local: &'a PointerEvent, global: &'a PointerEvent) -> Self {
        Self {
            local,
            global,
            capture: None,
        }
    }

    /// Claim exclusive delivery of later contact packets to this Down target.
    ///
    /// The original Down still reaches every target in its committed route.
    /// The first successful claimant wins; retaining the returned token keeps
    /// the claim until native termination or deferred explicit release.
    ///
    /// # Errors
    ///
    /// Returns [`PointerCaptureError`] for a non-Down event, a synthetic
    /// dispatch, a prior claim, or a contact whose authority has ended.
    pub fn capture(self) -> Result<PointerCapture, PointerCaptureError> {
        let PointerEvent::Down(press) = self.global else {
            return Err(PointerCaptureError::NotDown);
        };
        self.capture
            .ok_or(PointerCaptureError::Unavailable)?
            .claim(press.pointer)
    }
}

type PointerHandler = Rc<dyn Fn(PointerDispatch<'_>) + 'static>;
type ScrollHandler = Rc<dyn Fn(&ScrollEvent) -> EventPropagation + 'static>;
/// A native gesture in the receiving target's space and the root's space.
#[derive(Clone, Copy, Debug)]
pub struct PanZoomDispatch<'a> {
    /// The gesture localized to the receiving target.
    pub local: &'a PanZoomEvent,
    /// The original gesture delivered by the presentation.
    pub global: &'a PanZoomEvent,
}

type PanZoomHandler = Rc<dyn Fn(PanZoomDispatch<'_>) -> EventPropagation + 'static>;
type PathClipper = Rc<dyn Fn(Size) -> Path + 'static>;
type ShaderMaskFactory = Rc<dyn Fn(Rect<f64>) -> Shader + 'static>;

/// Callback for mouse enter events.
pub type MouseEnterCallback = Rc<dyn Fn(PointerInfo, Offset<f64>) + 'static>;

/// Callback for mouse exit events.
pub type MouseExitCallback = Rc<dyn Fn(PointerInfo, Offset<f64>) + 'static>;

/// Callback for mouse hover events.
pub type MouseHoverCallback = Rc<dyn Fn(PointerInfo, Offset<f64>) + 'static>;

/// Owner-local callback set for one mouse region target.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct MouseRegionCallbacks {
    /// Called when the mouse enters this region.
    pub on_enter: Option<MouseEnterCallback>,
    /// Called when the mouse exits this region.
    pub on_exit: Option<MouseExitCallback>,
    /// Called when the mouse hovers over this region.
    pub on_hover: Option<MouseHoverCallback>,
}

impl crate::retain::Retain for MouseRegionCallbacks {
    fn retain(self) {
        self.on_enter.retain();
        self.on_exit.retain();
        self.on_hover.retain();
    }
}

impl fmt::Debug for MouseRegionCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MouseRegionCallbacks")
            .field("has_on_enter", &self.on_enter.is_some())
            .field("has_on_exit", &self.on_exit.is_some())
            .field("has_on_hover", &self.on_hover.is_some())
            .finish_non_exhaustive()
    }
}

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

pub(super) struct MouseRegionCell {
    current: RefCell<MouseRegionCallbacks>,
}

impl MouseRegionCell {
    fn new(callbacks: MouseRegionCallbacks) -> Self {
        Self {
            current: RefCell::new(callbacks),
        }
    }

    pub(super) fn snapshot(&self) -> MouseRegionCallbacks {
        self.current.borrow().clone()
    }

    fn replace(&self, callbacks: MouseRegionCallbacks) -> MouseRegionCallbacks {
        std::mem::replace(&mut *self.current.borrow_mut(), callbacks)
    }
}

struct ScrollCell {
    current: RefCell<ScrollHandler>,
}

impl ScrollCell {
    fn new(handler: ScrollHandler) -> Self {
        Self {
            current: RefCell::new(handler),
        }
    }

    fn snapshot(&self) -> ScrollHandler {
        Rc::clone(&self.current.borrow())
    }

    fn replace(&self, handler: ScrollHandler) -> ScrollHandler {
        std::mem::replace(&mut *self.current.borrow_mut(), handler)
    }
}

struct PanZoomCell {
    current: RefCell<PanZoomHandler>,
}

impl PanZoomCell {
    fn new(handler: PanZoomHandler) -> Self {
        Self {
            current: RefCell::new(handler),
        }
    }

    fn snapshot(&self) -> PanZoomHandler {
        Rc::clone(&self.current.borrow())
    }

    fn replace(&self, handler: PanZoomHandler) -> PanZoomHandler {
        std::mem::replace(&mut *self.current.borrow_mut(), handler)
    }
}

struct PathClipCell {
    current: RefCell<PathClipper>,
}

impl PathClipCell {
    fn new(clipper: PathClipper) -> Self {
        Self {
            current: RefCell::new(clipper),
        }
    }

    fn snapshot(&self) -> PathClipper {
        Rc::clone(&self.current.borrow())
    }

    fn replace(&self, clipper: PathClipper) -> PathClipper {
        std::mem::replace(&mut *self.current.borrow_mut(), clipper)
    }
}

struct ShaderMaskCell {
    current: RefCell<ShaderMaskFactory>,
}

impl ShaderMaskCell {
    fn new(factory: ShaderMaskFactory) -> Self {
        Self {
            current: RefCell::new(factory),
        }
    }

    fn snapshot(&self) -> ShaderMaskFactory {
        Rc::clone(&self.current.borrow())
    }

    fn replace(&self, factory: ShaderMaskFactory) -> ShaderMaskFactory {
        std::mem::replace(&mut *self.current.borrow_mut(), factory)
    }
}

/// How a resolved entry maps the dispatched global event into its local space.
enum LocalEventTransform {
    /// The entry captured no transform; it receives the global event.
    Global,
    /// The already-composed global-to-local transform, applied directly.
    Local(Matrix4),
    /// The captured transform is singular; the entry is skipped, matching the
    /// pre-route dispatch behavior for non-invertible transforms.
    NonInvertible,
}

impl LocalEventTransform {
    #[inline]
    fn capture(transform: Option<Matrix4>) -> Self {
        match transform {
            None => Self::Global,
            // A composed root identity changes no source reading. Borrow the
            // original event, including both histories, instead of owning a
            // localized copy. Approximate identity would erase real motion.
            Some(transform) if transform == Matrix4::IDENTITY => Self::Global,
            // `HitTestResult` composes `transform` by left-multiplying each
            // ancestor level's own inverse as the walk descends (see
            // `HitTestEntry::transform`'s doc), so it already maps global to
            // local; the computed inverse is used only for admission, not
            // applied to the point again. This also refuses invalid composed
            // transforms on entries supplied directly by callers outside the
            // pipeline's guarded traversal (ADR-0113).
            Some(transform) => {
                if transform.is_invertible() {
                    Self::Local(transform)
                } else {
                    Self::NonInvertible
                }
            }
        }
    }
}

struct ResolvedHitEntry {
    target: PointerTarget,
    owner: Option<std::sync::Arc<DispatchOwner>>,
    handler_cell: Rc<HandlerCell>,
    local_transform: LocalEventTransform,
}

struct ResolvedHitRoute {
    entries: Vec<ResolvedHitEntry>,
}

impl Drop for ResolvedHitRoute {
    /// A saved route can outlive its target's owner. When that owner's close
    /// was preserving, it released its own clones of the cell, so the route
    /// may now hold the last one: the cell then follows that close's
    /// retention instead of running the capture's destructor here (ADR-0127).
    /// Retained entries are settled before any other entry is destroyed. The
    /// rest are destroyed one at a time: once one capture's destructor fails,
    /// or during an unwind, the remaining entries are retained, so a second
    /// failing capture can neither replace the first failure nor abort.
    fn drop(&mut self) {
        let mut released = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            if entry
                .owner
                .as_ref()
                .is_some_and(|owner| owner.retains_withdrawn())
            {
                entry.handler_cell.retain();
            } else {
                released.push(entry);
            }
        }
        let mut first = None;
        for entry in released {
            if first.is_some() || std::thread::panicking() {
                entry.handler_cell.retain();
            } else if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(entry)))
            {
                first = Some(payload);
            }
        }
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }
}

impl ResolvedHitRoute {
    /// Deliver `event` to every entry leaf-first, isolating per-target panics.
    ///
    /// The first panic payload is captured and returned so the dispatch owner
    /// can perform mandatory cleanup (arena close/sweep, route release) before
    /// resuming it; later panics are traced without replacing the first, so
    /// one target's panic never starves the entries after it.
    fn invoke(
        &self,
        event: &PointerEvent,
        capture: Option<&Rc<ContactCapture>>,
    ) -> Option<RoutePanic> {
        let mut first_panic = None;
        // Freeze the admitted selection for this observer round. A callback
        // may release its token or end the contact reentrantly, but it cannot
        // change which entries were committed to this packet's delivery.
        let selected = (!matches!(event, PointerEvent::Down(_)))
            .then(|| capture.and_then(|capture| capture.target()))
            .flatten();
        for entry in &self.entries {
            if selected.is_some_and(|target| target != entry.target) {
                continue;
            }
            if entry.owner.as_ref().is_some_and(|owner| owner.is_closed()) {
                continue;
            }
            let local_event = match &entry.local_transform {
                LocalEventTransform::Global => None,
                LocalEventTransform::Local(local) => {
                    let Some(event) = transform_pointer_event(event, local) else {
                        continue;
                    };
                    Some(event)
                }
                LocalEventTransform::NonInvertible => continue,
            };
            let handler = entry.handler_cell.snapshot();
            // No localised event means the entry composed no transform, so its
            // own space IS the root's.
            let dispatch = PointerDispatch {
                local: local_event.as_ref().unwrap_or(event),
                global: event,
                capture: capture.map(|contact| CaptureRequest {
                    contact,
                    target: entry.target,
                }),
            };
            let delivered = RoutePanic::capture(|| {
                handler(dispatch);
            });
            RoutePanic::preserve_first(&mut first_panic, delivered, "pointer target");

            // Re-entrant replacement or unregistration can make this snapshot
            // the callback's final owner. Keep that destructor inside the same
            // dispatch transaction so later hit targets, the pointer router,
            // and lifecycle cleanup still run before the first unwind resumes.
            let latch = OwnerLatch(entry.owner.clone());
            let snapshot_cleanup = RoutePanic::capture(|| latch.release(handler));
            RoutePanic::preserve_first(
                &mut first_panic,
                snapshot_cleanup,
                "pointer target snapshot cleanup",
            );
        }
        first_panic
    }
}

/// One hit-path entry resolved for
/// [`InteractionDispatchHandle::dispatch_hover_interleaved`]: whichever of a
/// pointer-target handler and a mouse-hover callback that entry carries,
/// captured together so a single per-entry invocation pass can deliver both
/// in the SAME step rather than as two independent full passes.
///
/// Distinct from [`ResolvedHitEntry`]/[`ResolvedHitRoute`] (the batched
/// pointer-only route Down, cached contact moves, Up, and Cancel dispatch
/// through): those resolve and invoke pointer targets as their own complete
/// pass, with no mouse-hover awareness at all, and are cached under a
/// [`ResolvedRouteToken`] for reuse across a whole pointer sequence. A hover
/// move never has a cached Down route to reuse, so this type is resolved and
/// invoked once, inline, with nothing stored in `lane.routes`.
struct ResolvedHoverInterleavedEntry {
    pointer: Option<(Rc<HandlerCell>, LocalEventTransform, OwnerLatch)>,
    hover_callback: Option<(MouseHoverCallback, OwnerLatch)>,
}

/// The first panic captured while invoking a resolved pointer route.
///
/// The dispatch owner must finish its mandatory cleanup (close the arena on
/// Down, sweep on Up, release on Up/Cancel, release an ephemeral route) and
/// then call [`resume`](Self::resume) so the panic propagates under the
/// repository panic policy.
#[doc(hidden)]
#[must_use = "a captured route panic must be resumed after dispatch cleanup"]
pub struct RoutePanic {
    payload: Box<dyn Any + Send>,
}

impl RoutePanic {
    /// Run one synchronous dispatch phase and retain its return value unless it
    /// unwinds.
    pub(crate) fn try_run<T>(run: impl FnOnce() -> T) -> Result<T, Self> {
        catch_unwind(AssertUnwindSafe(run)).map_err(|payload| Self { payload })
    }

    /// Capture an unwind from one synchronous dispatch phase.
    pub(crate) fn capture(run: impl FnOnce()) -> Option<Self> {
        Self::try_run(run).err()
    }

    /// Keep transaction ordering deterministic when several phases panic.
    ///
    /// The earliest payload is resumed after mandatory cleanup; later payloads
    /// are reported but deliberately cannot replace it.
    pub(crate) fn preserve_first(
        first: &mut Option<Self>,
        candidate: Option<Self>,
        phase: &'static str,
    ) {
        let Some(candidate) = candidate else {
            return;
        };
        if first.is_none() {
            *first = Some(candidate);
        } else {
            // Panic payloads are arbitrary user values and may themselves
            // panic in Drop. Discarding a secondary payload normally could
            // therefore replace the first panic (or abort during unwind).
            // Only a known inert payload is released.
            candidate.retain();
            // Diagnostics are user code too. Retire the superseded payload
            // before reporting it, and contain a failing subscriber without
            // recursively reporting that diagnostic's own failure.
            Self::capture(|| {
                tracing::error!(
                    phase,
                    "dispatch phase panicked after an earlier phase; only the first panic is resumed"
                );
            })
            .retain();
        }
    }

    /// Continue unwinding with the captured payload.
    pub fn resume(self) -> ! {
        resume_unwind(self.payload)
    }
}

impl crate::retain::Retain for RoutePanic {
    /// Retain a failure already superseded or reported (ADR-0119).
    fn retain(self) {
        flui_foundation::panic::retain_opaque_payload(self.payload);
    }
}

impl fmt::Debug for RoutePanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutePanic").finish_non_exhaustive()
    }
}

/// Runs a fresh hit test against a live render tree.
///
/// The lane cannot perform a hit test itself: [`HitTestResult`] is defined
/// here, but the render tree that fills one lives two layers up
/// (`flui_rendering::PipelineOwner`), and this crate must not depend upward to
/// reach it. So the capability is *declared* here, where its realm identity and
/// thread affinity already live, and *installed* by whoever owns the tree —
/// the same division `TextInputHandle` uses.
pub trait HitTestProbe {
    /// Fill `result` with everything under `position`, leaf-first.
    ///
    /// `position` is a GLOBAL position in logical pixels, the same space
    /// pointer events arrive in.
    ///
    /// # Errors
    ///
    /// [`InteractionDispatchError::TreeBusy`] when a frame phase holds the
    /// tree and it cannot be read. Answering an empty path there would be a
    /// lie a caller cannot detect — a drag over a live target would read as a
    /// drag over nothing.
    fn probe(
        &self,
        position: Offset<f64>,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError>;
}

/// An owned answer to one hit test, detached from the tree that produced it.
///
/// Owned rather than borrowed on purpose. The issue this implements asks for a
/// result "valid for immediate synchronous dispatch only", and a borrow would
/// enforce that by making the tree unusable for the borrow's whole life —
/// which is the opposite of what a drag needs, since dispatching to a target
/// is exactly when the tree must be free again. Owning the entries instead
/// makes a stale snapshot *useless* rather than *unsound*: the ids in it name
/// render objects that may since have moved or gone, so acting on one across a
/// frame boundary addresses the past, and nothing in the type system pretends
/// otherwise. Take one, dispatch from it, drop it.
#[derive(Debug, Clone)]
pub struct HitTestSnapshot {
    position: Offset<f64>,
    path: Vec<HitTestEntry>,
}

impl HitTestSnapshot {
    /// The global position this snapshot was taken at.
    #[must_use]
    pub fn position(&self) -> Offset<f64> {
        self.position
    }

    /// Everything under [`Self::position`], leaf-first — the same order and
    /// the same entries a pointer-down route would have resolved.
    #[must_use]
    pub fn path(&self) -> &[HitTestEntry] {
        &self.path
    }

    /// Whether anything at all was hit.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}

struct LocalLaneInner {
    target_owners: RefCell<HashMap<TargetId, std::sync::Arc<DispatchOwner>>>,
    ticket: LaneTicket,
    target_ids: MonotonicIdSource,
    route_ids: MonotonicIdSource,
    targets: RefCell<HashMap<TargetId, Rc<HandlerCell>>>,
    mouse_targets: RefCell<HashMap<TargetId, Rc<MouseRegionCell>>>,
    scroll_targets: RefCell<HashMap<TargetId, Rc<ScrollCell>>>,
    pan_zoom_targets: RefCell<HashMap<TargetId, Rc<PanZoomCell>>>,
    path_clip_targets: RefCell<HashMap<TargetId, Rc<PathClipCell>>>,
    shader_mask_targets: RefCell<HashMap<TargetId, Rc<ShaderMaskCell>>>,
    payload_targets: RefCell<HashMap<TargetId, Rc<dyn Any>>>,
    routes: RefCell<HashMap<RouteId, Rc<ResolvedHitRoute>>>,
}

impl LocalLaneInner {
    fn owner_latch(&self, id: TargetId) -> OwnerLatch {
        OwnerLatch(self.target_owners.borrow().get(&id).cloned())
    }
}

thread_local! {
    static LOCAL_LANES: RefCell<HashMap<LaneId, Weak<LocalLaneInner>>> =
        RefCell::new(HashMap::new());
    #[cfg_attr(
        target_os = "android",
        expect(
            clippy::missing_const_for_thread_local,
            reason = "Rust 1.99's OS TLS macro erases this explicit const initializer"
        )
    )]
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
            target_owners: RefCell::new(HashMap::new()),
            ticket: LaneTicket {
                lane_id,
                owner: thread::current().id(),
            },
            target_ids: MonotonicIdSource::new(),
            route_ids: MonotonicIdSource::new(),
            targets: RefCell::new(HashMap::new()),
            mouse_targets: RefCell::new(HashMap::new()),
            scroll_targets: RefCell::new(HashMap::new()),
            pan_zoom_targets: RefCell::new(HashMap::new()),
            path_clip_targets: RefCell::new(HashMap::new()),
            shader_mask_targets: RefCell::new(HashMap::new()),
            payload_targets: RefCell::new(HashMap::new()),
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
            owner: None,
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

/// The fresh-hit-test capability, and only that.
///
/// Reached from widget code as `LifecycleContext::hit_test_handle()`. Acquire it
/// in `init_state` / `did_change_dependencies` and call it from a gesture
/// callback; the frame-capability scope guard rejects acquisition inside
/// `build`, layout, or paint, because a hit test mid-frame reads a tree that
/// phase is still mutating.
///
/// It pairs two things that belong to different scopes: realm identity, from
/// the realm-wide dispatch handle, and the tree, from the **presentation** that
/// minted this handle. A realm may host several presentations, each with its
/// own `PipelineOwner`, so a probe held once per realm would answer every
/// presentation with the first one's tree.
#[derive(Clone)]
pub struct HitTestHandle {
    dispatch: InteractionDispatchHandle,
    probe: Rc<dyn HitTestProbe>,
}

impl HitTestHandle {
    /// Pair a realm ticket with the probe for one presentation's tree.
    #[must_use]
    pub fn new(dispatch: InteractionDispatchHandle, probe: Rc<dyn HitTestProbe>) -> Self {
        Self { dispatch, probe }
    }

    /// Run a fresh hit test at `position` and return an owned snapshot.
    ///
    /// This is the capability a drag needs: the reference's `_DragAvatar`
    /// re-tests at the pointer's *current* global position on every move,
    /// deliberately ignoring wherever the drag's own pointer went down. FLUI's
    /// dispatch resolves a route once at `PointerDown` and replays it, so
    /// without this there is no way to discover a target the drag has since
    /// moved over.
    ///
    /// `position` is GLOBAL, in logical pixels — the space pointer events
    /// arrive in.
    ///
    /// # Errors
    ///
    /// The realm checks [`InteractionDispatchHandle::check_realm`] makes, plus
    /// [`TreeBusy`](InteractionDispatchError::TreeBusy) when a frame phase
    /// holds the render tree and
    /// [`OwnerGone`](InteractionDispatchError::OwnerGone) when the
    /// presentation has closed.
    pub fn hit_test_at(
        &self,
        position: Offset<f64>,
    ) -> Result<HitTestSnapshot, InteractionDispatchError> {
        self.dispatch.check_realm()?;

        let mut result = HitTestResult::new();
        self.probe.probe(position, &mut result)?;
        Ok(HitTestSnapshot {
            position,
            path: result.path().to_vec(),
        })
    }
}

impl fmt::Debug for HitTestHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestHandle").finish_non_exhaustive()
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
        let mouse_targets = self.inner.mouse_targets.take();
        let scroll_targets = self.inner.scroll_targets.take();
        let pan_zoom_targets = self.inner.pan_zoom_targets.take();
        let path_clip_targets = self.inner.path_clip_targets.take();
        let shader_mask_targets = self.inner.shader_mask_targets.take();
        let payload_targets = self.inner.payload_targets.take();

        let mut routes: Vec<_> = routes.into_iter().collect();
        routes.sort_unstable_by_key(|(id, _)| *id);
        drop(routes);

        let mut targets: Vec<_> = targets.into_iter().collect();
        targets.sort_unstable_by_key(|(id, _)| *id);
        drop(targets);

        let mut mouse_targets: Vec<_> = mouse_targets.into_iter().collect();
        mouse_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(mouse_targets);

        let mut scroll_targets: Vec<_> = scroll_targets.into_iter().collect();
        scroll_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(scroll_targets);

        let mut pan_zoom_targets: Vec<_> = pan_zoom_targets.into_iter().collect();
        pan_zoom_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(pan_zoom_targets);

        let mut path_clip_targets: Vec<_> = path_clip_targets.into_iter().collect();
        path_clip_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(path_clip_targets);

        let mut shader_mask_targets: Vec<_> = shader_mask_targets.into_iter().collect();
        shader_mask_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(shader_mask_targets);

        let mut payload_targets: Vec<_> = payload_targets.into_iter().collect();
        payload_targets.sort_unstable_by_key(|(id, _)| *id);
        drop(payload_targets);
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
/// It carries identity and an optional terminal owner latch. Calls succeed while its lane is the
/// active top scope on the owner thread.
#[doc(hidden)]
#[derive(Clone)]
pub struct InteractionDispatchHandle {
    ticket: LaneTicket,
    owner: Option<std::sync::Arc<DispatchOwner>>,
}

#[derive(Default)]
struct DispatchOwner {
    closed: std::sync::atomic::AtomicBool,
    mode: crate::__runtime::CloseTombstone,
}

impl DispatchOwner {
    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Whether this owner closed preservingly: what its close withdrew stays
    /// under retention wherever the last clone lands.
    fn retains_withdrawn(&self) -> bool {
        self.is_closed() && self.mode.preserved()
    }
}

/// The terminal latch of the owner that registered one dispatch target.
///
/// A dispatch snapshots callbacks before invoking any of them, and a callback
/// can close its presentation reentrantly. Every snapshot carries this latch,
/// is skipped once the latch is closed, and is released under that close's
/// retention policy.
#[derive(Clone, Default)]
pub(crate) struct OwnerLatch(Option<std::sync::Arc<DispatchOwner>>);

impl OwnerLatch {
    /// Whether the registering owner has closed since the snapshot.
    pub(crate) fn is_closed(&self) -> bool {
        self.0.as_ref().is_some_and(|owner| owner.is_closed())
    }

    /// Drop a snapshot, or retain it when the owner's close was preserving
    /// and released its own clone first (ADR-0127).
    pub(crate) fn release<T: Retain>(&self, value: T) {
        if self
            .0
            .as_ref()
            .is_some_and(|owner| owner.retains_withdrawn())
        {
            value.retain();
        } else {
            drop(value);
        }
    }

    /// Run one target's snapshotted callback, then release the snapshot and
    /// the target cell inside the same containment. The callback can close
    /// its presentation, leaving these as the capture's last owners: a
    /// failing callback retains them rather than letting its unwind destroy
    /// them, and a successful one releases them under this latch's policy, so
    /// a panicking capture destructor resumes as an ordinary failure
    /// (ADR-0127).
    fn invoke<C: Retain, S: Retain, R>(
        &self,
        cell: C,
        snapshot: S,
        call: impl FnOnce(&S) -> R,
    ) -> R {
        match RoutePanic::try_run(|| call(&snapshot)) {
            Ok(value) => {
                if let Some(failure) = RoutePanic::capture(|| self.release(snapshot)) {
                    cell.retain();
                    failure.resume();
                }
                if let Some(failure) = RoutePanic::capture(|| self.release(cell)) {
                    failure.resume();
                }
                value
            }
            Err(failure) => {
                snapshot.retain();
                cell.retain();
                failure.resume()
            }
        }
    }
}

/// What a presentation's dispatch owner held when its authority was withdrawn.
///
/// Withdrawal detaches every target from the lane at once; the captures are
/// destroyed only by [`DispatchCustody::retire`], once the presentation has
/// withdrawn its other capabilities too, so a reentrant destructor finds the
/// whole presentation closed (ADR-0123). Dropped without `retire`, it retires
/// its contents under the active unwind's policy.
#[doc(hidden)]
#[must_use = "withdrawn dispatch captures must be retired"]
pub struct DispatchCustody {
    failure: Option<crate::__runtime::ClosePanic>,
    pointers: Vec<Rc<HandlerCell>>,
    mice: Vec<Rc<MouseRegionCell>>,
    scrolls: Vec<Rc<ScrollCell>>,
    pans: Vec<Rc<PanZoomCell>>,
    clips: Vec<Rc<PathClipCell>>,
    masks: Vec<Rc<ShaderMaskCell>>,
    payloads: Vec<Rc<dyn Any>>,
}

impl DispatchCustody {
    /// Destroy the withdrawn captures, or retain them once `mode` (or a
    /// failure during retirement) is preserving. The first failure resumes.
    pub(crate) fn retire(mut self, mode: crate::__runtime::CloseMode) {
        if let Some(failure) = self.retire_contents(mode) {
            failure.finish();
        }
    }

    fn retire_contents(
        &mut self,
        mode: crate::__runtime::CloseMode,
    ) -> Option<crate::__runtime::ClosePanic> {
        let mut failure = self.failure.take()?;
        failure.adopt(mode);
        for cell in std::mem::take(&mut self.pointers) {
            failure.retire(cell);
        }
        for cell in std::mem::take(&mut self.mice) {
            // A tracker annotation may still hold the cell; only the
            // callbacks it carried are user-owned.
            let callbacks = cell.replace(MouseRegionCallbacks::default());
            failure.retire(callbacks.on_enter);
            failure.retire(callbacks.on_exit);
            failure.retire(callbacks.on_hover);
            failure.retire(cell);
        }
        for cell in std::mem::take(&mut self.scrolls) {
            failure.retire(cell);
        }
        for cell in std::mem::take(&mut self.pans) {
            failure.retire(cell);
        }
        for cell in std::mem::take(&mut self.clips) {
            failure.retire(cell);
        }
        for cell in std::mem::take(&mut self.masks) {
            failure.retire(cell);
        }
        for payload in std::mem::take(&mut self.payloads) {
            failure.retire(payload);
        }
        Some(failure)
    }
}

impl fmt::Debug for DispatchCustody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DispatchCustody")
            .field("pointers", &self.pointers.len())
            .field("mice", &self.mice.len())
            .finish_non_exhaustive()
    }
}

impl Drop for DispatchCustody {
    fn drop(&mut self) {
        let mode = if std::thread::panicking() {
            crate::__runtime::CloseMode::PreservingFailure
        } else {
            crate::__runtime::CloseMode::Ordinary
        };
        if let Some(failure) = self.retire_contents(mode) {
            failure.finish_contained();
        }
    }
}

impl InteractionDispatchHandle {
    pub(crate) fn scoped_owner(&self) -> Self {
        Self {
            ticket: self.ticket,
            owner: Some(std::sync::Arc::new(DispatchOwner::default())),
        }
    }

    /// Admit a uniquely owned value (a user closure): a rejection during a
    /// preserving close retains it whole.
    fn admit<T>(&self, value: T) -> Result<T, InteractionDispatchError> {
        self.admit_retained(crate::retain::Owned(value))
            .map(|crate::retain::Owned(value)| value)
    }

    /// Admit a value under its own [`Retain`](crate::retain::Retain) policy: a
    /// shared `Rc` the caller still holds is released on rejection, since
    /// dropping a non-last clone runs no user code (ADR-0127).
    fn admit_retained<T: crate::retain::Retain>(
        &self,
        value: T,
    ) -> Result<T, InteractionDispatchError> {
        if let Some(owner) = &self.owner
            && owner.closed.load(std::sync::atomic::Ordering::Acquire)
        {
            let mut failure = crate::__runtime::ClosePanic::for_rejection(owner.mode.mode());
            failure.retire(value);
            failure.finish();
            return Err(InteractionDispatchError::OwnerGone);
        }
        Ok(value)
    }

    fn stamp(&self, lane: &LocalLaneInner, id: TargetId) {
        if let Some(owner) = &self.owner {
            lane.target_owners
                .borrow_mut()
                .insert(id, std::sync::Arc::clone(owner));
        }
    }

    pub(crate) fn close_tombstone(&self) -> Option<crate::__runtime::CloseTombstone> {
        self.owner.as_ref().map(|owner| owner.mode.clone())
    }

    pub(crate) fn close_owner(&self, mode: crate::__runtime::CloseMode) {
        if let Some(custody) = self.withdraw_owner(mode) {
            custody.retire(mode);
        }
    }

    pub(crate) fn close_owner_in(&self, lane: &InteractionLane, mode: crate::__runtime::CloseMode) {
        if let Some(custody) = self.withdraw_owner_in(lane, mode) {
            custody.retire(mode);
        }
    }

    /// Withdraw this owner's authority on the owner thread, keeping custody
    /// of its captures for a later [`DispatchCustody::retire`].
    pub(crate) fn withdraw_owner(
        &self,
        mode: crate::__runtime::CloseMode,
    ) -> Option<DispatchCustody> {
        let lane = LOCAL_LANES
            .try_with(|lanes| {
                lanes
                    .borrow()
                    .get(&self.ticket.lane_id)
                    .and_then(Weak::upgrade)
            })
            .ok()
            .flatten();
        self.withdraw_owner_inner(lane.as_deref(), mode)
    }

    /// Withdraw through the physical lane during realm destruction.
    pub(crate) fn withdraw_owner_in(
        &self,
        lane: &InteractionLane,
        mode: crate::__runtime::CloseMode,
    ) -> Option<DispatchCustody> {
        assert!(
            self.ticket == lane.inner.ticket,
            "BUG: terminal dispatch owner belongs to another realm"
        );
        self.withdraw_owner_inner(Some(&lane.inner), mode)
    }

    fn withdraw_owner_inner(
        &self,
        lane: Option<&LocalLaneInner>,
        mode: crate::__runtime::CloseMode,
    ) -> Option<DispatchCustody> {
        let owner = self.owner.as_ref()?;
        // The close's reentry window stays open until the custody retires.
        let failure = crate::__runtime::ClosePanic::for_close(mode, owner.mode.clone());
        owner
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        let mut custody = DispatchCustody {
            failure: Some(failure),
            pointers: Vec::new(),
            mice: Vec::new(),
            scrolls: Vec::new(),
            pans: Vec::new(),
            clips: Vec::new(),
            masks: Vec::new(),
            payloads: Vec::new(),
        };
        let Some(lane) = lane else {
            return Some(custody);
        };
        let ids = {
            let mut owners = lane.target_owners.borrow_mut();
            let mut ids: Vec<_> = owners
                .iter()
                .filter_map(|(id, held)| std::sync::Arc::ptr_eq(held, owner).then_some(*id))
                .collect();
            ids.sort_unstable();
            for id in &ids {
                owners.remove(id);
            }
            ids
        };
        // Detach every typed target before any capture can reenter this lane.
        // A cached route that still holds one of these cells keeps it; the
        // route's own release follows this close's policy.
        custody.pointers = take_targets(&lane.targets, &ids);
        custody.mice = take_targets(&lane.mouse_targets, &ids);
        custody.scrolls = take_targets(&lane.scroll_targets, &ids);
        custody.pans = take_targets(&lane.pan_zoom_targets, &ids);
        custody.clips = take_targets(&lane.path_clip_targets, &ids);
        custody.masks = take_targets(&lane.shader_mask_targets, &ids);
        custody.payloads = take_targets(&lane.payload_targets, &ids);
        Some(custody)
    }

    fn active_lane(&self) -> Result<Rc<LocalLaneInner>, InteractionDispatchError> {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| owner.closed.load(std::sync::atomic::Ordering::Acquire))
        {
            return Err(InteractionDispatchError::OwnerGone);
        }
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

    /// Confirm this handle may act on the currently active realm.
    ///
    /// The whole of the realm contract in one call — right thread, a realm
    /// active, that realm is this one, and its lane is still alive — for
    /// capabilities minted from this handle that do their own work.
    ///
    /// # Errors
    ///
    /// [`WrongThread`](InteractionDispatchError::WrongThread),
    /// [`InactiveRealm`](InteractionDispatchError::InactiveRealm),
    /// [`WrongRealm`](InteractionDispatchError::WrongRealm), or
    /// [`OwnerGone`](InteractionDispatchError::OwnerGone).
    pub fn check_realm(&self) -> Result<(), InteractionDispatchError> {
        self.active_lane().map(|_lane| ())
    }

    fn validate_lane(&self, lane_id: LaneId) -> Result<(), InteractionDispatchError> {
        if lane_id == self.ticket.lane_id {
            Ok(())
        } else {
            Err(InteractionDispatchError::WrongRealm)
        }
    }

    /// Admit a mutation of `target`: it belongs to this realm and, through a
    /// presentation-scoped handle, to the owner that registered it. Another
    /// presentation sharing the realm cannot replace, remove or detach it.
    fn validate_target(
        &self,
        lane: &LocalLaneInner,
        lane_id: LaneId,
        target_id: TargetId,
    ) -> Result<(), InteractionDispatchError> {
        self.validate_lane(lane_id)?;
        if let Some(owner) = &self.owner {
            let owned = lane
                .target_owners
                .borrow()
                .get(&target_id)
                .is_some_and(|held| std::sync::Arc::ptr_eq(held, owner));
            if !owned {
                return Err(InteractionDispatchError::TargetGone);
            }
        }
        Ok(())
    }

    /// Register an ordinary pointer handler in the active owner lane.
    pub fn register_pointer(
        &self,
        handler: impl Fn(PointerDispatch<'_>) + 'static,
    ) -> Result<PointerTarget, InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
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
        handler: impl Fn(PointerDispatch<'_>) + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
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
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Register mouse-region callbacks in the active owner lane.
    pub fn register_mouse_region(
        &self,
        callbacks: MouseRegionCallbacks,
    ) -> Result<MouseRegionTarget, InteractionDispatchError> {
        let callbacks = self.admit_retained(callbacks)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.mouse_targets
            .borrow_mut()
            .insert(target_id, Rc::new(MouseRegionCell::new(callbacks)));
        Ok(MouseRegionTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a mouse-region target's callbacks without changing identity.
    pub fn replace_mouse_region(
        &self,
        target: MouseRegionTarget,
        callbacks: MouseRegionCallbacks,
    ) -> Result<(), InteractionDispatchError> {
        let callbacks = self.admit_retained(callbacks)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let cell = lane
            .mouse_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_callbacks = cell.replace(callbacks);
        drop(old_callbacks);
        Ok(())
    }

    /// Remove a mouse-region target from future annotation resolution,
    /// without touching the target's shared cell contents.
    ///
    /// This is a lane-level primitive, not the still-mounted-region case: a
    /// render object that is merely rebuilt with an empty callback set stays
    /// registered (see `MouseRegion::sync_mouse_region_target`,
    /// `crates/flui-widgets/src/interaction/mouse_region.rs`, which calls
    /// [`replace_mouse_region`](Self::replace_mouse_region) with the empty
    /// set instead of this method: a mounted region's callbacks are replaced
    /// in place, never unregistered). What remains for this method is releasing a
    /// target this lane no longer wants to resolve fresh annotations
    /// against. Because it only drops the *lane's* map entry, an existing
    /// strong `Rc` clone held elsewhere — e.g.
    /// [`MouseTracker`](super::MouseTracker)'s own per-region cache,
    /// populated the last time this target was resolved from a hit test —
    /// keeps observing whatever the cell's contents happen to be until that
    /// clone is itself dropped, which is how a target already mid-resolution
    /// can still deliver one last, correctly-scoped callback. Use
    /// [`detach_mouse_region`](Self::detach_mouse_region) instead when the
    /// goal is to guarantee no further delivery at all, including from an
    /// already-cached clone — that is what a permanently unmounted render
    /// object needs.
    pub fn unregister_mouse_region(
        &self,
        target: MouseRegionTarget,
    ) -> Result<(), InteractionDispatchError> {
        drop(self.take_mouse_region_cell(target)?);
        Ok(())
    }

    /// Remove a mouse-region target AND immediately invalidate its cell's
    /// callbacks in place, for a region whose render object is being
    /// permanently detached (unmounted) — see
    /// [`unregister_mouse_region`](Self::unregister_mouse_region) for the
    /// softer lane-level primitive and why a still-mounted, rebuilt-with-no-
    /// callbacks region uses neither of these two methods at all.
    ///
    /// [`MouseTracker`](super::MouseTracker) caches an `Rc` clone of this
    /// same cell across frames (its `annotations` map, keyed by region id)
    /// so it can resolve an exit callback for a region that no longer
    /// appears in a fresh hit test. Removing only this lane's own map entry
    /// leaves that cached clone's callbacks intact, so a stationary
    /// device's next postframe recheck
    /// ([`MouseTracker::update_all_devices`](super::MouseTracker::update_all_devices))
    /// would still invoke the unmounted region's `on_exit` — the spurious
    /// exit a detached region must never produce. Overwriting the shared
    /// cell's contents here is the invalidation: every remaining holder of the `Rc`,
    /// including that cached clone, observes empty callbacks on its next
    /// `snapshot()`.
    pub fn detach_mouse_region(
        &self,
        target: MouseRegionTarget,
    ) -> Result<(), InteractionDispatchError> {
        let cell = self.take_mouse_region_cell(target)?;
        drop(cell.replace(MouseRegionCallbacks::default()));
        Ok(())
    }

    /// Shared removal step for [`unregister_mouse_region`](Self::unregister_mouse_region)
    /// and [`detach_mouse_region`](Self::detach_mouse_region): take this
    /// lane's own map entry, leaving the caller to decide whether the
    /// returned cell's callbacks should also be invalidated in place.
    fn take_mouse_region_cell(
        &self,
        target: MouseRegionTarget,
    ) -> Result<Rc<MouseRegionCell>, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        lane.mouse_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)
    }

    pub(super) fn resolve_mouse_region(
        &self,
        target: MouseRegionTarget,
    ) -> Result<(Rc<MouseRegionCell>, OwnerLatch), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .mouse_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        Ok((cell, lane.owner_latch(target.target_id)))
    }

    /// Register a scroll/pointer-signal handler in the active owner lane.
    pub fn register_scroll(
        &self,
        handler: impl Fn(&ScrollEvent) -> EventPropagation + 'static,
    ) -> Result<ScrollTarget, InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.scroll_targets
            .borrow_mut()
            .insert(target_id, Rc::new(ScrollCell::new(Rc::new(handler))));
        Ok(ScrollTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a scroll target's current handler without changing identity.
    pub fn replace_scroll(
        &self,
        target: ScrollTarget,
        handler: impl Fn(&ScrollEvent) -> EventPropagation + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let cell = lane
            .scroll_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_handler = cell.replace(Rc::new(handler));
        drop(old_handler);
        Ok(())
    }

    /// Remove a scroll target from future dispatch.
    pub fn unregister_scroll(&self, target: ScrollTarget) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .scroll_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Invoke one registered scroll target synchronously.
    pub fn invoke_scroll_target(
        &self,
        target: ScrollTarget,
        event: &ScrollEvent,
    ) -> Result<EventPropagation, InteractionDispatchError> {
        self.invoke_scroll_target_with_claim(target, event, || {})
    }

    /// Publish a consumed route before retiring its snapshotted captures.
    /// The hook belongs to the binding, not the consumer callback channel.
    pub(crate) fn invoke_scroll_target_with_claim(
        &self,
        target: ScrollTarget,
        event: &ScrollEvent,
        claimed: impl FnOnce(),
    ) -> Result<EventPropagation, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .scroll_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let latch = lane.owner_latch(target.target_id);
        if latch.is_closed() {
            return Err(InteractionDispatchError::TargetGone);
        }
        let handler = cell.snapshot();
        Ok(latch.invoke(cell, handler, |handler| {
            let propagation = handler(event);
            if propagation.should_stop() {
                claimed();
            }
            propagation
        }))
    }

    /// Register a trackpad pan-zoom claim handler in the active owner lane.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active on this
    /// thread, or when the lane's private identity source is exhausted.
    pub fn register_pan_zoom(
        &self,
        handler: impl Fn(PanZoomDispatch<'_>) -> EventPropagation + 'static,
    ) -> Result<PanZoomTarget, InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.pan_zoom_targets
            .borrow_mut()
            .insert(target_id, Rc::new(PanZoomCell::new(Rc::new(handler))));
        Ok(PanZoomTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a pan-zoom target's current handler without changing identity.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn replace_pan_zoom(
        &self,
        target: PanZoomTarget,
        handler: impl Fn(PanZoomDispatch<'_>) -> EventPropagation + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let handler = self.admit(handler)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let cell = lane
            .pan_zoom_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_handler = cell.replace(Rc::new(handler));
        drop(old_handler);
        Ok(())
    }

    /// Remove a pan-zoom target from future dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn unregister_pan_zoom(
        &self,
        target: PanZoomTarget,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .pan_zoom_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Invoke one registered pan-zoom target synchronously.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn invoke_pan_zoom_target(
        &self,
        target: PanZoomTarget,
        event: PanZoomDispatch<'_>,
    ) -> Result<EventPropagation, InteractionDispatchError> {
        self.invoke_pan_zoom_target_with_claim(target, event, || {})
    }

    pub(crate) fn invoke_pan_zoom_target_with_claim(
        &self,
        target: PanZoomTarget,
        event: PanZoomDispatch<'_>,
        claimed: impl FnOnce(),
    ) -> Result<EventPropagation, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .pan_zoom_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let latch = lane.owner_latch(target.target_id);
        if latch.is_closed() {
            return Err(InteractionDispatchError::TargetGone);
        }
        let handler = cell.snapshot();
        Ok(latch.invoke(cell, handler, |handler| {
            let propagation = handler(event);
            if propagation.should_stop() {
                claimed();
            }
            propagation
        }))
    }

    /// Register a path clipper in the active owner lane.
    pub fn register_path_clipper(
        &self,
        clipper: impl Fn(Size) -> Path + 'static,
    ) -> Result<PathClipTarget, InteractionDispatchError> {
        let clipper = self.admit(clipper)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.path_clip_targets
            .borrow_mut()
            .insert(target_id, Rc::new(PathClipCell::new(Rc::new(clipper))));
        Ok(PathClipTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a path clipper without changing its data-plane identity.
    pub fn replace_path_clipper(
        &self,
        target: PathClipTarget,
        clipper: impl Fn(Size) -> Path + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let clipper = self.admit(clipper)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let cell = lane
            .path_clip_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_clipper = cell.replace(Rc::new(clipper));
        drop(old_clipper);
        Ok(())
    }

    /// Remove a path clipper from future resolution.
    pub fn unregister_path_clipper(
        &self,
        target: PathClipTarget,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .path_clip_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Invoke a registered path clipper synchronously.
    pub fn invoke_path_clipper(
        &self,
        target: PathClipTarget,
        size: Size,
    ) -> Result<Path, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .path_clip_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let latch = lane.owner_latch(target.target_id);
        if latch.is_closed() {
            return Err(InteractionDispatchError::TargetGone);
        }
        let clipper = cell.snapshot();
        Ok(latch.invoke(cell, clipper, |clipper| clipper(size)))
    }

    /// Register a shader-mask factory in the active owner lane.
    pub fn register_shader_mask(
        &self,
        factory: impl Fn(Rect<f64>) -> Shader + 'static,
    ) -> Result<ShaderMaskTarget, InteractionDispatchError> {
        let factory = self.admit(factory)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.shader_mask_targets
            .borrow_mut()
            .insert(target_id, Rc::new(ShaderMaskCell::new(Rc::new(factory))));
        Ok(ShaderMaskTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a shader-mask factory without changing its data-plane identity.
    pub fn replace_shader_mask(
        &self,
        target: ShaderMaskTarget,
        factory: impl Fn(Rect<f64>) -> Shader + 'static,
    ) -> Result<(), InteractionDispatchError> {
        let factory = self.admit(factory)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let cell = lane
            .shader_mask_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let old_factory = cell.replace(Rc::new(factory));
        drop(old_factory);
        Ok(())
    }

    /// Remove a shader-mask factory from future resolution.
    pub fn unregister_shader_mask(
        &self,
        target: ShaderMaskTarget,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .shader_mask_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Invoke a registered shader-mask factory synchronously.
    pub fn invoke_shader_mask(
        &self,
        target: ShaderMaskTarget,
        bounds: Rect<f64>,
    ) -> Result<Shader, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        let cell = lane
            .shader_mask_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)?;
        let latch = lane.owner_latch(target.target_id);
        if latch.is_closed() {
            return Err(InteractionDispatchError::TargetGone);
        }
        let factory = cell.snapshot();
        Ok(latch.invoke(cell, factory, |factory| factory(bounds)))
    }

    /// Register an owner-local payload in the active owner lane.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active on this
    /// thread, or when the lane's private identity source is exhausted.
    pub fn register_local_payload(
        &self,
        payload: Rc<dyn Any>,
    ) -> Result<LocalPayloadTarget, InteractionDispatchError> {
        let payload = self.admit_retained(payload)?;
        let lane = self.active_lane()?;
        let target_id = TargetId(lane.target_ids.try_next()?);
        self.stamp(&lane, target_id);
        lane.payload_targets.borrow_mut().insert(target_id, payload);
        Ok(LocalPayloadTarget {
            lane_id: self.ticket.lane_id,
            target_id,
        })
    }

    /// Replace a payload without changing its data-plane identity.
    ///
    /// The previous payload is dropped after the lane's borrow is released,
    /// so its destructor may re-enter this handle.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn replace_local_payload(
        &self,
        target: LocalPayloadTarget,
        payload: Rc<dyn Any>,
    ) -> Result<(), InteractionDispatchError> {
        let payload = self.admit_retained(payload)?;
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let previous = {
            let mut payloads = lane.payload_targets.borrow_mut();
            let slot = payloads
                .get_mut(&target.target_id)
                .ok_or(InteractionDispatchError::TargetGone)?;
            std::mem::replace(slot, payload)
        };
        drop(previous);
        Ok(())
    }

    /// Remove a payload from future resolution.
    ///
    /// A caller that resolved the payload earlier keeps its own strong
    /// reference until it drops it.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn unregister_local_payload(
        &self,
        target: LocalPayloadTarget,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_target(&lane, target.lane_id, target.target_id)?;
        let removed = lane
            .payload_targets
            .borrow_mut()
            .remove(&target.target_id)
            .ok_or(InteractionDispatchError::TargetGone)?;
        lane.target_owners.borrow_mut().remove(&target.target_id);
        drop(removed);
        Ok(())
    }

    /// Resolve a payload to a strong owner-local reference.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError`] when no lane is active, when the
    /// target belongs to a different lane, or when it is already gone.
    pub fn resolve_local_payload(
        &self,
        target: LocalPayloadTarget,
    ) -> Result<Rc<dyn Any>, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(target.lane_id)?;
        lane.payload_targets
            .borrow()
            .get(&target.target_id)
            .cloned()
            .ok_or(InteractionDispatchError::TargetGone)
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
                        target,
                        owner: lane.target_owners.borrow().get(&target.target_id).cloned(),
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
        self.invoke_pointer_route_with_capture(token, event, None)
    }

    pub(crate) fn invoke_pointer_route_with_capture(
        &self,
        token: ResolvedRouteToken,
        event: &PointerEvent,
        capture: Option<&Rc<ContactCapture>>,
    ) -> Result<Option<RoutePanic>, InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(token.lane_id)?;
        let route = lane
            .routes
            .borrow()
            .get(&token.route_id)
            .cloned()
            .ok_or(InteractionDispatchError::StaleRoute)?;
        let mut first_panic = route.invoke(event, capture);
        // A hit callback may release this token re-entrantly, leaving the
        // invocation snapshot as the route's final owner. Keep its entry and
        // HandlerCell destructors in the transaction returned to the binding,
        // rather than unwinding before the root router and arena lifecycle.
        let snapshot_cleanup = RoutePanic::capture(|| drop(route));
        RoutePanic::preserve_first(
            &mut first_panic,
            snapshot_cleanup,
            "resolved pointer route snapshot cleanup",
        );
        Ok(first_panic)
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

    pub(crate) fn release_route_for_close(
        &self,
        token: ResolvedRouteToken,
        failure: &mut crate::__runtime::ClosePanic,
    ) -> Result<(), InteractionDispatchError> {
        let lane = self.active_lane()?;
        self.validate_lane(token.lane_id)?;
        let removed = lane
            .routes
            .borrow_mut()
            .remove(&token.route_id)
            .ok_or(InteractionDispatchError::StaleRoute)?;
        if failure.preserving() {
            failure.retire(removed);
        } else {
            match Rc::try_unwrap(removed) {
                Ok(mut route) => {
                    for entry in std::mem::take(&mut route.entries) {
                        failure.retire(entry.handler_cell);
                    }
                }
                Err(shared) => failure.retire(shared),
            }
        }
        Ok(())
    }

    /// Resolve and invoke a hit path's pointer targets and mouse-hover
    /// regions together, leaf-first, in a single per-entry pass — so a
    /// `Listener` and a nested `MouseRegion` on the same path fire in
    /// hit-test order relative to EACH OTHER (one per-entry loop over the
    /// leaf-first path) instead of two independent full passes over it.
    ///
    /// Used only by the coalesced ephemeral hover-move dispatch
    /// (`GestureBinding::flush_pending_moves_kernel`'s `PendingMove::Hover`
    /// arm). A hover move never has a cached Down route, so unlike
    /// [`resolve_pointer_route`](Self::resolve_pointer_route)/
    /// [`invoke_pointer_route`](Self::invoke_pointer_route) (shared by Down,
    /// cached contact moves, Up, and Cancel), nothing here is stored in
    /// `lane.routes` for later reuse or release — resolution and invocation
    /// both happen in this one call, and every OTHER event kind keeps
    /// dispatching exactly as before.
    ///
    /// Every pointer-target entry is resolved upfront, before any callback
    /// runs — matching the batched route's re-entrancy guarantee: a callback
    /// that unregisters a LATER entry's target mid-walk does not suppress
    /// that later entry, because its handler cell was already captured. A
    /// foreign-lane pointer target aborts the whole dispatch (matching
    /// `resolve_pointer_route`'s `?`-propagated validation); a foreign-lane
    /// or since-unregistered mouse-hover target is skipped and traced
    /// per-entry instead (matching [`resolve_mouse_region`](Self::resolve_mouse_region)'s
    /// existing per-annotation behavior), so one absent region does not drop
    /// its neighbors.
    pub(super) fn dispatch_hover_interleaved(
        &self,
        path: &[HitTestEntry],
        event: &PointerEvent,
    ) -> Option<RoutePanic> {
        if !path
            .iter()
            .any(|entry| entry.pointer_target.is_some() || entry.mouse_annotation.is_some())
        {
            return None;
        }
        let lane = match self.active_lane() {
            Ok(lane) => lane,
            Err(error) => {
                tracing::error!(
                    ?error,
                    "hover-interleaved dispatch outside an active interaction lane; \
                     event not delivered"
                );
                return None;
            }
        };
        for target in path.iter().filter_map(|entry| entry.pointer_target) {
            if let Err(error) = self.validate_lane(target.lane_id) {
                tracing::error!(
                    ?error,
                    "pointer target from a foreign lane during interleaved hover dispatch; \
                     event not delivered"
                );
                return None;
            }
        }

        // Mirrors `MouseTracker::dispatch_hover`'s own gate: only a
        // buttons-empty `Move` carries hover semantics (a contact drag is
        // not a hover). Gating resolution (not just invocation) means a
        // non-hover-shaped event never resolves mouse-region callbacks at
        // all.
        let hover_qualifies = matches!(
            event,
            PointerEvent::Move(update) if update.buttons.is_empty()
        );

        let resolved: Vec<ResolvedHoverInterleavedEntry> = {
            let targets = lane.targets.borrow();
            path.iter()
                .filter_map(|entry| {
                    let pointer = entry.pointer_target.and_then(|target| {
                        targets.get(&target.target_id).cloned().map(|cell| {
                            (
                                cell,
                                LocalEventTransform::capture(entry.transform),
                                lane.owner_latch(target.target_id),
                            )
                        })
                    });
                    let hover_callback = hover_qualifies
                        .then_some(entry.mouse_annotation)
                        .flatten()
                        .and_then(|annotation| {
                            match self.resolve_mouse_region(annotation.target) {
                                Ok((cell, latch)) => {
                                    cell.snapshot().on_hover.map(|callback| (callback, latch))
                                }
                                Err(error) => {
                                    tracing::debug!(
                                        ?error,
                                        "mouse-hover target unavailable during interleaved dispatch"
                                    );
                                    None
                                }
                            }
                        });
                    (pointer.is_some() || hover_callback.is_some()).then_some(
                        ResolvedHoverInterleavedEntry {
                            pointer,
                            hover_callback,
                        },
                    )
                })
                .collect()
        };

        let pointer = crate::events::pointer_info(event).copied();
        let position = event.position();
        let mut first_panic = None;
        for entry in resolved {
            if let Some((cell, transform, latch)) = entry.pointer {
                let local_event = match &transform {
                    LocalEventTransform::Local(local) => transform_pointer_event(event, local),
                    LocalEventTransform::Global | LocalEventTransform::NonInvertible => None,
                };
                // An earlier callback may have closed this entry's owner.
                if !latch.is_closed()
                    && !matches!(transform, LocalEventTransform::NonInvertible)
                    && (!matches!(transform, LocalEventTransform::Local(_))
                        || local_event.is_some())
                {
                    let handler = cell.snapshot();
                    let dispatch = match local_event.as_ref() {
                        Some(local) => PointerDispatch::new(local, event),
                        None => PointerDispatch::at_root(event),
                    };
                    let delivered = RoutePanic::capture(|| {
                        handler(dispatch);
                    });
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        delivered,
                        "pointer target (hover-interleaved)",
                    );
                    // Same re-entrancy care as `ResolvedHitRoute::invoke`: keep
                    // the snapshot's destructor inside this transaction rather
                    // than unwinding before later entries and the caller's
                    // mandatory cleanup run.
                    let snapshot_cleanup = RoutePanic::capture(|| latch.release(handler));
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        snapshot_cleanup,
                        "pointer target snapshot cleanup (hover-interleaved)",
                    );
                }
                let cell_cleanup = RoutePanic::capture(|| latch.release(cell));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    cell_cleanup,
                    "pointer target cell cleanup (hover-interleaved)",
                );
            }
            if let Some((callback, latch)) = entry.hover_callback {
                if !latch.is_closed()
                    && let (Some(pointer), Some(position)) = (pointer, position)
                {
                    let delivered = RoutePanic::capture(|| callback(pointer, position));
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        delivered,
                        "mouse hover callback (hover-interleaved)",
                    );
                }
                let snapshot_cleanup = RoutePanic::capture(|| latch.release(callback));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    snapshot_cleanup,
                    "mouse hover snapshot cleanup (hover-interleaved)",
                );
            }
        }
        first_panic
    }
}

fn take_targets<T: ?Sized>(
    targets: &RefCell<HashMap<TargetId, Rc<T>>>,
    ids: &[TargetId],
) -> Vec<Rc<T>> {
    let mut targets = targets.borrow_mut();
    ids.iter().filter_map(|id| targets.remove(id)).collect()
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
        .map(|ticket| InteractionDispatchHandle {
            ticket,
            owner: None,
        })
        .ok_or(InteractionDispatchError::InactiveRealm)
}

/// Resolve a path clipper target through the currently active owner lane.
///
/// Render objects use this narrow function to keep executable clipper closures
/// out of render storage while still resolving a size-dependent clip path at
/// paint/hit-test time.
pub fn resolve_path_clip_target(
    target: PathClipTarget,
    size: Size,
) -> Result<Path, InteractionDispatchError> {
    active_dispatch_handle()?.invoke_path_clipper(target, size)
}

/// Resolve a shader-mask target through the currently active owner lane.
///
/// Render objects use this narrow function to keep executable shader factories
/// out of render storage while still resolving a bounds-dependent shader at
/// paint time.
pub fn resolve_shader_mask_target(
    target: ShaderMaskTarget,
    bounds: Rect<f64>,
) -> Result<Shader, InteractionDispatchError> {
    active_dispatch_handle()?.invoke_shader_mask(target, bounds)
}

/// Resolve an owner-local payload through the currently active owner lane.
///
/// The dispatcher that found `target` in render data or hit-test metadata
/// calls this on the owner thread and downcasts the result to the type the
/// registering widget stored. Holding the returned `Rc` keeps the payload
/// alive past a later unregistration, the same as a resolved pointer route.
///
/// # Errors
///
/// [`InactiveRealm`](InteractionDispatchError::InactiveRealm) outside any lane
/// scope, [`WrongRealm`](InteractionDispatchError::WrongRealm) for a ticket another
/// lane minted (a dropped lane's included), and
/// [`TargetGone`](InteractionDispatchError::TargetGone) once it was
/// unregistered.
pub fn resolve_local_payload(
    target: LocalPayloadTarget,
) -> Result<Rc<dyn Any>, InteractionDispatchError> {
    active_dispatch_handle()?.resolve_local_payload(target)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::events::{PointerKind, make_down_event};
    use flui_foundation::geometry::Offset;

    assert_not_impl_any!(HandlerCell: Send, Sync);
    assert_not_impl_any!(ScrollCell: Send, Sync);
    assert_not_impl_any!(PathClipCell: Send, Sync);
    assert_not_impl_any!(ShaderMaskCell: Send, Sync);
    assert_not_impl_any!(ResolvedHitEntry: Send, Sync);
    assert_not_impl_any!(ResolvedHitRoute: Send, Sync);

    fn event() -> PointerEvent {
        make_down_event(Offset::ZERO, PointerKind::Touch).expect("finite input")
    }

    /// A transform-less hit entry addressing `target`, for resolver tests.
    fn hit_entry(target: PointerTarget) -> HitTestEntry {
        HitTestEntry::new(flui_foundation::RenderId::new(1)).pointer_target(target)
    }

    // Lane isolation matrix: per-target panics and reentrant replacement drops.
    #[test]
    fn lane_isolation_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "invoke_isolates_per_target_panics_and_returns_the_first_payload",
                invoke_isolates_per_target_panics_and_returns_the_first_payload,
            ),
            (
                "replacement_drop_can_reenter_public_registration_api",
                replacement_drop_can_reenter_public_registration_api,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

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

    // Owner-local payload tickets: identity, replacement, removal, realm
    // checks and lane-drop release, each alone.
    #[test]
    fn local_payload_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "resolve_returns_the_registered_payload",
                resolve_returns_the_registered_payload,
            ),
            (
                "replace_keeps_the_ticket_and_swaps_the_payload",
                replace_keeps_the_ticket_and_swaps_the_payload,
            ),
            (
                "an_unregistered_payload_is_target_gone",
                an_unregistered_payload_is_target_gone,
            ),
            (
                "resolving_outside_any_lane_is_inactive_realm",
                resolving_outside_any_lane_is_inactive_realm,
            ),
            (
                "another_lanes_ticket_is_wrong_realm",
                another_lanes_ticket_is_wrong_realm,
            ),
            (
                "dropping_the_lane_releases_its_payloads",
                dropping_the_lane_releases_its_payloads,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn resolve_returns_the_registered_payload() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let payload: Rc<dyn Any> = Rc::new(7_u32);
            let target = handle
                .register_local_payload(Rc::clone(&payload))
                .expect("register");
            let resolved = resolve_local_payload(target).expect("resolve");
            assert!(Rc::ptr_eq(&resolved, &payload));
        });
    }

    fn replace_keeps_the_ticket_and_swaps_the_payload() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let target = handle
                .register_local_payload(Rc::new(1_u32))
                .expect("register");
            handle
                .replace_local_payload(target, Rc::new(2_u32))
                .expect("replace");
            let resolved = resolve_local_payload(target).expect("resolve");
            assert_eq!(resolved.downcast_ref::<u32>(), Some(&2));
        });
    }

    fn an_unregistered_payload_is_target_gone() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        lane.enter(|| {
            let target = handle
                .register_local_payload(Rc::new(()))
                .expect("register");
            handle.unregister_local_payload(target).expect("unregister");
            assert_eq!(
                resolve_local_payload(target).err(),
                Some(InteractionDispatchError::TargetGone)
            );
            assert_eq!(
                handle.replace_local_payload(target, Rc::new(())).err(),
                Some(InteractionDispatchError::TargetGone)
            );
        });
    }

    fn resolving_outside_any_lane_is_inactive_realm() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let target = lane.enter(|| {
            handle
                .register_local_payload(Rc::new(()))
                .expect("register")
        });
        assert_eq!(
            resolve_local_payload(target).err(),
            Some(InteractionDispatchError::InactiveRealm)
        );
    }

    fn another_lanes_ticket_is_wrong_realm() {
        let first = InteractionLane::try_new().expect("first lane");
        let second = InteractionLane::try_new().expect("second lane");
        let first_handle = first.dispatch_handle();
        let target = first.enter(|| {
            first_handle
                .register_local_payload(Rc::new(()))
                .expect("register")
        });
        second.enter(|| {
            assert_eq!(
                resolve_local_payload(target).err(),
                Some(InteractionDispatchError::WrongRealm)
            );
        });
    }

    fn dropping_the_lane_releases_its_payloads() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let payload: Rc<dyn Any> = Rc::new(3_u32);
        let weak = Rc::downgrade(&payload);
        lane.enter(|| {
            let _target = handle.register_local_payload(payload).expect("register");
        });
        assert!(weak.upgrade().is_some(), "the lane holds the payload");
        drop(lane);
        assert!(weak.upgrade().is_none(), "the lane released it on drop");
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
}
