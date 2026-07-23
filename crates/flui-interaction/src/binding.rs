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
//! 2. **Pointer Move**: Use cached hit test → dispatch (coalesced)
//! 3. **Pointer Up/Cancel**: Use cached hit test → dispatch → sweep arena →
//!    clear cache
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

use dashmap::DashMap;
use flui_types::geometry::{Offset, Pixels};
use smallvec::SmallVec;
use ui_events::pointer::{PointerEvent, PointerType};

use crate::{
    arena::GestureArena,
    ids::PointerId,
    processing::{PointerEventResampler, SamplingClock},
    routing::{
        HitTestResult, PointerRouter, ResolvedRouteToken, RoutePanic, active_dispatch_handle,
    },
    settings::GestureSettings,
};

/// Per-pointer state cached at Down: the data-only hit path plus the
/// owner-local resolved route token that Move reuses and Up/Cancel releases.
///
/// `token` is `None` when no interaction lane was active at Down (a
/// gesture-only binding without a mounted tree) or when the path carried no
/// pointer targets; the pointer router still routes such events.
#[derive(Clone)]
struct CachedPointerRoute {
    result: HitTestResult,
    token: Option<ResolvedRouteToken>,
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
    pending_moves: DashMap<PointerId, PointerEvent>,

    /// Per-pointer event resamplers. One per active pointer; created
    /// lazily on first [`Self::handle_pointer_event`] and dropped on
    /// Up/Cancel. The resamplers are *opt-in*: only used when
    /// [`Self::set_resampling_enabled`] has been called with `true`.
    /// Off by default to preserve the pre-resampler dispatch path
    /// for callers that don't need frame-paced sampling.
    resamplers: DashMap<PointerId, PointerEventResampler>,

    /// Whether the per-pointer resamplers are consulted on
    /// [`Self::flush_pending_moves`]. Off by default.
    resampling_enabled: std::sync::atomic::AtomicBool,

    /// Frame-paced clock that produces `(now, next)` pairs for the
    /// resamplers. Only consulted when `resampling_enabled` is true.
    sampling_clock: parking_lot::RwLock<SamplingClock>,

