//! Gesture Binding - owner-runtime coordinator for pointer event handling
//!
//! GestureBinding is the main entry point for handling pointer events in the
//! gesture system. It is a thin composition root (ADR-0217 §2) over services
//! that hold no reference back to it or to each other:
//!
//! | Service | Holds |
//! |---|---|
//! | `PointerSequences` | admitted contacts, their routes, captures, resamplers and refusals |
//! | `MoveCoalescer` | one frame-coalesced move per pointer and surface |
//! | `ScrollSequences` | wheel and trackpad scroll latches per source and surface |
//! | `PanZoomSequences` | native pan/zoom leases per source and surface |
//! | [`MouseTracker`] | hover, enter/exit and cursor state per source and surface |
//! | `ArenaDriver` | the close-on-down / sweep-on-up arena lifecycle |
//! | `CloseCoordinator` | the closed flag and when a close retires state |
//!
//! Every service is keyed by [`InputSurface`]: the presentation's window, or a
//! surface embedded in it ([`GestureBinding::embed_surface`]). A contact stays
//! with the surface that admitted its Down; its tail is delivered there
//! wherever the platform reports it.
//!
//! A refused Down admits no route. Its Move/Up/Cancel tail stays refused until
//! that contact terminates. Refusals occupy a fixed array bounded by the same
//! limit as admitted contacts. If it fills, the binding suppresses every
//! untracked Move/Up/Cancel until a lifecycle reset, while admitted contacts and
//! fresh valid Downs continue. This conservative fallback prevents a hostile
//! stream of distinct refused identities from growing owner state indefinitely.
//!
//! # Closing
//!
//! Closing sets a flag. Retired state leaves each service through its
//! `drain(&self)` (`RefCell::take`) once no dispatch of the binding is on the
//! stack: at once when the binding is idle, otherwise when the outermost
//! dispatch returns. A close requested from a callback first withdraws the
//! arena's and the pointer router's authority, so the rest of that dispatch
//! reaches no further handler; callbacks the drain runs (rejections, capture
//! loss, destructors) run after every service borrow is released, and input
//! they dispatch is refused by the closed binding.
//! `a_closed_binding_drains_services_after_dispatch` pins this.
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
//!    hit test at the device's last-known hover position on that surface (the
//!    events carry no position of their own)
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
//! Hover paths retire opaque metadata individually after delivery, preserving
//! an already caught first failure while the remaining accepted frame peers
//! run. Replacing a queued hover commits the newer packet before retiring its
//! outgoing path. Entries retain metadata during unwind (ADR-0127).
//! `binding_input_contract_matrix` covers these boundaries and fresh recovery.
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

mod arena_driver;
mod close_coordinator;
mod move_coalescer;
mod pan_zoom_sequences;
mod pointer_sequences;
mod scroll_sequences;

use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use flui_foundation::{IdExhausted, IdMint, InputSequenceId, MonotonicClock};
use flui_platform_api::pointer::{PanZoomEvent, ScrollEvent};
use smallvec::SmallVec;

use self::{
    arena_driver::ArenaDriver,
    close_coordinator::{CloseCoordinator, Retirement},
    move_coalescer::{FrozenMove, MoveCoalescer, MoveKey, PendingMove, PendingMoveState},
    pan_zoom_sequences::{PanZoomAdmission, PanZoomSequences},
    pointer_sequences::{
        CachedPointerRoute, MAX_SIMULTANEOUS_POINTERS, PointerSequence, PointerSequences,
        ResamplerSnapshot,
    },
    scroll_sequences::{ScrollAdmission, ScrollSequences},
};
use crate::events::{
    CancelReason, DeviceId, PointerCancel, PointerEvent, PointerEventExt, PointerInfo, PointerKind,
};
use crate::{
    close::{CloseMode, ClosePanic, CloseTombstone},
    containment::RoutePanic,
    gesture::arena::{DetachedArenaBatch, GestureArena},
    gesture::settings::{GestureProfiles, GestureSettings},
    ids::{EmbeddedSurfaceId, InputSurface, PointerId, SurfaceIdsExhausted},
    pointer::{
        HitTestResult, MouseTracker, PanZoomAdmissionAuthority, PointerEventResampler,
        PointerMotionKind, PointerRouter, ResolvedRouteToken, SamplingClock,
        active_dispatch_handle, pointer_capture::ContactCapture,
    },
};

fn terminal_hit_test(_: Offset<f64>) -> HitTestResult {
    unreachable!("BUG: terminal Cancel never hit-tests")
}

/// A wheel or pan-zoom source: native hardware when reported, else the contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

struct DetachedPointerSequence {
    cached: Option<CachedPointerRoute>,
    pending_moves: SmallVec<[PendingMoveState; 1]>,
    arena: DetachedArenaBatch,
}

#[derive(Clone, Copy)]
enum MotionFlush {
    Frame(Option<(web_time::Instant, web_time::Instant)>),
    BeforeInput,
}

/// What [`GestureBinding::dispatch_pointer_on`] did with one input.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchOutcome {
    sequence: Option<InputSequenceId>,
}

impl DispatchOutcome {
    /// The input sequence the event belonged to: its contact's, or its wheel
    /// or trackpad latch's. `None` for hover, device lifecycle, and input the
    /// binding refused.
    #[must_use]
    pub const fn sequence(&self) -> Option<InputSequenceId> {
        self.sequence
    }
}

/// A source of input that is not a pointer contact (ADR-0207 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContactlessSource {
    /// Keys driving a gesture's equivalent, such as arrow-key scrolling.
    Keyboard,
    /// An assistive-technology action.
    Assistive,
    /// The application itself: `reveal`, a programmatic scroll.
    Programmatic,
    /// Scrolling the framework drives while something is dragged near an edge.
    AutoScroll,
}

impl ContactlessSource {
    /// The reserved device a sequence of this source runs on.
    #[must_use]
    pub const fn device(self) -> DeviceId {
        match self {
            Self::Keyboard => DeviceId::KEYBOARD,
            Self::Assistive => DeviceId::ASSISTIVE,
            Self::Programmatic | Self::AutoScroll => DeviceId::PROGRAMMATIC,
        }
    }
}

/// A contactless input sequence a binding started: its identity and the
/// device it runs on, which with the surface keys its latches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContactlessSequence {
    /// The sequence's identity.
    pub id: InputSequenceId,
    /// The reserved device of its source.
    pub device: DeviceId,
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
/// the active `HeadlessBinding` / `UiRuntime`; process-global gesture ownership is
/// intentionally not part of ADR-0027.
///
/// # Event Coalescing
///
/// Desktop platforms can generate 100+ mouse move events per second.
/// GestureBinding delivers one current move per pointer, surface and frame,
/// retaining earlier hardware samples in its coalesced history for velocity
/// consumers. Call `flush_pending_moves()` once per frame to process the events.
///
/// # Thread affinity
///
/// `GestureBinding` is owner-local and `!Send + !Sync` (ADR-0207 §8). Its
/// pointer router and executable gesture callbacks are not `Send + Sync`;
/// render hit-test entries and route tokens remain on the separate data plane.
pub struct GestureBinding {
    close: CloseCoordinator,
    capture_wake: RefCell<Option<std::sync::Weak<dyn flui_platform_api::PlatformWindow>>>,
    contacts: PointerSequences,
    moves: MoveCoalescer,
    scrolls: ScrollSequences,
    pan_zooms: PanZoomSequences,

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

