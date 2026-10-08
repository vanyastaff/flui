//! Gesture Binding - owner-runtime coordinator for pointer event handling
//!
//! GestureBinding is the main entry point for handling pointer events in the
//! gesture system. It coordinates hit testing, event routing, arena management,
//! and pointer move event coalescing.
//!
//! A refused Down admits no route. Its Move/Up/Cancel tail stays refused until
//! that contact terminates. Refusals occupy a fixed array bounded by the same
//! limit as admitted contacts. If it fills, the binding suppresses every
//! untracked Move/Up/Cancel until a lifecycle reset, while admitted contacts and
//! fresh valid Downs continue. This conservative fallback prevents a hostile
//! stream of distinct refused identities from growing owner state indefinitely.
//!
//! # Architecture
//!
//! ```text
//! Platform Events (winit, etc.)
//!         │
//!         ▼
//! ┌─────────────────────┐
//! │   GestureBinding    │ (owned by UiRealm/HeadlessBinding)
//! │  ┌───────────────┐  │
//! │  │ Hit Test Cache│  │  (RefCell<HashMap<PointerId, CachedPointerRoute>>)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ Pending Moves │  │  (RefCell<HashMap<PointerId, PendingMoveState>>)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ PointerRouter │  │  (routes events to handlers)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureArena  │  │  (conflict resolution)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ GestureSettings│ │  (device-specific config)
//! │  └───────────────┘  │
//! └─────────────────────┘
//!         │
//!         ▼
//!    Gesture Recognizers
//! ```
//!
//! # Lifecycle
//!
//! 1. **Pointer Down**: Hit test → cache result → dispatch → close arena
//! 2. **Contact Move**: Reuse the Down route → dispatch (coalesced)
//! 3. **Hover Move**: Fresh hit test → ephemeral dispatch (coalesced)
//! 4. **Pointer Up**: Use resolved Down route → dispatch → sweep arena → clear cache
//! 5. **Pointer Cancel**: Use resolved Down route → dispatch recognizer rejection →
//!    clear cache without a binding sweep
//! 6. **Enter/Leave**: cached route mid-contact; otherwise a fresh ephemeral
//!    hit test at the device's last-known hover position (the events carry
//!    no position of their own)
//!
//! # Scroll routing
//!
//! Scroll observers follow a fresh hit path for every packet. Consumption is
//! selected by the first handler returning `Stop`, then remains on that exact
//! target and admitted transform through the sequence, including at an extent
//! or after the target retires. End/Cancel, device removal and owner lifecycle
//! withdrawal release selection before callbacks. A phase-less wheel burst
//! expires after 500 ms without a packet on the presentation's monotonic clock.
//! Source identity is the native `DeviceId`, or `PointerId` when the platform
//! supplies no device; tool and role remain mutable metadata. At most 32 sources
//! are admitted; overflow reaches fresh observers but cannot start an unlatched
//! consumptive sequence.
//! `binding_input_contract_matrix` covers first failure, retired captures and
//! reentrant replacement; `scroll_physics_and_activity` covers nested scrollers
//! and clock-driven wheel inactivity through the widget consumer.
//!
//! Native pan/zoom uses an independent bounded source lease, selected on first
//! consumption and localized with its admitted transform while retaining the
//! actual root-space event. Repeated Start for the same contact replaces its
//! admission authority but preserves its consumer through old-session
//! cancellation; only a terminal edge permits the next fresh source to choose
//! a different target. `binding_input_contract_matrix` covers native observer,
//! claimant and capture-retirement failures, replacement, and recovery.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::{GestureBinding, HitTestResult};
//! use flui_foundation::geometry::Point;
//! use flui_platform_api::{EventTime, pointer::{PointerButton, PointerButtons,
//!     PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition,
//!     PointerPress, PointerSample}};
//!
//! let binding = GestureBinding::new();
//! let pointer = PointerInfo::new(PointerId::try_from(1_u64)?, PointerKind::Touch);
//! let sample = PointerSample::new(EventTime::from_nanos(1),
//!     PointerPosition::try_new(Point::new(12.0, 24.0))?);
//! let event = PointerEvent::Down(PointerPress::new(pointer, PointerButton::PRIMARY,
//!     PointerButtons::NONE, sample));
//!
//! binding.handle_pointer_event(&event, |position| {
//!     assert_eq!(position.dx, 12.0);
//!     assert_eq!(position.dy, 24.0);
//!     // An embedding's render tree supplies entries here.
//!     HitTestResult::new()
//! });
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::events::{
    CancelReason, DeviceId, PointerCancel, PointerEvent, PointerEventExt, PointerInfo, PointerKind,
};
use crate::routing::pointer_capture::ContactCapture;
use flui_foundation::MonotonicClock;
use flui_foundation::geometry::Offset;
use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase, ScrollEvent, ScrollPhase};
use smallvec::SmallVec;

use crate::{
    arena::{DetachedArenaBatch, GestureArena},
    ids::PointerId,
    processing::{PointerEventResampler, SamplingClock},
    routing::{
        HitTestResult, MouseTracker, PanZoomRoute, PointerMotionKind, PointerRouter,
        ResolvedRouteToken, RoutePanic, ScrollRoute, active_dispatch_handle,
    },
    settings::GestureSettings,
};

fn terminal_hit_test(_: Offset<f64>) -> HitTestResult {
    unreachable!("BUG: terminal Cancel never hit-tests")
}

/// Contact admission identity, independent of a reusable platform pointer ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerSequence(u64);

/// Per-pointer state cached at Down. The owner-local resolved token retains
/// the admitted handlers and transforms that Move reuses and Up/Cancel releases.
/// The original spatial hit snapshot is no longer needed after resolution.
///
/// `token` is `None` when no interaction lane was active at Down (a
/// gesture-only binding without a mounted tree) or when the path carried no
/// pointer targets; the pointer router still routes such events.
#[derive(Clone)]
struct CachedPointerRoute {
    token: Option<ResolvedRouteToken>,
    sequence: PointerSequence,
    capture: Rc<ContactCapture>,
    resampler: PointerEventResampler,
    /// Latest admitted source metadata, so lifecycle cancellation preserves
    /// the tool and role most recently observed on this contact.
    pointer: PointerInfo,
    /// Last accepted measured contact timestamp, retained for lifecycle
    /// cancellation that has no new platform timestamp of its own.
    time: flui_platform_api::EventTime,
}

struct DetachedPointerSequence {
    cached: Option<CachedPointerRoute>,
    pending_move: Option<PendingMoveState>,
    arena: DetachedArenaBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingMoveGeneration(u64);

/// One frame-coalesced move and the only routing state needed to deliver it.
///
/// Contact moves reuse the generation-checked route cached at pointer-down.
/// Hover moves have no contact sequence, so they retain the fresh hit-test
/// result produced for that move and are dispatched ephemerally at frame flush.
enum PendingMove {
    Contact {
        event: PointerEvent,
        sequence: PointerSequence,
    },
    Hover {
        event: PointerEvent,
        hit_test: HitTestResult,
    },
}

/// Exact slot for a direct/coalesced move in the canonical pending map.
///
/// `pending: Some` is queued; a flush takes the payload and leaves
/// `pending: None` as its in-flight marker before invoking any user callback.
/// Re-entrant input replaces or removes that marker, making the detached
/// payload stale without a second epoch map.
struct PendingMoveState {
    generation: PendingMoveGeneration,
    pending: Option<PendingMove>,
}

impl PendingMoveState {
    const fn queued(generation: PendingMoveGeneration, pending: PendingMove) -> Self {
        Self {
            generation,
            pending: Some(pending),
        }
    }

    const fn generation(&self) -> PendingMoveGeneration {
        self.generation
    }

    const fn is_queued(&self) -> bool {
        self.pending.is_some()
    }

    fn is_in_flight(&self, expected: PendingMoveGeneration) -> bool {
        self.generation == expected && self.pending.is_none()
    }
}

struct ResamplerSnapshot {
    pointer_id: PointerId,
    sequence: PointerSequence,
    token: Option<ResolvedRouteToken>,
    resampler: PointerEventResampler,
    capture: Rc<ContactCapture>,
}

/// A resampling policy change was requested while contact sequences were
/// active.
///
/// Sampling is a sequence-level invariant: one contact is either direct or
/// resampled for its entire lifetime. Rejecting a mid-sequence mode change
/// prevents duplicate queues and discontinuities in velocity estimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "cannot change pointer resampling while {active_pointer_count} pointer sequence(s) are active"
)]
pub struct ResamplingModeChangeError {
    active_pointer_count: usize,
}

impl ResamplingModeChangeError {
    /// Number of active pointer sequences that prevented the change.
    #[inline]
    #[must_use]
    pub const fn active_pointer_count(self) -> usize {
        self.active_pointer_count
    }
}

/// An explicit resampling window did not advance beyond its sample time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("pointer sampling window must satisfy next_sample_time > sample_time")]
pub struct InvalidSamplingWindow;

/// Re-entrancy barrier for abnormal pointer-sequence teardown.
struct PointerTeardownGuard<'a> {
    active: &'a RefCell<HashSet<PointerId>>,
    pointer: PointerId,
}

impl<'a> PointerTeardownGuard<'a> {
    fn try_enter(active: &'a RefCell<HashSet<PointerId>>, pointer: PointerId) -> Option<Self> {
        if active.borrow_mut().insert(pointer) {
            Some(Self { active, pointer })
        } else {
            None
        }
    }
}

impl Drop for PointerTeardownGuard<'_> {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.pointer);
    }
}

struct AllPointerTeardownGuard<'a> {
    active: &'a Cell<bool>,
}

impl<'a> AllPointerTeardownGuard<'a> {
    fn try_enter(active: &'a Cell<bool>) -> Option<Self> {
        if active.replace(true) {
            None
        } else {
            Some(Self { active })
        }
    }
}

impl Drop for AllPointerTeardownGuard<'_> {
    fn drop(&mut self) {
        self.active.set(false);
    }
}

/// Truncate a `f64` to `f64` for pointer position conversion.
///
/// Lossless for any screen-pixel coordinate: a `f64` mantissa rounds
/// at ~7 decimal digits and physical pointer positions are reported
/// in device pixels (≤ 2^23 ≈ 8M), so `f64 → f64` is exact in that
/// range. Used at the W3C→flui boundary where upstream carries `f64`
/// physical pixels and our `Offset` stores `f64`.
///
/// Upper bound on simultaneously-tracked pointers.
///
/// Per-pointer state (hit tests, resamplers, arena entries, recogniser maps)
/// grows with active pointers; an untrusted event source could open unbounded
/// pointers to exhaust memory. Real hardware tops out around 10–16 touches, so
/// this generous cap never rejects a legitimate gesture.
const MAX_SIMULTANEOUS_POINTERS: usize = 32;

/// A phase-less wheel burst ends after owner-clock inactivity, independently
/// of native timestamps (which may be absent or restart).
const SCROLL_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SignalSource {
    Device(DeviceId),
    Pointer(PointerId),
}

impl From<PointerInfo> for SignalSource {
    fn from(pointer: PointerInfo) -> Self {
        pointer
            .device
            .map_or(Self::Pointer(pointer.id), Self::Device)
    }
}

/// Rc identity is the admission authority: a reentrant Begin cannot be
/// overwritten by the Stop or terminal cleanup of an older scroll round.
/// No user callback or capture is owned by this record.
struct ScrollSequence {
    route: Cell<Option<ScrollRoute>>,
    last_packet: Cell<web_time::Instant>,
    phase_less: Cell<bool>,
}

enum ScrollAdmission {
    Active(Rc<ScrollSequence>),
    Terminal(Option<ScrollRoute>),
    Refused,
}

struct PanZoomSequence {
    route: Cell<Option<PanZoomRoute>>,
    pointer: PointerId,
}

enum PanZoomAdmission {
    Active(Rc<PanZoomSequence>),
    Terminal(Option<PanZoomRoute>),
    Refused,
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
    Tracking([Option<RefusedContact>; MAX_SIMULTANEOUS_POINTERS]),
    Saturated,
}

