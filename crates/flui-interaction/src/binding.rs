//! Gesture Binding - owner-runtime coordinator for pointer event handling
//!
//! GestureBinding is the main entry point for handling pointer events in the
//! gesture system. It coordinates hit testing, event routing, arena management,
//! and pointer move event coalescing.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `GestureBinding` mixin:
//!
//! ```dart
//! mixin GestureBinding on BindingBase implements HitTestable, HitTestDispatcher, HitTestTarget {
//!   @override
//!   void initInstances() {
//!     super.initInstances();
//!     _instance = this;
//!     // ...
//!   }
//!
//!   static GestureBinding get instance => BindingBase.checkInstance(_instance);
//!   static GestureBinding? _instance;
//! }
//! ```
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
//! │  │ Hit Test Cache│  │  (DashMap<PointerId, HitTestResult>)
//! │  └───────────────┘  │
//! │  ┌───────────────┐  │
//! │  │ Pending Moves │  │  (DashMap<PointerId, PointerEvent> - coalescing)
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
//! 4. **Pointer Up**: Use cached hit test → dispatch → sweep arena → clear cache
//! 5. **Pointer Cancel**: Use cached hit test → dispatch recognizer rejection →
//!    clear cache without a binding sweep
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::GestureBinding;
//! use flui_interaction::events::PointerEvent;
//!
//! // Create or use the owner runtime's binding.
//! let binding = GestureBinding::new();
//!
//! // Handle platform events
//! fn handle_event(event: &PointerEvent) {
//!     binding.handle_pointer_event(event, |hit_test_position| {
//!         // Perform hit testing on your render tree
//!         my_render_tree.hit_test(hit_test_position)
//!     });
//! }
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
};

use dashmap::DashMap;
use flui_foundation::MonotonicClock;
use flui_types::geometry::{Offset, Pixels};
use smallvec::SmallVec;
use ui_events::pointer::{PointerEvent, PointerType};

use crate::{
    arena::{DetachedArenaBatch, GestureArena},
    ids::PointerId,
    processing::{PointerEventResampler, SamplingClock},
    routing::{
        HitTestResult, MouseTracker, PointerMotionKind, PointerRouter, ResolvedRouteToken,
        RoutePanic, active_dispatch_handle,
    },
    settings::GestureSettings,
};

/// Per-pointer state cached at Down: the data-only hit path plus the
/// owner-local resolved route token that Move reuses and Up/Cancel releases.
///
/// `token` is `None` when no interaction lane was active at Down (a
/// gesture-only binding without a mounted tree) or when the path carried no
/// pointer targets; the pointer router still routes such events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerSequence(u64);