    /// Routes pointer events to registered handlers.
    pointer_router: PointerRouter,

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
        Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            resamplers: DashMap::new(),
            resampling_enabled: std::sync::atomic::AtomicBool::new(false),
            sampling_clock: parking_lot::RwLock::new(SamplingClock::default()),
            pointer_router: PointerRouter::new(),
            // Binding-owned arena: `handle_pointer_event` closes it on down and
            // sweeps it on up, so recognizers that share it (via a shell-mounted
            // scope) must never run that lifecycle themselves.
            arena: GestureArena::binding_driven(std::sync::Arc::new(flui_foundation::SystemClock)),
            default_settings: GestureSettings::default(),
        }
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            resamplers: DashMap::new(),
            resampling_enabled: std::sync::atomic::AtomicBool::new(false),
            sampling_clock: parking_lot::RwLock::new(SamplingClock::default()),
            pointer_router: PointerRouter::new(),
            // Binding-owned arena — see `new`.
            arena: GestureArena::binding_driven(std::sync::Arc::new(flui_foundation::SystemClock)),
            default_settings: settings,
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
    /// Idempotent. Disabling mid-stream clears per-pointer resamplers
    /// so the binding returns to the direct dispatch path on the next
    /// frame.
    pub fn set_resampling_enabled(&self, enabled: bool) {
        use std::sync::atomic::Ordering;
        self.resampling_enabled.store(enabled, Ordering::Release);
        if !enabled {
            self.resamplers.clear();
        }
    }

    /// Returns whether per-pointer resampling is enabled.
    #[inline]
    #[must_use]
    pub fn is_resampling_enabled(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.resampling_enabled.load(Ordering::Acquire)
    }

    /// Replace the sampling clock used to pace resamplers.
    ///
    /// Existing per-pointer resamplers are **not** reset — they pick
    /// up the new cadence on their next `sample()` call. Caller is
    /// responsible for ensuring the new clock's period is sensible
    /// for the input devices currently being tracked.
    pub fn set_sampling_clock(&self, clock: SamplingClock) {
        *self.sampling_clock.write() = clock;
    }

    /// Returns a copy of the active sampling clock.
    #[inline]
    #[must_use]
    pub fn sampling_clock(&self) -> SamplingClock {
        *self.sampling_clock.read()
    }

    /// Number of active per-pointer resamplers.
    #[inline]
    #[must_use]
    pub fn active_resampler_count(&self) -> usize {
        self.resamplers.len()
    }

    // ========================================================================
    // Component Accessors
    // ========================================================================

    /// Get the pointer router.
    #[inline]
    pub fn pointer_router(&self) -> &PointerRouter {
        &self.pointer_router
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
        match event {
            PointerEvent::Down(e) => {
                let pointer_id = Self::extract_pointer_id(event);

                // Bound per-pointer state growth (memory-DoS guard). A re-down of
                // an already-tracked pointer replaces rather than grows, so it is
                // always allowed; only a genuinely new pointer beyond the cap is
                // dropped.
                if self.hit_tests.len() >= MAX_SIMULTANEOUS_POINTERS
                    && !self.hit_tests.contains_key(&pointer_id)
                {
                    tracing::warn!(
                        ?pointer_id,
                        active = self.hit_tests.len(),
                        "dropping Down: simultaneous-pointer cap reached"
                    );
                    return;
                }

                let position = Offset::new(px_f32(e.state.position.x), px_f32(e.state.position.y));

                // Perform hit test
                let result = hit_test_fn(position);

                // Resolve the owner-local route ONCE, before the arena closes:
                // Move reuses the cached token and Up/Cancel releases it. A
                // re-down of a tracked pointer replaces the cached entry, so
                // the superseded route is released rather than leaked.
                let token = Self::resolve_route(&result);
                let superseded = self.hit_tests.insert(
                    pointer_id,
                    CachedPointerRoute {
                        result: result.clone(),
                        token,
                    },
                );
                if let Some(superseded) = superseded {
                    Self::release_route(superseded.token);
                }

                // Lazy-create the per-pointer resampler AND mark it tracked.
                // The resampler only paces moves once it is tracked; without
                // this, `sample()` early-returns while `!is_tracked` and
                // `flush_pending_moves` then clears the coalesced queue,
                // silently dropping every move when resampling is enabled. We
                // use `start_tracking()` (not `add_event`) because the Down is
                // dispatched directly below — queueing it would double-dispatch
                // it on the next sample. The DashMap guard is scoped so it is
                // released before dispatch.
                {
                    let resampler = self
                        .resamplers
                        .entry(pointer_id)
                        .or_insert_with(|| PointerEventResampler::new(pointer_id));
                    if self.is_resampling_enabled() {
                        resampler.start_tracking();
                    }
                }

                // Dispatch to targets, THEN close the arena — Flutter's
                // `GestureBinding.handleEvent` order. A per-target panic is
                // captured so the close still runs before the unwind resumes.
                let panic = self.dispatch_event(event, token);
                self.arena.close(pointer_id);
                if let Some(panic) = panic {
                    panic.resume();
                }
            }

            PointerEvent::Move(_) => {
                let pointer_id = Self::extract_pointer_id(event);

                // Coalesce move events - store only the latest, process on flush
                self.pending_moves.insert(pointer_id, event.clone());

                // Mirror to the resampler's per-pointer queue so the
                // resampler can pace the event on the next sample().
                if self.is_resampling_enabled()
                    && let Some(r) = self.resamplers.get(&pointer_id)
                {
                    r.add_event(event.clone());
                }
            }

            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let pointer_id = Self::extract_pointer_id(event);

                // Deliver on the cached route, THEN sweep, THEN release the
                // route — the cached cells stay strong through delivery even
                // if the target unmounted mid-gesture (Flutter's retained
                // `HitTestEntry.target`). A per-target panic is captured so
                // sweep + release still run before the unwind resumes.
                let cached = self.hit_tests.remove(&pointer_id).map(|(_, cached)| cached);
                let panic = cached
                    .as_ref()
                    .and_then(|cached| self.dispatch_event(event, cached.token));

                // Sweep the arena
                self.arena.sweep(pointer_id);

                // Drop the resampler for this pointer. `remove` returns
                // the owned value, so the resampler's Arc drops with
                // last reference.
                self.resamplers.remove(&pointer_id);

                if let Some(cached) = cached {
                    Self::release_route(cached.token);
                }
                if let Some(panic) = panic {
                    panic.resume();
                }
            }

            PointerEvent::Enter(_) | PointerEvent::Leave(_) => {
                // Enter/Leave don't participate in gesture recognition
                // but we still dispatch them
                let pointer_id = Self::extract_pointer_id(event);
                if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                    panic.resume();
                }
            }

            PointerEvent::Scroll(e) => {
                let pointer_id = Self::extract_pointer_id(event);

                // Scroll events might not have a cached hit test
                // Use the position to do a hit test if needed
                if self.hit_tests.contains_key(&pointer_id) {
                    if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                        panic.resume();
                    }
                } else {
                    let position =
                        Offset::new(px_f32(e.state.position.x), px_f32(e.state.position.y));
                    let result = hit_test_fn(position);
                    self.dispatch_ephemeral(event, &result);
                }
            }

            PointerEvent::Gesture(_) => {
                // Gesture events are high-level and handled separately
                let pointer_id = Self::extract_pointer_id(event);
                if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                    panic.resume();
                }
            }
        }
    }

    /// Handle pointer event without hit testing.
    ///
    /// Use this when you already have a hit test result or want to
    /// manually control hit testing.
    pub fn handle_pointer_event_with_result(&self, event: &PointerEvent, result: &HitTestResult) {
        let pointer_id = Self::extract_pointer_id(event);

        match event {
            PointerEvent::Down(_) => {
                let token = Self::resolve_route(result);
                let superseded = self.hit_tests.insert(
                    pointer_id,
                    CachedPointerRoute {
                        result: result.clone(),
                        token,
                    },
                );
                if let Some(superseded) = superseded {
                    Self::release_route(superseded.token);
                }
                let panic = self.dispatch_event(event, token);
                self.arena.close(pointer_id);
                if let Some(panic) = panic {
                    panic.resume();
                }
            }

            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                let cached = self.hit_tests.remove(&pointer_id).map(|(_, cached)| cached);
                // Flutter parity: Up/Cancel deliver on the route cached at
                // Down, then sweep, then release.
                if let Some(cached) = cached {
                    let panic = self.dispatch_event(event, cached.token);
                    self.arena.sweep(pointer_id);
                    Self::release_route(cached.token);
                    if let Some(panic) = panic {
                        panic.resume();
                    }
                } else {
                    self.dispatch_ephemeral(event, result);
                    self.arena.sweep(pointer_id);
                }
            }

            _ => {
                if self.hit_tests.contains_key(&pointer_id) {
                    if let Some(panic) = self.dispatch_on_cached_route(pointer_id, event) {
                        panic.resume();
                    }
                } else {
                    self.dispatch_ephemeral(event, result);
                }
            }
        }
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
        let mut count = 0;

        // If resampling is enabled, sample every per-pointer resampler
        // *first* and dispatch the resampled events through the same
        // routing path. The resampler's own event queue was fed in
        // `handle_pointer_event` (Move branch). When resampling is
        // off, we fall through to the direct dispatch path below.
        if self.is_resampling_enabled() && !self.resamplers.is_empty() {
            // Compute `(now, next)` from the configured clock once per
            // frame. For a `Fixed` clock this is one `Instant::now()`
            // call; for `Manual` it's the matching branch (returns
            // `None` ⇒ we cannot pace the resamplers, so skip the
            // resampling pass and fall back to direct dispatch).
            let clock = *self.sampling_clock.read();
            if let Some((now, next)) = clock.tick() {
                // Collect the per-pointer resamplers into a Vec so we
                // can release the DashMap shard before dispatching —
                // dispatch calls back into the binding through
                // handlers, which would re-acquire the same shard and
                // deadlock under DashMap's shard-per-key design.
                let pointers: Vec<PointerId> =
                    self.resamplers.iter().map(|entry| *entry.key()).collect();
                for pointer_id in pointers {
                    // Bail early if the pointer's hit test was
                    // removed (Up/Cancel landed between the collect
                    // and the per-pointer sample).
                    if !self.hit_tests.contains_key(&pointer_id) {
                        continue;
                    }
                    let Some(resampler) = self.resamplers.get(&pointer_id) else {
                        continue;
                    };
                    resampler.sample(now, next, |resampled| {
                        // Re-check the cache for each resampled event; the
                        // cached route token is copied out so no DashMap
                        // guard is held across handler re-entry. A Move has
                        // no pending arena cleanup, so a captured per-target
                        // panic resumes immediately.
                        if self.hit_tests.contains_key(&pointer_id) {
                            if let Some(panic) =
                                self.dispatch_on_cached_route(pointer_id, &resampled)
                            {
                                panic.resume();
                            }
                            count += 1;
                        }
                    });
                }
                // Resampling consumed the resampler's view of the
                // pending move stream — clear the coalesced queue so
                // direct dispatch doesn't replay the same events.
                self.pending_moves.clear();
                return count;
            }
            // `Manual` clock with no tick source: caller is using the
            // binding off-line and the resampling pass is unusable.
            // Fall through to the direct path. We do *not* clear
            // `pending_moves` so the user can still see the events, but we DO
            // clear the resampler queues — they were fed in the Move branch
            // and would otherwise grow unbounded to `MAX_BUFFERED_EVENTS` and
            // emit drop warnings every frame, since this path never drains
            // them via `sample()`.
            for resampler in &self.resamplers {
                resampler.clear();
            }
        }

        // Direct dispatch path: drain every coalesced move, then
        // dispatch each to the cached hit test result.
        //
        // Draining happens up front, before any handler runs, for two
        // reasons. First, `dispatch_event` re-enters the binding through
        // its handlers; holding a `DashMap` shard guard across that call
        // would deadlock on the shard-per-key design, so every guard must
        // be released first. Second, removing all entries before dispatch
        // preserves coalescing semantics: a re-entrant insert during
        // dispatch lands in the *next* frame, never this one.
        //
        // `remove` returns the owned event, so each move is *moved* out of
        // the map rather than cloned (`PointerEvent` is 152 bytes). The
        // key snapshot and the drained buffer stay inline for the common
        // case (a handful of simultaneous pointers; the hard cap is
        // `MAX_SIMULTANEOUS_POINTERS`), spilling to the heap only under
        // heavy multitouch.
        let pointers: SmallVec<[PointerId; 4]> = self
            .pending_moves
            .iter()
            .map(|entry| *entry.key())
            .collect();
        let mut drained: SmallVec<[(PointerId, PointerEvent); 4]> =
            SmallVec::with_capacity(pointers.len());
        for pointer_id in pointers {
            if let Some(entry) = self.pending_moves.remove(&pointer_id) {
                drained.push(entry);
            }
        }

        for (pointer_id, event) in drained {
            // Move reuses the route resolved at Down; no cleanup is pending,
            // so a captured per-target panic resumes immediately.
            if self.hit_tests.contains_key(&pointer_id) {
                if let Some(panic) = self.dispatch_on_cached_route(pointer_id, &event) {
                    panic.resume();
                }
                count += 1;
            }
        }

        count
    }

    /// Check if there are pending move events to process.
    #[inline]
    pub fn has_pending_moves(&self) -> bool {
        !self.pending_moves.is_empty()
    }

    /// Get the number of pending move events.
    #[inline]
    pub fn pending_move_count(&self) -> usize {
        self.pending_moves.len()
    }

    // ========================================================================
    // Hit Test Cache Management
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

    /// Clear the hit test cache for a pointer.
    pub fn clear_hit_test(&self, pointer_id: PointerId) {
        self.hit_tests.remove(&pointer_id);
    }

    /// Clear all cached hit tests.
    pub fn clear_all_hit_tests(&self) {
        self.hit_tests.clear();
    }

    /// Defensive cleanup for app-lifecycle pause / detach transitions.
    ///
    /// Drains the per-pointer hit-test cache. Call this from the app
    /// binding when `AppLifecycleState` transitions to `Paused`, `Hidden`,
    /// or `Detached` — pointer-down events landed before the lifecycle
    /// change may never receive a corresponding Up/Cancel (the platform
    /// may suspend us before the user lifts the finger). The Up/Cancel
    /// branch in [`Self::handle_pointer_event`] already drains entries on
    /// normal completion; this method covers the abnormal-disconnect
    /// case audit Finding I-8 raised.
    ///
    /// Closes audit Finding I-8 (hit_tests DashMap leak on device
    /// disconnect mid-down).
    pub fn handle_lifecycle_pause(&self) {
        let cleared = self.hit_tests.len();
        if cleared > 0 {
            tracing::debug!(
                cleared,
                "GestureBinding draining hit_tests on lifecycle pause"
            );
            // Release the owner-local routes the drained entries were
            // holding; best-effort when no lane scope is active here (the
            // lane's own teardown drains any remainder).
            let cached_tokens: Vec<ResolvedRouteToken> = self
                .hit_tests
                .iter()
                .filter_map(|entry| entry.value().token)
                .collect();
            self.hit_tests.clear();
            for token in cached_tokens {
                Self::release_route(Some(token));
            }
        }
        // Resamplers reference the same pointers as the hit test
        // cache. Drop them too — the resampled event queue is keyed
        // by pointer id and a stale resampler would consume the next
        // Move on the same id post-resume.
        if !self.resamplers.is_empty() {
            let cleared = self.resamplers.len();
            self.resamplers.clear();
            tracing::debug!(
                cleared,
                "GestureBinding draining resamplers on lifecycle pause"
            );
        }
        // Pending moves are coalesced by pointer id; clearing the
        // hit test cache leaves them orphaned (their dispatch would
        // skip the routing path entirely). Drop them alongside.
        if !self.pending_moves.is_empty() {
            let cleared = self.pending_moves.len();
            self.pending_moves.clear();
            tracing::debug!(
                cleared,
                "GestureBinding draining pending_moves on lifecycle pause"
            );
        }
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
    /// Normally called automatically on pointer up/cancel.
    pub fn sweep_arena(&self, pointer_id: PointerId) {
        self.arena.sweep(pointer_id);
    }

    /// Resolve any timed out arenas.
    ///
    /// Call this periodically (e.g., on frame tick) to handle disambiguation
    /// timeouts.
    pub fn resolve_timed_out_arenas(&self) -> usize {
        self.arena.resolve_default_timed_out_arenas()
    }

    /// Advance time-based recognizer deadlines (e.g. long-press hold).
    ///
    /// Call once per frame on the UI thread, beside [`Self::flush_pending_moves`].
    /// A recognizer deadline is otherwise only advanced opportunistically on the
    /// next pointer event, so a stationary held pointer past its deadline would
    /// never fire (e.g. long-press on a finger held perfectly still). This is
    /// distinct from [`Self::resolve_timed_out_arenas`], which handles arena
    /// disambiguation timeouts, not per-recognizer deadlines.
    pub fn tick_deadlines(&self) {
        self.arena.poll_deadlines();
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

    // ========================================================================
    // Internal Methods
    // ========================================================================

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

    /// Release a cached route after its pointer sequence completes.
    fn release_route(token: Option<ResolvedRouteToken>) {
        let Some(token) = token else {
            return;
        };
        if let Err(error) = active_dispatch_handle().and_then(|handle| handle.release_route(token))
        {
            tracing::debug!(
                ?error,
                "cached pointer route not released through the active lane"
            );
        }
    }

    /// Route `event` to the pointer router and invoke the resolved route.
    ///
    /// Returns the first per-target panic so the caller can finish its arena
    /// cleanup before resuming it.
    fn dispatch_event(
        &self,
        event: &PointerEvent,
        token: Option<ResolvedRouteToken>,
    ) -> Option<RoutePanic> {
        // Route the event through the pointer router
        self.pointer_router.route(event);

        let token = token?;
        match active_dispatch_handle().and_then(|handle| handle.invoke_pointer_route(token, event))
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
    /// untracked pointer): `HitTestResult::dispatch` resolves, invokes, and
    /// releases the ephemeral route through the same owner-lane seam.
    fn dispatch_ephemeral(&self, event: &PointerEvent, result: &HitTestResult) {
        self.pointer_router.route(event);
        result.dispatch(event);
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
    use std::time::Duration;

    use super::*;
    use crate::events::{make_down_event, make_move_event, make_up_event};

    /// A cached entry with no hit path and no resolved route, for cache tests.
    fn empty_cached_route() -> CachedPointerRoute {
        CachedPointerRoute {
            result: HitTestResult::new(),
            token: None,
        }
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

    /// The binding's arena must advertise [`SweepModel::BindingDriven`]:
    /// `handle_pointer_event` closes it on down and sweeps it on up, so a
    /// recognizer that shares this arena (handed to a view subtree via a
    /// shell-mounted `GestureArenaScope`) must never run that lifecycle a
    /// second time — `RecognizerBase::stop_tracking` reads the sweep model
    /// to decide, and a detector reads it to decide whether to self-close.
    #[test]
    fn binding_arena_is_binding_driven() {
        assert_eq!(
            GestureBinding::new().arena().sweep_model(),
            crate::arena::SweepModel::BindingDriven,
        );
        assert_eq!(
            GestureBinding::with_settings(GestureSettings::default())
                .arena()
                .sweep_model(),
            crate::arena::SweepModel::BindingDriven,
            "with_settings must build the same binding-owned arena as new()",
        );
    }

    #[test]
    fn test_hit_test_cache() {
        let binding = GestureBinding::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        binding.hit_tests.insert(pointer, empty_cached_route());
        assert!(binding.has_hit_test(pointer));

        let cached = binding.get_hit_test(pointer);
        assert!(cached.is_some());

        binding.clear_hit_test(pointer);
        assert!(!binding.has_hit_test(pointer));
    }

    #[test]
    fn test_clear_all_hit_tests() {
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

        binding.clear_all_hit_tests();
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
        binding.set_resampling_enabled(true);
        assert!(binding.is_resampling_enabled());
        binding.set_resampling_enabled(false);
        assert!(!binding.is_resampling_enabled());
    }

    #[test]
    fn set_resampling_enabled_false_clears_resamplers() {
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
        // Seed a resampler via the Down path so the DashMap is non-empty.
        let down = make_down_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        assert!(binding.active_resampler_count() >= 1);

        binding.set_resampling_enabled(false);
        assert_eq!(binding.active_resampler_count(), 0);
    }

    #[test]
    fn down_creates_per_pointer_resampler() {
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
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
        binding.set_resampling_enabled(true);
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
        assert!(binding.has_pending_moves());
        assert_eq!(binding.pending_move_count(), 1);
        // The resampler is created on Down but no Move is fed to it
        // when resampling is off.
        assert!(
            !binding
                .resamplers
                .iter()
                .any(|r| r.value().has_pending_events())
        );
    }

    #[test]
    fn move_with_resampling_on_feeds_resampler() {
        // Resampling on: move events are mirrored to the resampler in
        // addition to the coalesced queue.
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        assert_eq!(binding.active_resampler_count(), 1);
        assert!(binding.has_pending_moves());
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
        binding.set_resampling_enabled(true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        assert!(binding.active_resampler_count() >= 1);
        assert!(binding.has_pending_moves());

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
        assert!(!binding.has_pending_moves());
    }

    #[test]
    fn flush_pending_moves_with_resampling_on_uses_resamplers() {
        // With resampling on, the resampler absorbs the move and
        // either dispatches it through the resampled path or holds
        // it for the next sample window. The contract is that
        // `pending_moves` is drained (so the direct path does not
        // replay the same event).
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let mv = make_move_event(Offset::new(Pixels(10.0), Pixels(20.0)), PointerType::Mouse);
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());

        let _ = binding.flush_pending_moves();
        // After flush, the coalesced queue is drained (the resampler
        // owns the move-stream view from now on).
        assert!(!binding.has_pending_moves());
        // Resampler still alive (pointer is still down).
        assert_eq!(binding.active_resampler_count(), 1);
    }

    #[test]
    fn down_marks_resampler_tracked() {
        // The resampler created on Down must be marked tracked so `sample()`
        // does not early-return; otherwise every coalesced move is dropped on
        // flush when resampling is enabled.
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
        let down = make_down_event(Offset::new(Pixels(5.0), Pixels(5.0)), PointerType::Mouse);
        binding.handle_pointer_event(&down, |_| HitTestResult::new());

        let tracked = binding
            .resamplers
            .get(&PointerId::PRIMARY)
            .is_some_and(|r| r.is_tracked());
        assert!(tracked, "resampler must be tracked after the Down");
    }

    #[test]
    fn flush_with_resampling_on_dispatches_move_not_drops_it() {
        // Regression: with resampling on, a move must reach dispatch (be
        // resampled), not be silently dropped because the resampler was never
        // tracked.
        let binding = GestureBinding::new();
        binding.set_resampling_enabled(true);
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
    use crate::routing::{HitTestEntry, InteractionLane, PointerTarget, RenderId};

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
}
