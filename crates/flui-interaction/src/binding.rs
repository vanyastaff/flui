//! Gesture Binding - Singleton coordinator for pointer event handling
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
//! │   GestureBinding    │ (singleton)
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
//! // Get the singleton instance
//! let binding = GestureBinding::instance();
//!
//! // Handle platform events
//! fn handle_event(event: &PointerEvent) {
//!     GestureBinding::instance().handle_pointer_event(event, |hit_test_position| {
//!         // Perform hit testing on your render tree
//!         my_render_tree.hit_test(hit_test_position)
//!     });
//! }
//! ```

use dashmap::DashMap;
use flui_foundation::{BindingBase, impl_binding_singleton};
use flui_types::geometry::{Offset, Pixels};
use smallvec::SmallVec;
use ui_events::pointer::{PointerEvent, PointerType};

use crate::{
    arena::GestureArena,
    ids::PointerId,
    processing::{PointerEventResampler, SamplingClock},
    routing::{HitTestResult, PointerRouter},
    settings::GestureSettings,
};

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

/// Central coordinator for gesture event handling (singleton).
///
/// GestureBinding manages the complete lifecycle of pointer events:
/// - Performs hit testing on pointer down
/// - Caches hit test results for subsequent events
/// - Coalesces high-frequency pointer move events (100+ events/sec → 1 per
///   frame)
/// - Routes events through the PointerRouter
/// - Manages arena lifecycle (close on down, sweep on up)
///
/// # Singleton Pattern
///
/// Access via `GestureBinding::instance()`:
///
/// ```rust,ignore
/// let binding = GestureBinding::instance();
/// binding.handle_pointer_event(&event, hit_test_fn);
/// ```
///
/// # Event Coalescing
///
/// Desktop platforms can generate 100+ mouse move events per second.
/// GestureBinding coalesces these by storing only the latest move event
/// per pointer. Call `flush_pending_moves()` once per frame to process
/// the coalesced events.
///
/// # Thread Safety
///
/// GestureBinding is fully thread-safe and can be shared across threads.
/// All internal state is protected by appropriate synchronization primitives.
pub struct GestureBinding {
    /// Cached hit test results per pointer.
    /// Avoids redundant hit testing for move/up events.
    hit_tests: DashMap<PointerId, HitTestResult>,

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
    arena: GestureArena,

    /// Default gesture settings (can be overridden per device).
    default_settings: GestureSettings,
}

// Implement BindingBase trait
impl BindingBase for GestureBinding {
    fn init_instances(&mut self) {
        // GestureBinding initialization is done in new()
        // This is called automatically by the singleton macro
        tracing::debug!("GestureBinding initialized");
    }
}

// Implement singleton pattern via macro
impl_binding_singleton!(GestureBinding);