#[derive(Clone)]
struct CachedPointerRoute {
    result: HitTestResult,
    token: Option<ResolvedRouteToken>,
    sequence: PointerSequence,
    resampler: PointerEventResampler,
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

/// Truncate a `f64` to `f32` for pointer position conversion.
///
/// Lossless for any screen-pixel coordinate: a `f32` mantissa rounds
/// at ~7 decimal digits and physical pointer positions are reported
/// in device pixels (≤ 2^23 ≈ 8M), so `f64 → f32` is exact in that
/// range. Used at the W3C→flui boundary where upstream carries `f64`
/// physical pixels and our `Offset<Pixels>` stores `f32`.
///
/// Upper bound on simultaneously-tracked pointers.
///
/// Per-pointer state (hit tests, resamplers, arena entries, recogniser maps)
/// grows with active pointers; an untrusted event source could open unbounded
/// pointers to exhaust memory. Real hardware tops out around 10–16 touches, so
/// this generous cap never rejects a legitimate gesture.
const MAX_SIMULTANEOUS_POINTERS: usize = 32;

/// Local mirror of the helper in `events.rs` / `pan_zoom.rs` —
/// duplicated here to keep the binding module's hot path free of
/// cross-module indirection.
#[inline]
const fn px_f32(v: f64) -> Pixels {
    // f64 → f32 is intentionally lossy at extreme values; for
    // pointer coordinates the dynamic range fits in `f32` exactly.
    Pixels(v as f32)
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
/// GestureBinding coalesces these by storing only the latest move event
/// per pointer. Call `flush_pending_moves()` once per frame to process
/// the coalesced events.
///
/// # Thread affinity
///
/// `GestureBinding` is owner-local. Its pointer router and executable gesture
/// callbacks are not `Send + Sync`; render hit-test entries and route tokens
/// remain on the separate data plane.
pub struct GestureBinding {
    /// Cached hit paths and resolved routes per pointer.
    /// Down resolves once; move/up events reuse the cached route.
    hit_tests: DashMap<PointerId, CachedPointerRoute>,

    /// Pending move events for coalescing.
    /// Only the latest move per pointer is kept.
    pending_moves: DashMap<PointerId, PendingMoveState>,

    /// Sampling mode captured by each pointer sequence at Down.
    ///
    /// Mode changes are accepted only while there are no active contacts.
    resampling_enabled: Cell<bool>,

    /// Frame-paced clock that produces `(now, next)` pairs for the
    /// resamplers. Only consulted when `resampling_enabled` is true.
    sampling_clock: Cell<SamplingClock>,

    /// Routes pointer events to registered handlers.
    pointer_router: PointerRouter,

    /// Per-device enter/exit/hover/cursor state for this presentation.
    mouse_tracker: MouseTracker,

    /// Resolves conflicts between competing gesture recognizers.
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
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            resampling_enabled: Cell::new(false),
            sampling_clock: Cell::new(SamplingClock::default()),
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
        let active_pointer_count = self.hit_tests.len();
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
            .iter()
            .filter(|cached| cached.resampler.is_tracked())
            .count()
    }

    // ========================================================================
    // Component Accessors
    // ========================================================================

    /// Get the pointer router.
    #[inline]
    pub fn pointer_router(&self) -> &PointerRouter {
        &self.pointer_router
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

    /// Get settings for a specific device type.
    pub fn settings_for_device(&self, device_type: PointerType) -> GestureSettings {
        GestureSettings::for_device(device_type)
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
    /// # Example
    ///
    /// ```rust,ignore
    /// binding.handle_pointer_event(&event, |position| {
    ///     render_tree.hit_test(position)
    /// });
    /// ```
    pub fn handle_pointer_event<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<Pixels>) -> HitTestResult,
    {
        self.handle_pointer_event_kernel(event, hit_test_fn);
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
    /// ```rust,ignore
    /// // In your frame loop:
    /// fn on_frame(&mut self) {
    ///     // Process coalesced move events
    ///     self.binding.flush_pending_moves();
    ///
    ///     // Then do layout, paint, etc.
    /// }
    /// ```
    pub fn flush_pending_moves(&self) -> usize {
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
        sample_time: std::time::Instant,
        next_sample_time: std::time::Instant,
    ) -> Result<usize, InvalidSamplingWindow> {
        if next_sample_time <= sample_time {
            return Err(InvalidSamplingWindow);
        }
        Ok(self.flush_pending_moves_kernel(Some((sample_time, next_sample_time))))
    }

    fn flush_pending_moves_kernel(
        &self,
        sample_window: Option<(std::time::Instant, std::time::Instant)>,
    ) -> usize {
        if self.tearing_down_all_pointers.get() || !self.tearing_down_pointers.borrow().is_empty() {
            return 0;
        }

        // Freeze the complete direct/coalesced frame batch before any user
        // callback runs. Re-entrant moves replace their exact marker and
        // therefore always belong to the next frame.
        let pointers: SmallVec<[PointerId; 4]> = self
            .pending_moves
            .iter()
            .filter_map(|entry| entry.is_queued().then_some(*entry.key()))
            .collect();
        let mut drained: SmallVec<[(PointerId, PendingMoveGeneration, PendingMove); 4]> =
            SmallVec::with_capacity(pointers.len());
        for pointer_id in pointers {
            let Some(mut entry) = self.pending_moves.get_mut(&pointer_id) else {
                continue;
            };
            let generation = entry.generation();
            let pending = entry.pending.take();
            drop(entry);
            if let Some(pending) = pending {
                drained.push((pointer_id, generation, pending));
            }
        }

        let mut count = 0;
        let mut first_panic = None;

        // A resampled contact has exactly one queue: the resampler owned by
        // its cached Down route. Snapshot route capabilities before callbacks
        // so no DashMap guard crosses executable user code.
        if self.is_resampling_enabled()
            && let Some((sample_time, next_sample_time)) = sample_window
        {
            let samples: SmallVec<[ResamplerSnapshot; 4]> = self
                .hit_tests
                .iter()
                .filter(|cached| cached.resampler.is_tracked())
                .map(|cached| ResamplerSnapshot {
                    pointer_id: *cached.key(),
                    sequence: cached.sequence,
                    token: cached.token,
                    resampler: cached.resampler.clone(),
                })
                .collect();

            for snapshot in samples {
                let ResamplerSnapshot {
                    pointer_id,
                    sequence,
                    token,
                    resampler,
                } = snapshot;
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
                    let delivered = self.dispatch_ephemeral(&event, &hit_test);
                    RoutePanic::preserve_first(&mut first_panic, delivered, "coalesced hover move");
                    count += 1;
                }
            }
            self.remove_pending_move_if_in_flight(pointer_id, generation);
        }

        if let Some(panic) = first_panic {
            panic.resume();
        }
        count
    }

    /// Check whether direct/coalesced or resampled motion still needs a frame.
    #[inline]
    pub fn has_pending_motion(&self) -> bool {
        self.pending_moves.iter().any(|state| state.is_queued())
            || self
                .hit_tests
                .iter()
                .any(|route| route.resampler.has_pending_events())
    }

    /// Get the number of queued motion events across both canonical paths.
    #[inline]
    pub fn pending_move_count(&self) -> usize {
        let direct = self
            .pending_moves
            .iter()
            .filter(|state| state.is_queued())
            .count();
        let resampled = self
            .hit_tests
            .iter()
            .map(|route| route.resampler.pending_event_count())
            .sum::<usize>();
        direct + resampled
    }

    // ========================================================================
    // Pointer Sequence State
    // ========================================================================

    /// Get the cached hit test result for a pointer.
    pub fn get_hit_test(&self, pointer_id: PointerId) -> Option<HitTestResult> {
        self.hit_tests
            .get(&pointer_id)
            .map(|cached| cached.result.clone())
    }

    /// Check if there's a cached hit test for a pointer.
    #[inline]
    pub fn has_hit_test(&self, pointer_id: PointerId) -> bool {
        self.hit_tests.contains_key(&pointer_id)
    }

    /// Cancel every binding-owned state slot for a pointer.
    ///
    /// This is the abnormal-sequence counterpart to receiving Up/Cancel: it
    /// releases the retained hit route, discards queued/resampled movement,
    /// and rejects the unresolved arena without selecting a winner.
    pub fn cancel_pointer_sequence(&self, pointer_id: PointerId) {
        let Some(guard) = PointerTeardownGuard::try_enter(&self.tearing_down_pointers, pointer_id)
        else {
            return;
        };
        let detached = self.detach_pointer_sequence(pointer_id);
        let panic = Self::abandon_detached_sequence(detached);
        drop(guard);
        if let Some(panic) = panic {
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
        let hit_tests = self.hit_tests.len();
        let resamplers = self.active_resampler_count();
        let pending_moves = self.pending_moves.len();
        let arenas = self.arena.len();
        if hit_tests > 0 || resamplers > 0 || pending_moves > 0 || arenas > 0 {
            tracing::debug!(
                hit_tests,
                resamplers,
                pending_moves,
                arenas,
                "GestureBinding draining interrupted pointer state on lifecycle pause"
            );
        }

        self.cancel_all_pointer_sequences();
    }

    /// Get the number of active pointers (with cached hit tests).
    #[inline]
    pub fn active_pointer_count(&self) -> usize {
        self.hit_tests.len()
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
        self.arena.poll_deadlines();
    }

    // ========================================================================
    // Internal Methods
    // ========================================================================

    fn handle_pointer_event_kernel<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<Pixels>) -> HitTestResult,
    {
        let pointer_id = Self::extract_pointer_id(event);
        if self.tearing_down_all_pointers.get()
            || self.tearing_down_pointers.borrow().contains(&pointer_id)
        {
            tracing::debug!(
                ?pointer_id,
                "ignoring re-entrant input during pointer teardown"
            );
            return;
        }

        match event {
            PointerEvent::Down(down) => {
                let mut first_panic = None;
                let has_superseded_sequence = self.hit_tests.contains_key(&pointer_id)
                    || self.pending_moves.contains_key(&pointer_id)
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

                if self.hit_tests.len() >= MAX_SIMULTANEOUS_POINTERS {
                    tracing::warn!(
                        ?pointer_id,
                        active = self.hit_tests.len(),
                        "dropping Down: simultaneous-pointer cap reached"
                    );
                    if let Some(panic) = first_panic {
                        panic.resume();
                    }
                    return;
                }

                let position =
                    Offset::new(px_f32(down.state.position.x), px_f32(down.state.position.y));
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
                let sequence = self.allocate_pointer_sequence();
                let resampler = PointerEventResampler::new(pointer_id);
                if self.is_resampling_enabled() {
                    resampler.start_tracking();
                }
                self.hit_tests.insert(
                    pointer_id,
                    CachedPointerRoute {
                        result,
                        token,
                        sequence,
                        resampler,
                    },
                );

                let delivered = self.dispatch_event(event, token);
                RoutePanic::preserve_first(&mut first_panic, delivered, "pointer Down dispatch");
                let lifecycle = RoutePanic::capture(|| self.arena.close(pointer_id));
                RoutePanic::preserve_first(&mut first_panic, lifecycle, "pointer Down arena close");
                if let Some(panic) = first_panic {
                    panic.resume();
                }
            }
            PointerEvent::Move(pointer_move) => {
                let position = Offset::new(
                    px_f32(pointer_move.current.position.x),
                    px_f32(pointer_move.current.position.y),
                );
                if let Some((sequence, resampler)) = self
                    .hit_tests
                    .get(&pointer_id)
                    .map(|cached| (cached.sequence, cached.resampler.clone()))
                {
                    if self.is_resampling_enabled() {
                        resampler.add_event(event.clone());
                    } else {
                        self.queue_pending_move(
                            pointer_id,
                            PendingMove::Contact {
                                event: event.clone(),
                                sequence,
                            },
                        );
                    }

                    if matches!(
                        pointer_move.pointer.pointer_type,
                        PointerType::Mouse | PointerType::Pen
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
                if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                    panic.resume();
                }
            }
            PointerEvent::Gesture(gesture) => {
                if self.hit_tests.contains_key(&pointer_id) {
                    if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                        panic.resume();
                    }
                } else {
                    // `ui_events::PointerEvent::Gesture` is a complete
                    // high-level gesture tick, not Flutter's explicit
                    // PanZoomStart/Update/End stream. Without an active
                    // contact route it therefore resolves one fresh,
                    // ephemeral path at its own focal position.
                    let position = Offset::new(
                        px_f32(gesture.state.position.x),
                        px_f32(gesture.state.position.y),
                    );
                    let result = hit_test_fn(position);
                    if let Some(panic) = self.dispatch_ephemeral(event, &result) {
                        panic.resume();
                    }
                }
            }
            PointerEvent::Scroll(scroll) => {
                if self.hit_tests.contains_key(&pointer_id) {
                    if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                        panic.resume();
                    }
                } else {
                    let position = Offset::new(
                        px_f32(scroll.state.position.x),
                        px_f32(scroll.state.position.y),
                    );
                    let result = hit_test_fn(position);
                    if let Some(panic) = self.dispatch_ephemeral(event, &result) {
                        panic.resume();
                    }
                }
            }
        }
    }

    fn detach_pointer_sequence(&self, pointer_id: PointerId) -> DetachedPointerSequence {
        DetachedPointerSequence {
            cached: self.hit_tests.remove(&pointer_id).map(|(_, cached)| cached),
            pending_move: self
                .pending_moves
                .remove(&pointer_id)
                .map(|(_, event)| event),
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

    fn allocate_pending_move_generation(&self) -> PendingMoveGeneration {
        let generation = self
            .next_pending_move_generation
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic!("BUG: pending move generation exhausted"));
        self.next_pending_move_generation.set(generation);
        PendingMoveGeneration(generation)
    }

    fn queue_pending_move(&self, pointer_id: PointerId, pending: PendingMove) {
        let generation = self.allocate_pending_move_generation();
        self.pending_moves
            .insert(pointer_id, PendingMoveState::queued(generation, pending));
    }

    fn is_pending_move_in_flight(
        &self,
        pointer_id: PointerId,
        generation: PendingMoveGeneration,
    ) -> bool {
        self.pending_moves
            .get(&pointer_id)
            .is_some_and(|state| state.is_in_flight(generation))
    }

    fn remove_pending_move_if_in_flight(
        &self,
        pointer_id: PointerId,
        generation: PendingMoveGeneration,
    ) {
        use dashmap::mapref::entry::Entry;

        if let Entry::Occupied(entry) = self.pending_moves.entry(pointer_id)
            && entry.get().is_in_flight(generation)
        {
            entry.remove();
        }
    }

    fn is_current_sequence(&self, pointer_id: PointerId, sequence: PointerSequence) -> bool {
        self.hit_tests
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
            let delivered = self.dispatch_event(event, cached.token);
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
                let delivered = self.dispatch_event(&event, cached.token);
                RoutePanic::preserve_first(
                    &mut first_panic,
                    delivered,
                    "terminal resampled Move dispatch",
                );
            });
        }

        let delivered =
            self.dispatch_event(terminal, cached.as_ref().and_then(|route| route.token));
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
    fn extract_pointer_id(event: &PointerEvent) -> PointerId {
        crate::events::extract_pointer_id(event)
    }

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
        let mut hit_pointers: Vec<PointerId> =
            self.hit_tests.iter().map(|entry| *entry.key()).collect();
        hit_pointers.sort_unstable();
        let cached_routes: Vec<CachedPointerRoute> = hit_pointers
            .into_iter()
            .filter_map(|pointer| self.hit_tests.remove(&pointer).map(|(_, cached)| cached))
            .collect();

        let mut move_pointers: Vec<PointerId> = self
            .pending_moves
            .iter()
            .map(|entry| *entry.key())
            .collect();
        move_pointers.sort_unstable();
        let pending_moves: Vec<PendingMoveState> = move_pointers
            .into_iter()
            .filter_map(|pointer| self.pending_moves.remove(&pointer).map(|(_, event)| event))
            .collect();