impl Default for RefusedContacts {
    fn default() -> Self {
        Self::Tracking([None; MAX_SIMULTANEOUS_POINTERS])
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

    fn refuse(&mut self, pointer: PointerId) {
        self.insert(pointer, RefusalPolicy::NativeTerminal);
    }

    fn release(&mut self, pointer: PointerInfo) {
        self.insert(pointer.id, RefusalPolicy::FreshDown(pointer.device));
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

/// Central coordinator for gesture event handling.
///
/// GestureBinding manages the complete lifecycle of pointer events:
/// - Performs hit testing on pointer down
/// - Caches hit test results for subsequent events
/// - Coalesces high-frequency pointer move events (100+ events/sec → 1 per
///   frame)
/// - Routes events through the PointerRouter
/// - Manages arena lifecycle (close on down, sweep on up)
///
/// # Ownership
///
/// A UI runtime owns one `GestureBinding`. Prefer accessing the binding through
/// the active `HeadlessBinding` / `UiRealm`; process-global gesture ownership is
/// intentionally not part of ADR-0027.
///
/// # Event Coalescing
///
/// Desktop platforms can generate 100+ mouse move events per second.
/// GestureBinding delivers one current move per pointer and frame, retaining
/// earlier hardware samples in its coalesced history for velocity consumers.
/// Call `flush_pending_moves()` once per frame to process the events.
///
/// # Thread affinity
///
/// `GestureBinding` is owner-local. Its pointer router and executable gesture
/// callbacks are not `Send + Sync`; render hit-test entries and route tokens
/// remain on the separate data plane.
pub struct GestureBinding {
    closed: Cell<bool>,
    close_mode: crate::__runtime::CloseTombstone,
    capture_wake: RefCell<Option<std::sync::Weak<dyn flui_platform_api::PlatformWindow>>>,
    /// Resolved Down routes per pointer.
    /// Down resolves once; move/up events reuse the cached route.
    hit_tests: RefCell<HashMap<PointerId, CachedPointerRoute>>,

    /// First consumptive scroll route per source, bounded like contact routes.
    scroll_sequences: RefCell<HashMap<SignalSource, Rc<ScrollSequence>>>,
    pan_zoom_sequences: RefCell<HashMap<SignalSource, Rc<PanZoomSequence>>>,

    /// Pending move events for coalescing.
    /// Only the latest move per pointer is kept.
    pending_moves: RefCell<HashMap<PointerId, PendingMoveState>>,

    /// Refused contacts cannot fall through to hover delivery.
    refused_contacts: RefCell<RefusedContacts>,

    /// Sampling mode captured by each pointer sequence at Down.
    ///
    /// Mode changes are accepted only while there are no active contacts.
    resampling_enabled: Cell<bool>,

    /// Frame-paced clock that produces `(now, next)` pairs for the
    /// resamplers. Only consulted when `resampling_enabled` is true.
    sampling_clock: Cell<SamplingClock>,
    clock: std::sync::Arc<dyn MonotonicClock>,

    /// Routes pointer events to registered handlers.
    pointer_router: PointerRouter,

    /// Per-device enter/exit/hover/cursor state for this presentation.
    mouse_tracker: MouseTracker,

    /// Resolves conflicts between competing gesture recognizers.
    ///
    /// Binding-owned ([`SweepModel::BindingDriven`](crate::arena::SweepModel)):
    /// this binding runs the close-on-down / sweep-on-up lifecycle itself in
    /// [`handle_pointer_event`](Self::handle_pointer_event), and an app shell
    /// hands a clone of this handle to the view subtree (via flui-widgets'
    /// `GestureArenaScope`) so every detector below competes here without any
    /// recognizer self-closing or self-sweeping the shared arena.
    arena: GestureArena,

    /// Default gesture settings (can be overridden per device).
    default_settings: GestureSettings,

    /// Prevent route/member destructors from starting a replacement pointer
    /// transaction while abnormal teardown is still draining its snapshot.
    tearing_down_pointers: RefCell<HashSet<PointerId>>,
    tearing_down_all_pointers: Cell<bool>,

    /// Monotonic contact identity. Platform pointer IDs are reusable, so
    /// frame-delayed work must also match this generation before dispatch.
    next_pointer_sequence: Cell<u64>,

    /// Monotonic identity for direct/coalesced move slots. Unlike a pointer
    /// sequence, hover has no Down transaction, so it needs its own identity
    /// while moving through `Queued -> InFlight`.
    next_pending_move_generation: Cell<u64>,
}

impl Default for GestureBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureBinding {
    /// Create a new GestureBinding with default settings.
    ///
    /// `GestureBinding` is owner-local; prefer using the binding owned by the
    /// active UI runtime (`HeadlessBinding`/`UiRealm`) over a process global.
    pub fn new() -> Self {
        Self::with_settings_and_clock(
            GestureSettings::default(),
            std::sync::Arc::new(flui_foundation::SystemClock),
        )
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        Self::with_settings_and_clock(settings, std::sync::Arc::new(flui_foundation::SystemClock))
    }

    /// Create against an explicit monotonic clock.
    ///
    /// This is the canonical constructor for deterministic runtimes such as
    /// `HeadlessBinding`: the binding, its arena, and every recognizer deadline
    /// observe the same clock instead of a test harness owning a second arena.
    pub fn with_clock(clock: std::sync::Arc<dyn MonotonicClock>) -> Self {
        Self::with_settings_and_clock(GestureSettings::default(), clock)
    }

    fn with_settings_and_clock(
        settings: GestureSettings,
        clock: std::sync::Arc<dyn MonotonicClock>,
    ) -> Self {
        Self {
            closed: Cell::new(false),
            close_mode: crate::__runtime::CloseTombstone::default(),
            capture_wake: RefCell::new(None),
            hit_tests: RefCell::new(HashMap::new()),
            scroll_sequences: RefCell::new(HashMap::new()),
            pan_zoom_sequences: RefCell::new(HashMap::new()),
            pending_moves: RefCell::new(HashMap::new()),
            refused_contacts: RefCell::new(RefusedContacts::default()),
            resampling_enabled: Cell::new(false),
            sampling_clock: Cell::new(SamplingClock::default()),
            clock: std::sync::Arc::clone(&clock),
            pointer_router: PointerRouter::new(),
            mouse_tracker: MouseTracker::new(),
            arena: GestureArena::binding_driven(clock),
            default_settings: settings,
            tearing_down_pointers: RefCell::new(HashSet::new()),
            tearing_down_all_pointers: Cell::new(false),
            next_pointer_sequence: Cell::new(0),
            next_pending_move_generation: Cell::new(0),
        }
    }

    // ========================================================================
    // Resampler Wiring
    // ========================================================================

    pub(crate) fn set_pointer_capture_wake(
        &self,
        window: std::sync::Weak<dyn flui_platform_api::PlatformWindow>,
    ) {
        *self.capture_wake.borrow_mut() = Some(window);
    }

    /// Enable per-pointer event resampling on [`Self::flush_pending_moves`].
    ///
    /// Off by default. When on, every pointer that emits a `Move` event
    /// gets its own [`PointerEventResampler`] (one per `PointerId`),
    /// paced by the configured [`SamplingClock`]. On
    /// [`Self::flush_pending_moves`], each resampler is sampled once
    /// per frame and the resampled events are dispatched through the
    /// same routing path as direct events.
    ///
    /// Idempotent. A mode change is rejected while any contact is active:
    /// resampling is a property of the complete Down-to-Up sequence, not a
    /// switchable event filter.
    ///
    /// # Errors
    ///
    /// Returns [`ResamplingModeChangeError`] when `enabled` differs from the
    /// current mode and at least one pointer sequence is active.
    pub fn set_resampling_enabled(&self, enabled: bool) -> Result<(), ResamplingModeChangeError> {
        if self.resampling_enabled.get() == enabled {
            return Ok(());
        }
        let active_pointer_count = self.hit_tests.borrow().len();
        if active_pointer_count != 0 {
            return Err(ResamplingModeChangeError {
                active_pointer_count,
            });
        }
        self.resampling_enabled.set(enabled);
        Ok(())
    }

    /// Returns whether per-pointer resampling is enabled.
    #[inline]
    #[must_use]
    pub fn is_resampling_enabled(&self) -> bool {
        self.resampling_enabled.get()
    }

    /// Replace the sampling clock used to pace resamplers.
    ///
    /// Existing per-pointer resamplers are **not** reset — they pick
    /// up the new cadence on their next `sample()` call. Caller is
    /// responsible for ensuring the new clock's period is sensible
    /// for the input devices currently being tracked.
    pub fn set_sampling_clock(&self, clock: SamplingClock) {
        self.sampling_clock.set(clock);
    }

    /// Returns a copy of the active sampling clock.
    #[inline]
    #[must_use]
    pub fn sampling_clock(&self) -> SamplingClock {
        self.sampling_clock.get()
    }

    /// Number of active per-pointer resamplers.
    #[inline]
    #[must_use]
    pub fn active_resampler_count(&self) -> usize {
        self.hit_tests
            .borrow()
            .values()
            .filter(|cached| cached.resampler.is_tracked())
            .count()
    }

    /// Whether accepted contact samples still need an owner frame.
    /// An active contact with an empty queue creates no sampling demand.
    #[must_use]
    pub fn has_pending_pointer_samples(&self) -> bool {
        self.hit_tests.borrow().values().any(|cached| {
            cached.capture.release_requested() || cached.resampler.has_pending_events()
        })
    }

    // ========================================================================
    // Component Accessors
    // ========================================================================

    /// Get the pointer router.
    #[inline]
    pub fn pointer_router(&self) -> &PointerRouter {
        &self.pointer_router
    }

    /// The pointer left the hosting window: fire `on_exit` for every
    /// hovered region and reset the cursor. The platform's window-leave
    /// signal (winit `CursorLeft`) routes here; without it a widget hovered
    /// at the moment the cursor crosses the window edge stays hovered
    /// forever. Must run inside the owner lane scope, like every other
    /// dispatch on this binding.
    pub fn handle_pointer_left_window(&self) {
        self.mouse_tracker.dispatch_window_left();
    }

    /// Mouse tracking state owned by the same presentation as gesture routes.
    #[inline]
    #[must_use]
    pub fn mouse_tracker(&self) -> &MouseTracker {
        &self.mouse_tracker
    }

    /// Get the gesture arena.
    #[inline]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// Get the default gesture settings.
    #[inline]
    pub fn default_settings(&self) -> &GestureSettings {
        &self.default_settings
    }

    // ========================================================================
    // Event Handling
    // ========================================================================

    /// Handle a pointer event.
    ///
    /// This is the main entry point for processing pointer events.
    /// The `hit_test_fn` is called on pointer down to determine which
    /// targets are under the pointer.
    ///
    /// # Arguments
    ///
    /// * `event` - The pointer event to handle
    /// * `hit_test_fn` - Function to perform hit testing (called on pointer
    ///   down)
    ///
    /// See the [`crate::binding`] module example for constructing a checked Down and supplying
    /// the hit-test callback.
    pub fn handle_pointer_event<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.closed.get() {
            let mut failure = crate::__runtime::ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(crate::retain::Owned(hit_test_fn));
            failure.finish();
            return;
        }
        let ending = matches!(
            event,
            PointerEvent::Down(_) | PointerEvent::Up(_) | PointerEvent::Cancel(_)
        )
        .then(|| crate::PointerEventExt::pointer_id(event))
        .flatten();
        let mut first = self.drain_capture_losses(ending);
        let delivered =
            RoutePanic::capture(|| self.handle_pointer_event_kernel(event, hit_test_fn));
        RoutePanic::preserve_first(&mut first, delivered, "pointer input after capture loss");
        if let Some(failure) = first {
            failure.resume();
        }
    }
    /// Handle pointer event without hit testing.
    ///
    /// Use this when you already have a hit test result or want to
    /// manually control hit testing.
    pub fn handle_pointer_event_with_result(&self, event: &PointerEvent, result: &HitTestResult) {
        self.handle_pointer_event(event, |_| result.clone());
    }

    // ========================================================================
    // Event Coalescing
    // ========================================================================

    /// Flush pending coalesced move events.
    ///
    /// Call this once per frame to process all coalesced pointer move events.
    /// Returns the number of events processed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_interaction::GestureBinding;
    /// let binding = GestureBinding::new();
    /// // The presentation calls this before layout and paint each frame.
    /// assert_eq!(binding.flush_pending_moves(), 0); // no input queued yet
    /// ```
    pub fn flush_pending_moves(&self) -> usize {
        if self.closed.get() {
            return 0;
        }
        let sample_window = self
            .is_resampling_enabled()
            .then(|| self.sampling_clock.get().tick())
            .flatten();
        self.flush_pending_moves_kernel(sample_window)
    }

    /// Flush pending moves with an explicit sampling window.
    ///
    /// This is the deterministic entry point for replay and test clocks. It
    /// also makes the ownership boundary explicit: the presentation scheduler
    /// supplies frame time, while each active pointer sequence owns its
    /// resampler state.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidSamplingWindow`] when
    /// `next_sample_time <= sample_time`.
    pub fn flush_pending_moves_at(
        &self,
        sample_time: web_time::Instant,
        next_sample_time: web_time::Instant,
    ) -> Result<usize, InvalidSamplingWindow> {
        if next_sample_time <= sample_time {
            return Err(InvalidSamplingWindow);
        }
        if self.closed.get() {
            return Ok(0);
        }
        Ok(self.flush_pending_moves_kernel(Some((sample_time, next_sample_time))))
    }

    fn flush_pending_moves_kernel(
        &self,
        sample_window: Option<(web_time::Instant, web_time::Instant)>,
    ) -> usize {
        if self.tearing_down_all_pointers.get() || !self.tearing_down_pointers.borrow().is_empty() {
            return 0;
        }

        let mut first_panic = self.drain_capture_losses(None);
        // Freeze the complete direct/coalesced frame batch before any user
        // callback runs. Re-entrant moves replace their exact marker and
        // therefore always belong to the next frame.
        let drained = {
            let mut moves = self.pending_moves.borrow_mut();
            let mut pointers: SmallVec<[PointerId; 4]> = moves
                .iter()
                .filter_map(|(&pointer, state)| state.is_queued().then_some(pointer))
                .collect();
            pointers.sort_unstable();
            let mut drained: SmallVec<[(PointerId, PendingMoveGeneration, PendingMove); 4]> =
                SmallVec::with_capacity(pointers.len());
            for pointer_id in pointers {
                let entry = moves
                    .get_mut(&pointer_id)
                    .expect("BUG: snapshotted pending move remains present");
                let generation = entry.generation();
                if let Some(pending) = entry.pending.take() {
                    drained.push((pointer_id, generation, pending));
                }
            }
            drained
        };

        let mut count = 0;
        // A resampled contact has exactly one queue: the resampler owned by
        // its cached Down route. Snapshot route capabilities before callbacks
        // so no map borrow crosses executable user code.
        if self.is_resampling_enabled()
            && let Some((sample_time, next_sample_time)) = sample_window
        {
            let mut samples: SmallVec<[ResamplerSnapshot; 4]> = self
                .hit_tests
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
            samples.sort_unstable_by_key(|snapshot| snapshot.pointer_id);

            for snapshot in samples {
                let ResamplerSnapshot {
                    pointer_id,
                    sequence,
                    token,
                    resampler,
                    capture,
                } = snapshot;
                let delivery = capture.begin_delivery();
                resampler.sample(sample_time, next_sample_time, |resampled| {
                    if self.is_current_sequence(pointer_id, sequence) {
                        let delivered = self.dispatch_event(&resampled, token);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "resampled pointer move",
                        );
                        count += 1;
                    }
                });
                drop(delivery);
            }
        }

        for (pointer_id, generation, pending) in drained {
            if !self.is_pending_move_in_flight(pointer_id, generation) {
                continue;
            }
            match pending {
                PendingMove::Contact { event, sequence } => {
                    if self.is_current_sequence(pointer_id, sequence) {
                        let delivered = self.dispatch_on_cached_route(pointer_id, &event);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "coalesced contact move",
                        );
                        count += 1;
                    }
                }
                PendingMove::Hover { event, hit_test } => {
                    // `MouseRegion::on_hover` has no device-kind gate
                    // (unlike the mouse/stylus-only enter/exit tracking)
                    // and rides the same coalesced dispatch cadence as the
                    // ordinary hit-test targets, not the immediate
                    // per-raw-event enter/exit update. It interleaves with
                    // those ordinary targets in ONE per-entry, leaf-first
                    // walk (`dispatch_ephemeral_with_hover_interleaved`) so a
                    // `Listener` and a nested `MouseRegion` fire in hit-test
                    // order relative to each other, not as two separate full
                    // passes.
                    let delivered =
                        self.dispatch_ephemeral_with_hover_interleaved(&event, &hit_test);
                    RoutePanic::preserve_first(&mut first_panic, delivered, "coalesced hover move");
                    count += 1;
                }
            }
            self.remove_pending_move_if_in_flight(pointer_id, generation);
        }

        let losses = self.drain_capture_losses(None);
        RoutePanic::preserve_first(&mut first_panic, losses, "frame capture loss");

        if let Some(panic) = first_panic {
            panic.resume();
        }
        count
    }