impl Default for GestureBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureBinding {
    /// Create a new GestureBinding with default settings.
    ///
    /// Note: Prefer using `GestureBinding::instance()` for singleton access.
    pub fn new() -> Self {
        let mut binding = Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            resamplers: DashMap::new(),
            resampling_enabled: std::sync::atomic::AtomicBool::new(false),
            sampling_clock: parking_lot::RwLock::new(SamplingClock::default()),
            pointer_router: PointerRouter::new(),
            arena: GestureArena::new(),
            default_settings: GestureSettings::default(),
        };
        binding.init_instances();
        binding
    }

    /// Create with specific settings.
    pub fn with_settings(settings: GestureSettings) -> Self {
        let mut binding = Self {
            hit_tests: DashMap::new(),
            pending_moves: DashMap::new(),
            resamplers: DashMap::new(),
            resampling_enabled: std::sync::atomic::AtomicBool::new(false),
            sampling_clock: parking_lot::RwLock::new(SamplingClock::default()),
            pointer_router: PointerRouter::new(),
            arena: GestureArena::new(),
            default_settings: settings,
        };
        binding.init_instances();
        binding
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
    /// GestureBinding::instance().handle_pointer_event(&event, |position| {
    ///     render_tree.hit_test(position)
    /// });
    /// ```
    pub fn handle_pointer_event<F>(&self, event: &PointerEvent, hit_test_fn: F)
    where
        F: FnOnce(Offset<Pixels>) -> HitTestResult,
    {
        match event {
            PointerEvent::Down(e) => {
                let pointer_id = self.extract_pointer_id(event);

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

                // Cache the result
                self.hit_tests.insert(pointer_id, result.clone());

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

                // Dispatch to targets
                self.dispatch_event(event, &result);

                // Close the arena for this pointer
                self.arena.close(pointer_id);
            }

            PointerEvent::Move(_) => {
                let pointer_id = self.extract_pointer_id(event);

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
                let pointer_id = self.extract_pointer_id(event);

                // Use cached hit test result
                if let Some((_, result)) = self.hit_tests.remove(&pointer_id) {
                    self.dispatch_event(event, &result);
                }

                // Sweep the arena
                self.arena.sweep(pointer_id);

                // Drop the resampler for this pointer. `remove` returns
                // the owned value, so the resampler's Arc drops with
                // last reference.
                self.resamplers.remove(&pointer_id);
            }

            PointerEvent::Enter(_) | PointerEvent::Leave(_) => {
                // Enter/Leave don't participate in gesture recognition
                // but we still dispatch them
                let pointer_id = self.extract_pointer_id(event);
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                }
            }

            PointerEvent::Scroll(e) => {
                let pointer_id = self.extract_pointer_id(event);

                // Scroll events might not have a cached hit test
                // Use the position to do a hit test if needed
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                } else {
                    let position =
                        Offset::new(px_f32(e.state.position.x), px_f32(e.state.position.y));
                    let result = hit_test_fn(position);
                    self.dispatch_event(event, &result);
                }
            }

            PointerEvent::Gesture(_) => {
                // Gesture events are high-level and handled separately
                let pointer_id = self.extract_pointer_id(event);
                if let Some(result) = self.hit_tests.get(&pointer_id) {
                    self.dispatch_event(event, &result);
                }
            }
        }
    }

    /// Handle pointer event without hit testing.
    ///
    /// Use this when you already have a hit test result or want to
    /// manually control hit testing.
    pub fn handle_pointer_event_with_result(&self, event: &PointerEvent, result: &HitTestResult) {
        let pointer_id = self.extract_pointer_id(event);

        match event {
            PointerEvent::Down(_) => {
                self.hit_tests.insert(pointer_id, result.clone());
                self.dispatch_event(event, result);
                self.arena.close(pointer_id);
            }

            PointerEvent::Up(_) | PointerEvent::Cancel(_) => {
                self.dispatch_event(event, result);
                self.hit_tests.remove(&pointer_id);
                self.arena.sweep(pointer_id);
            }

            _ => {
                self.dispatch_event(event, result);
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
    ///     GestureBinding::instance().flush_pending_moves();
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
                        // Re-fetch the hit test result for each
                        // resampled event. The DashMap entry was
                        // acquired above; this re-read is cheap and
                        // always finds a value (caller removed the
                        // entry on Up/Cancel, so we are inside the
                        // active-pointer window).
                        if let Some(r) = self.hit_tests.get(&pointer_id) {
                            self.dispatch_event(&resampled, &r);
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
            for resampler in self.resamplers.iter() {
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
        let pointers: SmallVec<[PointerId; 4]> =
            self.pending_moves.iter().map(|entry| *entry.key()).collect();
        let mut drained: SmallVec<[(PointerId, PointerEvent); 4]> =
            SmallVec::with_capacity(pointers.len());
        for pointer_id in pointers {
            if let Some(entry) = self.pending_moves.remove(&pointer_id) {
                drained.push(entry);
            }
        }

        for (pointer_id, event) in drained {
            // Use cached hit test result
            if let Some(result) = self.hit_tests.get(&pointer_id) {
                self.dispatch_event(&event, &result);
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
        self.hit_tests.get(&pointer_id).map(|r| r.clone())
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
            self.hit_tests.clear();
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

    // ========================================================================
    // Internal Methods
    // ========================================================================

    /// Extract pointer ID from event.
    #[inline]
    fn extract_pointer_id(&self, event: &PointerEvent) -> PointerId {
        crate::events::extract_pointer_id(event)
    }

    /// Dispatch event to hit test targets.
    fn dispatch_event(&self, event: &PointerEvent, result: &HitTestResult) {
        // Route the event through the pointer router
        self.pointer_router.route(event);

        // Also dispatch to hit test entries with handlers
        for entry in result.path() {
            if let Some(ref handler) = entry.handler {
                handler(event);
            }
        }
    }
}

impl std::fmt::Debug for GestureBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureBinding")
            .field("active_pointers", &self.hit_tests.len())
            .field("pending_moves", &self.pending_moves.len())
            .field("arena_count", &self.arena.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_foundation::HasInstance;

    use super::*;
    use crate::events::{make_down_event, make_move_event, make_up_event};

    #[test]
    fn test_binding_singleton() {
        let binding1 = GestureBinding::instance();
        let binding2 = GestureBinding::instance();

        // Should be the same instance
        assert!(std::ptr::eq(binding1, binding2));
    }

    #[test]
    fn test_binding_is_initialized() {
        // Ensure instance exists
        let _ = GestureBinding::instance();

        // Should be initialized
        assert!(GestureBinding::is_initialized());
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
    fn test_hit_test_cache() {
        let binding = GestureBinding::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");
        let result = HitTestResult::new();

        binding.hit_tests.insert(pointer, result.clone());
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
            HitTestResult::new(),
        );
        binding.hit_tests.insert(
            PointerId::new(3).expect("nonzero pointer id"),
            HitTestResult::new(),
        );
        binding.hit_tests.insert(
            PointerId::new(4).expect("nonzero pointer id"),
            HitTestResult::new(),
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
            binding.hit_tests.insert(id, HitTestResult::new());
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
            .map(|r| r.is_tracked())
            .unwrap_or(false);
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
}