        // Every map above is empty before any arena callback or destructor
        // runs. Arena abandonment likewise removes all exact slots before
        // notifying members.
        let mut first_panic = RoutePanic::capture(|| self.arena.abandon_all());

        for cached in cached_routes {
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
        for event in pending_moves {
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
    /// Flutter's binding is the final/root hit-test entry, so leaf hit targets
    /// run before its `PointerRouter` and arena lifecycle. Returns the first
    /// panic in that transaction order so the caller can finish later phases
    /// and mandatory cleanup before resuming it.
    fn dispatch_event(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
    ) -> Option<RoutePanic> {
        let mut first_panic = if let Some(token) = token {
            match active_dispatch_handle()
                .and_then(|handle| handle.invoke_pointer_route(token, event))
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
    /// The cached token is copied out before dispatch so no `DashMap` shard
    /// guard is held while handlers re-enter the binding.
    fn dispatch_on_cached_route(
        &self,
        pointer_id: PointerId,
        event: &PointerEvent,
    ) -> Option<RoutePanic> {
        let token = self.hit_tests.get(&pointer_id)?.token;
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
}

impl std::fmt::Debug for GestureBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureBinding")
            .field("active_pointers", &self.hit_tests.len())
            .field("pending_moves", &self.pending_moves.len())
            .field("arena_count", &self.arena.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::arena::SweepModel;
    use crate::events::{
        make_down_event, make_down_event_for_id, make_move_event, make_move_event_for_id,
        make_up_event, make_up_event_for_id,
    };

    #[derive(Debug, Default)]
    struct CountingArenaMember {
        accepts: AtomicUsize,
        rejects: AtomicUsize,
    }

    impl crate::sealed::CustomGestureRecognizer for CountingArenaMember {
        fn on_arena_accept(&self, _pointer: PointerId) {
            self.accepts.fetch_add(1, Ordering::Relaxed);
        }

        fn on_arena_reject(&self, _pointer: PointerId) {
            self.rejects.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[derive(Debug)]
    struct LoggingArenaMember {
        log: Arc<parking_lot::Mutex<Vec<&'static str>>>,
    }

    impl crate::sealed::CustomGestureRecognizer for LoggingArenaMember {
        fn on_arena_accept(&self, _pointer: PointerId) {
            self.log.lock().push("arena");
        }

        fn on_arena_reject(&self, _pointer: PointerId) {}
    }

    #[derive(Debug, Default)]
    struct PanickingAcceptArenaMember;

    impl crate::sealed::CustomGestureRecognizer for PanickingAcceptArenaMember {
        fn on_arena_accept(&self, _pointer: PointerId) {
            panic!("arena accept panic");
        }

        fn on_arena_reject(&self, _pointer: PointerId) {}
    }

    /// A cached entry with no hit path and no resolved route, for cache tests.
    fn empty_cached_route() -> CachedPointerRoute {
        CachedPointerRoute {
            result: HitTestResult::new(),
            token: None,
            sequence: PointerSequence(1),
            resampler: PointerEventResampler::new(PointerId::PRIMARY),
        }
    }

    fn set_resampling(binding: &GestureBinding, enabled: bool) {
        binding
            .set_resampling_enabled(enabled)
            .expect("tests configure resampling without active pointers");
    }

    #[test]
    fn test_binding_creation() {
        let binding = GestureBinding::new();
        assert_eq!(binding.active_pointer_count(), 0);
    }

    #[test]
    fn test_binding_with_settings() {
        let settings = GestureSettings::mouse_defaults();
        let binding = GestureBinding::with_settings(settings.clone());
        assert_eq!(
            binding.default_settings().touch_slop(),
            settings.touch_slop()
        );
    }

    #[test]
    fn production_bindings_use_binding_driven_arenas() {
        assert_eq!(
            GestureBinding::new().arena().sweep_model(),
            SweepModel::BindingDriven,
            "the production binding must own close-on-down and sweep-on-up"
        );
        assert_eq!(
            GestureBinding::with_settings(GestureSettings::mouse_defaults())
                .arena()
                .sweep_model(),
            SweepModel::BindingDriven,
            "the settings constructor must preserve the same lifecycle model"
        );
    }

    #[test]
    fn cancel_does_not_force_a_winner_in_an_unresolved_binding_arena() {
        let binding = GestureBinding::new();
        let pointer = PointerId::PRIMARY;
        let first = Arc::new(CountingArenaMember::default());
        let second = Arc::new(CountingArenaMember::default());

        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| {
            binding.arena().add(pointer, first.clone());
            binding.arena().add(pointer, second.clone());
            HitTestResult::new()
        });
        assert_eq!(first.accepts.load(Ordering::Relaxed), 0);
        assert_eq!(second.accepts.load(Ordering::Relaxed), 0);

        let cancel = make_cancel_event(PointerType::Touch);
        binding.handle_pointer_event(&cancel, |_| HitTestResult::new());

        assert_eq!(
            first.accepts.load(Ordering::Relaxed),
            0,
            "cancel must not sweep and force the first unresolved member to win"
        );
        assert_eq!(
            second.accepts.load(Ordering::Relaxed),
            0,
            "cancel must not choose any unresolved member"
        );
        assert_eq!(first.rejects.load(Ordering::Relaxed), 1);
        assert_eq!(second.rejects.load(Ordering::Relaxed), 1);
        assert!(binding.arena().is_empty());
    }

    #[test]
    fn test_hit_test_cache() {
        let binding = GestureBinding::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        binding.hit_tests.insert(pointer, empty_cached_route());
        assert!(binding.has_hit_test(pointer));

        let cached = binding.get_hit_test(pointer);
        assert!(cached.is_some());

        binding.cancel_pointer_sequence(pointer);
        assert!(!binding.has_hit_test(pointer));
    }

    #[test]
    fn test_cancel_all_pointer_sequences() {
        let binding = GestureBinding::new();

        binding.hit_tests.insert(
            PointerId::new(2).expect("nonzero pointer id"),
            empty_cached_route(),
        );
        binding.hit_tests.insert(
            PointerId::new(3).expect("nonzero pointer id"),
            empty_cached_route(),
        );
        binding.hit_tests.insert(
            PointerId::new(4).expect("nonzero pointer id"),
            empty_cached_route(),
        );

        assert_eq!(binding.active_pointer_count(), 3);

        binding.cancel_all_pointer_sequences();
        assert_eq!(binding.active_pointer_count(), 0);
    }

    #[test]
    fn test_settings_for_device() {
        let binding = GestureBinding::new();

        let touch_settings = binding.settings_for_device(PointerType::Touch);
        let mouse_settings = binding.settings_for_device(PointerType::Mouse);

        // Touch should have larger slop than mouse
        assert!(touch_settings.touch_slop() > mouse_settings.touch_slop());
    }

    // ========================================================================
    // Resampler wiring tests
    // ========================================================================

    #[test]
    fn resampling_disabled_by_default() {
        let binding = GestureBinding::new();
        assert!(!binding.is_resampling_enabled());
        assert_eq!(binding.active_resampler_count(), 0);
    }

    #[test]
    fn set_resampling_enabled_toggles_flag() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        assert!(binding.is_resampling_enabled());
        set_resampling(&binding, false);
        assert!(!binding.is_resampling_enabled());
    }

    #[test]
    fn resampling_mode_change_rejects_an_active_sequence() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        // Seed a resampler via the Down path so the DashMap is non-empty.
        let down = make_down_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        assert!(binding.active_resampler_count() >= 1);

        let error = binding
            .set_resampling_enabled(false)
            .expect_err("an active pointer fixes its sampling mode");
        assert_eq!(error.active_pointer_count(), 1);
        assert_eq!(binding.active_resampler_count(), 1);
    }

    #[test]
    fn down_creates_per_pointer_resampler() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        // 1 active resampler for the primary pointer.
        assert_eq!(binding.active_resampler_count(), 1);
    }

    #[test]
    fn down_beyond_pointer_cap_is_dropped() {
        let binding = GestureBinding::new();
        // Fill to the cap with distinct synthetic pointers (ids 2..) so the
        // primary id (1) is a genuinely new pointer below.
        for i in 0..MAX_SIMULTANEOUS_POINTERS {
            let id = PointerId::new(i as u64 + 2).expect("nonzero pointer id");
            binding.hit_tests.insert(id, empty_cached_route());
        }
        assert_eq!(binding.active_pointer_count(), MAX_SIMULTANEOUS_POINTERS);

        // A brand-new pointer beyond the cap must be refused, not tracked.
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        assert_eq!(
            binding.active_pointer_count(),
            MAX_SIMULTANEOUS_POINTERS,
            "a Down beyond the simultaneous-pointer cap must not be tracked"
        );
    }

    #[test]
    fn up_removes_per_pointer_resampler() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&up, |_| HitTestResult::new());
        assert_eq!(binding.active_resampler_count(), 0);
    }

    #[test]
    fn move_with_resampling_off_uses_coalescing_path() {
        // Resampling off (default): move events go into pending_moves
        // and are *not* mirrored to the resampler (the resampler
        // already exists but stays empty for this move). On flush
        // the direct dispatch path drains the queue.
        let binding = GestureBinding::new();
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        // Coalesced queue has the move; resampler exists but is
        // untouched on the off path.
        assert!(binding.has_pending_motion());
        assert_eq!(binding.pending_move_count(), 1);
        // The resampler is created on Down but no Move is fed to it
        // when resampling is off.
        assert!(
            !binding
                .hit_tests
                .iter()
                .any(|route| route.resampler.has_pending_events())
        );
    }

    #[test]
    fn hover_move_without_down_is_hit_tested_and_coalesced() {
        use std::{cell::Cell, rc::Rc};

        let binding = GestureBinding::new();
        let hit_tests = Rc::new(Cell::new(0));
        let deliveries = Rc::new(Cell::new(0));
        let delivered = Rc::clone(&deliveries);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            if matches!(event, PointerEvent::Move(_)) {
                delivered.set(delivered.get() + 1);
            }
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        for position in [
            Offset::new(Pixels(10.0), Pixels(20.0)),
            Offset::new(Pixels(30.0), Pixels(40.0)),
        ] {
            let hit_tests_for_move = Rc::clone(&hit_tests);
            binding.handle_pointer_event(
                &make_move_event(position, PointerType::Mouse),
                move |_| {
                    hit_tests_for_move.set(hit_tests_for_move.get() + 1);
                    HitTestResult::new()
                },
            );
        }

        assert_eq!(
            hit_tests.get(),
            2,
            "each untracked mouse move needs a fresh hover hit test"
        );
        assert_eq!(
            binding.pending_move_count(),
            1,
            "only the latest hover move for a pointer is retained per frame"
        );
        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(deliveries.get(), 1);

        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn hover_move_uses_the_latest_ephemeral_hit_route() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let delivered_x = Rc::new(RefCell::new(Vec::new()));
        let route_released = Rc::new(Cell::new(false));

        lane.enter(|| {
            let release_probe = SetOnDrop(Rc::clone(&route_released));
            let delivered_x_for_target = Rc::clone(&delivered_x);
            let target = handle
                .register_pointer(move |event| {
                    let _keep_probe_alive = &release_probe;
                    if let PointerEvent::Move(pointer_move) = event {
                        delivered_x_for_target
                            .borrow_mut()
                            .push(pointer_move.current.position.x);
                    }
                })
                .expect("register hover target");

            for coordinate in [10.0, 30.0] {
                let pointer_move = make_move_event(
                    Offset::new(Pixels(coordinate), Pixels(5.0)),
                    PointerType::Mouse,
                );
                binding.handle_pointer_event(&pointer_move, |_| hit_result(target));
            }

            assert_eq!(binding.flush_pending_moves(), 1);
            assert_eq!(delivered_x.borrow().as_slice(), [30.0]);
            assert!(
                !binding.has_hit_test(PointerId::PRIMARY),
                "hover must not create contact route state"
            );

            handle
                .unregister_pointer(target)
                .expect("unregister hover target");
            assert!(
                route_released.get(),
                "the one-shot hover route must release its handler after dispatch"
            );
        });
    }

    #[test]
    fn reentrant_down_invalidates_a_later_hover_from_the_frozen_batch() {
        let binding = Rc::new(GestureBinding::new());
        let first_pointer = PointerId::new(11).expect("nonzero pointer id");
        let second_pointer = PointerId::new(12).expect("nonzero pointer id");
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let replaced = Rc::new(Cell::new(false));

        let callback_binding = Rc::clone(&binding);
        let callback_delivered = Rc::clone(&delivered);
        let callback_replaced = Rc::clone(&replaced);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            if !matches!(event, PointerEvent::Move(_)) {
                return;
            }
            let pointer = GestureBinding::extract_pointer_id(event);
            callback_delivered.borrow_mut().push(pointer);
            if callback_replaced.replace(true) {
                return;
            }

            let replacement = if pointer == first_pointer {
                second_pointer
            } else {
                first_pointer
            };
            let down = make_down_event_for_id(
                replacement,
                Offset::new(Pixels(50.0), Pixels(50.0)),
                PointerType::Mouse,
            );
            callback_binding.handle_pointer_event(&down, |_| HitTestResult::new());
        });
        binding
            .pointer_router()
            .add_route(first_pointer, Rc::clone(&handler));
        binding
            .pointer_router()
            .add_route(second_pointer, Rc::clone(&handler));