    /// Check whether direct/coalesced or resampled motion still needs a frame.
    #[inline]
    pub fn has_pending_motion(&self) -> bool {
        self.pending_moves
            .borrow()
            .values()
            .any(|state| state.is_queued())
            || self.hit_tests.borrow().values().any(|route| {
                route.capture.release_requested() || route.resampler.has_pending_events()
            })
    }

    /// Get the number of queued motion events across both canonical paths.
    #[inline]
    pub fn pending_move_count(&self) -> usize {
        let direct = self
            .pending_moves
            .borrow()
            .values()
            .filter(|state| state.is_queued())
            .count();
        let resampled = self
            .hit_tests
            .borrow()
            .values()
            .map(|route| route.resampler.pending_event_count())
            .sum::<usize>();
        direct + resampled
    }

    // ========================================================================
    // Pointer Sequence State
    // ========================================================================

    /// Check if there's a cached hit test for a pointer.
    #[inline]
    pub fn has_hit_test(&self, pointer_id: PointerId) -> bool {
        self.hit_tests.borrow().contains_key(&pointer_id)
    }

    /// Cancel every binding-owned state slot for a pointer.
    ///
    /// This is the abnormal-sequence counterpart to receiving Up/Cancel: it
    /// releases the retained hit route, discards queued/resampled movement,
    /// and rejects the unresolved arena without selecting a winner.
    pub fn cancel_pointer_sequence(&self, pointer_id: PointerId) {
        let mut first = self.drain_capture_losses(Some(pointer_id));
        let Some(guard) = PointerTeardownGuard::try_enter(&self.tearing_down_pointers, pointer_id)
        else {
            if let Some(failure) = first {
                failure.resume();
            }
            return;
        };
        let detached = self.detach_pointer_sequence(pointer_id);
        let panic = Self::abandon_detached_sequence(detached);
        drop(guard);
        RoutePanic::preserve_first(&mut first, panic, "explicit pointer cancellation");
        if let Some(panic) = first {
            panic.resume();
        }
    }

    /// Cancel every binding-owned state slot for all active pointers.
    pub fn cancel_all_pointer_sequences(&self) {
        let Some(guard) = AllPointerTeardownGuard::try_enter(&self.tearing_down_all_pointers)
        else {
            return;
        };
        let panic = self.clear_all_pointer_state_capturing_panic();
        drop(guard);
        if let Some(panic) = panic {
            panic.resume();
        }
    }

    pub(crate) fn close_tombstones(&self) -> [crate::__runtime::CloseTombstone; 4] {
        [
            self.close_mode.clone(),
            self.arena.close_tombstone(),
            self.pointer_router.close_tombstone(),
            self.mouse_tracker.close_tombstone(),
        ]
    }

    pub(crate) fn close_with_mode(&self, mode: crate::__runtime::CloseMode) {
        let mut failure = crate::__runtime::ClosePanic::for_close(mode, self.close_mode.clone());
        self.closed.set(true);
        self.scroll_sequences.borrow_mut().clear();
        self.pan_zoom_sequences.borrow_mut().clear();
        if !failure.preserving() {
            failure.invoke(|| {
                if let Some(loss) = self.drain_capture_losses(None) {
                    loss.resume();
                }
            });
        }
        *self.refused_contacts.borrow_mut() = RefusedContacts::default();
        let mut routes: Vec<_> = self.hit_tests.borrow_mut().drain().collect();
        for (_, route) in &routes {
            route.capture.close();
        }
        routes.sort_unstable_by_key(|(pointer, _)| *pointer);
        let mut moves: Vec<_> = self.pending_moves.borrow_mut().drain().collect();
        moves.sort_unstable_by_key(|(pointer, _)| *pointer);
        let arena_mode = if failure.preserving() {
            crate::__runtime::CloseMode::PreservingFailure
        } else {
            crate::__runtime::CloseMode::Ordinary
        };
        failure.invoke(|| self.arena.close_owner(arena_mode));
        let router_mode = if failure.preserving() {
            crate::__runtime::CloseMode::PreservingFailure
        } else {
            crate::__runtime::CloseMode::Ordinary
        };
        failure.invoke(|| self.pointer_router.close_with_mode(router_mode));
        for (_, route) in routes {
            route.resampler.clear();
            if let Some(token) = route.token
                && let Ok(handle) = active_dispatch_handle()
            {
                let _ = handle.release_route_for_close(token, &mut failure);
            }
            failure.retire(crate::retain::Owned(route));
        }
        for (_, event) in moves {
            failure.retire(crate::retain::Owned(event));
        }
        if failure.preserving() {
            failure.invoke(|| {
                self.arena
                    .close_owner(crate::__runtime::CloseMode::PreservingFailure);
            });
            failure.invoke(|| {
                self.pointer_router
                    .close_with_mode(crate::__runtime::CloseMode::PreservingFailure);
            });
        }
        failure.finish();
    }

    /// Synthesize a terminal [`PointerEvent::Cancel`] for every pointer
    /// with an active contact sequence, delivered through the normal
    /// terminal path — recognizers observe a real Cancel (route dispatch,
    /// arena abandonment, cache release), exactly as if the platform had
    /// sent one.
    ///
    /// Call this when the window loses OS focus: a defocused window may
    /// never receive the Up matching an in-flight Down (alt-tab mid-drag),
    /// which would otherwise strand the sequence until a superseding Down.
    /// Platforms that honor this contract send a cancel for in-flight
    /// contacts when the view deactivates; FLUI's backends send no such
    /// event, so the app runner synthesizes it here.
    ///
    /// Unlike [`Self::cancel_all_pointer_sequences`] — the silent teardown
    /// for pause/detach, where user code should no longer run — this
    /// delivers the Cancel to handlers, so a widget mid-drag can undo the
    /// gesture's visible effect. Hover state (pending hover moves, mouse
    /// tracker) is untouched: a pointer can keep hovering an unfocused
    /// window.
    pub fn cancel_active_pointers(&self) {
        self.scroll_sequences.borrow_mut().clear();
        self.pan_zoom_sequences.borrow_mut().clear();
        let mut first_panic = self.drain_capture_losses(None);
        *self.refused_contacts.borrow_mut() = RefusedContacts::default();
        let mut pointers: Vec<_> = self
            .hit_tests
            .borrow()
            .iter()
            .map(|(&pointer_id, entry)| (pointer_id, entry.pointer, entry.time, entry.sequence))
            .collect();
        pointers.sort_unstable_by_key(|(pointer_id, _, _, _)| *pointer_id);
        for (pointer_id, pointer, time, sequence) in pointers {
            // A handler run by an earlier iteration may have re-entered the
            // binding and terminated or replaced this sequence. The numeric
            // pointer identity alone does not identify that admission.
            if !self.is_current_sequence(pointer_id, sequence) {
                continue;
            }
            let cancel =
                PointerEvent::Cancel(PointerCancel::new(pointer, time, CancelReason::FocusLost));
            // The terminal arm never invokes the hit-test callback: Cancel
            // resolves over the route cached at Down. A panicking handler
            // must not exempt the remaining contacts from cancellation —
            // every snapshotted pointer is cancelled first, then the first
            // panic resumes (the same all-work-first posture the kernel
            // itself has within one sequence).
            let delivered = RoutePanic::capture(|| {
                self.handle_pointer_event_after_signal_withdrawal(&cancel, |_| {
                    unreachable!("BUG: a terminal Cancel must never hit-test")
                });
            });
            RoutePanic::preserve_first(&mut first_panic, delivered, "focus-loss pointer cancel");
        }
        if let Some(panic) = first_panic {
            panic.resume();
        }
    }

    /// Defensive cleanup for app-lifecycle pause / detach transitions.
    ///
    /// Cancels every binding-owned pointer sequence. Call this from the app
    /// binding when `AppLifecycleState` transitions to `Paused`, `Hidden`, or
    /// `Detached` — pointer-down events landed before the lifecycle change may
    /// never receive a corresponding Up/Cancel (the platform may suspend us
    /// before the user lifts the finger). The Up/Cancel branch in
    /// [`Self::handle_pointer_event`] already drains entries on normal
    /// completion; this method covers abnormal interruption while a pointer is
    /// still down.
    pub fn handle_lifecycle_pause(&self) {
        let hit_tests = self.hit_tests.borrow().len();
        let resamplers = self.active_resampler_count();
        let pending_moves = self.pending_moves.borrow().len();
        let arenas = self.arena.len();
        // Mandatory state retirement precedes diagnostics, whose subscriber
        // is user code and can panic or reenter the binding.
        let mut first_panic = RoutePanic::capture(|| self.cancel_all_pointer_sequences());
        let diagnostic = RoutePanic::capture(|| {
            if hit_tests > 0 || resamplers > 0 || pending_moves > 0 || arenas > 0 {
                tracing::debug!(
                    hit_tests,
                    resamplers,
                    pending_moves,
                    arenas,
                    "GestureBinding draining interrupted pointer state on lifecycle pause"
                );
            }
        });
        RoutePanic::preserve_first(&mut first_panic, diagnostic, "lifecycle pause diagnostic");
        if let Some(panic) = first_panic {
            panic.resume();
        }
    }

    /// Get the number of active pointers (with cached hit tests).
    #[inline]
    pub fn active_pointer_count(&self) -> usize {
        self.hit_tests.borrow().len()
    }

    // ========================================================================
    // Arena Management
    // ========================================================================

    /// Manually close the arena for a pointer.
    ///
    /// Normally called automatically on pointer down.
    pub fn close_arena(&self, pointer_id: PointerId) {
        self.arena.close(pointer_id);
    }

    /// Manually sweep the arena for a pointer.
    ///
    /// Normally called automatically on pointer up. Cancelled recognizers
    /// reject themselves instead of asking the binding to force a winner.
    pub fn sweep_arena(&self, pointer_id: PointerId) {
        self.arena.sweep(pointer_id);
    }