    /// Per-surface, per-device enter/exit/hover/cursor state for this presentation.
    mouse_tracker: MouseTracker,

    /// Resolves conflicts between competing gesture recognizers.
    ///
    /// Binding-owned ([`SweepModel::BindingDriven`](crate::gesture::SweepModel)):
    /// this binding runs the close-on-down / sweep-on-up lifecycle itself in
    /// [`handle_pointer_event`](Self::handle_pointer_event), and an app shell
    /// hands a clone of this handle to the view subtree (via flui-widgets'
    /// `GestureArenaScope`) so every detector below competes here without any
    /// recognizer self-closing or self-sweeping the shared arena.
    arena: ArenaDriver,

    /// The per-kind gesture presets host observations resolve over.
    default_profiles: GestureProfiles,

    /// The last embedded-surface identity minted; refuses at the terminal value.
    last_embedded_surface: Cell<u64>,

    /// Issues every input sequence identity this binding's contacts, wheel
    /// latches and contactless sources carry; refuses at exhaustion
    /// (ADR-0207 §4).
    sequences: IdMint<InputSequenceId>,
    /// The sequence the input being dispatched belongs to, published by the
    /// admission that resolved it; saved and restored around each dispatch so
    /// a nested one cannot leak its own.
    dispatched_sequence: Cell<Option<InputSequenceId>>,
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
    /// active UI runtime (`HeadlessBinding`/`UiRuntime`) over a process global.
    pub fn new() -> Self {
        Self::with_settings_and_clock(
            GestureSettings::default(),
            std::sync::Arc::new(flui_foundation::SystemClock),
        )
    }

    /// Create with specific settings: one profile, or one per pointer kind.
    pub fn with_settings(settings: impl Into<GestureProfiles>) -> Self {
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

    /// Create against an explicit clock with chosen gesture presets, the
    /// baseline that host observations resolve over for each pointer kind
    /// (ADR-0212).
    pub fn with_settings_and_clock(
        settings: impl Into<GestureProfiles>,
        clock: std::sync::Arc<dyn MonotonicClock>,
    ) -> Self {
        Self {
            close: CloseCoordinator::default(),
            capture_wake: RefCell::new(None),
            contacts: PointerSequences::default(),
            moves: MoveCoalescer::default(),
            scrolls: ScrollSequences::default(),
            pan_zooms: PanZoomSequences::default(),
            resampling_enabled: Cell::new(false),
            sampling_clock: Cell::new(SamplingClock::default()),
            clock: std::sync::Arc::clone(&clock),
            pointer_router: PointerRouter::new(),
            mouse_tracker: MouseTracker::new(),
            arena: ArenaDriver::new(GestureArena::binding_driven(clock)),
            default_profiles: settings.into(),
            last_embedded_surface: Cell::new(0),
            sequences: IdMint::new(),
            dispatched_sequence: Cell::new(None),
        }
    }

    /// Mint the identity of a surface embedded in this binding's window.
    ///
    /// Input the embedder routes through
    /// [`handle_pointer_event_on`](Self::handle_pointer_event_on) with this
    /// surface keeps its own hover, move coalescing and wheel and pan-zoom
    /// latches, apart from the window's.
    ///
    /// # Errors
    ///
    /// [`SurfaceIdsExhausted`] once every identity was minted; the binding then
    /// refuses permanently rather than reissue one.
    pub fn embed_surface(&self) -> Result<InputSurface, SurfaceIdsExhausted> {
        let next = self
            .last_embedded_surface
            .get()
            .checked_add(1)
            .ok_or(SurfaceIdsExhausted)?;
        self.last_embedded_surface.set(next);
        let id = NonZeroU64::new(next).expect("BUG: a successor is non-zero");
        Ok(InputSurface::Embedded(EmbeddedSurfaceId::new(id)))
    }

    /// This binding with its next sequence identity `last + 1`, so a test can
    /// reach exhaustion without minting every identity before it.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn with_sequences_after(mut self, last: u64) -> Self {
        self.sequences = IdMint::starting_after(last);
        self
    }

    /// Start a contactless input sequence: an assistive action, keyboard
    /// scrolling, a programmatic scroll or an auto-scroll.
    ///
    /// The binding is the only issuer, so the sequence never shares an id with
    /// a contact or latch of this binding. Its device is the source's reserved
    /// one ([`ContactlessSource::device`]), which no hardware takes.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once every identity was issued; the binding refuses
    /// permanently rather than reissue one.
    pub fn begin_contactless(
        &self,
        source: ContactlessSource,
    ) -> Result<ContactlessSequence, IdExhausted> {
        Ok(ContactlessSequence {
            id: self.sequences.next()?,
            device: source.device(),
        })
    }

    /// Run `body` as one dispatch of this binding. A close requested while any
    /// dispatch runs retires the binding's state when the outermost returns,
    /// after `body`'s borrows are gone; a failure of `body` stays the first
    /// and makes that retirement preserving.
    fn dispatch<R>(&self, body: impl FnOnce() -> R) -> R {
        self.close.enter();
        let outcome = catch_unwind(AssertUnwindSafe(body));
        let retirement_failure = self.leave_dispatch(outcome.is_err());
        match outcome {
            Ok(value) => {
                if let Some(payload) = retirement_failure {
                    resume_unwind(payload);
                }
                value
            }
            Err(payload) => resume_unwind(payload),
        }
    }

    /// Leave one dispatch. The outermost settles every close owed to it,
    /// including one a callback of that settlement requests, and returns the
    /// first failure of a settlement when `failed` did not already own one.
    fn leave_dispatch(&self, mut failed: bool) -> Option<Box<dyn std::any::Any + Send>> {
        if !self.close.leave() {
            return None;
        }
        let mut first = None;
        while let Some(mode) = self.close.take_pending() {
            let mode = if failed {
                CloseMode::PreservingFailure
            } else {
                mode
            };
            // The drain is a dispatch too: a close it causes waits for this loop.
            self.close.enter();
            let retired = catch_unwind(AssertUnwindSafe(|| self.retire_closed(mode)));
            self.close.leave();
            if let Err(payload) = retired {
                if failed {
                    flui_foundation::panic::retain_opaque_payload(payload);
                } else {
                    failed = true;
                    first = Some(payload);
                }
            }
        }
        first
    }

    // ========================================================================
    // Resampler Wiring
    // ========================================================================

    pub(crate) fn set_pointer_capture_wake(
        &self,
        window: std::sync::Weak<dyn flui_platform_api::PlatformWindow>,
    ) {
        self.arena.arena().set_deadline_wake(window.clone());
        *self.capture_wake.borrow_mut() = Some(window);
    }

