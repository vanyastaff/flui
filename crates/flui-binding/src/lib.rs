//! # `flui_binding`
//!
//! A deterministic, **non-singleton** headless frame driver for FLUI.
//!
//! [`HeadlessBinding`] owns a virtual [`ManualClock`] and a clock-bound
//! [`GestureArena`], and advances time one frame at a time via
//! [`HeadlessBinding::pump_frame`]. It is the FLUI-native equivalent of Flutter's
//! `TestWidgetsFlutterBinding.pump(dt)`: every deadline-driven gesture (long-press,
//! and the press-delay of double-tap) is driven off a single virtual timeline, so
//! tests are deterministic with **no wall-clock `thread::sleep`**.
//!
//! Unlike Flutter's `WidgetsFlutterBinding` (and FLUI's `RenderingFlutterBinding`),
//! this binding is an ordinary instantiable value, not a process global — many can
//! exist at once, so test suites run in parallel without contending on shared
//! singleton state.
//!
//! ## Scope (Phase 1)
//!
//! `pump_frame` advances the virtual clock and fires due gesture deadlines. Two
//! steps are deliberately deferred — *deferred, not stubbed*; nothing fake runs in
//! their place, and the frame body marks exactly where each slots in:
//!
//! - **Animation controllers (Phase 3).** Advancing registered
//!   [`AnimationController`](flui_animation::AnimationController)s each frame needs a
//!   restart-aware run-epoch model (a controller re-zeros its run on
//!   `forward()`/`reverse()`, so a fixed registration epoch desyncs on a second
//!   run). That belongs with the view-layer `TickerProvider` work, not here.
//! - **Tree rebuilds (Phase 1b).** Driving `BuildOwner::build_scope` +
//!   `PipelineOwner::run_frame` needs a mounted `ElementTree`/`PipelineOwner` this
//!   binding does not yet own.
//!
//! ## Example
//!
//! ```
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicBool, Ordering};
//! use std::time::Duration;
//!
//! use flui_binding::HeadlessBinding;
//! use flui_interaction::settings::GestureSettings;
//! use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, PointerId};
//! use flui_types::Offset;
//! use flui_types::geometry::px;
//!
//! let mut binding = HeadlessBinding::new();
//!
//! let fired = Arc::new(AtomicBool::new(false));
//! let in_callback = Arc::clone(&fired);
//! let recognizer = LongPressGestureRecognizer::with_settings(
//!     binding.arena().clone(),
//!     GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
//! )
//! .with_on_long_press_start(move |_details| in_callback.store(true, Ordering::SeqCst));
//!
//! recognizer.add_pointer(PointerId::new(1).unwrap(), Offset::new(px(10.0), px(10.0)));
//!
//! // 300ms of virtual time — the 500ms deadline has not elapsed.
//! binding.pump_frame(Duration::from_millis(300));
//! assert!(!fired.load(Ordering::SeqCst));
//!
//! // Crossing 500ms fires the deadline inside the frame, deterministically.
//! binding.pump_frame(Duration::from_millis(300));
//! assert!(fired.load(Ordering::SeqCst));
//! ```

use std::sync::Arc;
use std::time::Duration;

use flui_interaction::arena::GestureArena;
use flui_interaction::{ManualClock, MonotonicClock};

/// A deterministic, non-singleton headless frame driver.
///
/// Owns the single virtual time authority ([`ManualClock`]) and a clock-bound
/// [`GestureArena`] whose deadline checks read that clock. Drive it with
/// [`pump_frame`](Self::pump_frame).
#[derive(Debug)]
pub struct HeadlessBinding {
    /// The single virtual time authority. Every time-based read flows from here.
    clock: ManualClock,
    /// The shared, clock-bound arena. Deadline-driven recognizers added to it (via
    /// [`arena`](Self::arena)) resolve against the virtual clock.
    arena: GestureArena,
}

impl HeadlessBinding {
    /// Create a headless binding with a fresh virtual clock and a clock-bound
    /// gesture arena.
    ///
    /// The arena is built via `GestureArena::with_clock(Arc::new(clock.clone()))`,
    /// so the arena and the binding observe the *same* virtual timeline (the
    /// clock's elapsed counter is `Arc`-backed and shared across clones).
    #[must_use]
    pub fn new() -> Self {
        let clock = ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()) as Arc<dyn MonotonicClock>);
        Self { clock, arena }
    }

    /// The shared, clock-bound gesture arena.
    ///
    /// Add a deadline-driven recognizer to the same virtual timeline the frame
    /// driver polls by constructing it against `binding.arena().clone()` (the
    /// arena's entries are `Arc`-backed, so the clone shares them).
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// The virtual clock this binding advances each frame.
    ///
    /// Exposed for inspection (`now()` / `elapsed()`). Prefer
    /// [`pump_frame`](Self::pump_frame) to move time forward, so the per-frame
    /// ordering below is honored.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.clock
    }

    /// Advance one deterministic frame by `dt`.
    ///
    /// # Ordering
    ///
    /// The steps mirror Flutter's `TestWidgetsFlutterBinding.pump(dt)`
    /// (`fakeAsync.elapse(dt)` → `handleBeginFrame` → `handleDrawFrame`), and the
    /// order is load-bearing:
    ///
    /// 1. **Advance the virtual clock.** Everything time-based reads from here, so
    ///    the new instant must be visible before anything observes it — the
    ///    analogue of `fakeAsync.elapse(dt)`.
    /// 2. **Fire gesture deadlines** at the new time. Flutter fires due `Timer`s
    ///    inside `elapse`, *ahead* of `handleBeginFrame`; a deadline (e.g. a
    ///    long-press) that has now elapsed resolves here, before any later frame
    ///    work — so the deadline poll is the first thing after the clock moves.
    ///
    /// The invariant this order protects, as later steps land: everything that can
    /// dirty the tree (deadline callbacks, and — Phase 3 — controller ticks) runs
    /// *before* the tree is rebuilt.
    ///
    /// # Deferred steps
    ///
    /// After the deadline poll, two steps slot in (see the crate docs); they are
    /// *deferred, not stubbed*:
    /// 3. **Animation controllers (Phase 3)** — tick registered controllers on the
    ///    virtual timeline (`AnimationController::tick_at`), with a restart-aware
    ///    run epoch.
    /// 4. **Tree rebuild (Phase 1b)** — `BuildOwner::build_scope` (drains the inbox
    ///    the callbacks above fill) then `PipelineOwner::run_frame`.
    pub fn pump_frame(&mut self, dt: Duration) {
        // 1. Advance the virtual clock. Every subsequent read sees the new instant.
        self.clock.advance(dt);

        // 2. Fire gesture deadlines at the NEW time. A long-press deadline that has
        //    now elapsed fires here, inside the frame.
        self.arena.poll_deadlines();

        // 3. Phase 3 — tick registered animation controllers (deferred).
        // 4. Phase 1b — a mounted tree's build_scope + run_frame (deferred).
    }
}

impl Default for HeadlessBinding {
    fn default() -> Self {
        Self::new()
    }
}