        for (pointer, coordinate) in [(first_pointer, 10.0), (second_pointer, 20.0)] {
            let pointer_move = make_move_event_for_id(
                pointer,
                Offset::new(Pixels(coordinate), Pixels(coordinate)),
                PointerType::Mouse,
            );
            binding.handle_pointer_event(&pointer_move, |_| HitTestResult::new());
        }

        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(
            delivered.borrow().len(),
            1,
            "a hover invalidated by a newer Down must not escape the frozen frame batch"
        );
        assert_eq!(binding.pending_move_count(), 0);
        assert_eq!(binding.active_pointer_count(), 1);

        binding.cancel_all_pointer_sequences();
        binding
            .pointer_router()
            .remove_route(first_pointer, &handler);
        binding
            .pointer_router()
            .remove_route(second_pointer, &handler);
    }

    #[test]
    fn reentrant_hover_replaces_in_flight_move_for_the_next_frame() {
        let binding = Rc::new(GestureBinding::new());
        let delivered_x = Rc::new(RefCell::new(Vec::new()));
        let queued_replacement = Rc::new(Cell::new(false));

        let callback_binding = Rc::clone(&binding);
        let callback_delivered_x = Rc::clone(&delivered_x);
        let callback_queued_replacement = Rc::clone(&queued_replacement);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            let PointerEvent::Move(pointer_move) = event else {
                return;
            };
            callback_delivered_x
                .borrow_mut()
                .push(pointer_move.current.position.x);
            if callback_queued_replacement.replace(true) {
                return;
            }
            let replacement =
                make_move_event(Offset::new(Pixels(20.0), Pixels(20.0)), PointerType::Mouse);
            callback_binding.handle_pointer_event(&replacement, |_| HitTestResult::new());
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        let first = make_move_event(Offset::new(Pixels(10.0), Pixels(10.0)), PointerType::Mouse);
        binding.handle_pointer_event(&first, |_| HitTestResult::new());

        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(delivered_x.borrow().as_slice(), [10.0]);
        assert_eq!(
            binding.pending_move_count(),
            1,
            "the re-entrant hover belongs to the next frame"
        );

        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(delivered_x.borrow().as_slice(), [10.0, 20.0]);
        assert_eq!(binding.pending_move_count(), 0);

        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn contact_move_uses_down_route_and_fresh_mouse_tracking_route() {
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
                .register_pointer(move |event| {
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
            let position = Offset::new(Pixels(10.0), Pixels(10.0));
            binding.handle_pointer_event(&make_down_event(position, PointerType::Mouse), |_| {
                down_result
            });

            let mouse_target_id = RenderId::new(2);
            let mut fresh_result = HitTestResult::new();
            fresh_result.add(
                HitTestEntry::new(mouse_target_id)
                    .mouse_annotation(MouseTrackerAnnotation::new(mouse_target_id, mouse_target)),
            );
            let callback_fresh_hit_tests = Rc::clone(&fresh_hit_tests);
            binding.handle_pointer_event(
                &make_move_event(position, PointerType::Mouse),
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
                    .device_active_regions(0)
                    .contains(&mouse_target_id)
            );

            binding.handle_pointer_event(&make_up_event(position, PointerType::Mouse), |_| {
                HitTestResult::new()
            });
            handle
                .unregister_pointer(pointer_target)
                .expect("release pointer target");
            handle
                .unregister_mouse_region(mouse_target)
                .expect("release mouse target");
        });
    }

    #[test]
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
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        let down = make_down_event(Offset::new(Pixels(1.0), Pixels(2.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let move_event = make_move_event(Offset::new(Pixels(3.0), Pixels(4.0)), PointerType::Touch);
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        assert_eq!(events.borrow().as_slice(), ["down"]);

        let up = make_up_event(Offset::new(Pixels(5.0), Pixels(6.0)), PointerType::Touch);
        binding.handle_pointer_event(&up, |_| HitTestResult::new());

        assert_eq!(events.borrow().as_slice(), ["down", "move", "up"]);
        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn superseding_down_abandons_old_arena_and_pending_move_before_hit_test() {
        use std::cell::Cell;

        let binding = GestureBinding::new();
        let old_member = Arc::new(CountingArenaMember::default());
        let down = make_down_event(Offset::new(Pixels(1.0), Pixels(2.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| {
            binding.arena().add(PointerId::PRIMARY, old_member.clone());
            HitTestResult::new()
        });
        let move_event = make_move_event(Offset::new(Pixels(3.0), Pixels(4.0)), PointerType::Touch);
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        assert_eq!(binding.pending_move_count(), 1);

        let old_sequence_gone_before_hit_test = Cell::new(false);
        binding.handle_pointer_event(&down, |_| {
            old_sequence_gone_before_hit_test.set(
                old_member.rejects.load(Ordering::Relaxed) == 1
                    && binding.pending_move_count() == 0
                    && binding.active_resampler_count() == 0
                    && !binding.arena().has_active(PointerId::PRIMARY),
            );
            HitTestResult::new()
        });

        assert!(old_sequence_gone_before_hit_test.get());
        assert_eq!(old_member.accepts.load(Ordering::Relaxed), 0);
        assert_eq!(binding.pending_move_count(), 0);
    }

    #[test]
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
                    && !binding_from_route.arena().has_active(PointerId::PRIMARY)
                {
                    observations.set(observations.get() + 1);
                }
                let down =
                    make_down_event(Offset::new(Pixels(9.0), Pixels(9.0)), PointerType::Touch);
                let reentrant_calls = Rc::clone(&reentrant_calls);
                binding_from_route.handle_pointer_event(&down, move |_| {
                    reentrant_calls.set(reentrant_calls.get() + 1);
                    HitTestResult::new()
                });
            }
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        let down = make_down_event(Offset::new(Pixels(1.0), Pixels(2.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let move_event = make_move_event(Offset::new(Pixels(3.0), Pixels(4.0)), PointerType::Touch);
        binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        let up = make_up_event(Offset::new(Pixels(5.0), Pixels(6.0)), PointerType::Touch);
        binding.handle_pointer_event(&up, |_| HitTestResult::new());

        assert_eq!(detached_observations.get(), 2);
        assert_eq!(reentrant_hit_tests.get(), 0);
        assert_eq!(binding.active_pointer_count(), 0);
        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn move_with_resampling_on_feeds_resampler() {
        // Resampling on: the sequence-owned resampler is the sole owner of
        // contact moves.
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        assert_eq!(binding.active_resampler_count(), 1);
        assert!(binding.has_pending_motion());
    }

    #[test]
    fn sampling_clock_round_trip() {
        let binding = GestureBinding::new();
        let clock = SamplingClock::Fixed {
            period: Duration::from_millis(8),
        };
        binding.set_sampling_clock(clock);
        let read = binding.sampling_clock();
        assert_eq!(read.period(), Duration::from_millis(8));
    }

    #[test]
    fn lifecycle_pause_clears_resamplers_and_pending_moves() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        assert!(binding.active_resampler_count() >= 1);
        assert!(binding.has_pending_motion());

        binding.handle_lifecycle_pause();

        assert_eq!(binding.active_resampler_count(), 0);
        assert_eq!(binding.pending_move_count(), 0);
    }

    #[test]
    fn flush_pending_moves_with_resampling_off_dispatches_directly() {
        // Off-path moves dispatch on flush.
        let binding = GestureBinding::new();
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        let processed = binding.flush_pending_moves();
        // Direct path emits 1 dispatch per pending move.
        assert_eq!(processed, 1);
        assert!(!binding.has_pending_motion());
    }

    #[test]
    fn flush_pending_moves_with_resampling_on_uses_resamplers() {
        // With resampling on, the resampler absorbs the move and
        // either dispatches it through the resampled path or holds
        // it for the next sample window. The contract is that
        // No direct queue mirrors the sequence-owned resampler.
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        let _ = binding.flush_pending_moves();
        // A future-timestamp sample may remain pending for the next frame.
        let _still_needs_frame = binding.has_pending_motion();
        // Resampler still alive (pointer is still down).
        assert_eq!(binding.active_resampler_count(), 1);
    }

    #[test]
    fn terminal_flushes_a_resampled_move_that_the_sample_window_cannot_emit() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let order = Rc::new(RefCell::new(Vec::new()));
        let callback_order = Rc::clone(&order);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| match event {
            PointerEvent::Move(_) => callback_order.borrow_mut().push("move"),
            PointerEvent::Up(_) => callback_order.borrow_mut().push("up"),
            _ => {}
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        let sample_time = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("test sample time remains representable");
        let position = Offset::new(Pixels(8.0), Pixels(13.0));
        binding.handle_pointer_event(&make_down_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });
        binding.handle_pointer_event(&make_move_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });

        assert_eq!(
            binding
                .flush_pending_moves_at(sample_time, sample_time + Duration::from_millis(8))
                .expect("test supplies an advancing sampling window"),
            0,
            "a future input event cannot be emitted in an earlier sample window"
        );
        assert!(binding.has_pending_motion());

        binding.handle_pointer_event(&make_up_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });

        assert_eq!(order.borrow().as_slice(), ["move", "up"]);
        assert!(!binding.has_pending_motion());
        assert_eq!(binding.active_pointer_count(), 0);
        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn resampled_move_panic_does_not_prevent_terminal_delivery_or_cleanup() {
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let terminal_deliveries = Rc::new(Cell::new(0));

        let panicking: crate::routing::PointerRouteHandler = Rc::new(|event| {
            if matches!(event, PointerEvent::Move(_)) {
                panic!("resampled move panic");
            }
        });
        let callback_terminal_deliveries = Rc::clone(&terminal_deliveries);
        let observer: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            if matches!(event, PointerEvent::Up(_)) {
                callback_terminal_deliveries.set(callback_terminal_deliveries.get() + 1);
            }
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&panicking));
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&observer));

        let position = Offset::new(Pixels(8.0), Pixels(13.0));
        binding.handle_pointer_event(&make_down_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });
        binding.handle_pointer_event(&make_move_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&make_up_event(position, PointerType::Touch), |_| {
                HitTestResult::new()
            });
        }));
        let payload = unwind.expect_err("the captured Move panic must resume after cleanup");

        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"resampled move panic")
        );
        assert_eq!(terminal_deliveries.get(), 1);
        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert!(!binding.has_pending_motion());
        assert!(binding.arena().is_empty());

        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &panicking);
        binding
            .pointer_router()
            .remove_route(PointerId::PRIMARY, &observer);
    }

    #[test]
    fn resampled_dispatch_releases_route_map_guard_before_callback() {
        use dashmap::try_result::TryResult;
        use std::{cell::Cell, rc::Rc};

        let binding = Rc::new(GestureBinding::new());
        set_resampling(&binding, true);
        let pointer = PointerId::PRIMARY;
        let shard_was_released = Rc::new(Cell::new(false));
        let callback_binding = Rc::clone(&binding);
        let callback_observation = Rc::clone(&shard_was_released);
        let handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
            if matches!(event, PointerEvent::Move(_)) {
                callback_observation.set(!matches!(
                    callback_binding.hit_tests.try_get_mut(&pointer),
                    TryResult::Locked
                ));
            }
        });
        binding
            .pointer_router()
            .add_route(pointer, Rc::clone(&handler));

        let position = Offset::new(Pixels(1.0), Pixels(1.0));
        binding.handle_pointer_event(&make_down_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });
        binding.handle_pointer_event(&make_move_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });
        assert_eq!(binding.flush_pending_moves(), 1);

        assert!(
            shard_was_released.get(),
            "resampled dispatch must not retain the cached-route map guard"
        );

        binding.handle_pointer_event(&make_up_event(position, PointerType::Touch), |_| {
            HitTestResult::new()
        });
        binding.pointer_router().remove_route(pointer, &handler);
    }

    #[test]
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
                .push(pointer_move.current.position.x);
            if callback_replaced_sequence.replace(true) {
                return;
            }

            let up = make_up_event(Offset::new(Pixels(10.0), Pixels(10.0)), PointerType::Touch);
            callback_binding.handle_pointer_event(&up, |_| HitTestResult::new());

            let down = make_down_event(Offset::new(Pixels(90.0), Pixels(90.0)), PointerType::Touch);
            callback_binding.handle_pointer_event(&down, |_| HitTestResult::new());
            let next_move =
                make_move_event(Offset::new(Pixels(99.0), Pixels(99.0)), PointerType::Touch);
            callback_binding.handle_pointer_event(&next_move, |_| HitTestResult::new());
        });
        binding
            .pointer_router()
            .add_route(PointerId::PRIMARY, Rc::clone(&handler));

        let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Touch);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        for coordinate in [10.0, 20.0] {
            let pointer_move = make_move_event(
                Offset::new(Pixels(coordinate), Pixels(coordinate)),
                PointerType::Touch,
            );
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
            .remove_route(PointerId::PRIMARY, &handler);
    }

    #[test]
    fn down_marks_resampler_tracked() {
        // The resampler created on Down must be marked tracked so `sample()`
        // does not early-return; otherwise every coalesced move is dropped on
        // flush when resampling is enabled.
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let tracked = binding
            .hit_tests
            .get(&PointerId::PRIMARY)
            .is_some_and(|route| route.resampler.is_tracked());
        assert!(tracked, "resampler must be tracked after the Down");
    }

    #[test]
    fn flush_with_resampling_on_dispatches_move_not_drops_it() {
        // Regression: with resampling on, a move must reach dispatch (be
        // resampled), not be silently dropped because the resampler was never
        // tracked.
        let binding = GestureBinding::new();
        set_resampling(&binding, true);
        binding.set_sampling_clock(SamplingClock::Fixed {
            period: Duration::from_millis(8),
        });
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        let dispatched = binding.flush_pending_moves();
        assert!(
            dispatched >= 1,
            "resampled move must be dispatched, not dropped (got {dispatched})"
        );
    }

    // ========================================================================
    // Owner-routed route lifecycle (ADR-0027 Task 3)
    // ========================================================================

    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use crate::events::make_cancel_event;
    use crate::routing::{
        HitTestEntry, InteractionLane, MouseRegionCallbacks, MouseTrackerAnnotation, PointerTarget,
        RenderId,
    };

    fn hit_result(target: PointerTarget) -> HitTestResult {
        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        result
    }

    #[test]
    fn standalone_high_level_gesture_uses_one_fresh_ephemeral_route() {
        use ui_events::pointer::{PointerGesture, PointerGestureEvent, PointerInfo, PointerState};

        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let delivered = Rc::new(Cell::new(0));
        let hit_tests = Rc::new(Cell::new(0));

        lane.enter(|| {
            let delivered_to_target = Rc::clone(&delivered);
            let target = handle
                .register_pointer(move |event| {
                    if matches!(event, PointerEvent::Gesture(_)) {
                        delivered_to_target.set(delivered_to_target.get() + 1);
                    }
                })
                .expect("register gesture target");
            let result = hit_result(target);
            let mut state = PointerState::default();
            state.position.x = 42.0;
            state.position.y = 24.0;
            let event = PointerEvent::Gesture(PointerGestureEvent {
                pointer: PointerInfo {
                    pointer_id: Some(PointerId::PRIMARY),
                    pointer_type: PointerType::Mouse,
                    persistent_device_id: None,
                },
                gesture: PointerGesture::Pinch(0.25),
                state,
            });
            let observed_hit_tests = Rc::clone(&hit_tests);
            binding.handle_pointer_event(&event, move |position| {
                assert_eq!(position, Offset::new(Pixels(42.0), Pixels(24.0)));
                observed_hit_tests.set(observed_hit_tests.get() + 1);
                result
            });
        });

        assert_eq!(hit_tests.get(), 1);
        assert_eq!(delivered.get(), 1);
        assert_eq!(binding.active_pointer_count(), 0);
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
            let down = make_down_event_for_id(
                self.pointer,
                Offset::new(Pixels(12.0), Pixels(12.0)),
                PointerType::Touch,
            );
            self.binding
                .handle_pointer_event(&down, |_| HitTestResult::new());
        }
    }

    #[test]
    fn down_caches_route_and_up_delivers_after_target_unregisters() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let delivered = Rc::new(RefCell::new(Vec::new()));
        lane.enter(|| {
            let log = Rc::clone(&delivered);
            let target = handle
                .register_pointer(move |event| {
                    log.borrow_mut().push(match event {
                        PointerEvent::Down(_) => "down",
                        PointerEvent::Move(_) => "move",
                        PointerEvent::Up(_) => "up",
                        _ => "other",
                    });
                })
                .expect("register");
            let result = hit_result(target);

            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&down, |_| result.clone());
            assert_eq!(&*delivered.borrow(), &["down"]);

            // Unmount analog: the target leaves NEW route resolution, but the
            // route cached at Down keeps its strong handler cell.
            handle.unregister_pointer(target).expect("unregister");

            let mv = make_move_event(Offset::new(Pixels(9.0), Pixels(9.0)), PointerType::Mouse);
            binding.handle_pointer_event(&mv, |_| HitTestResult::new());
            binding.flush_pending_moves();
            assert_eq!(&*delivered.borrow(), &["down", "move"]);

            let up = make_up_event(Offset::new(Pixels(9.0), Pixels(9.0)), PointerType::Mouse);
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
            assert_eq!(&*delivered.borrow(), &["down", "move", "up"]);
            assert!(!binding.has_hit_test(PointerId::PRIMARY));

            // A fresh Down on the removed target is a typed miss: nothing is
            // delivered and nothing panics.
            let second_down =
                make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&second_down, |_| hit_result(target));
            assert_eq!(&*delivered.borrow(), &["down", "move", "up"]);
        });
    }

    #[test]
    fn up_releases_the_cached_route_and_drops_the_last_handler_owner() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let handler_dropped = Rc::new(Cell::new(false));
        lane.enter(|| {
            let probe = SetOnDrop(Rc::clone(&handler_dropped));
            let target = handle
                .register_pointer(move |_| {
                    let _keep_probe_alive = &probe;
                })
                .expect("register");
            let result = hit_result(target);

            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&down, |_| result.clone());
            handle.unregister_pointer(target).expect("unregister");
            assert!(
                !handler_dropped.get(),
                "the cached route must keep the handler cell alive"
            );

            let cancel = make_cancel_event(PointerType::Mouse);
            binding.handle_pointer_event(&cancel, |_| HitTestResult::new());
            assert!(
                handler_dropped.get(),
                "Cancel must release the cached route, dropping the last handler owner"
            );
        });
    }

    #[test]
    fn per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let later_deliveries = Rc::new(Cell::new(0));
        let handler_dropped = Rc::new(Cell::new(false));
        lane.enter(|| {
            let drop_probe = SetOnDrop(Rc::clone(&handler_dropped));
            let panicking = handle
                .register_pointer(move |event| {
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

            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&down, |_| result.clone());
            assert_eq!(later_deliveries.get(), 1);
            handle
                .unregister_pointer(panicking)
                .expect("route owns the panicking cell now");

            let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }));
            assert!(unwind.is_err(), "the first target panic must propagate");

            // Later targets still received the Up, and the mandatory cleanup
            // (cache removal + route release) ran before the resumed unwind.
            assert_eq!(later_deliveries.get(), 2);
            assert!(!binding.has_hit_test(PointerId::PRIMARY));
            assert!(
                handler_dropped.get(),
                "Up must sweep and release the route before resuming the panic"
            );
        });
    }

    #[test]
    fn down_dispatches_hit_targets_before_pointer_router_and_arena_lifecycle() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));

        lane.enter(|| {
            let router_log = Arc::clone(&log);
            let pointer_route: crate::routing::PointerRouteHandler = Rc::new(move |event| {
                if matches!(event, PointerEvent::Down(_)) {
                    router_log.lock().push("router");
                }
            });
            let arena_member = Arc::new(LoggingArenaMember {
                log: Arc::clone(&log),
            });
            let binding_for_target = Rc::clone(&binding);
            let target_log = Arc::clone(&log);
            let target = handle
                .register_pointer(move |event| {
                    if matches!(event, PointerEvent::Down(_)) {
                        target_log.lock().push("hit");
                        binding_for_target
                            .pointer_router()
                            .add_route(PointerId::PRIMARY, Rc::clone(&pointer_route));
                        binding_for_target
                            .arena()
                            .add(PointerId::PRIMARY, arena_member.clone());
                    }
                })
                .expect("register hit target");
            let result = hit_result(target);

            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
            binding.handle_pointer_event(&down, |_| result);

            assert_eq!(
                &*log.lock(),
                &["hit", "router"],
                "closing a lone arena must not accept it inside Down dispatch"
            );
            assert_eq!(binding.drain_deferred_arena_resolutions(), 1);
            assert_eq!(&*log.lock(), &["hit", "router", "arena"]);

            let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
            binding
                .pointer_router()
                .remove_all_routes(PointerId::PRIMARY);
            handle
                .unregister_pointer(target)
                .expect("unregister target");
        });
    }

    #[test]
    fn pointer_router_panics_do_not_abort_later_delivery_or_sequence_cleanup() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let hit_up_deliveries = Rc::new(Cell::new(0));
        let later_router_deliveries = Rc::new(Cell::new(0));
        let global_router_deliveries = Rc::new(Cell::new(0));
        let handler_dropped = Rc::new(Cell::new(false));

        lane.enter(|| {
            let drop_probe = SetOnDrop(Rc::clone(&handler_dropped));
            let hit_up_count = Rc::clone(&hit_up_deliveries);
            let target = handle
                .register_pointer(move |event| {
                    let _keep_probe_alive = &drop_probe;
                    if matches!(event, PointerEvent::Up(_)) {
                        hit_up_count.set(hit_up_count.get() + 1);
                    }
                })
                .expect("register hit target");
            let result = hit_result(target);
            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&down, |_| result);
            handle
                .unregister_pointer(target)
                .expect("cached route owns the target");

            let first: crate::routing::PointerRouteHandler = Rc::new(|event| {
                if matches!(event, PointerEvent::Up(_)) {
                    panic!("router first panic");
                }
            });
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, first);

            let later_count = Rc::clone(&later_router_deliveries);
            let later: crate::routing::PointerRouteHandler = Rc::new(move |event| {
                if matches!(event, PointerEvent::Up(_)) {
                    later_count.set(later_count.get() + 1);
                }
            });
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, later);

            let global_count = Rc::clone(&global_router_deliveries);
            let global: crate::routing::GlobalPointerHandler = Rc::new(move |event| {
                if matches!(event, PointerEvent::Up(_)) {
                    global_count.set(global_count.get() + 1);
                    panic!("router secondary panic");
                }
            });
            binding.pointer_router().add_global_handler(global);

            let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }));
            let payload = unwind.expect_err("the first router panic must propagate");

            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&"router first panic"),
                "the first panic in transaction order must win"
            );
            assert_eq!(hit_up_deliveries.get(), 1);
            assert_eq!(later_router_deliveries.get(), 1);
            assert_eq!(global_router_deliveries.get(), 1);
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert!(binding.arena().is_empty());
            assert!(
                handler_dropped.get(),
                "the cached route must release its last handler owner before unwind resumes"
            );
        });
    }

    #[test]
    fn target_panic_wins_over_a_later_route_cleanup_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();

        lane.enter(|| {
            let panic_on_cleanup = PanicOnDrop;
            let target = handle
                .register_pointer(move |event| {
                    let _keep_cleanup_panic_alive = &panic_on_cleanup;
                    if matches!(event, PointerEvent::Up(_)) {
                        panic!("target first panic");
                    }
                })
                .expect("register target");
            let result = hit_result(target);
            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
            binding.handle_pointer_event(&down, |_| result);
            handle
                .unregister_pointer(target)
                .expect("cached route owns the target");

            let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
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

    #[test]
    fn move_batch_delivers_every_pointer_before_resuming_the_first_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let first_pointer = PointerId::new(11).expect("nonzero pointer id");
        let second_pointer = PointerId::new(12).expect("nonzero pointer id");
        let later_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let first_target = handle.register_pointer(|_| {}).expect("first target");
            let second_target = handle.register_pointer(|_| {}).expect("second target");

            let first_down = make_down_event_for_id(
                first_pointer,
                Offset::new(Pixels(1.0), Pixels(1.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&first_down, |_| hit_result(first_target));
            let second_down = make_down_event_for_id(
                second_pointer,
                Offset::new(Pixels(2.0), Pixels(2.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&second_down, |_| hit_result(second_target));

            let first_move = make_move_event_for_id(
                first_pointer,
                Offset::new(Pixels(3.0), Pixels(3.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&first_move, |_| HitTestResult::new());
            let second_move = make_move_event_for_id(
                second_pointer,
                Offset::new(Pixels(4.0), Pixels(4.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&second_move, |_| HitTestResult::new());

            // Match the callback roles to the map's actual drain order so the
            // regression is deterministic even though DashMap iteration has
            // no key-order contract.
            let drain_order: Vec<PointerId> = binding
                .pending_moves
                .iter()
                .map(|entry| *entry.key())
                .collect();
            assert_eq!(drain_order.len(), 2);
            let target_for = |pointer| {
                if pointer == first_pointer {
                    first_target
                } else {
                    second_target
                }
            };
            handle
                .replace_pointer(target_for(drain_order[0]), |event| {
                    if matches!(event, PointerEvent::Move(_)) {
                        panic!("first move panic");
                    }
                })
                .expect("replace first drained target");
            let later_count = Rc::clone(&later_deliveries);
            handle
                .replace_pointer(target_for(drain_order[1]), move |event| {
                    if matches!(event, PointerEvent::Move(_)) {
                        later_count.set(later_count.get() + 1);
                    }
                })
                .expect("replace later drained target");

            let unwind = catch_unwind(AssertUnwindSafe(|| binding.flush_pending_moves()));
            let payload = unwind.expect_err("the first move panic must propagate after the batch");
            assert_eq!(payload.downcast_ref::<&str>(), Some(&"first move panic"));
            assert_eq!(
                later_deliveries.get(),
                1,
                "a panic for one pointer must not discard another pointer's drained move"
            );
            assert!(binding.pending_moves.is_empty());

            for (pointer, position) in [
                (first_pointer, Offset::new(Pixels(3.0), Pixels(3.0))),
                (second_pointer, Offset::new(Pixels(4.0), Pixels(4.0))),
            ] {
                let up = make_up_event_for_id(pointer, position, PointerType::Touch);
                binding.handle_pointer_event(&up, |_| HitTestResult::new());
            }
            handle
                .unregister_pointer(first_target)
                .expect("unregister first target");
            handle
                .unregister_pointer(second_target)
                .expect("unregister second target");
        });
    }

    #[derive(Clone, Copy)]
    enum BindingEntryPoint {
        HitTestClosure,
        ExplicitResult,
    }

    impl BindingEntryPoint {
        fn dispatch(self, binding: &GestureBinding, event: &PointerEvent, result: &HitTestResult) {
            match self {
                Self::HitTestClosure => {
                    binding.handle_pointer_event(event, |_| result.clone());
                }
                Self::ExplicitResult => {
                    binding.handle_pointer_event_with_result(event, result);
                }
            }
        }
    }

    fn assert_superseded_route_panic_does_not_abort_new_down(entry_point: BindingEntryPoint) {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let target_deliveries = Rc::new(Cell::new(0));
        let router_down_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let panic_on_old_route_release = PanicOnDrop;
            let old_target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &panic_on_old_route_release;
                })
                .expect("register old target");
            let old_result = hit_result(old_target);
            let down = make_down_event(Offset::new(Pixels(3.0), Pixels(3.0)), PointerType::Touch);
            entry_point.dispatch(&binding, &down, &old_result);
            handle
                .unregister_pointer(old_target)
                .expect("old cached route owns its target");

            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let target_count = Rc::clone(&target_deliveries);
            let new_target = handle
                .register_pointer(move |_| target_count.set(target_count.get() + 1))
                .expect("register replacement target");
            let new_result = hit_result(new_target);

            let router_count = Rc::clone(&router_down_deliveries);
            let router_handler: crate::routing::PointerRouteHandler = Rc::new(move |event| {
                if matches!(event, PointerEvent::Down(_)) {
                    router_count.set(router_count.get() + 1);
                }
            });
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, Rc::clone(&router_handler));

            let unwind = catch_unwind(AssertUnwindSafe(|| {
                entry_point.dispatch(&binding, &down, &new_result);
            }));
            let payload = unwind.expect_err("superseded route cleanup panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(target_deliveries.get(), 1);
            assert_eq!(router_down_deliveries.get(), 1);
            assert!(!binding.arena().is_open(PointerId::PRIMARY));
            assert_eq!(binding.active_pointer_count(), 1);

            handle
                .unregister_pointer(new_target)
                .expect("new cached route retains the replacement target");
            let up = make_up_event(Offset::new(Pixels(3.0), Pixels(3.0)), PointerType::Touch);
            entry_point.dispatch(&binding, &up, &HitTestResult::new());
            assert_eq!(
                target_deliveries.get(),
                2,
                "the replacement sequence must remain usable after the caught Down panic"
            );
            assert_eq!(binding.active_pointer_count(), 0);
            assert!(binding.arena().is_empty());
            binding
                .pointer_router()
                .remove_route(PointerId::PRIMARY, &router_handler);
        });
    }

    #[test]
    fn superseded_route_panic_does_not_abort_handle_pointer_event_down() {
        assert_superseded_route_panic_does_not_abort_new_down(BindingEntryPoint::HitTestClosure);
    }

    #[test]
    fn superseded_route_panic_does_not_abort_explicit_result_down() {
        assert_superseded_route_panic_does_not_abort_new_down(BindingEntryPoint::ExplicitResult);
    }

    #[test]
    fn cancel_pointer_sequence_releases_route_and_all_pointer_state() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let handler_dropped = Rc::new(Cell::new(false));

        lane.enter(|| {
            let drop_probe = SetOnDrop(Rc::clone(&handler_dropped));
            let target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &drop_probe;
                })
                .expect("register target");
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let down = make_down_event(Offset::new(Pixels(4.0), Pixels(4.0)), PointerType::Touch);
            binding.handle_pointer_event(&down, |_| hit_result(target));
            let move_event =
                make_move_event(Offset::new(Pixels(8.0), Pixels(8.0)), PointerType::Touch);
            binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
            handle
                .unregister_pointer(target)
                .expect("cached route owns target");

            binding.cancel_pointer_sequence(PointerId::PRIMARY);

            assert!(
                handler_dropped.get(),
                "clear must release the resolved route"
            );
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert_eq!(binding.pending_move_count(), 0);
            assert!(!binding.arena().contains(PointerId::PRIMARY));
        });
    }

    #[test]
    fn cancel_pointer_sequence_finishes_cleanup_before_resuming_route_drop_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();

        lane.enter(|| {
            let owner = PanicOnDrop;
            let target = handle
                .register_pointer(move |_| {
                    let _keep_owner_alive = &owner;
                })
                .expect("register target");
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            let down = make_down_event(Offset::new(Pixels(4.0), Pixels(4.0)), PointerType::Touch);
            binding.handle_pointer_event(&down, |_| hit_result(target));
            let move_event =
                make_move_event(Offset::new(Pixels(8.0), Pixels(8.0)), PointerType::Touch);
            binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
            handle
                .unregister_pointer(target)
                .expect("cached route owns target");

            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.cancel_pointer_sequence(PointerId::PRIMARY);
            }));
            let payload = unwind.expect_err("route Drop panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert_eq!(binding.pending_move_count(), 0);
            assert!(binding.arena().is_empty());
        });
    }

    #[test]
    fn cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let first_pointer = PointerId::new(21).expect("nonzero pointer id");
        let later_pointer = PointerId::new(22).expect("nonzero pointer id");
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

            for pointer in [first_pointer, later_pointer] {
                binding
                    .arena()
                    .add(pointer, Arc::new(CountingArenaMember::default()));
                binding
                    .arena()
                    .add(pointer, Arc::new(CountingArenaMember::default()));
            }

            let first_down = make_down_event_for_id(
                first_pointer,
                Offset::new(Pixels(1.0), Pixels(1.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&first_down, |_| hit_result(first_target));
            let later_down = make_down_event_for_id(
                later_pointer,
                Offset::new(Pixels(2.0), Pixels(2.0)),
                PointerType::Touch,
            );
            binding.handle_pointer_event(&later_down, |_| hit_result(later_target));

            let first_token = binding
                .hit_tests
                .get(&first_pointer)
                .and_then(|cached| cached.token)
                .expect("first cached route token");
            let later_token = binding
                .hit_tests
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
                    Offset::new(Pixels(coordinate), Pixels(coordinate)),
                    PointerType::Touch,
                );
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

    #[test]
    fn cancel_all_rejects_reentrant_input_from_route_destructors() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let reentrant_pointer = PointerId::new(31).expect("nonzero pointer id");

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
            let down = make_down_event(Offset::new(Pixels(2.0), Pixels(2.0)), PointerType::Touch);
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

    #[test]
    fn reentrant_target_replacement_drop_stays_inside_the_down_transaction() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let target_slot = Rc::new(Cell::new(None));
        let later_target_deliveries = Rc::new(Cell::new(0));
        let router_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let handle_for_target = handle.clone();
            let slot_for_target = Rc::clone(&target_slot);
            let panic_on_snapshot_drop = PanicOnDrop;
            let replacing_target = handle
                .register_pointer(move |event| {
                    let _keep_owner_alive = &panic_on_snapshot_drop;
                    if matches!(event, PointerEvent::Down(_)) {
                        let target = slot_for_target.get().expect("target installed");
                        handle_for_target
                            .replace_pointer(target, |_| {})
                            .expect("replace current target reentrantly");
                    }
                })
                .expect("register replacing target");
            target_slot.set(Some(replacing_target));

            let later_count = Rc::clone(&later_target_deliveries);
            let later_target = handle
                .register_pointer(move |_| later_count.set(later_count.get() + 1))
                .expect("register later target");
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(replacing_target));
            result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later_target));

            let router_count = Rc::clone(&router_deliveries);
            let router_handler: crate::routing::PointerRouteHandler =
                Rc::new(move |_| router_count.set(router_count.get() + 1));
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, Rc::clone(&router_handler));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let down = make_down_event(Offset::new(Pixels(7.0), Pixels(7.0)), PointerType::Touch);
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&down, |_| result);
            }));
            let payload = unwind.expect_err("snapshot owner Drop panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(later_target_deliveries.get(), 1);
            assert_eq!(router_deliveries.get(), 1);
            assert!(!binding.arena().is_open(PointerId::PRIMARY));

            handle
                .unregister_pointer(replacing_target)
                .expect("unregister replaced target");
            handle
                .unregister_pointer(later_target)
                .expect("unregister later target");
            let up = make_up_event(Offset::new(Pixels(7.0), Pixels(7.0)), PointerType::Touch);
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
            assert_eq!(later_target_deliveries.get(), 2);
            assert_eq!(binding.active_pointer_count(), 0);
            assert!(binding.arena().is_empty());
            binding
                .pointer_router()
                .remove_route(PointerId::PRIMARY, &router_handler);
        });
    }

    #[test]
    fn reentrant_sequence_cancellation_route_drop_stays_inside_transaction() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let target_slot = Rc::new(Cell::new(None));
        let later_target_deliveries = Rc::new(Cell::new(0));
        let router_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let binding_for_target = Rc::clone(&binding);
            let handle_for_target = handle.clone();
            let slot_for_target = Rc::clone(&target_slot);
            let panic_on_route_drop = PanicOnDrop;
            let cancelling_target = handle
                .register_pointer(move |event| {
                    let _keep_owner_alive = &panic_on_route_drop;
                    if matches!(event, PointerEvent::Down(_)) {
                        handle_for_target
                            .unregister_pointer(
                                slot_for_target
                                    .get()
                                    .expect("target installed before dispatch"),
                            )
                            .expect("unregister current target reentrantly");
                        binding_for_target.cancel_pointer_sequence(PointerId::PRIMARY);
                    }
                })
                .expect("register cancelling target");
            target_slot.set(Some(cancelling_target));

            let later_count = Rc::clone(&later_target_deliveries);
            let later_target = handle
                .register_pointer(move |_| later_count.set(later_count.get() + 1))
                .expect("register later target");
            let mut result = HitTestResult::new();
            result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(cancelling_target));
            result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later_target));

            let router_count = Rc::clone(&router_deliveries);
            let router_handler: crate::routing::PointerRouteHandler =
                Rc::new(move |_| router_count.set(router_count.get() + 1));
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, Rc::clone(&router_handler));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let down = make_down_event(Offset::new(Pixels(7.0), Pixels(7.0)), PointerType::Touch);
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&down, |_| result);
            }));
            let payload = unwind.expect_err("resolved route Drop panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(later_target_deliveries.get(), 1);
            assert_eq!(router_deliveries.get(), 1);
            assert_eq!(binding.active_pointer_count(), 0);
            assert_eq!(binding.active_resampler_count(), 0);
            assert_eq!(binding.pending_move_count(), 0);
            assert!(binding.arena().is_empty());

            handle
                .unregister_pointer(later_target)
                .expect("unregister later target");
            binding
                .pointer_router()
                .remove_route(PointerId::PRIMARY, &router_handler);
        });
    }

    #[test]
    fn reentrant_pointer_route_self_removal_drop_stays_inside_transaction() {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let self_route_slot = Rc::new(RefCell::new(None));
        let hit_deliveries = Rc::new(Cell::new(0));
        let later_router_deliveries = Rc::new(Cell::new(0));

        lane.enter(|| {
            let hit_count = Rc::clone(&hit_deliveries);
            let target = handle
                .register_pointer(move |_| hit_count.set(hit_count.get() + 1))
                .expect("register hit target");
            let result = hit_result(target);

            let binding_for_route = Rc::clone(&binding);
            let slot_for_route = Rc::clone(&self_route_slot);
            let panic_on_snapshot_drop = PanicOnDrop;
            let self_removing: crate::routing::PointerRouteHandler = Rc::new(move |event| {
                let _keep_owner_alive = &panic_on_snapshot_drop;
                if matches!(event, PointerEvent::Down(_)) {
                    let handler = slot_for_route
                        .borrow()
                        .as_ref()
                        .cloned()
                        .expect("self route installed");
                    assert!(
                        binding_for_route
                            .pointer_router()
                            .remove_route(PointerId::PRIMARY, &handler,)
                    );
                    let stored = slot_for_route
                        .borrow_mut()
                        .take()
                        .expect("self route stored");
                    drop(stored);
                    drop(handler);
                }
            });
            self_route_slot
                .borrow_mut()
                .replace(Rc::clone(&self_removing));
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, Rc::clone(&self_removing));
            drop(self_removing);

            let later_count = Rc::clone(&later_router_deliveries);
            let later: crate::routing::PointerRouteHandler =
                Rc::new(move |_| later_count.set(later_count.get() + 1));
            binding
                .pointer_router()
                .add_route(PointerId::PRIMARY, Rc::clone(&later));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let down = make_down_event(Offset::new(Pixels(9.0), Pixels(9.0)), PointerType::Touch);
            let unwind = catch_unwind(AssertUnwindSafe(|| {
                binding.handle_pointer_event(&down, |_| result);
            }));
            let payload = unwind.expect_err("router snapshot Drop panic must propagate");

            assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
            assert_eq!(hit_deliveries.get(), 1);
            assert_eq!(later_router_deliveries.get(), 1);
            assert!(!binding.arena().is_open(PointerId::PRIMARY));

            handle
                .unregister_pointer(target)
                .expect("unregister target");
            let up = make_up_event(Offset::new(Pixels(9.0), Pixels(9.0)), PointerType::Touch);
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
            assert_eq!(hit_deliveries.get(), 2);
            assert_eq!(later_router_deliveries.get(), 2);
            assert_eq!(binding.active_pointer_count(), 0);
            assert!(binding.arena().is_empty());
            binding
                .pointer_router()
                .remove_route(PointerId::PRIMARY, &later);
        });
    }

    #[test]
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
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
            binding
                .arena()
                .add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));

            let down = make_down_event(Offset::new(Pixels(6.0), Pixels(6.0)), PointerType::Touch);
            binding.handle_pointer_event(&down, |_| result);
            assert!(did_unregister.get());
            assert_eq!(later_deliveries.get(), 1);
            assert!(!binding.arena().is_open(PointerId::PRIMARY));

            handle
                .unregister_pointer(later_target)
                .expect("unregister later target");
            let up = make_up_event(Offset::new(Pixels(6.0), Pixels(6.0)), PointerType::Touch);
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
            let target = handle
                .register_pointer(move |event| {
                    let _keep_probe_alive = &probe;
                    if matches!(event, PointerEvent::Down(_)) {
                        arena.add(PointerId::PRIMARY, Arc::new(PanickingAcceptArenaMember));
                        arena.add(PointerId::PRIMARY, Arc::new(CountingArenaMember::default()));
                    }
                })
                .expect("register hit target");
            let result = hit_result(target);

            let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
            entry_point.dispatch(&binding, &down, &result);
            handle
                .unregister_pointer(target)
                .expect("cached route owns the target");
            assert!(!handler_dropped.get());

            let up = make_up_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Touch);
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

    #[test]
    fn arena_accept_panic_cleans_up_handle_pointer_event() {
        assert_arena_accept_panic_cleanup(BindingEntryPoint::HitTestClosure);
    }

    #[test]
    fn arena_accept_panic_cleans_up_handle_pointer_event_with_result() {
        assert_arena_accept_panic_cleanup(BindingEntryPoint::ExplicitResult);
    }
}