    pub(crate) fn deadlines_with_recovery(
        &self,
        now: web_time::Instant,
        poll_due: bool,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) {
        self.dispatch(|| {
            let arena = self.arena.arena();
            arena.with_recovery_context(recovery, |scope| {
                if poll_due {
                    arena.invoke_with_recovery(
                        flui_foundation::panic::FailureClass::Delivery,
                        scope,
                        |_| {
                            if let Some(failure) = self.drain_capture_losses(None) {
                                failure.resume();
                            }
                        },
                    );
                }
                let now = crate::events::event_time_of(self.clock.as_ref(), now);
                arena.deadlines_with_recovery(now, poll_due, scope);
            });
        });
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
        let active_pointer_count = self.contacts.len();
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
        self.contacts.active_resampler_count()
    }

    /// Whether accepted contact samples still need an owner frame.
    /// An active contact with an empty queue creates no sampling demand.
    #[must_use]
    pub fn has_pending_pointer_samples(&self) -> bool {
        self.contacts.has_pending_samples()
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
    /// hovered region, on the window and every surface embedded in it, and
    /// reset the cursor. The platform's window-leave signal (winit
    /// `CursorLeft`) routes here; without it a widget hovered at the moment
    /// the cursor crosses the window edge stays hovered forever. Must run
    /// inside the owner lane scope, like every other dispatch on this binding.
    pub fn handle_pointer_left_window(&self) {
        self.dispatch(|| self.mouse_tracker.dispatch_window_left());
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
        self.arena.arena()
    }

    /// The per-kind gesture presets this binding was created with.
    #[inline]
    pub fn default_profiles(&self) -> &GestureProfiles {
        &self.default_profiles
    }

    // ========================================================================
    // Event Handling
    // ========================================================================

    /// Handle a pointer event the window reported.
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
        self.handle_pointer_event_on(InputSurface::Window, event, hit_test_fn);
    }