    /// Resolve lone arena members queued during the preceding event/frame
    /// transaction.
    pub fn drain_deferred_arena_resolutions(&self) -> usize {
        self.arena.drain_deferred_resolutions()
    }

    /// Advance time-based recognizer deadlines (e.g. long-press hold).
    ///
    /// Call once per frame on the UI thread, beside [`Self::flush_pending_moves`].
    /// A recognizer deadline is otherwise only advanced opportunistically on the
    /// next pointer event, so a stationary held pointer past its deadline would
    /// never fire (e.g. long-press on a finger held perfectly still).
    pub fn tick_deadlines(&self) {
        let mut first = self.drain_capture_losses(None);
        let deadlines = RoutePanic::capture(|| self.arena.poll_deadlines());
        RoutePanic::preserve_first(&mut first, deadlines, "deadlines after capture loss");
        if let Some(panic) = first {
            panic.resume();
        }
    }

    /// Whether any recognizer in the arena has an armed time-based deadline.
    ///
    /// The frame loop reads this right after [`tick_deadlines`](Self::tick_deadlines)
    /// to decide whether another frame must be requested: the tick only runs on
    /// frames, so without a frame scheduled at the deadline an idle app would
    /// never fire a held long-press or an expired double-tap window.
    pub fn has_pending_deadlines(&self) -> bool {
        self.arena.has_pending_deadlines()
    }

    /// The earliest instant any armed recognizer deadline in this binding's
    /// arena will fire, if any — the wall-clock-wake counterpart of
    /// [`has_pending_deadlines`](Self::has_pending_deadlines). A caller
    /// computing a platform `ControlFlow::WaitUntil` target reads this so a
    /// presentation with nothing else scheduled (no dirty tree, no running
    /// animation) still wakes at the right instant to resolve an in-flight
    /// long-press/double-tap deadline, rather than only lazily on the next
    /// unrelated event.
    pub fn next_deadline(&self) -> Option<web_time::Instant> {
        self.arena.next_deadline()
    }

    // ========================================================================
    // Internal Methods
    // ========================================================================

    fn prepare_scroll_sequence(&self, event: &ScrollEvent) -> ScrollAdmission {
        let now = self.clock.now();
        let source = SignalSource::from(event.pointer);
        let mut sequences = self.scroll_sequences.borrow_mut();
        sequences.retain(|_, sequence| {
            !sequence.phase_less.get()
                || now.saturating_duration_since(sequence.last_packet.get())
                    < SCROLL_INACTIVITY_TIMEOUT
        });
        if matches!(
            event.phase,
            Some(ScrollPhase::Ended | ScrollPhase::Cancelled | ScrollPhase::MomentumEnded)
        ) {
            // Withdrawal precedes terminal observers and consumer callbacks.
            return ScrollAdmission::Terminal(
                sequences
                    .remove(&source)
                    .and_then(|sequence| sequence.route.get()),
            );
        }
        if matches!(
            event.phase,
            Some(ScrollPhase::Began | ScrollPhase::MomentumBegan)
        ) {
            sequences.remove(&source);
        }
        if let Some(sequence) = sequences.get(&source) {
            sequence.last_packet.set(now);
            sequence.phase_less.set(event.phase.is_none());
            return ScrollAdmission::Active(Rc::clone(sequence));
        }
        if sequences.len() >= MAX_SIMULTANEOUS_POINTERS {
            // Refuse consumption rather than publish an unlatched gesture.
            // Its fresh observers still receive the packet.
            return ScrollAdmission::Refused;
        }
        let sequence = Rc::new(ScrollSequence {
            route: Cell::new(None),
            last_packet: Cell::new(now),
            phase_less: Cell::new(event.phase.is_none()),
        });
        sequences.insert(source, Rc::clone(&sequence));
        ScrollAdmission::Active(sequence)
    }

    fn is_current_scroll_sequence(
        &self,
        pointer: PointerInfo,
        sequence: &Rc<ScrollSequence>,
    ) -> bool {
        self.scroll_sequences
            .borrow()
            .get(&SignalSource::from(pointer))
            .is_some_and(|current| Rc::ptr_eq(current, sequence))
    }

    fn prepare_pan_zoom_sequence(&self, event: &PanZoomEvent) -> PanZoomAdmission {
        let pointer = *event.pointer();
        let source = SignalSource::from(pointer);
        let mut sequences = self.pan_zoom_sequences.borrow_mut();
        if let Some(sequence) = sequences.get(&source)
            && sequence.pointer != pointer.id
            && !matches!(event.phase, PanZoomPhase::Start)
        {
            return PanZoomAdmission::Refused;
        }
        if matches!(event.phase, PanZoomPhase::End | PanZoomPhase::Cancelled) {
            return PanZoomAdmission::Terminal(
                sequences
                    .remove(&source)
                    .and_then(|sequence| sequence.route.get()),
            );
        }
        let route = if matches!(event.phase, PanZoomPhase::Start) {
            // Repeated exact Start replaces the cumulative session, while its
            // admitted consumer receives retirement and the new staged pair.
            sequences.remove(&source).and_then(|sequence| {
                (sequence.pointer == pointer.id)
                    .then(|| sequence.route.get())
                    .flatten()
            })
        } else if let Some(sequence) = sequences.get(&source) {
            return PanZoomAdmission::Active(Rc::clone(sequence));
        } else {
            None
        };
        if sequences.len() >= MAX_SIMULTANEOUS_POINTERS {
            return PanZoomAdmission::Refused;
        }
        let sequence = Rc::new(PanZoomSequence {
            route: Cell::new(route),
            pointer: pointer.id,
        });
        sequences.insert(source, Rc::clone(&sequence));
        PanZoomAdmission::Active(sequence)
    }

    fn is_current_pan_zoom_sequence(
        &self,
        pointer: PointerInfo,
        sequence: &Rc<PanZoomSequence>,
    ) -> bool {
        self.pan_zoom_sequences
            .borrow()
            .get(&SignalSource::from(pointer))
            .is_some_and(|current| Rc::ptr_eq(current, sequence))
    }