    /// Handle a pointer event `surface` reported; `hit_test_fn` answers in
    /// that surface's coordinates.
    ///
    /// Hover, move coalescing and wheel and pan-zoom latches are kept per
    /// surface. A contact belongs to the surface that admitted its Down: its
    /// Move/Up/Cancel are delivered on that route even when another surface
    /// reports them, so a drag that leaves its surface stays with it.
    pub fn handle_pointer_event_on<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        hit_test_fn: F,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        let _ = self.dispatch_pointer_on(surface, event, hit_test_fn);
    }

    /// Dispatch a pointer event `surface` reported, as
    /// [`handle_pointer_event_on`](Self::handle_pointer_event_on) does, and
    /// report the input sequence it belonged to.
    ///
    /// A Down mints its contact's [`InputSequenceId`]; the contact's Move, Up
    /// and Cancel carry the same one. A wheel or trackpad packet carries its
    /// latch's. Hover and device lifecycle belong to no sequence. A binding
    /// whose identities are exhausted refuses new contacts and latches.
    pub fn dispatch_pointer_on<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        hit_test_fn: F,
    ) -> DispatchOutcome
    where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        // A nested dispatch from a callback reports its own sequence, then
        // hands this one back, also when it unwinds.
        struct Restore<'a> {
            cell: &'a Cell<Option<InputSequenceId>>,
            outer: Option<InputSequenceId>,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.cell.set(self.outer);
            }
        }
        let restore = Restore {
            outer: self.dispatched_sequence.take(),
            cell: &self.dispatched_sequence,
        };
        self.dispatch_pointer_kernel(surface, event, hit_test_fn);
        let outcome = DispatchOutcome {
            sequence: crate::PointerEventExt::pointer_id(event)
                .and(self.dispatched_sequence.get()),
        };
        drop(restore);
        outcome
    }

    fn dispatch_pointer_kernel<F>(&self, surface: InputSurface, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.close.is_closed() {
            let mut failure = ClosePanic::for_rejection(self.close.tombstone().mode());
            failure.retire(crate::retain::Owned(hit_test_fn));
            failure.finish();
            return;
        }
        self.dispatch(|| {
            let ending = matches!(
                event,
                PointerEvent::Down(_) | PointerEvent::Up(_) | PointerEvent::Cancel(_)
            )
            .then(|| crate::PointerEventExt::pointer_id(event))
            .flatten();
            let mut first = self.drain_capture_losses(ending);
            let delivered = RoutePanic::capture(|| {
                self.handle_pointer_event_kernel(surface, event, hit_test_fn, &mut first);
            });
            RoutePanic::preserve_first(&mut first, delivered, "pointer input after capture loss");
            if let Some(failure) = first {
                failure.resume();
            }
        });
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
        if self.close.is_closed() {
            return 0;
        }
        // Anchored at the binding's clock, never the wall clock: a replay
        // driven by a virtual clock samples the same window (ADR-0207 §2).
        let sample_window = self
            .is_resampling_enabled()
            .then(|| {
                let now = self.clock.now();
                now.checked_add(self.sampling_clock.get().period())
                    .map(|next| (now, next))
            })
            .flatten();
        self.dispatch(|| self.flush_pending_moves_kernel(MotionFlush::Frame(sample_window)))
    }

    /// Deliver this presentation's accepted measured motion before an observing
    /// input operation, without advancing frame time or ending contacts.
    /// Reentrant motion remains queued for a later operation or frame.
    ///
    /// The runtime calls this before Keyboard and IME dispatch. Ordinary frame
    /// interpolation remains the responsibility of [`Self::flush_pending_moves`].
    pub fn flush_pending_input(&self) -> usize {
        if self.close.is_closed() {
            return 0;
        }
        self.dispatch(|| self.flush_pending_moves_kernel(MotionFlush::BeforeInput))
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
        if self.close.is_closed() {
            return Ok(0);
        }
        Ok(self.dispatch(|| {
            self.flush_pending_moves_kernel(MotionFlush::Frame(Some((
                sample_time,
                next_sample_time,
            ))))
        }))
    }

    fn flush_pending_moves_kernel(&self, mode: MotionFlush) -> usize {
        if self.contacts.is_retiring_any() {
            return 0;
        }

        let mut first_panic = match mode {
            MotionFlush::Frame(_) => self.drain_capture_losses(None),
            MotionFlush::BeforeInput => None,
        };
        // Freeze the complete direct/coalesced frame batch before any user
        // callback runs. Re-entrant moves replace their exact marker and
        // therefore always belong to the next frame.
        let drained = self.moves.freeze();

        let mut count = 0;
        let contact_captures: SmallVec<[Rc<ContactCapture>; 4]> =
            if matches!(mode, MotionFlush::BeforeInput) {
                drained
                    .iter()
                    .filter_map(|frozen| {
                        let PendingMove::Contact { sequence, .. } = &frozen.pending else {
                            return None;
                        };
                        self.contacts.capture_of(frozen.key.1, *sequence)
                    })
                    .collect()
            } else {
                SmallVec::new()
            };
        // Replacement queued markers do not revoke this committed input prefix.
        // Capture release must therefore also recognize its delivery authority.
        let contact_deliveries: SmallVec<[_; 4]> = contact_captures
            .iter()
            .map(|capture| capture.begin_delivery())
            .collect();
        if matches!(mode, MotionFlush::BeforeInput) {
            // Freeze every measured prefix before callbacks or capture-loss
            // settlement. A callback for A cannot pull newer B motion into this
            // accepted round. Capture guards protect each committed prefix from
            // reentrant release until all its measured packets are delivered.
            let measured: SmallVec<[_; 4]> = self
                .contacts
                .resampler_snapshots()
                .into_iter()
                .map(|snapshot| {
                    let events = snapshot.resampler.prepare_pending_events();
                    (snapshot, events)
                })
                .collect();
            let deliveries: SmallVec<[_; 4]> = measured
                .iter()
                .map(|(snapshot, _)| snapshot.capture.begin_delivery())
                .collect();
            let losses = self.drain_capture_losses(None);
            RoutePanic::preserve_first(&mut first_panic, losses, "input prefix capture loss");
            for (snapshot, events) in &measured {
                for event in events.deliveries() {
                    if self
                        .contacts
                        .is_current(snapshot.pointer_id, snapshot.sequence)
                    {
                        let delivered = self.dispatch_event_with_capture(
                            &event,
                            snapshot.token,
                            Some(&snapshot.capture),
                        );
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "measured motion before input",
                        );
                        count += 1;
                    }
                }
            }
            drop(deliveries);
        }
        // A resampled contact has exactly one queue: the resampler owned by
        // its cached Down route. Snapshot route capabilities before callbacks
        // so no map borrow crosses executable user code.
        if self.is_resampling_enabled()
            && let MotionFlush::Frame(Some((sample_time, next_sample_time))) = mode
        {
            for snapshot in self.contacts.resampler_snapshots() {
                let ResamplerSnapshot {
                    pointer_id,
                    sequence,
                    token,
                    resampler,
                    capture,
                } = snapshot;
                let delivery = capture.begin_delivery();
                resampler.sample(sample_time, next_sample_time, |resampled| {
                    if self.contacts.is_current(pointer_id, sequence) {
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

        for FrozenMove {
            key,
            generation,
            pending,
        } in drained
        {
            let pointer_id = key.1;
            if !self.moves.is_in_flight(key, generation) {
                let accepted_contact = matches!(mode, MotionFlush::BeforeInput)
                    && matches!(&pending, PendingMove::Contact { sequence, .. }
                        if self.contacts.is_current(pointer_id, *sequence));
                if !accepted_contact {
                    if let PendingMove::Hover { hit_test, .. } = pending {
                        hit_test.retire_metadata(&mut first_panic);
                    }
                    continue;
                }
            }
            match pending {
                PendingMove::Contact { event, sequence } => {
                    if self.contacts.is_current(pointer_id, sequence) {
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
                    hit_test.retire_metadata(&mut first_panic);
                    count += 1;
                }
            }
            drop(self.moves.remove_if_in_flight(key, generation));
        }

        drop(contact_deliveries);
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
        self.moves.has_queued() || self.contacts.has_pending_samples()
    }

    /// Get the number of queued motion events across both canonical paths.
    #[inline]
    pub fn pending_move_count(&self) -> usize {
        self.moves.queued_count() + self.contacts.pending_sample_count()
    }

    // ========================================================================
    // Pointer Sequence State
    // ========================================================================

    /// Check if there's a cached hit test for a pointer.
    #[inline]
    pub fn has_hit_test(&self, pointer_id: PointerId) -> bool {
        self.contacts.contains(pointer_id)
    }

    /// Cancel every binding-owned state slot for a pointer.
    ///
    /// This is the abnormal-sequence counterpart to receiving Up/Cancel: it
    /// releases the retained hit route, discards queued/resampled movement,
    /// and rejects the unresolved arena without selecting a winner.
    pub fn cancel_pointer_sequence(&self, pointer_id: PointerId) {
        self.dispatch(|| {
            let mut first = self.drain_capture_losses(Some(pointer_id));
            let Some(guard) = self.contacts.try_retire(pointer_id) else {
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
        });
    }

    /// Cancel every binding-owned state slot for all active pointers.
    pub fn cancel_all_pointer_sequences(&self) {
        self.dispatch(|| {
            let Some(guard) = self.contacts.try_retire_all() else {
                return;
            };
            let panic = self.clear_all_pointer_state_capturing_panic();
            drop(guard);
            if let Some(panic) = panic {
                panic.resume();
            }
        });
    }

    pub(crate) fn close_tombstones(&self) -> [CloseTombstone; 4] {
        [
            self.close.tombstone().clone(),
            self.arena.arena().close_tombstone(),
            self.pointer_router.close_tombstone(),
            self.mouse_tracker.close_tombstone(),
        ]
    }

    /// Close the binding. Idle, it retires its state now; from inside one of
    /// its own dispatches, it withdraws the arena's and the router's
    /// authority now and retires everything when the outermost dispatch
    /// returns.
    pub(crate) fn close_with_mode(&self, mode: CloseMode) {
        match self.close.close(mode) {
            Retirement::Now => self.dispatch(|| self.retire_closed(mode)),
            Retirement::AfterDispatch => {
                if mode == CloseMode::PreservingFailure {
                    for terminal in self.close_tombstones() {
                        terminal.preserve();
                    }
                }
                self.arena.withdraw();
                self.pointer_router.withdraw();
            }
        }
    }

    /// Drain every service of a closed binding and settle what they yield.
    fn retire_closed(&self, mode: CloseMode) {
        let _draining = self.close.draining();
        let mut failure = ClosePanic::for_close(mode, self.close.tombstone().clone());
        self.scrolls.drain();
        let native_sequences = self.pan_zooms.drain();
        if !failure.preserving() {
            failure.invoke(|| {
                if let Some(loss) = self.drain_capture_losses(None) {
                    loss.resume();
                }
            });
        }
        let routes = self.contacts.drain();
        for (_, route) in &routes {
            route.capture.close();
        }
        let moves = self.moves.drain();
        let native_tickets: Vec<_> = native_sequences
            .into_iter()
            .flat_map(|sequence| sequence.take_staged())
            .collect();
        for (_, ticket) in native_tickets {
            failure.run(|| (ticket.0)());
            failure.retire(crate::retain::Owned(ticket.0));
        }
        let arena_mode = if failure.preserving() {
            CloseMode::PreservingFailure
        } else {
            CloseMode::Ordinary
        };
        failure.invoke(|| self.arena.drain(arena_mode));
        let router_mode = if failure.preserving() {
            CloseMode::PreservingFailure
        } else {
            CloseMode::Ordinary
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
            failure.invoke(|| self.arena.drain(CloseMode::PreservingFailure));
            failure.invoke(|| {
                self.pointer_router
                    .close_with_mode(CloseMode::PreservingFailure);
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
        self.dispatch(|| {
            self.scrolls.drain();
            let native_sequences = self.pan_zooms.drain();
            let mut first_panic = self.drain_capture_losses(None);
            self.contacts.reset_refusals();
            let contacts = self.contacts.contacts(None);
            pan_zoom_sequences::retire_sequences(native_sequences, &mut first_panic);
            for contact in contacts {
                // A handler run by an earlier iteration may have re-entered the
                // binding and terminated or replaced this sequence. The numeric
                // pointer identity alone does not identify that admission.
                if !self
                    .contacts
                    .is_current(contact.pointer.id, contact.sequence)
                {
                    continue;
                }
                let cancel = PointerEvent::Cancel(PointerCancel::new(
                    contact.pointer,
                    contact.time,
                    CancelReason::FocusLost,
                ));
                // The terminal arm never invokes the hit-test callback: Cancel
                // resolves over the route cached at Down. A panicking handler
                // must not exempt the remaining contacts from cancellation —
                // every snapshotted pointer is cancelled first, then the first
                // panic resumes (the same all-work-first posture the kernel
                // itself has within one sequence).
                let delivered = RoutePanic::capture(|| {
                    self.handle_pointer_event_after_signal_withdrawal(
                        contact.surface,
                        &cancel,
                        |_| unreachable!("BUG: a terminal Cancel must never hit-test"),
                        &mut first_panic,
                    );
                });
                RoutePanic::preserve_first(
                    &mut first_panic,
                    delivered,
                    "focus-loss pointer cancel",
                );
            }
            if let Some(panic) = first_panic {
                panic.resume();
            }
        });
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
        self.dispatch(|| {
            let hit_tests = self.contacts.len();
            let resamplers = self.active_resampler_count();
            let pending_moves = self.moves.len();
            let arenas = self.arena.arena().len();
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
        });
    }

    /// Get the number of active pointers (with cached hit tests).
    #[inline]
    pub fn active_pointer_count(&self) -> usize {
        self.contacts.len()
    }

    // ========================================================================
    // Arena Management
    // ========================================================================

    /// Manually close the arena for a pointer.
    ///
    /// Normally called automatically on pointer down.
    pub fn close_arena(&self, pointer_id: PointerId) {
        self.dispatch(|| self.arena.arena().close(pointer_id));
    }

    /// Manually sweep the arena for a pointer.
    ///
    /// Normally called automatically on pointer up. Cancelled recognizers
    /// reject themselves instead of asking the binding to force a winner.
    pub fn sweep_arena(&self, pointer_id: PointerId) {
        self.dispatch(|| self.arena.arena().sweep(pointer_id));
    }

    /// Resolve lone arena members queued during the preceding event/frame
    /// transaction.
    pub fn drain_deferred_arena_resolutions(&self) -> usize {
        self.dispatch(|| self.arena.arena().drain_deferred_resolutions())
    }

    /// Advance time-based recognizer deadlines (e.g. long-press hold).
    ///
    /// Call once per frame on the UI thread, beside [`Self::flush_pending_moves`].
    /// A recognizer deadline is otherwise only advanced opportunistically on the
    /// next pointer event, so a stationary held pointer past its deadline would
    /// never fire (e.g. long-press on a finger held perfectly still).
    pub fn tick_deadlines(&self) {
        self.dispatch(|| {
            let mut first = self.drain_capture_losses(None);
            let deadlines = RoutePanic::capture(|| self.arena.arena().poll_deadlines());
            RoutePanic::preserve_first(&mut first, deadlines, "deadlines after capture loss");
            if let Some(panic) = first {
                panic.resume();
            }
        });
    }

    /// Whether any recognizer in the arena has an armed time-based deadline.
    ///
    /// The frame loop reads this right after [`tick_deadlines`](Self::tick_deadlines)
    /// to decide whether another frame must be requested: the tick only runs on
    /// frames, so without a frame scheduled at the deadline an idle app would
    /// never fire a held long-press or an expired double-tap window.
    pub fn has_pending_deadlines(&self) -> bool {
        self.arena.arena().has_pending_deadlines()
    }

    /// The earliest instant any armed recognizer deadline in this binding's
    /// arena will fire, if any — the wall-clock-wake counterpart of
    /// [`has_pending_deadlines`](Self::has_pending_deadlines). A caller
    /// computing a platform `ControlFlow::WaitUntil` target reads this so a
    /// presentation with nothing else scheduled (no dirty tree, no running
    /// animation) still wakes at the right instant to resolve an in-flight
    /// long-press/double-tap deadline, rather than only lazily on the next
    /// unrelated event.
    pub fn next_deadline(&self) -> Option<flui_platform_api::EventTime> {
        self.arena.arena().next_deadline()
    }

    /// The instant `time` names on this binding's clock, so a host can turn a
    /// deadline into a native wake.
    pub(crate) fn instant_at(&self, time: flui_platform_api::EventTime) -> web_time::Instant {
        crate::events::instant_at(self.clock.as_ref(), time)
    }

    // ========================================================================
    // Internal Methods
    // ========================================================================

    fn handle_pointer_event_kernel<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        hit_test_fn: F,
        first: &mut Option<RoutePanic>,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.contacts.is_retiring_all() {
            return;
        }
        if let PointerEvent::Cancel(cancel) = event {
            let source = SignalSource::from(cancel.pointer);
            self.scrolls.withdraw_source(source);
            let native = self.pan_zooms.withdraw_source(source);
            pan_zoom_sequences::retire_sequences(native, first);
        }
        self.handle_pointer_event_after_signal_withdrawal(surface, event, hit_test_fn, first);
    }

    /// Owner cancellation batches withdraw signal admissions before invoking
    /// their first callback. Later old contacts must not withdraw a newly
    /// admitted signal from an earlier callback in that same batch.
    fn handle_pointer_event_after_signal_withdrawal<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        hit_test_fn: F,
        incoming: &mut Option<RoutePanic>,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        if self.contacts.is_retiring_all() {
            return;
        }
        let Some(pointer_id) = crate::PointerEventExt::pointer_id(event) else {
            // Device lifecycle has no contact identity. It still reaches
            // global observers without fabricating a per-pointer route.
            let mut first_panic = incoming.take();
            if let PointerEvent::DeviceRemoved(device) = event {
                let source = SignalSource::Device(device.device);
                self.scrolls.withdraw_source(source);
                let native = self.pan_zooms.withdraw_source(source);
                pan_zoom_sequences::retire_sequences(native, &mut first_panic);
                for contact in self.contacts.contacts(Some(device.device)) {
                    if !self
                        .contacts
                        .is_current(contact.pointer.id, contact.sequence)
                    {
                        continue;
                    }
                    let cancel = PointerEvent::Cancel(PointerCancel::new(
                        contact.pointer,
                        device.time,
                        CancelReason::DeviceRemoved,
                    ));
                    let delivered = RoutePanic::capture(|| {
                        self.handle_pointer_event_after_signal_withdrawal(
                            contact.surface,
                            &cancel,
                            terminal_hit_test,
                            &mut first_panic,
                        );
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
        if self.contacts.is_retiring_all() || self.contacts.is_retiring(pointer_id) {
            tracing::debug!(
                ?pointer_id,
                "ignoring re-entrant input during pointer teardown"
            );
            return;
        }

        if matches!(event, PointerEvent::Move(_) | PointerEvent::ButtonChange(_))
            && self.contacts.release_requested(pointer_id)
        {
            return;
        }
        let refused_tail = self.contacts.refuses_tail(pointer_id, event.device_id());
        if !matches!(event, PointerEvent::Down(_))
            && let Some(sequence) = self.contacts.sequence_of(pointer_id)
        {
            self.dispatched_sequence.set(Some(sequence));
        }
        if matches!(
            event,
            PointerEvent::Move(_)
                | PointerEvent::Up(_)
                | PointerEvent::Cancel(_)
                | PointerEvent::ButtonChange(_)
        ) && let Some(pointer) = crate::events::pointer_info(event)
            && !self
                .contacts
                .observe(pointer, crate::events::event_time(event))
        {
            return;
        }
        match event {
            PointerEvent::Down(_) => {
                if self.contacts.refuses_down(pointer_id) {
                    return;
                }
                self.contacts.finish_refusal(pointer_id);
            }
            PointerEvent::Move(_) | PointerEvent::Up(_) | PointerEvent::Cancel(_)
                if refused_tail =>
            {
                if matches!(event, PointerEvent::Up(_) | PointerEvent::Cancel(_)) {
                    self.contacts.finish_refusal(pointer_id);
                }
                return;
            }
            _ => {}
        }

        match event {
            PointerEvent::Down(down) => {
                let mut first_panic = None;
                let has_superseded_sequence = self.contacts.contains(pointer_id)
                    || self.moves.contains_pointer(pointer_id)
                    || self.arena.arena().has_active(pointer_id);
                if has_superseded_sequence {
                    let guard = self
                        .contacts
                        .try_retire(pointer_id)
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

                let active = self.contacts.len();
                if active >= MAX_SIMULTANEOUS_POINTERS {
                    self.contacts.refuse(pointer_id);
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
                // Minted before hit testing: an exhausted binding refuses the
                // contact before any target sees it, and never reissues an id.
                let Ok(sequence) = self.sequences.next().map(PointerSequence) else {
                    self.contacts.refuse(pointer_id);
                    tracing::warn!(
                        ?pointer_id,
                        "dropping Down: input sequence identities are exhausted"
                    );
                    if let Some(panic) = first_panic {
                        panic.resume();
                    }
                    return;
                };
                self.dispatched_sequence.set(Some(sequence.0));

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
                        let lifecycle =
                            RoutePanic::capture(|| self.arena.arena().close(pointer_id));
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
                let capture = ContactCapture::new(
                    down.pointer,
                    sequence.0.get(),
                    self.capture_wake.borrow().clone(),
                );
                let resampler = PointerEventResampler::new(pointer_id);
                if self.is_resampling_enabled() {
                    resampler.start_tracking();
                }
                self.contacts.admit(
                    pointer_id,
                    CachedPointerRoute {
                        token,
                        sequence,
                        surface,
                        capture,
                        resampler,
                        pointer: down.pointer,
                        time: down.sample.time,
                    },
                );

                let delivered = self.dispatch_event(event, token);
                RoutePanic::preserve_first(&mut first_panic, delivered, "pointer Down dispatch");
                let lifecycle = RoutePanic::capture(|| self.arena.arena().close(pointer_id));
                RoutePanic::preserve_first(&mut first_panic, lifecycle, "pointer Down arena close");
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::Move(pointer_move) => {
                let position = event
                    .position()
                    .expect("BUG: Move carries a checked position");
                if let Some((sequence, admitted, resampler)) = self
                    .contacts
                    .advance(pointer_id, pointer_move.current().time)
                {
                    if self.is_resampling_enabled() {
                        resampler.add_event_with_arrival(event.clone(), self.clock.now());
                    } else {
                        // A contact queues under the surface that admitted it.
                        self.queue_pending_move(
                            (admitted, pointer_id),
                            PendingMove::Contact {
                                event: event.clone(),
                                sequence,
                            },
                        );
                    }

                    // A metadata boundary may synchronously deliver an older
                    // packet whose callback replaces this contact's route.
                    if !self.contacts.is_current(pointer_id, sequence) {
                        return;
                    }

                    if matches!(
                        pointer_move.pointer.kind,
                        PointerKind::Mouse | PointerKind::Pen { .. }
                    ) {
                        // Hover follows the surface under the pointer.
                        let fresh_hit_test = hit_test_fn(position);
                        self.mouse_tracker.update_with_motion_on(
                            surface,
                            event,
                            PointerMotionKind::Contact,
                            &fresh_hit_test,
                        );
                    }
                } else {
                    let fresh_hit_test = hit_test_fn(position);
                    self.queue_pending_move(
                        (surface, pointer_id),
                        PendingMove::Hover {
                            event: event.clone(),
                            hit_test: fresh_hit_test.clone(),
                        },
                    );
                    self.mouse_tracker.update_with_motion_on(
                        surface,
                        event,
                        PointerMotionKind::Hover,
                        &fresh_hit_test,
                    );
                }
            }
            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let guard = self
                    .contacts
                    .try_retire(pointer_id)
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
                let panic = if self.contacts.contains(pointer_id) {
                    self.dispatch_on_cached_route(pointer_id, event)
                } else {
                    // A HOVERING pointer — the only state in which a
                    // window-boundary Enter/Leave normally arrives — has no
                    // cached route, so resolve a fresh ephemeral path the
                    // way Scroll does. The event itself carries no position
                    // (the macOS and Android producers only identify the
                    // pointer), so the path is resolved at the device's
                    // last-known hover position on this surface. A device
                    // this surface has never seen has no position to
                    // resolve; the event still reaches the pointer router (an
                    // added/removed-class event dispatches with no hit path,
                    // router only).
                    let result = match event.position().or_else(|| {
                        crate::events::pointer_info(event)
                            .and_then(|info| self.mouse_tracker.source_position_on(surface, info))
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
                self.handle_pan_zoom(surface, event, gesture, hit_test_fn, incoming);
            }
            PointerEvent::Scroll(scroll) => {
                self.handle_scroll(surface, event, scroll, hit_test_fn);
            }
            PointerEvent::ButtonChange(_) => {
                let mut first_panic = None;
                if let Some((sequence, admitted, token, resampler)) =
                    self.contacts.route_of(pointer_id)
                {
                    // Take the older accepted packet before any callback. A
                    // callback may enqueue its successor or replace this contact;
                    // neither belongs to the button edge admitted here.
                    let pending = self.moves.remove((admitted, pointer_id));
                    if let Some(PendingMove::Contact {
                        event: movement,
                        sequence: queued,
                    }) = pending.as_ref().and_then(|state| state.pending.as_ref())
                        && *queued == sequence
                        && self.contacts.is_current(pointer_id, sequence)
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
                            if self.contacts.is_current(pointer_id, sequence) {
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
                    if self.contacts.is_current(pointer_id, sequence) {
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
            PointerEvent::Revise(_) => {
                // Final sensor values for a reading already delivered: the
                // contact's listeners redraw with them; recognizers already
                // decided on the estimate, so the router never sees it.
                let mut first_panic = None;
                if let Some((sequence, admitted, token, _)) = self.contacts.route_of(pointer_id) {
                    // The revised reading may still wait in the coalescer:
                    // deliver it before its correction.
                    let pending = self.moves.remove((admitted, pointer_id));
                    if let Some(PendingMove::Contact {
                        event: movement,
                        sequence: queued,
                    }) = pending.as_ref().and_then(|state| state.pending.as_ref())
                        && *queued == sequence
                        && self.contacts.is_current(pointer_id, sequence)
                    {
                        let delivered = self.dispatch_event(movement, token);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "revised pending Move dispatch",
                        );
                    }
                    if self.contacts.is_current(pointer_id, sequence) {
                        let delivered = self.dispatch_listeners_only(event, token);
                        RoutePanic::preserve_first(
                            &mut first_panic,
                            delivered,
                            "revision dispatch",
                        );
                    }
                }
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::PenSqueeze(_) => {
                // A pen action, not a contact: delivered where the pen hovers
                // (or to global observers alone), never opening a sequence.
                let result = event
                    .position()
                    .map_or_else(HitTestResult::new, hit_test_fn);
                if let Some(panic) = self.dispatch_ephemeral(event, &result) {
                    panic.resume();
                }
            }
            PointerEvent::ScrollInertiaCancel(_) => {
                let panic = if self.contacts.contains(pointer_id) {
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

    fn handle_scroll<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        scroll: &ScrollEvent,
        hit_test_fn: F,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        // Commit source admission (or terminal withdrawal) before
        // hit testing and observers, which may reenter this binding.
        let sequence = self.scrolls.prepare(
            surface,
            scroll,
            crate::events::interaction_now(self.clock.as_ref()),
            &self.sequences,
        );
        if let ScrollAdmission::Active(latch) | ScrollAdmission::Terminal(Some(latch)) = &sequence {
            self.dispatched_sequence.set(Some(latch.id));
        }
        let position = event
            .position()
            .expect("BUG: Scroll carries a checked position");
        // Raw observers always follow the current focal point. Only
        // the consumptive channel stays with the first scroll claimant.
        let fresh_result = hit_test_fn(position);
        let mut first_panic = self.dispatch_ephemeral(event, &fresh_result);
        let claim = RoutePanic::capture(|| {
            let claimed = match &sequence {
                ScrollAdmission::Terminal(ended) => match ended.as_ref().and_then(|s| s.route.get()) {
                    Some(route) => route.dispatch(scroll, || {}),
                    // A final delta may be the first meaningful packet.
                    // It is deliverable, but cannot publish a lasting lease.
                    None => fresh_result.dispatch_scroll(scroll),
                },
                ScrollAdmission::Refused => false,
                ScrollAdmission::Active(sequence) => {
                    if let Some(route) = sequence.route.get() {
                        // A gone target stays inert until the sequence ends;
                        // it never hands this gesture to a new hit path.
                        route.dispatch(scroll, || {})
                    } else if self.scrolls.is_current(surface, scroll.pointer, sequence) {
                        fresh_result.dispatch_scroll_with_claim(scroll, |route| {
                            if self.scrolls.is_current(surface, scroll.pointer, sequence) {
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

    fn handle_pan_zoom<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        gesture: &PanZoomEvent,
        hit_test_fn: F,
        incoming: &mut Option<RoutePanic>,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        let (admission, replaced) = self.pan_zooms.prepare(surface, gesture, &self.sequences);
        if let PanZoomAdmission::Active(session) | PanZoomAdmission::Terminal(Some(session)) =
            &admission
        {
            self.dispatched_sequence.set(Some(session.id));
        }
        pan_zoom_sequences::retire_tickets_capturing_panic(replaced, incoming, false);
        let mut first_panic = incoming.take();
        let position = event
            .position()
            .expect("BUG: PanZoom carries a checked position");
        // Raw observation follows fresh geometry. The session's consumer is
        // chosen along the path hit at its first packet's focal point (its
        // Start) and kept like a captured pointer: the focal point moves with
        // a pinch's translation (ADR-0217 §4) and must not carry the session
        // to whatever lies under it. Accepted consumption stays with its
        // exact owner across rebuilds.
        let mut fresh = HitTestResult::new();
        RoutePanic::preserve_first(
            &mut first_panic,
            RoutePanic::capture(|| fresh = hit_test_fn(position)),
            "native fresh observation geometry",
        );
        RoutePanic::preserve_first(
            &mut first_panic,
            self.dispatch_ephemeral(event, &fresh),
            "native observers after admission retirement",
        );
        let path = match &admission {
            PanZoomAdmission::Active(sequence) => sequence.path_or_hit_test(|| fresh.clone()),
            PanZoomAdmission::Terminal(Some(sequence)) => {
                sequence.path().unwrap_or_else(|| fresh.clone())
            }
            PanZoomAdmission::Terminal(None) | PanZoomAdmission::Refused => fresh,
        };
        // Borrowed admission callbacks need the failure policy, while
        // this delivery keeps custody of the authoritative payload.
        let preserving_failure = Cell::new(first_panic.is_some());
        let pointer = *gesture.pointer();
        let claim = RoutePanic::capture(|| {
            let claimed = match &admission {
                PanZoomAdmission::Terminal(Some(sequence)) => {
                    let mut handled = false;
                    let mut first = first_panic.take();
                    let delivery = RoutePanic::capture(|| {
                        handled = sequence.route.get().map_or_else(
                            || {
                                path.dispatch_pan_zoom_admitted(
                                    gesture,
                                    &|_| {},
                                    &|route, ticket| {
                                        pan_zoom_sequences::retire_tickets(
                                            vec![(route, ticket)],
                                            &preserving_failure,
                                        );
                                    },
                                    Some(&|route| sequence.terminal_is_current(route)),
                                )
                            },
                            |route| {
                                let current = || sequence.terminal_is_current(route);
                                route.dispatch_admitted(
                                    gesture,
                                    || {},
                                    Some(PanZoomAdmissionAuthority {
                                        stage: &|ticket| {
                                            pan_zoom_sequences::retire_tickets(
                                                vec![(route, ticket)],
                                                &preserving_failure,
                                            );
                                        },
                                        claim: &|| {},
                                        terminal: Some(&current),
                                    }),
                                )
                            },
                        );
                    });
                    RoutePanic::preserve_first(&mut first, delivery, "native terminal delivery");
                    pan_zoom_sequences::retire_sequences(vec![Rc::clone(sequence)], &mut first);
                    if let Some(first) = first {
                        first.resume();
                    }
                    handled
                }
                PanZoomAdmission::Terminal(None) => path.dispatch_pan_zoom_admitted(
                    gesture,
                    &|_| {},
                    &|route, ticket| {
                        pan_zoom_sequences::retire_tickets(
                            vec![(route, ticket)],
                            &preserving_failure,
                        );
                    },
                    Some(&|_| false),
                ),
                PanZoomAdmission::Refused => false,
                PanZoomAdmission::Active(sequence) => {
                    if let Some(route) = sequence.route.get()
                        && self.pan_zooms.is_current(surface, pointer, sequence)
                    {
                        let stage = |ticket| {
                            self.pan_zooms.stage(
                                surface,
                                pointer,
                                sequence,
                                (route, ticket),
                                &preserving_failure,
                            );
                        };
                        let claim = || {
                            self.pan_zooms.claim(
                                surface,
                                pointer,
                                sequence,
                                route,
                                &preserving_failure,
                            );
                        };
                        route.dispatch_admitted(
                            gesture,
                            || {},
                            Some(PanZoomAdmissionAuthority {
                                stage: &stage,
                                claim: &claim,
                                terminal: None,
                            }),
                        )
                    } else if self.pan_zooms.is_current(surface, pointer, sequence) {
                        path.dispatch_pan_zoom_admitted(
                            gesture,
                            &|route| {
                                self.pan_zooms.claim(
                                    surface,
                                    pointer,
                                    sequence,
                                    route,
                                    &preserving_failure,
                                );
                            },
                            &|route, ticket| {
                                self.pan_zooms.stage(
                                    surface,
                                    pointer,
                                    sequence,
                                    (route, ticket),
                                    &preserving_failure,
                                );
                            },
                            None,
                        )
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

    fn detach_pointer_sequence(&self, pointer_id: PointerId) -> DetachedPointerSequence {
        let cached = self.contacts.detach(pointer_id);
        let pending_moves = self.moves.remove_pointer(pointer_id);
        DetachedPointerSequence {
            cached,
            pending_moves,
            arena: self.arena.detach(pointer_id),
        }
    }

    /// Settle release debt without running any callback under a state borrow.
    /// A detached frame payload is already committed to its observer round;
    /// defer its loss until that in-flight marker retires. A native terminal
    /// or replacement Down ends that generation under the existing stale-work
    /// policy and may settle it before admitting the newer transaction.
    fn drain_capture_losses(&self, ending: Option<PointerId>) -> Option<RoutePanic> {
        let closing = self.close.is_closed() || self.contacts.is_retiring_all();
        let mut losses: SmallVec<[_; 4]> = self
            .contacts
            .release_requests()
            .into_iter()
            .filter(|request| {
                let contact = &request.contact;
                closing
                    || ending == Some(contact.pointer.id)
                    || (!request.delivering
                        && !self
                            .moves
                            .has_in_flight((contact.surface, contact.pointer.id)))
            })
            .map(|request| request.contact)
            .collect();
        losses.sort_unstable_by_key(|contact| contact.pointer.id);
        let mut first = None;
        for contact in losses {
            let pointer = contact.pointer;
            if !self.contacts.is_current(pointer.id, contact.sequence) {
                continue;
            }
            let Some(guard) = self.contacts.try_retire(pointer.id) else {
                continue;
            };
            self.contacts.refuse_released(pointer);
            let detached = self.detach_pointer_sequence(pointer.id);
            let cancel = PointerEvent::Cancel(PointerCancel::new(
                pointer,
                contact.time,
                CancelReason::CaptureLost,
            ));
            let delivered = self.finish_terminal_sequence(&cancel, detached);
            RoutePanic::preserve_first(&mut first, delivered, "released pointer capture");
            drop(guard);
        }
        first
    }

    fn queue_pending_move(&self, key: MoveKey, mut pending: PendingMove) {
        let pointer_id = key.1;
        let generation = self.moves.allocate_generation();
        let mut previous = self.moves.remove(key);
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
                .moves
                .insert(key, PendingMoveState::queued(generation, pending));
            let mut first_panic = match RoutePanic::try_run(|| match prior.pending.as_ref() {
                Some(PendingMove::Contact { event, sequence })
                    if self.contacts.is_current(pointer_id, *sequence) =>
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
            .moves
            .insert(key, PendingMoveState::queued(generation, pending));
        // Commit the accepted replacement before retiring outgoing ownership,
        // and never retire a route or payload while holding a map guard.
        drop(previous);
        drop(replaced);
    }

    fn abandon_detached_sequence(detached: DetachedPointerSequence) -> Option<RoutePanic> {
        let DetachedPointerSequence {
            cached,
            pending_moves,
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
        let dropped_move = RoutePanic::capture(|| drop(pending_moves));
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
            pending_moves,
            arena,
        } = detached;
        if let Some(cached) = cached.as_ref() {
            // The detached contact no longer owns a prepared delivery prefix.
            // Keep queued source samples for the terminal flush, but cancel
            // output already claimed by an outer, reentrant frame/input drive.
            cached.resampler.cancel_prepared_events();
        }
        let mut first_panic = None;
        if let Some(cached) = cached.as_ref()
            && let Some(event) = pending_moves.iter().find_map(|state| match &state.pending {
                Some(PendingMove::Contact { event, sequence }) if *sequence == cached.sequence => {
                    Some(event)
                }
                _ => None,
            })
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

        let lifecycle = RoutePanic::capture(|| self.arena.settle(terminal, arena));
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
        let dropped_move = RoutePanic::capture(|| drop(pending_moves));
        RoutePanic::preserve_first(&mut first_panic, dropped_move, "terminal pending move drop");
        first_panic
    }

    /// Resolve a hit path into an owner-local route before the arena closes.
    ///
    /// Returns `None` when the path carries no pointer targets, or when the
    /// typed lane boundary rejects the resolution (no active lane, wrong
    /// UI runtime) — the failure is traced, never a panic, and the pointer router
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
        self.scrolls.drain();
        let native_sequences = self.pan_zooms.drain();
        let mut first_panic = self.drain_capture_losses(None);
        let cached_routes = self.contacts.drain();
        for (_, cached) in &cached_routes {
            cached.capture.end();
        }
        let pending_moves = self.moves.drain();
        pan_zoom_sequences::retire_sequences(native_sequences, &mut first_panic);

        // Every map above is empty before any arena callback or destructor
        // runs. Arena abandonment likewise removes all exact slots before
        // notifying members.
        let abandoned = RoutePanic::capture(|| self.arena.arena().abandon_all());
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
        let capture = crate::PointerEventExt::pointer_id(event)
            .and_then(|pointer| self.contacts.capture_for(pointer, token));
        self.dispatch_event_with_capture(event, token, capture.as_ref())
    }

    fn dispatch_event_with_capture(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
        capture: Option<&Rc<ContactCapture>>,
    ) -> Option<RoutePanic> {
        // A close requested from a callback withdraws the binding's routes for
        // the rest of the dispatch in flight; only the cancellations its own
        // drain owes still reach them.
        let mut first_panic = match token {
            Some(token) if !self.close.withholds_delivery() => match active_dispatch_handle()
                .and_then(|handle| handle.invoke_pointer_route_with_capture(token, event, capture))
            {
                Ok(panic) => panic,
                Err(error) => {
                    tracing::error!(?error, "cached pointer route invocation failed");
                    None
                }
            },
            _ => None,
        };

        let router_panic = self.pointer_router.route_capturing_panics(event);
        RoutePanic::preserve_first(&mut first_panic, router_panic, "pointer router");
        first_panic
    }

    /// Deliver `event` to the hit targets of the cached route `token` and not
    /// to the pointer router, whose members are gesture recognizers.
    fn dispatch_listeners_only(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
    ) -> Option<RoutePanic> {
        let token = token.filter(|_| !self.close.withholds_delivery())?;
        match active_dispatch_handle()
            .and_then(|handle| handle.invoke_pointer_route_with_capture(token, event, None))
        {
            Ok(panic) => panic,
            Err(error) => {
                tracing::error!(?error, "cached pointer route invocation failed");
                None
            }
        }
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
        let token = self.contacts.token(pointer_id)?;
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
            .field("active_pointers", &self.contacts.len())
            .field("pending_moves", &self.moves.len())
            .field("arena_count", &self.arena.arena().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