    fn handle_pointer_event_kernel<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.tearing_down_all_pointers.get() {
            return;
        }
        if let PointerEvent::Cancel(cancel) = event {
            self.scroll_sequences
                .borrow_mut()
                .remove(&SignalSource::from(cancel.pointer));
            self.pan_zoom_sequences
                .borrow_mut()
                .remove(&SignalSource::from(cancel.pointer));
        }
        self.handle_pointer_event_after_signal_withdrawal(event, hit_test_fn);
    }

    /// Owner cancellation batches withdraw signal admissions before invoking
    /// their first callback. Later old contacts must not withdraw a newly
    /// admitted signal from an earlier callback in that same batch.
    fn handle_pointer_event_after_signal_withdrawal<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.tearing_down_all_pointers.get() {
            return;
        }
        let Some(pointer_id) = crate::PointerEventExt::pointer_id(event) else {
            // Device lifecycle has no contact identity. It still reaches
            // global observers without fabricating a per-pointer route.
            let mut first_panic = None;
            if let PointerEvent::DeviceRemoved(device) = event {
                self.scroll_sequences
                    .borrow_mut()
                    .remove(&SignalSource::Device(device.device));
                self.pan_zoom_sequences
                    .borrow_mut()
                    .remove(&SignalSource::Device(device.device));
                let mut pointers: Vec<_> = self
                    .hit_tests
                    .borrow()
                    .values()
                    .filter(|entry| entry.pointer.device == Some(device.device))
                    .map(|entry| (entry.pointer, entry.sequence))
                    .collect();
                pointers.sort_unstable_by_key(|(pointer, _)| pointer.id);
                for (pointer, sequence) in pointers {
                    if !self.is_current_sequence(pointer.id, sequence) {
                        continue;
                    }
                    let cancel = PointerEvent::Cancel(PointerCancel::new(
                        pointer,
                        device.time,
                        CancelReason::DeviceRemoved,
                    ));
                    let delivered = RoutePanic::capture(|| {
                        self.handle_pointer_event_after_signal_withdrawal(
                            &cancel,
                            terminal_hit_test,
                        )
                    });
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        delivered,
                        "removed-device contact cancellation",
                    );
                }
                let removed =
                    RoutePanic::capture(|| self.mouse_tracker.remove_device(device.device));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    removed,
                    "removed-device hover cleanup",
                );
            }
            let delivered = self.dispatch_ephemeral(event, &HitTestResult::new());
            RoutePanic::preserve_first(&mut first_panic, delivered, "device lifecycle dispatch");
            if let Some(panic) = first_panic {
                panic.resume();
            }
            return;
        };
        if self.tearing_down_all_pointers.get()
            || self.tearing_down_pointers.borrow().contains(&pointer_id)
        {
            tracing::debug!(
                ?pointer_id,
                "ignoring re-entrant input during pointer teardown"
            );
            return;
        }

        if matches!(event, PointerEvent::Move(_) | PointerEvent::ButtonChange(_))
            && self
                .hit_tests
                .borrow()
                .get(&pointer_id)
                .is_some_and(|cached| cached.capture.release_requested())
        {
            return;
        }
        let refused_tail = !self.hit_tests.borrow().contains_key(&pointer_id)
            && self
                .refused_contacts
                .borrow()
                .contains(pointer_id, event.device_id());
        if matches!(
            event,
            PointerEvent::Move(_)
                | PointerEvent::Up(_)
                | PointerEvent::Cancel(_)
                | PointerEvent::ButtonChange(_)
        ) && let Some(pointer) = crate::events::pointer_info(event)
        {
            let mut routes = self.hit_tests.borrow_mut();
            if let Some(cached) = routes.get_mut(&pointer_id) {
                // The map key already matches PointerId. Device identity completes
                // the source; kind/tool and primary role may change during contact.
                if cached.pointer.device != pointer.device {
                    return;
                }
                cached.pointer = *pointer;
                if let Some(time) = crate::events::event_time(event) {
                    cached.time = time;
                }
            }
        }
        match event {
            PointerEvent::Down(_) => {
                if self.refused_contacts.borrow().refuses_down(pointer_id) {
                    return;
                }
                self.refused_contacts.borrow_mut().finish(pointer_id);
            }
            PointerEvent::Move(_) | PointerEvent::Up(_) | PointerEvent::Cancel(_)
                if refused_tail =>
            {
                if matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) {
                    self.refused_contacts.borrow_mut().finish(pointer_id);
                }
                return;
            }
            _ => {}
        }

        match event {
            PointerEvent::Down(down) => {
                let mut first_panic = None;
                let has_superseded_sequence = self.hit_tests.borrow().contains_key(&pointer_id)
                    || self.pending_moves.borrow().contains_key(&pointer_id)
                    || self.arena.has_active(pointer_id);
                if has_superseded_sequence {
                    let guard =
                        PointerTeardownGuard::try_enter(&self.tearing_down_pointers, pointer_id)
                            .expect("BUG: pointer teardown was checked before superseding Down");
                    let detached = self.detach_pointer_sequence(pointer_id);
                    let cleanup = Self::abandon_detached_sequence(detached);
                    RoutePanic::preserve_first(
                        &mut first_panic,
                        cleanup,
                        "superseded pointer teardown",
                    );
                    drop(guard);
                }

                let active = self.hit_tests.borrow().len();
                if active >= MAX_SIMULTANEOUS_POINTERS {
                    self.refused_contacts.borrow_mut().refuse(pointer_id);
                    tracing::warn!(
                        ?pointer_id,
                        active,
                        "dropping Down: simultaneous-pointer cap reached"
                    );
                    if let Some(panic) = first_panic {
                        panic.resume();
                    }
                    return;
                }

                let position = event
                    .position()
                    .expect("BUG: Down carries a checked position");
                let result = match RoutePanic::try_run(|| hit_test_fn(position)) {
                    Ok(result) => result,
                    Err(panic) => {
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            Some(panic),
                            "pointer Down hit test",
                        );
                        let lifecycle = RoutePanic::capture(|| self.arena.close(pointer_id));
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            lifecycle,
                            "pointer Down arena close after failed hit test",
                        );
                        first_panic
                            .expect("a failed hit test records a panic")
                            .resume();
                    }
                };

                let token = Self::resolve_route(&result);
                // Hit entries contain inert target identities and transforms;
                // the resolved token owns the actual admitted handler route.
                drop(result);
                let sequence = self.allocate_pointer_sequence();
                let capture = ContactCapture::new(
                    down.pointer,
                    sequence.0,
                    self.capture_wake.borrow().clone(),
                );
                let resampler = PointerEventResampler::new(pointer_id);
                if self.is_resampling_enabled() {
                    resampler.start_tracking();
                }
                let replaced = self.hit_tests.borrow_mut().insert(
                    pointer_id,
                    CachedPointerRoute {
                        token,
                        sequence,
                        capture,
                        resampler,
                        pointer: down.pointer,
                        time: down.sample.time,
                    },
                );
                drop(replaced);

                let delivered = self.dispatch_event(event, token);
                RoutePanic::preserve_first(&mut first_panic, delivered, "pointer Down dispatch");
                let lifecycle = RoutePanic::capture(|| self.arena.close(pointer_id));
                RoutePanic::preserve_first(&mut first_panic, lifecycle, "pointer Down arena close");
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::Move(pointer_move) => {
                let position = event
                    .position()
                    .expect("BUG: Move carries a checked position");
                let cached = {
                    let mut routes = self.hit_tests.borrow_mut();
                    routes.get_mut(&pointer_id).map(|cached| {
                        cached.time = pointer_move.current().time;
                        (cached.sequence, cached.resampler.clone())
                    })
                };
                if let Some((sequence, resampler)) = cached {
                    if self.is_resampling_enabled() {
                        resampler.add_event_with_arrival(event.clone(), self.clock.now());
                    } else {
                        self.queue_pending_move(
                            pointer_id,
                            PendingMove::Contact {
                                event: event.clone(),
                                sequence,
                            },
                        );
                    }

                    // A metadata boundary may synchronously deliver an older
                    // packet whose callback replaces this contact's route.
                    if !self.is_current_sequence(pointer_id, sequence) {
                        return;
                    }

                    if matches!(
                        pointer_move.pointer.kind,
                        PointerKind::Mouse | PointerKind::Pen { .. }
                    ) {
                        let fresh_hit_test = hit_test_fn(position);
                        self.mouse_tracker.update_with_motion(
                            event,
                            PointerMotionKind::Contact,
                            &fresh_hit_test,
                        );
                    }
                } else {
                    let fresh_hit_test = hit_test_fn(position);
                    self.queue_pending_move(
                        pointer_id,
                        PendingMove::Hover {
                            event: event.clone(),
                            hit_test: fresh_hit_test.clone(),
                        },
                    );
                    self.mouse_tracker.update_with_motion(
                        event,
                        PointerMotionKind::Hover,
                        &fresh_hit_test,
                    );
                }
            }
            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let guard =
                    PointerTeardownGuard::try_enter(&self.tearing_down_pointers, pointer_id)
                        .expect("BUG: pointer teardown was checked before terminal event");
                let detached = self.detach_pointer_sequence(pointer_id);
                let panic = self.finish_terminal_sequence(event, detached);
                drop(guard);
                if let Some(panic) = panic {
                    panic.resume();
                }
            }
            PointerEvent::Enter(_) | PointerEvent::Leave(_) => {
                // An active contact keeps capture semantics: deliver on the
                // route hit-tested at Down, like every other mid-contact
                // event of the sequence (a down pointer's events go over the
                // result stored for it at Down).
                let panic = if self.hit_tests.borrow().contains_key(&pointer_id) {
                    self.dispatch_on_cached_route(pointer_id, event)
                } else {
                    // A HOVERING pointer — the only state in which a
                    // window-boundary Enter/Leave normally arrives — has no
                    // cached route, so resolve a fresh ephemeral path the
                    // way Scroll does. The event itself carries no position
                    // (the macOS and Android producers only identify the
                    // pointer), so the path is resolved at the device's
                    // last-known hover position. A device this binding has
                    // never seen has no position to resolve; the event
                    // still reaches the pointer router (an added/removed-class
                    // event dispatches with no hit path, router only).
                    use crate::events::PointerEventExt as _;
                    let result = match event.position().or_else(|| {
                        crate::events::pointer_info(event)
                            .and_then(|info| self.mouse_tracker.source_position(info))
                    }) {
                        Some(position) => hit_test_fn(position),
                        None => HitTestResult::new(),
                    };
                    self.dispatch_ephemeral(event, &result)
                };
                if let Some(panic) = panic {
                    panic.resume();
                }
            }
            PointerEvent::PanZoom(gesture) => {
                let admission = self.prepare_pan_zoom_sequence(gesture);
                let position = event
                    .position()
                    .expect("BUG: PanZoom carries a checked position");
                // Native gesture observation follows fresh geometry; accepted
                // consumption stays with its exact owner across rebuilds.
                let path = hit_test_fn(position);
                let mut first_panic = self.dispatch_ephemeral(event, &path);
                let claim = RoutePanic::capture(|| {
                    let claimed = match &admission {
                        PanZoomAdmission::Terminal(Some(route)) => route.dispatch(gesture, || {}),
                        PanZoomAdmission::Terminal(None) => path.dispatch_pan_zoom(gesture),
                        PanZoomAdmission::Refused => false,
                        PanZoomAdmission::Active(sequence) => {
                            if let Some(route) = sequence.route.get() {
                                route.dispatch(gesture, || {})
                            } else if self
                                .is_current_pan_zoom_sequence(*gesture.pointer(), sequence)
                            {
                                path.dispatch_pan_zoom_with_claim(gesture, |route| {
                                    if self
                                        .is_current_pan_zoom_sequence(*gesture.pointer(), sequence)
                                    {
                                        sequence.route.set(Some(route));
                                    }
                                })
                            } else {
                                false
                            }
                        }
                    };
                    tracing::trace!(
                        claimed,
                        pan_zoom_targets = path.entries_with_pan_zoom_targets().count(),
                        "pan-zoom arbitration"
                    );
                });
                RoutePanic::preserve_first(
                    &mut first_panic,
                    claim,
                    "pan-zoom claim after pointer dispatch",
                );
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::Scroll(scroll) => {
                // Commit source admission (or terminal withdrawal) before
                // hit testing and observers, which may reenter this binding.
                let sequence = self.prepare_scroll_sequence(scroll);
                let position = event
                    .position()
                    .expect("BUG: Scroll carries a checked position");
                // Raw observers always follow the current focal point. Only
                // the consumptive channel stays with the first scroll claimant.
                let fresh_result = hit_test_fn(position);
                let mut first_panic = self.dispatch_ephemeral(event, &fresh_result);
                let claim = RoutePanic::capture(|| {
                    let claimed = match &sequence {
                        ScrollAdmission::Terminal(Some(route)) => route.dispatch(scroll, || {}),
                        // A final delta may be the first meaningful packet.
                        // It is deliverable, but cannot publish a lasting lease.
                        ScrollAdmission::Terminal(None) => fresh_result.dispatch_scroll(scroll),
                        ScrollAdmission::Refused => false,
                        ScrollAdmission::Active(sequence) => {
                            if let Some(route) = sequence.route.get() {
                                // A gone target stays inert until the sequence ends;
                                // it never hands this gesture to a new hit path.
                                route.dispatch(scroll, || {})
                            } else if self.is_current_scroll_sequence(scroll.pointer, sequence) {
                                fresh_result.dispatch_scroll_with_claim(scroll, |route| {
                                    if self.is_current_scroll_sequence(scroll.pointer, sequence) {
                                        sequence.route.set(Some(route));
                                    }
                                })
                            } else {
                                false
                            }
                        }
                    };
                    tracing::trace!(
                        claimed,
                        scroll_targets = fresh_result.entries_with_scroll_targets().count(),
                        "pointer-signal arbitration"
                    );
                });
                RoutePanic::preserve_first(
                    &mut first_panic,
                    claim,
                    "scroll claim after pointer dispatch",
                );
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::ButtonChange(_) => {
                let cached = self
                    .hit_tests
                    .borrow()
                    .get(&pointer_id)
                    .map(|cached| (cached.sequence, cached.token, cached.resampler.clone()));
                let mut first_panic = None;
                if let Some((sequence, token, resampler)) = cached {
                    // Take the older accepted packet before any callback. A
                    // callback may enqueue its successor or replace this contact;
                    // neither belongs to the button edge admitted here.
                    let pending = self.pending_moves.borrow_mut().remove(&pointer_id);
                    if let Some(PendingMove::Contact {
                        event: movement,
                        sequence: admitted,
                    }) = pending.as_ref().and_then(|state| state.pending.as_ref())
                        && *admitted == sequence
                        && self.is_current_sequence(pointer_id, sequence)
                    {
                        let delivered = self.dispatch_event(movement, token);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "button pending Move dispatch",
                        );
                    }
                    if self.is_resampling_enabled()
                        && let Some(time) = crate::events::event_time(event)
                    {
                        resampler.flush_through(time, |movement| {
                            if self.is_current_sequence(pointer_id, sequence) {
                                let delivered = self.dispatch_event(&movement, token);
                                RoutePanic::preserve_first(
                                    &mut first_panic,
                                    delivered,
                                    "button measured Move dispatch",
                                );
                            }
                        });
                    }
                    // A failed movement still owes the accepted edge, but an
                    // ended or replaced sequence must not receive stale input.
                    if self.is_current_sequence(pointer_id, sequence) {
                        let delivered = self.dispatch_event(event, token);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "pointer button edge dispatch",
                        );
                    }
                } else {
                    let result = event
                        .position()
                        .map_or_else(HitTestResult::new, hit_test_fn);
                    first_panic = self.dispatch_ephemeral(event, &result);
                }
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::ScrollInertiaCancel(_) => {
                let panic = if self.hit_tests.borrow().contains_key(&pointer_id) {
                    self.dispatch_on_cached_route(pointer_id, event)
                } else {
                    let result = event
                        .position()
                        .map_or_else(HitTestResult::new, hit_test_fn);
                    self.dispatch_ephemeral(event, &result)
                };
                if let Some(panic) = panic {
                    panic.resume();
                }
            }
            _ => {}
        }
    }

    fn detach_pointer_sequence(&self, pointer_id: PointerId) -> DetachedPointerSequence {
        let cached = self.hit_tests.borrow_mut().remove(&pointer_id);
        if let Some(cached) = cached.as_ref() {
            cached.capture.end();
        }
        let pending_move = self.pending_moves.borrow_mut().remove(&pointer_id);
        DetachedPointerSequence {
            cached,
            pending_move,
            arena: self.arena.detach(pointer_id),
        }
    }

    fn allocate_pointer_sequence(&self) -> PointerSequence {
        let sequence = self
            .next_pointer_sequence
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic!("BUG: pointer sequence generation exhausted"));
        self.next_pointer_sequence.set(sequence);
        PointerSequence(sequence)
    }

    /// Settle release debt without running any callback under a state borrow.
    /// A detached frame payload is already committed to its observer round;
    /// defer its loss until that in-flight marker retires. A native terminal
    /// or replacement Down ends that generation under the existing stale-work
    /// policy and may settle it before admitting the newer transaction.
    fn drain_capture_losses(&self, ending: Option<PointerId>) -> Option<RoutePanic> {
        let closing = self.closed.get() || self.tearing_down_all_pointers.get();
        let mut losses: SmallVec<
            [(PointerInfo, flui_platform_api::EventTime, PointerSequence); 4],
        > = {
            let routes = self.hit_tests.borrow();
            let moves = self.pending_moves.borrow();
            let retiring = self.tearing_down_pointers.borrow();
            routes
                .values()
                .filter(|cached| {
                    cached.capture.release_requested()
                        && !retiring.contains(&cached.pointer.id)
                        && (closing
                            || ending == Some(cached.pointer.id)
                            || (!cached.capture.is_delivering()
                                && !moves
                                    .get(&cached.pointer.id)
                                    .is_some_and(|state| state.pending.is_none())))
                })
                .map(|cached| (cached.pointer, cached.time, cached.sequence))
                .collect()
        };
        losses.sort_unstable_by_key(|(pointer, _, _)| pointer.id);
        let mut first = None;
        for (pointer, time, sequence) in losses {
            if !self.is_current_sequence(pointer.id, sequence) {
                continue;
            }
            let Some(guard) =
                PointerTeardownGuard::try_enter(&self.tearing_down_pointers, pointer.id)
            else {
                continue;
            };
            self.refused_contacts.borrow_mut().release(pointer);
            let detached = self.detach_pointer_sequence(pointer.id);
            let cancel =
                PointerEvent::Cancel(PointerCancel::new(pointer, time, CancelReason::CaptureLost));
            let delivered = self.finish_terminal_sequence(&cancel, detached);
            RoutePanic::preserve_first(&mut first, delivered, "released pointer capture");
            drop(guard);
        }
        first
    }

    fn allocate_pending_move_generation(&self) -> PendingMoveGeneration {
        let generation = self
            .next_pending_move_generation
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic!("BUG: pending move generation exhausted"));
        self.next_pending_move_generation.set(generation);
        PendingMoveGeneration(generation)
    }

    fn queue_pending_move(&self, pointer_id: PointerId, mut pending: PendingMove) {
        let generation = self.allocate_pending_move_generation();
        let mut previous = self.pending_moves.borrow_mut().remove(&pointer_id);
        let mismatch = match (&mut previous, &mut pending) {
            (
                Some(PendingMoveState {
                    pending:
                        Some(PendingMove::Contact {
                            event: previous,
                            sequence: old,
                        }),
                    ..
                }),
                PendingMove::Contact {
                    event: latest,
                    sequence: new,
                },
            ) if old == new => match (previous, latest) {
                (PointerEvent::Move(older), PointerEvent::Move(newer)) => {
                    newer.try_coalesce(older).is_err()
                }
                _ => false,
            },
            (
                Some(PendingMoveState {
                    pending:
                        Some(PendingMove::Hover {
                            event: previous, ..
                        }),
                    ..
                }),
                PendingMove::Hover { event: latest, .. },
            ) => match (previous, latest) {
                (PointerEvent::Move(older), PointerEvent::Move(newer)) => {
                    newer.try_coalesce(older).is_err()
                }
                _ => false,
            },
            _ => false,
        };
        if mismatch {
            let prior = previous
                .take()
                .expect("BUG: a coalescing refusal has an older dispatch");
            // Both packets are accepted. Publish the newer delivery debt before
            // running the older packet's callbacks; reentry may replace or
            // cancel it, and no later insertion may resurrect that old debt.
            let replaced = self
                .pending_moves
                .borrow_mut()
                .insert(pointer_id, PendingMoveState::queued(generation, pending));
            let mut first_panic = match RoutePanic::try_run(|| match prior.pending.as_ref() {
                Some(PendingMove::Contact { event, sequence })
                    if self.is_current_sequence(pointer_id, *sequence) =>
                {
                    self.dispatch_on_cached_route(pointer_id, event)
                }
                Some(PendingMove::Hover { event, hit_test }) => {
                    self.dispatch_ephemeral(event, hit_test)
                }
                _ => None,
            }) {
                Ok(failure) => failure,
                Err(failure) => Some(failure),
            };
            if first_panic.is_some() {
                crate::retain::Retain::retain(crate::retain::Owned(prior));
                crate::retain::Retain::retain(crate::retain::Owned(replaced));
            } else {
                let retired = RoutePanic::capture(|| drop(prior));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    retired,
                    "older metadata packet retirement",
                );
                if first_panic.is_some() {
                    crate::retain::Retain::retain(crate::retain::Owned(replaced));
                } else {
                    first_panic = RoutePanic::capture(|| drop(replaced));
                }
            }
            if let Some(failure) = first_panic {
                failure.resume();
            }
            return;
        }
        let replaced = self
            .pending_moves
            .borrow_mut()
            .insert(pointer_id, PendingMoveState::queued(generation, pending));
        // Commit the accepted replacement before retiring outgoing ownership,
        // and never retire a route or payload while holding a map guard.
        drop(previous);
        drop(replaced);
    }

    fn is_pending_move_in_flight(
        &self,
        pointer_id: PointerId,
        generation: PendingMoveGeneration,
    ) -> bool {
        self.pending_moves
            .borrow()
            .get(&pointer_id)
            .is_some_and(|state| state.is_in_flight(generation))
    }

    fn remove_pending_move_if_in_flight(
        &self,
        pointer_id: PointerId,
        generation: PendingMoveGeneration,
    ) {
        let retired = {
            let mut moves = self.pending_moves.borrow_mut();
            if moves
                .get(&pointer_id)
                .is_some_and(|state| state.is_in_flight(generation))
            {
                moves.remove(&pointer_id)
            } else {
                None
            }
        };
        drop(retired);
    }

    fn is_current_sequence(&self, pointer_id: PointerId, sequence: PointerSequence) -> bool {
        self.hit_tests
            .borrow()
            .get(&pointer_id)
            .is_some_and(|cached| cached.sequence == sequence)
    }

    fn abandon_detached_sequence(detached: DetachedPointerSequence) -> Option<RoutePanic> {
        let DetachedPointerSequence {
            cached,
            pending_move,
            arena,
        } = detached;
        let mut first_panic = RoutePanic::capture(|| GestureArena::abandon_detached(arena));
        if let Some(cached) = cached {
            cached.resampler.clear();
            let release = Self::release_route_capturing_panic(cached.token);
            RoutePanic::preserve_first(
                &mut first_panic,
                release,
                "abandoned pointer route release",
            );
            let dropped = RoutePanic::capture(|| drop(cached));
            RoutePanic::preserve_first(&mut first_panic, dropped, "abandoned cached route drop");
        }
        let dropped_move = RoutePanic::capture(|| drop(pending_move));
        RoutePanic::preserve_first(
            &mut first_panic,
            dropped_move,
            "abandoned pending move drop",
        );
        first_panic
    }

    fn finish_terminal_sequence(
        &self,
        terminal: &PointerEvent,
        detached: DetachedPointerSequence,
    ) -> Option<RoutePanic> {
        let DetachedPointerSequence {
            cached,
            pending_move,
            arena,
        } = detached;
        let mut first_panic = None;
        if let (Some(cached), Some(PendingMove::Contact { event, sequence })) = (
            cached.as_ref(),
            pending_move
                .as_ref()
                .and_then(|state| state.pending.as_ref()),
        ) && cached.sequence == *sequence
        {
            let delivered =
                self.dispatch_event_with_capture(event, cached.token, Some(&cached.capture));
            RoutePanic::preserve_first(
                &mut first_panic,
                delivered,
                "terminal pending Move dispatch",
            );
        }

        if let Some(cached) = cached.as_ref()
            && cached.resampler.is_tracked()
        {
            cached.resampler.stop(|event| {
                let delivered =
                    self.dispatch_event_with_capture(&event, cached.token, Some(&cached.capture));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    delivered,
                    "terminal resampled Move dispatch",
                );
            });
        }

        let delivered = self.dispatch_event_with_capture(
            terminal,
            cached.as_ref().and_then(|route| route.token),
            cached.as_ref().map(|route| &route.capture),
        );
        RoutePanic::preserve_first(&mut first_panic, delivered, "terminal pointer dispatch");

        let lifecycle = match terminal {
            PointerEvent::Up(_) => RoutePanic::capture(|| self.arena.sweep_detached(arena)),
            PointerEvent::Cancel(_) => {
                RoutePanic::capture(|| GestureArena::abandon_detached(arena))
            }
            _ => unreachable!("terminal sequence requires Up or Cancel"),
        };
        RoutePanic::preserve_first(
            &mut first_panic,
            lifecycle,
            "terminal gesture arena lifecycle",
        );

        if let Some(cached) = cached {
            let release = Self::release_route_capturing_panic(cached.token);
            RoutePanic::preserve_first(&mut first_panic, release, "terminal route release");
            let dropped = RoutePanic::capture(|| drop(cached));
            RoutePanic::preserve_first(&mut first_panic, dropped, "terminal cached route drop");
        }
        let dropped_move = RoutePanic::capture(|| drop(pending_move));
        RoutePanic::preserve_first(&mut first_panic, dropped_move, "terminal pending move drop");
        first_panic
    }

    /// Extract pointer ID from event.
    #[inline]
    /// Resolve a hit path into an owner-local route before the arena closes.
    ///
    /// Returns `None` when the path carries no pointer targets, or when the
    /// typed lane boundary rejects the resolution (no active lane, wrong
    /// realm) — the failure is traced, never a panic, and the pointer router
    /// still routes the sequence.
    fn resolve_route(result: &HitTestResult) -> Option<ResolvedRouteToken> {
        if !result.iter().any(|entry| entry.pointer_target.is_some()) {
            return None;
        }
        match active_dispatch_handle()
            .and_then(|handle| handle.resolve_pointer_route(result.path()))
        {
            Ok(resolution) => {
                for miss in resolution.misses() {
                    tracing::debug!(
                        path_index = miss.path_index(),
                        "hit path target unregistered before Down resolution"
                    );
                }
                Some(resolution.token())
            }
            Err(error) => {
                tracing::error!(
                    ?error,
                    "pointer route resolution failed; hit targets will not receive this sequence"
                );
                None
            }
        }
    }

    /// Release a cached route while returning a destructor panic to the
    /// transaction that still owns later cleanup.
    fn release_route_capturing_panic(token: Option<ResolvedRouteToken>) -> Option<RoutePanic> {
        let token = token?;
        match RoutePanic::try_run(|| {
            active_dispatch_handle().and_then(|handle| handle.release_route(token))
        }) {
            Ok(Ok(())) => None,
            Ok(Err(error)) => {
                tracing::debug!(
                    ?error,
                    "cached pointer route not released through the active lane"
                );
                None
            }
            Err(panic) => Some(panic),
        }
    }

    /// Detach and clean every interrupted pointer transaction.
    fn clear_all_pointer_state_capturing_panic(&self) -> Option<RoutePanic> {
        self.scroll_sequences.borrow_mut().clear();
        self.pan_zoom_sequences.borrow_mut().clear();
        let mut first_panic = self.drain_capture_losses(None);
        *self.refused_contacts.borrow_mut() = RefusedContacts::default();
        let mut cached_routes: Vec<_> = self.hit_tests.borrow_mut().drain().collect();
        for (_, cached) in &cached_routes {
            cached.capture.end();
        }
        cached_routes.sort_unstable_by_key(|(pointer, _)| *pointer);
        let mut pending_moves: Vec<_> = self.pending_moves.borrow_mut().drain().collect();
        pending_moves.sort_unstable_by_key(|(pointer, _)| *pointer);

        // Every map above is empty before any arena callback or destructor
        // runs. Arena abandonment likewise removes all exact slots before
        // notifying members.
        let abandoned = RoutePanic::capture(|| self.arena.abandon_all());
        RoutePanic::preserve_first(
            &mut first_panic,
            abandoned,
            "interrupted pointer arena cleanup",
        );

        for (_, cached) in cached_routes {
            cached.resampler.clear();
            let route_cleanup = Self::release_route_capturing_panic(cached.token);
            RoutePanic::preserve_first(
                &mut first_panic,
                route_cleanup,
                "interrupted pointer route cleanup",
            );
            let cached_drop = RoutePanic::capture(|| drop(cached));
            RoutePanic::preserve_first(
                &mut first_panic,
                cached_drop,
                "interrupted cached hit-test cleanup",
            );
        }
        for (_, event) in pending_moves {
            let pending_drop = RoutePanic::capture(|| drop(event));
            RoutePanic::preserve_first(
                &mut first_panic,
                pending_drop,
                "interrupted pending-move cleanup",
            );
        }
        first_panic
    }

    /// Invoke the resolved hit route, then route through the pointer router.
    ///
    /// The binding is the final/root hit-test entry, so leaf hit targets
    /// run before its `PointerRouter` and arena lifecycle. Returns the first
    /// panic in that transaction order so the caller can finish later phases
    /// and mandatory cleanup before resuming it.
    fn dispatch_event(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
    ) -> Option<RoutePanic> {
        let capture = crate::PointerEventExt::pointer_id(event).and_then(|pointer| {
            self.hit_tests
                .borrow()
                .get(&pointer)
                .filter(|cached| cached.token == token)
                .map(|cached| Rc::clone(&cached.capture))
        });
        self.dispatch_event_with_capture(event, token, capture.as_ref())
    }

    fn dispatch_event_with_capture(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
        capture: Option<&Rc<ContactCapture>>,
    ) -> Option<RoutePanic> {
        let mut first_panic = if let Some(token) = token {
            match active_dispatch_handle()
                .and_then(|handle| handle.invoke_pointer_route_with_capture(token, event, capture))
            {
                Ok(panic) => panic,
                Err(error) => {
                    tracing::error!(?error, "cached pointer route invocation failed");
                    None
                }
            }
        } else {
            None
        };

        let router_panic = self.pointer_router.route_capturing_panics(event);
        RoutePanic::preserve_first(&mut first_panic, router_panic, "pointer router");
        first_panic
    }

    /// Dispatch on the route cached for `pointer_id`, if any.
    ///
    /// The cached token is copied out before dispatch so no map borrow
    /// guard is held while handlers re-enter the binding.
    fn dispatch_on_cached_route(
        &self,
        pointer_id: PointerId,
        event: &PointerEvent,
    ) -> Option<RoutePanic> {
        let token = self.hit_tests.borrow().get(&pointer_id)?.token;
        self.dispatch_event(event, token)
    }

    /// Route `event` and deliver it over a one-shot route for `result`.
    ///
    /// Used for events with no cached Down route (e.g. a scroll landing on an
    /// untracked pointer): hit targets run leaf-first and release their
    /// ephemeral route, then the root pointer router runs. The first panic is
    /// returned only after both phases have completed.
    fn dispatch_ephemeral(
        &self,
        event: &PointerEvent,
        result: &HitTestResult,
    ) -> Option<RoutePanic> {
        let mut first_panic = result.dispatch_capturing_panic(event);
        let router_panic = self.pointer_router.route_capturing_panics(event);
        RoutePanic::preserve_first(&mut first_panic, router_panic, "pointer router");
        first_panic
    }

    /// As [`dispatch_ephemeral`](Self::dispatch_ephemeral), but every
    /// ordinary pointer target AND every mouse-hover region on `result`'s
    /// path deliver together, leaf-first, in one per-entry pass instead of
    /// [`dispatch_ephemeral`](Self::dispatch_ephemeral)'s ordinary-targets-only
    /// walk followed by a separate full pass over mouse regions.
    ///
    /// Used only for the coalesced hover-move arm of
    /// [`flush_pending_moves_kernel`](Self::flush_pending_moves_kernel): a
    /// hover move never has a cached Down route, so this — like
    /// [`dispatch_ephemeral`](Self::dispatch_ephemeral) — resolves and
    /// releases everything within this one call, then still runs the root
    /// pointer router after the walk (the binding is the path's
    /// least-specific entry and routes the event last).
    fn dispatch_ephemeral_with_hover_interleaved(
        &self,
        event: &PointerEvent,
        result: &HitTestResult,
    ) -> Option<RoutePanic> {
        let mut first_panic = result.dispatch_hover_interleaved_capturing_panic(event);
        let router_panic = self.pointer_router.route_capturing_panics(event);
        RoutePanic::preserve_first(&mut first_panic, router_panic, "pointer router");
        first_panic
    }
}

impl std::fmt::Debug for GestureBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureBinding")
            .field("active_pointers", &self.hit_tests.borrow().len())
            .field("pending_moves", &self.pending_moves.borrow().len())
            .field("arena_count", &self.arena.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::events::{
        make_down_event, make_down_event_for_id, make_move_event, make_move_event_for_id,
        make_up_event,
    };

    #[derive(Debug, Default)]
    struct CountingArenaMember {
        accepts: AtomicUsize,
        rejects: AtomicUsize,
    }

    impl crate::arena::GestureArenaMember for CountingArenaMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            self.accepts.fetch_add(1, Ordering::Relaxed);
        }

        fn reject_gesture(&self, _pointer: PointerId) {
            self.rejects.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[derive(Debug, Default)]
    struct PanickingAcceptArenaMember;

    impl crate::arena::GestureArenaMember for PanickingAcceptArenaMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            panic!("arena accept panic");
        }

        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    fn set_resampling(binding: &GestureBinding, enabled: bool) {
        binding
            .set_resampling_enabled(enabled)
            .expect("tests configure resampling without active pointers");
    }

    // ========================================================================
    // Resampler wiring tests
    // ========================================================================

    // Pointer-sequence routing matrix: route caching, pending moves, supersession,
    /// resampling and cancel.
    #[test]
    fn pointer_sequence_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "contact_move_uses_down_route_and_fresh_mouse_tracking_route",
                contact_move_uses_down_route_and_fresh_mouse_tracking_route,
            ),
            (
                "terminal_dispatches_the_pending_move_before_up_on_the_detached_route",
                terminal_dispatches_the_pending_move_before_up_on_the_detached_route,
            ),
            (
                "superseding_down_abandons_old_arena_and_pending_move_before_hit_test",
                superseding_down_abandons_old_arena_and_pending_move_before_hit_test,
            ),
            (
                "resampling_never_crosses_a_reused_pointer_sequence",
                resampling_never_crosses_a_reused_pointer_sequence,
            ),
            (
                "cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover",
                cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn contact_move_uses_down_route_and_fresh_mouse_tracking_route() {
        let device = crate::events::DeviceId::try_from(1_u64).expect("known mouse device");
        let from_device = |mut event: PointerEvent| {
            match &mut event {
                PointerEvent::Down(data) => data.pointer = data.pointer.with_device(device),
                PointerEvent::Move(data) => data.pointer = data.pointer.with_device(device),
                PointerEvent::Up(data) => data.pointer = data.pointer.with_device(device),
                _ => unreachable!("fixture contains only measured contact events"),
            }
            event
        };
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let gesture_moves = Rc::new(Cell::new(0));
        let mouse_enters = Rc::new(Cell::new(0));
        let mouse_hovers = Rc::new(Cell::new(0));
        let fresh_hit_tests = Rc::new(Cell::new(0));

        lane.enter(|| {
            let callback_gesture_moves = Rc::clone(&gesture_moves);
            let pointer_target = handle
                .register_pointer(move |dispatch| {
                    let event = dispatch.local;
                    if matches!(event, PointerEvent::Move(_)) {
                        callback_gesture_moves.set(callback_gesture_moves.get() + 1);
                    }
                })
                .expect("register gesture target");
            let callback_mouse_enters = Rc::clone(&mouse_enters);
            let callback_mouse_hovers = Rc::clone(&mouse_hovers);
            let mouse_target = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_enter: Some(Rc::new(move |_device, _position| {
                        callback_mouse_enters.set(callback_mouse_enters.get() + 1);
                    })),
                    on_hover: Some(Rc::new(move |_device, _position| {
                        callback_mouse_hovers.set(callback_mouse_hovers.get() + 1);
                    })),
                    on_exit: None,
                })
                .expect("register mouse target");

            let down_target_id = RenderId::new(1);
            let mut down_result = HitTestResult::new();
            down_result.add(HitTestEntry::new(down_target_id).pointer_target(pointer_target));
            let position = Offset::new(10.0, 10.0);
            binding.handle_pointer_event(
                &from_device(make_down_event(position, PointerKind::Mouse).expect("finite input")),
                |_| down_result,
            );

            let mouse_target_id = RenderId::new(2);
            let mut fresh_result = HitTestResult::new();
            fresh_result.add(
                HitTestEntry::new(mouse_target_id)
                    .mouse_annotation(MouseTrackerAnnotation::new(mouse_target_id, mouse_target)),
            );
            let callback_fresh_hit_tests = Rc::clone(&fresh_hit_tests);
            binding.handle_pointer_event(
                &from_device(make_move_event(position, PointerKind::Mouse).expect("finite input")),
                move |_| {
                    callback_fresh_hit_tests.set(callback_fresh_hit_tests.get() + 1);
                    fresh_result
                },
            );

            assert_eq!(fresh_hit_tests.get(), 1);
            assert_eq!(mouse_enters.get(), 1);
            assert_eq!(
                mouse_hovers.get(),
                0,
                "a mouse drag updates enter/exit/cursor state but is not hover"
            );
            assert_eq!(gesture_moves.get(), 0);
            assert_eq!(binding.flush_pending_moves(), 1);
            assert_eq!(
                gesture_moves.get(),
                1,
                "contact delivery remains pinned to the target resolved at Down"
            );
            assert!(
                binding
                    .mouse_tracker()
                    .device_active_regions(device)
                    .contains(&mouse_target_id)
            );

            binding.handle_pointer_event(
                &from_device(make_up_event(position, PointerKind::Mouse).expect("finite input")),
                |_| HitTestResult::new(),
            );
            handle
                .unregister_pointer(pointer_target)
                .expect("release pointer target");
            handle
                .unregister_mouse_region(mouse_target)
                .expect("release mouse target");
        });
    }

    fn terminal_dispatches_the_pending_move_before_up_on_the_detached_route() {
        use std::{cell::RefCell, rc::Rc};

        let binding = GestureBinding::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let routed_events = Rc::clone(&events);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            routed_events.borrow_mut().push(match event {
                PointerEvent::Down(_) => "down",
                PointerEvent::Move(_) => "move",
                PointerEvent::Up(_) => "up",
                _ => "other",
            });
        });
        binding.pointer_router().add_route(
            PointerId::new(core::num::NonZeroU64::MIN),
            Rc::clone(&handler),
        );

        let down =
            make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let move_event =
            make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        assert_eq!(events.borrow().as_slice(), ["down"]);

        let up = make_up_event(Offset::new(5.0, 6.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&up, |_| HitTestResult::new());

        assert_eq!(events.borrow().as_slice(), ["down", "move", "up"]);
        binding
            .pointer_router()
            .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
    }

    fn superseding_down_abandons_old_arena_and_pending_move_before_hit_test() {
        use std::cell::Cell;

        let binding = GestureBinding::new();
        let old_member = Rc::new(CountingArenaMember::default());
        let down =
            make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| {
            binding
                .arena()
                .add(PointerId::new(core::num::NonZeroU64::MIN), &old_member);
            HitTestResult::new()
        });
        let move_event =
            make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        assert_eq!(binding.pending_move_count(), 1);

        let old_sequence_gone_before_hit_test = Cell::new(false);
        binding.handle_pointer_event(&down, |_| {
            old_sequence_gone_before_hit_test.set(
                old_member.rejects.load(Ordering::Relaxed) == 1
                    && binding.pending_move_count() == 0
                    && binding.active_resampler_count() == 0
                    && !binding
                        .arena()
                        .has_active(PointerId::new(core::num::NonZeroU64::MIN)),
            );
            HitTestResult::new()
        });

        assert!(old_sequence_gone_before_hit_test.get());
        assert_eq!(old_member.accepts.load(Ordering::Relaxed), 0);
        assert_eq!(binding.pending_move_count(), 0);
    }

    // Reentrancy matrix: input arriving from inside callbacks and destructors.
    #[test]
    fn reentrancy_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks",
                same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks,
            ),
            (
                "cancel_all_rejects_reentrant_input_from_route_destructors",
                cancel_all_rejects_reentrant_input_from_route_destructors,
            ),
            (
                "reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup",
                reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks() {
        use std::{cell::Cell, rc::Rc};

        let binding = Rc::new(GestureBinding::new());
        let reentrant_hit_tests = Rc::new(Cell::new(0));
        let detached_observations = Rc::new(Cell::new(0));
        let binding_from_route = Rc::clone(&binding);
        let reentrant_calls = Rc::clone(&reentrant_hit_tests);
        let observations = Rc::clone(&detached_observations);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            if matches!(event, PointerEvent::Move(_) | PointerEvent::Up(_)) {
                if binding_from_route.active_pointer_count() == 0
                    && binding_from_route.pending_move_count() == 0
                    && binding_from_route.active_resampler_count() == 0
                    && !binding_from_route
                        .arena()
                        .has_active(PointerId::new(core::num::NonZeroU64::MIN))
                {
                    observations.set(observations.get() + 1);
                }
                let down = make_down_event(Offset::new(9.0, 9.0), PointerKind::Touch)
                    .expect("finite input");
                let reentrant_calls = Rc::clone(&reentrant_calls);
                binding_from_route.handle_pointer_event(&down, move |_| {
                    reentrant_calls.set(reentrant_calls.get() + 1);
                    HitTestResult::new()
                });
            }
        });
        binding.pointer_router().add_route(
            PointerId::new(core::num::NonZeroU64::MIN),
            Rc::clone(&handler),
        );

        let down =
            make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let move_event =
            make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        let up = make_up_event(Offset::new(5.0, 6.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&up, |_| HitTestResult::new());

        assert_eq!(detached_observations.get(), 2);
        assert_eq!(reentrant_hit_tests.get(), 0);
        assert_eq!(binding.active_pointer_count(), 0);
        binding
            .pointer_router()
            .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
    }

    fn resampling_never_crosses_a_reused_pointer_sequence() {
        use std::{cell::RefCell, rc::Rc};

        let binding = Rc::new(GestureBinding::new());
        set_resampling(&binding, true);
        binding.set_sampling_clock(SamplingClock::Fixed {
            period: Duration::from_millis(8),
        });

        let moves = Rc::new(RefCell::new(Vec::new()));
        let replaced_sequence = Rc::new(Cell::new(false));
        let callback_binding = Rc::clone(&binding);
        let callback_moves = Rc::clone(&moves);
        let callback_replaced_sequence = Rc::clone(&replaced_sequence);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            let PointerEvent::Move(pointer_move) = event else {
                return;
            };

            callback_moves
                .borrow_mut()
                .push(pointer_move.current().position.get().x);
            if callback_replaced_sequence.replace(true) {
                return;
            }

            let up =
                make_up_event(Offset::new(10.0, 10.0), PointerKind::Touch).expect("finite input");
            callback_binding.handle_pointer_event(&up, |_| HitTestResult::new());

            let down =
                make_down_event(Offset::new(90.0, 90.0), PointerKind::Touch).expect("finite input");
            callback_binding.handle_pointer_event(&down, |_| HitTestResult::new());
            let next_move =
                make_move_event(Offset::new(99.0, 99.0), PointerKind::Touch).expect("finite input");
            callback_binding.handle_pointer_event(&next_move, |_| HitTestResult::new());
        });
        binding.pointer_router().add_route(
            PointerId::new(core::num::NonZeroU64::MIN),
            Rc::clone(&handler),
        );

        let down =
            make_down_event(Offset::new(0.0, 0.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        for coordinate in [10.0, 20.0] {
            let pointer_move =
                make_move_event(Offset::new(coordinate, coordinate), PointerKind::Touch)
                    .expect("finite input");
            binding.handle_pointer_event(&pointer_move, |_| HitTestResult::new());
        }

        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(
            moves.borrow().as_slice(),
            [10.0],
            "samples owned by the detached contact must not reach its replacement"
        );
        assert_eq!(
            binding.pending_move_count(),
            1,
            "a Move queued by the replacement contact belongs to the next frame"
        );

        binding
            .pointer_router()
            .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
    }

    // ========================================================================
    // Owner-routed route lifecycle (ADR-0027 Task 3)
    // ========================================================================

    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use crate::routing::{
        HitTestEntry, InteractionLane, MouseRegionCallbacks, MouseTrackerAnnotation, PointerTarget,
        RenderId,
    };

    fn hit_result(target: PointerTarget) -> HitTestResult {
        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        result
    }

    /// Sets its cell when dropped, so a test can observe the moment the
    /// owner-local handler (and its captures) is released.
    struct SetOnDrop(Rc<Cell<bool>>);

    impl Drop for SetOnDrop {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    struct PanicOnDrop;

    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("route cleanup panic");
        }
    }

    struct ReenterDownOnDrop {
        binding: Rc<GestureBinding>,
        pointer: PointerId,
    }

    impl Drop for ReenterDownOnDrop {
        fn drop(&mut self) {
            let down =
                make_down_event_for_id(self.pointer, Offset::new(12.0, 12.0), PointerKind::Touch)
                    .expect("finite input");
            self.binding
                .handle_pointer_event(&down, |_| HitTestResult::new());
        }
    }

    /// Losing OS focus mid-contact must terminate the sequence like a real
    /// platform Cancel: the route cached at Down observes the Cancel, the
    /// arena rejects its members, and a following Move — the pointer may
    /// keep moving while the window is defocused — is a hover, not a drag
    /// update on the dead sequence's route.
    fn cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let log = Rc::new(RefCell::new(Vec::new()));
        let arena_member = Rc::new(CountingArenaMember::default());

        lane.enter(|| {
            let sink = Rc::clone(&log);
            let target = handle
                .register_pointer(move |dispatch| {
                    let event = dispatch.local;
                    sink.borrow_mut().push(match event {
                        PointerEvent::Down(_) => "down",
                        PointerEvent::Move(_) => "move",
                        PointerEvent::Cancel(_) => "cancel",
                        _ => "other",
                    });
                })
                .expect("register");
            let result = hit_result(target);

            let down =
                make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
            let binding_for_down = Rc::clone(&binding);
            let member = arena_member.clone();
            binding.handle_pointer_event(&down, move |_| {
                binding_for_down
                    .arena()
                    .add(PointerId::new(core::num::NonZeroU64::MIN), &member);
                result
            });
            assert_eq!(&*log.borrow(), &["down"]);
            assert!(binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));

            binding.cancel_active_pointers();

            assert_eq!(
                &*log.borrow(),
                &["down", "cancel"],
                "the cached route must observe a synthesized terminal Cancel"
            );
            assert_eq!(
                arena_member.rejects.load(Ordering::Relaxed),
                1,
                "the arena must reject its members, never silently keep them"
            );
            assert!(!binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));
            assert!(binding.arena().is_empty());

            // The pointer keeps moving after defocus: with the sequence
            // gone this is a hover (fresh hit test, ephemeral dispatch) —
            // the dead sequence's route must not receive it as a drag.
            let mv =
                make_move_event(Offset::new(9.0, 9.0), PointerKind::Touch).expect("finite input");
            binding.handle_pointer_event(&mv, |_| HitTestResult::new());
            binding.flush_pending_moves();
            assert_eq!(
                &*log.borrow(),
                &["down", "cancel"],
                "a post-cancel Move must not be delivered on the cancelled route"
            );

            // Idempotent: nothing left to cancel.
            binding.cancel_active_pointers();
            assert_eq!(&*log.borrow(), &["down", "cancel"]);
        });
    }

    // Panic containment matrix: each row is one failure point (alone, or in
    /// competition with a later one) and must leave the binding able to run the next
    /// operation.
    #[test]
    fn panic_containment_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "arena_accept_panic_cleans_up_handle_pointer_event",
                arena_accept_panic_cleans_up_handle_pointer_event,
            ),
            (
                "per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence",
                per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence,
            ),
            (
                "target_panic_wins_over_a_later_route_cleanup_panic",
                target_panic_wins_over_a_later_route_cleanup_panic,
            ),
            (
                "cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic",
                cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let later_deliveries = Rc::new(Cell::new(0));
        let handler_dropped = Rc::new(Cell::new(false));
        lane.enter(|| {
            let drop_probe = SetOnDrop(Rc::clone(&handler_dropped));
            let panicking = handle
                .register_pointer(move |dispatch| {
                    let event = dispatch.local;
                    let _keep_probe_alive = &drop_probe;
                    if matches!(event, PointerEvent::Up(_)) {
                        panic!("target panic on Up");
                    }
                })
                .expect("register panicking");
            let count = Rc::clone(&later_deliveries);
            let later = handle
                .register_pointer(move |_| count.set(count.get() + 1))
                .expect("register later");

            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(panicking));
            result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later));

            let down =
                make_down_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
            binding.handle_pointer_event(&down, |_| result.clone());
            assert_eq!(later_deliveries.get(), 1);
            handle
                .unregister_pointer(panicking)
                .expect("route owns the panicking cell now");

            let up =
                make_up_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }));
            assert!(unwind.is_err(), "the first target panic must propagate");

            // Later targets still received the Up, and the mandatory cleanup
            // (cache removal + route release) ran before the resumed unwind.
            assert_eq!(later_deliveries.get(), 2);
            assert!(!binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));
            assert!(
                handler_dropped.get(),
                "Up must sweep and release the route before resuming the panic"
            );
        });
    }

    fn target_panic_wins_over_a_later_route_cleanup_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();

        lane.enter(|| {
            let panic_on_cleanup = PanicOnDrop;
            let target = handle
                .register_pointer(move |dispatch| {
                    let event = dispatch.local;
                    let _keep_cleanup_panic_alive = &panic_on_cleanup;
                    if matches!(event, PointerEvent::Up(_)) {
                        panic!("target first panic");
                    }
                })
                .expect("register target");
            let result = hit_result(target);
            let down =
                make_down_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
            binding.handle_pointer_event(&down, |_| result);
            handle
                .unregister_pointer(target)
                .expect("cached route owns the target");

            let up =
                make_up_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }));
            let payload = unwind.expect_err("the transaction must resume its first panic");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"target first panic"));
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert!(binding.arena().is_empty());
        });
    }

    #[derive(Clone, Copy)]
    enum BindingEntryPoint {
        HitTestClosure,
    }

    impl BindingEntryPoint {
        fn dispatch(self, binding: &GestureBinding, event: &PointerEvent, result: &HitTestResult) {
            match self {
                Self::HitTestClosure => {
                    binding.handle_pointer_event(event, |_| result.clone());
                }
            }
        }
    }

    fn cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let first_pointer = PointerId::try_from(21).expect("nonzero pointer id");
        let later_pointer = PointerId::try_from(22).expect("nonzero pointer id");
        let later_handler_dropped = Rc::new(Cell::new(false));

        lane.enter(|| {
            let first_owner = PanicOnDrop;
            let first_target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &first_owner;
                })
                .expect("register first target");
            let later_owner = SetOnDrop(Rc::clone(&later_handler_dropped));
            let later_target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &later_owner;
                })
                .expect("register later target");

            let mut members = Vec::new();
            for pointer in [first_pointer, later_pointer] {
                for _ in 0..2 {
                    let member = Rc::new(CountingArenaMember::default());
                    binding.arena().add(pointer, &member);
                    members.push(member);
                }
            }

            let first_down =
                make_down_event_for_id(first_pointer, Offset::new(1.0, 1.0), PointerKind::Touch)
                    .expect("finite input");
            binding.handle_pointer_event(&first_down, |_| hit_result(first_target));
            let later_down =
                make_down_event_for_id(later_pointer, Offset::new(2.0, 2.0), PointerKind::Touch)
                    .expect("finite input");
            binding.handle_pointer_event(&later_down, |_| hit_result(later_target));

            let first_token = binding
                .hit_tests
                .borrow()
                .get(&first_pointer)
                .and_then(|cached| cached.token)
                .expect("first cached route token");
            let later_token = binding
                .hit_tests
                .borrow()
                .get(&later_pointer)
                .and_then(|cached| cached.token)
                .expect("later cached route token");

            handle
                .unregister_pointer(first_target)
                .expect("first route owns target");
            handle
                .unregister_pointer(later_target)
                .expect("later route owns target");
            for (pointer, coordinate) in [(first_pointer, 5.0), (later_pointer, 6.0)] {
                let move_event = make_move_event_for_id(
                    pointer,
                    Offset::new(coordinate, coordinate),
                    PointerKind::Touch,
                )
                .expect("finite input");
                binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
            }

            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.cancel_all_pointer_sequences();
            }));

            // Keep the RED failure safe: the old implementation loses the
            // tokens without releasing their lane routes. Explicitly release
            // them before asserting so PanicOnDrop cannot double-panic during
            // test teardown.
            if unwind.is_ok() {
                let _ = catch_unwind(AssertUnwindSafe(|| handle.release_route(first_token)));
                let _ = handle.release_route(later_token);
            }

            let payload = unwind.expect_err("the first cleanup panic must propagate");
            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert!(
                later_handler_dropped.get(),
                "cleanup after the first panic must still release later handler owners"
            );
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert_eq!(binding.pending_move_count(), 0);
            assert!(binding.arena().is_empty());
        });
    }

    fn cancel_all_rejects_reentrant_input_from_route_destructors() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let reentrant_pointer = PointerId::try_from(31).expect("nonzero pointer id");

        lane.enter(|| {
            let owner = ReenterDownOnDrop {
                binding: Rc::clone(&binding),
                pointer: reentrant_pointer,
            };
            let target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &owner;
                })
                .expect("register target");
            let down =
                make_down_event(Offset::new(2.0, 2.0), PointerKind::Touch).expect("finite input");
            binding.handle_pointer_event(&down, |_| hit_result(target));
            handle
                .unregister_pointer(target)
                .expect("cached route owns target");

            binding.cancel_all_pointer_sequences();

            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert_eq!(binding.pending_move_count(), 0);
            assert!(binding.arena().is_empty());
            assert!(!binding.has_hit_test(reentrant_pointer));
        });
    }

    fn reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let target_slot = Rc::new(Cell::new(None));
        let did_unregister = Rc::new(Cell::new(false));
        let later_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let handle_for_target = handle.clone();
            let slot_for_target = Rc::clone(&target_slot);
            let did_unregister_in_target = Rc::clone(&did_unregister);
            let panic_on_terminal_drop = PanicOnDrop;
            let unregistering_target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &panic_on_terminal_drop;
                    if !did_unregister_in_target.replace(true) {
                        handle_for_target
                            .unregister_pointer(
                                slot_for_target
                                    .get()
                                    .expect("target installed before dispatch"),
                            )
                            .expect("unregister target reentrantly");
                    }
                })
                .expect("register unregistering target");
            target_slot.set(Some(unregistering_target));

            let later_count = Rc::clone(&later_deliveries);
            let later_target = handle
                .register_pointer(move |_| later_count.set(later_count.get() + 1))
                .expect("register later target");
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(unregistering_target));
            result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later_target));
            let members = [
                Rc::new(CountingArenaMember::default()),
                Rc::new(CountingArenaMember::default()),
            ];
            for member in &members {
                binding
                    .arena()
                    .add(PointerId::new(core::num::NonZeroU64::MIN), member);
            }

            let down =
                make_down_event(Offset::new(6.0, 6.0), PointerKind::Touch).expect("finite input");
            binding.handle_pointer_event(&down, |_| result);
            assert!(did_unregister.get());
            assert_eq!(later_deliveries.get(), 1);
            assert!(
                !binding
                    .arena()
                    .is_open(PointerId::new(core::num::NonZeroU64::MIN))
            );

            handle
                .unregister_pointer(later_target)
                .expect("unregister later target");
            let up =
                make_up_event(Offset::new(6.0, 6.0), PointerKind::Touch).expect("finite input");
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }));
            let payload = unwind.expect_err("terminal route cleanup must propagate Drop panic");
            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(later_deliveries.get(), 2);
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert!(binding.arena().is_empty());
        });
    }

    fn assert_arena_accept_panic_cleanup(entry_point: BindingEntryPoint) {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let handler_dropped = Rc::new(Cell::new(false));

        lane.enter(|| {
            let probe = SetOnDrop(Rc::clone(&handler_dropped));
            let arena = binding.arena().clone();
            let accepting_member = Rc::new(PanickingAcceptArenaMember);
            let competing_member = Rc::new(CountingArenaMember::default());
            let target = handle
                .register_pointer(move |dispatch| {
                    let event = dispatch.local;
                    let _keep_probe_alive = &probe;
                    if matches!(event, PointerEvent::Down(_)) {
                        arena.add(
                            PointerId::new(core::num::NonZeroU64::MIN),
                            &accepting_member,
                        );
                        arena.add(
                            PointerId::new(core::num::NonZeroU64::MIN),
                            &competing_member,
                        );
                    }
                })
                .expect("register hit target");
            let result = hit_result(target);

            let down =
                make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
            entry_point.dispatch(&binding, &down, &result);
            handle
                .unregister_pointer(target)
                .expect("cached route owns the target");
            assert!(!handler_dropped.get());

            let up =
                make_up_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                entry_point.dispatch(&binding, &up, &HitTestResult::new());
            }));
            let payload = unwind.expect_err("arena accept panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"arena accept panic"));
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert!(
                binding.arena().is_empty(),
                "the arena slot must be removed before invoking its members"
            );
            assert!(
                handler_dropped.get(),
                "the cached route must be released before unwind resumes"
            );
        });
    }

    fn arena_accept_panic_cleans_up_handle_pointer_event() {
        assert_arena_accept_panic_cleanup(BindingEntryPoint::HitTestClosure);
    }
}
