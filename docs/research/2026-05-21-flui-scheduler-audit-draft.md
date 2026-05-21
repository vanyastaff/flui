---
title: "Mythos Audit — flui-scheduler (draft)"
date: 2026-05-21
status: audit-draft
audit_methodology: claude-mythos (12-phase rust audit, frame-loop layer pass)
crates_audited:
  - flui-scheduler
reference_sources:
  - flutter/packages/flutter/lib/src/scheduler/binding.dart
  - flutter/packages/flutter/lib/src/scheduler/ticker.dart
  - flutter/packages/flutter/lib/src/scheduler/priority.dart
  - flutter/packages/flutter/lib/src/scheduler/debug.dart
  - flutter/packages/flutter/lib/src/scheduler/service_extensions.dart
authors:
  - Mythos (via Claude Opus 4.7)
---

# Mythos Audit: `flui-scheduler`

> Single-pass deep audit of the FLUI frame-loop crate, followed by cross-reference against Flutter `scheduler/` (5 .dart files, ≈ 2,700 LOC).
>
> Goal: identify zombie abstractions, half-implemented FSM transitions, lifecycle leaks, lock contention hot-spots, and Flutter-parity gaps — without breaking active integration with the rest of the workspace.

---

## Table of Contents

- [Part I — Self-Audit Findings](#part-i--self-audit-findings)
  - [Mythos Improvement Verdict](#mythos-improvement-verdict)
  - [Project Map](#project-map)
  - [Findings](#findings)
  - [Dead Code Table](#dead-code-table)
  - [Restructuring Plan](#restructuring-plan)
  - [Optimization Plan](#optimization-plan)
  - [What to Preserve](#what-to-preserve)
  - [Priority Order (initial)](#priority-order-initial)
- [Part II — Flutter Cross-Reference](#part-ii--flutter-cross-reference)
  - [Section 2 — flui-scheduler vs scheduler/](#section-2--flui-scheduler-vs-flutter-scheduler)
- [Appendix A — Investigation Trail](#appendix-a--investigation-trail)

---

# Part I — Self-Audit Findings

## Mythos Improvement Verdict

`flui-scheduler` is **API-bloated and FSM-divergent** but its frame-loop core (`handle_begin_frame` + `handle_draw_frame` + the SchedulerPhase enum) is competently structured. The Flutter-equivalent skeleton — `SchedulerPhase` (Idle / TransientCallbacks / MidFrameMicrotasks / PersistentCallbacks / PostFrameCallbacks), `handleBeginFrame` → `handleDrawFrame` ordering, `addPostFrameCallback`/`addPersistentFrameCallback`/`scheduleFrameCallback` triple, performance-mode request handle with Drop cleanup, time dilation atomic, epoch reset — all match Flutter's `scheduler/binding.dart`. The phase transition validation (`SchedulerPhase::can_transition_to`) is a Rust-native improvement over Flutter's `assert(_schedulerPhase == ...)`.

**Worst complexity tax**: **Ticker is a parallel reimplementation, not a port**. FLUI ships *three* Ticker shapes: `Ticker` (manual tick), `ScheduledTicker` (auto-scheduling, Flutter-aligned), and `TypestateTicker<Idle/Active/Muted/Stopped>` (compile-time FSM). Flutter has one `Ticker`. None of FLUI's three match Flutter's API: Flutter's `Ticker.start()` **returns `TickerFuture`** and **throws if already active** ([ticker.dart:185-208](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)); FLUI's `Ticker::start` returns `()` and silently overwrites state. Flutter has no `Stopped` state and no `Idle`/`Active` distinct types — it has `_future: Option<TickerFuture>` and a `muted` flag. FLUI's TypestateTicker has 0 workspace consumers; the runtime `Ticker` is what `flui-animation` (disabled) imports. The `TickerProvider` trait is misshaped — Flutter's signature is `Ticker createTicker(TickerCallback)`, FLUI's is `schedule_tick(Box<dyn FnOnce(f64) + Send>)` which schedules ONE callback rather than vending a Ticker.

**Where dead code hides**: the entire `prelude_advanced` surface — `TypestateTicker<Active>/<Idle>/<Muted>/<Stopped>` (392 LOC), `UserInputPriority`/`AnimationPriority`/`BuildPriority`/`IdlePriority` ZSTs with `PriorityLevel` sealed trait (~70 LOC declarative), `TypedTask<P>` (75 LOC), `FrameHandle`/`TaskHandle` (Handle<Marker> 110 LOC) — sum ≈ 650 LOC across `typestate.rs`, `id.rs`, `task.rs`, `traits.rs`. **Zero workspace consumers outside flui-scheduler's own tests**. Extension traits `PriorityExt`/`FrameBudgetExt`/`FrameTimingExt` (~150 LOC in `traits.rs`) — same: 0 external consumers. `ToMilliseconds`/`ToSeconds` conversion traits (~30 LOC): 0 external consumers. Two preludes (`prelude` + `prelude_advanced`): no production code imports `prelude_advanced` (grep confirmed). `Scheduler::arc_instance()` (a static `OnceLock<Arc<Scheduler>>` *parallel* singleton to `BindingBase::instance()`): 0 callers outside `flui-scheduler/src/scheduler.rs` itself. `VsyncDrivenScheduler` (134 LOC of vsync.rs): 0 production consumers; the `Scheduler::set_vsync(VsyncScheduler)` path covers the same use case.

**Flutter-divergence cluster**: Flutter's `addPersistentFrameCallback`/`addPostFrameCallback` are **explicitly non-removable** ([binding.dart:773 "cannot be unregistered"](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart), [binding.dart:802 "called exactly once"](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)). FLUI added `remove_persistent_frame_callback` + `cancel_post_frame_callback` returning CallbackIds — Rust-native ergonomic but **not Flutter behavior**. Flutter's Priority is an open class with `idle=0`, `animation=100000`, `touch=200000` + operator overloading for `+offset`/`-offset` ([priority.dart:11-54](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/priority.dart)); FLUI's `Priority::{Idle, Build, Animation, UserInput}` is a closed 4-variant enum — inserted `Build` between Idle/Animation, renamed `touch` → `UserInput`, lost relative priority. Flutter doesn't have FLUI's `lifecycle_state_listeners` registry — Flutter's `handleAppLifecycleStateChanged` only updates the binding state ([binding.dart:414-441](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)); listeners come via `WidgetsBindingObserver.didChangeAppLifecycleState`, not the scheduler.

**Principle 6 violations**: `SchedulerPhase::from_u8`, `FrameSkipPolicy::from_u8`, `AppLifecycleState::from_u8` all `panic!` on invalid value, called via raw atomic loads in scheduler.rs production paths ([:496, :501, :1035, :1065, :1111, :1140](../../crates/flui-scheduler/src/scheduler.rs)). `VsyncScheduler::new(0)` asserts panic ([vsync.rs:167](../../crates/flui-scheduler/src/vsync.rs)). `FrameDuration::from_fps(0)` asserts panic ([duration.rs:513](../../crates/flui-scheduler/src/duration.rs)). `set_time_dilation` asserts panic on non-positive ([config.rs:97](../../crates/flui-scheduler/src/config.rs)). `Microseconds::to_std_duration` panics on negative — `Microseconds` is `i64` allowing negative values, but every production constructor uses `u128 as i64` or positive constants → use of `i64` itself is the misshape (should be `u64`).

**Ticker lifecycle leaks**: `Ticker` has **no `Drop` impl, no `dispose()`, no disposed-state assertion**. Flutter's `Ticker.dispose()` ([ticker.dart:363-379](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)) is `@mustCallSuper`, cancels the pending TickerFuture via `_cancel()`, unschedules the tick callback, leaves `_startTime = Duration.zero` as a weak use-after-dispose signal. PR #84 added disposed-state assertions to `ChangeNotifier` ([notifier.rs](../../crates/flui-foundation/src/notifier.rs)) — same pattern is **missing here**. `Ticker::start` silently overwrites prior state instead of panicking on active-restart (Flutter's mandatory `throw FlutterError('A ticker was started twice.')` at [ticker.dart:188-194](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)).

**Lock contention surface**: `TaskQueue` is `Arc<Mutex<BinaryHeap<PriorityTask>>>` ([task.rs:335](../../crates/flui-scheduler/src/task.rs)) — every `add()` from any thread takes the lock; per-frame `execute_until` drains under one lock acquisition (good), but the `is_empty`/`len`/`peek_priority`/`count_by_priority` getters all lock for trivial reads. `Ticker::state()`/`is_muted()` similarly lock for single-field reads ([ticker.rs:307-320](../../crates/flui-scheduler/src/ticker.rs)) — should be `AtomicU8`. `ScheduledTicker`'s `tick_and_reschedule` chain re-locks the inner Mutex 3× per tick + creates a fresh closure capturing `Arc<Mutex<>>` + `Arc<Scheduler>` every frame ([ticker.rs:772-822](../../crates/flui-scheduler/src/ticker.rs)) — single largest per-frame allocation hot path in this crate.

**Biggest LOC win**: collapse `prelude_advanced` + `TypestateTicker` + `Handle<M>` (+`FrameHandle`/`TaskHandle`) + `VsyncDrivenScheduler` + extension trait ZSTs to `pub(crate)` or delete; consolidate the 3 Ticker variants to 1; drop the parallel `arc_instance()` singleton. Estimated reduction: ≈ 1,500 LOC (≈ 17% of 9,064 LOC).

**Don't touch**: `SchedulerPhase` + `can_transition_to` validation, `handle_begin_frame`/`handle_draw_frame` phase orchestration, `BindingBase` + `impl_binding_singleton!` integration, `PerformanceModeRequestHandle` (Drop cleanup matches Flutter's `dispose()` semantics), `FrameCompletionFuture` (correct Future impl with waker storage + lock-protected state), the consolidated `FrameState`/`CallbackState`/`BindingState` struct grouping (single Arc each, vs Flutter's 30+ scattered fields on the binding mixin), `Priority::ALL`/`PriorityCount` ergonomics, `BudgetPolicy::more_restrictive`/`less_restrictive` navigation, the type-safe duration newtypes (`Milliseconds`/`Seconds`/`Microseconds`/`Percentage`/`FrameDuration`) — these earn their place over `f64`-everywhere via Constitution Principle 4.

---

## Project Map

```text
flui-scheduler (9,064 LOC, 12 source files + 4 examples + 1 integration test)
  owns: Scheduler (singleton via BindingBase; FrameState/CallbackState/BindingState
        triplets of Arc-wrapped containers), SchedulerPhase enum (5 variants matching
        Flutter), AppLifecycleState enum (5 variants matching Flutter), FramePhase enum
        (5 variants — Build/Layout/Paint/Composite/Idle — for inside-PersistentCallbacks
        sub-phase tracking), Ticker + TickerProvider + TickerCanceled + TickerFuture +
        TickerFutureOrCancel + TickerGroup + ScheduledTicker + TypestateTicker<Idle/
        Active/Muted/Stopped> (3 parallel Ticker impls — one runtime FSM, one
        auto-scheduling, one compile-time typestate), TaskQueue (BinaryHeap<PriorityTask>
        behind Arc<Mutex>) + Task + TypedTask<P> + Priority enum + PriorityLevel sealed
        trait + 4 ZST priority types (UserInputPriority/AnimationPriority/BuildPriority/
        IdlePriority), FrameBudget + PhaseStats + AllPhaseStats + BudgetPolicy +
        FrameBudgetBuilder + SharedBudget alias, VsyncScheduler + VsyncDrivenScheduler +
        VsyncMode + VsyncStats + VsyncCallback, FrameSkipPolicy enum + frame-skip
        accounting, FrameCompletionFuture (lock+waker Future), FrameTiming +
        FrameTimingBuilder, time_dilation atomic + set_time_dilation + epoch reset,
        PerformanceMode enum + PerformanceModeRequestHandle (Drop cleanup),
        TimingsCallback + report_timings batching, SchedulingStrategy callback,
        IdGenerator<M> + Handle<M> + FrameHandle/TaskHandle aliases,
        Milliseconds/Seconds/Microseconds/Percentage/FrameDuration newtypes (`duration.rs`),
        PriorityExt/FrameBudgetExt/FrameTimingExt extension traits + ToMilliseconds/ToSeconds
        conversion traits, SERVICE_EXT_TIME_DILATION const, two preludes (prelude +
        prelude_advanced), parallel arc_instance() singleton via OnceLock<Arc<Scheduler>>
  depends on: flui-foundation (BindingBase, HasInstance, impl_binding_singleton!,
              markers, Id<M>, FrameCallbackId, FrameId, TaskId, TickerId, Identifier),
              parking_lot, dashmap, crossbeam, event-listener, tracing, web-time
  public surface: ~75 top-level + ~25 prelude + ~14 prelude_advanced exports
  suspected hot paths: Scheduler::handle_begin_frame (lock × 4 — current_vsync_time,
                       frame_duration, current_frame, transient callbacks Vec drain),
                       Scheduler::handle_draw_frame (lock × ~10 across frame state,
                       persistent/post-frame callbacks, pending_timings, budget),
                       TaskQueue::add (lock per push from any thread),
                       ScheduledTicker::tick_and_reschedule (3 lock acquisitions +
                       Box allocation per tick + Arc clones)
  risk: TypestateTicker + Handle<M>/FrameHandle/TaskHandle + VsyncDrivenScheduler +
        prelude_advanced + extension trait ZSTs + Scheduler::arc_instance() — all have
        ZERO workspace consumers. Three Ticker shapes (Ticker + ScheduledTicker +
        TypestateTicker) where Flutter has one. TickerProvider signature
        (`schedule_tick(callback)`) misshapes Flutter's pattern
        (`createTicker(callback) -> Ticker`). No Ticker::dispose() — lifecycle leak.
        Several `from_u8` panic paths called from production atomic-load round-trips.
        Three `from_u8(value).unwrap_or_else(panic!())` patterns instead of saturating
        to default. `arc_instance()` static `Arc<Scheduler>` parallel to BindingBase
        singleton — silent dual-state risk.
```

**Cross-crate dependency state** (clean):

```
flui-foundation → (nothing in scheduler scope)
flui-scheduler → flui-foundation only
```

No circular deps. The crate is at the right layer — between foundation and rendering. `flui-app` and `flui-animation` (disabled) consume Scheduler.

---

## Findings

### 💀 [DUPLICATION | CRITICAL]: Three parallel Ticker implementations — `Ticker` + `ScheduledTicker` + `TypestateTicker<S>`

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs:176-377`](../../crates/flui-scheduler/src/ticker.rs) — `Ticker` (TickerInner: `state: TickerState, start_time: Option<Instant>, callback: Option<TickerCallback>, muted_elapsed: Seconds`) — manual `tick(&self, provider: &T)` per frame. Runtime `TickerState::{Idle, Active, Muted, Stopped}` enum.
- [`crates/flui-scheduler/src/ticker.rs:610-823`](../../crates/flui-scheduler/src/ticker.rs) — `ScheduledTicker` (ScheduledTickerInner same fields + `scheduled: bool`) — auto-registers transient callback via `Arc<Scheduler>::schedule_frame_callback` in `tick_and_reschedule`. Flutter-aligned.
- [`crates/flui-scheduler/src/typestate.rs:148-306`](../../crates/flui-scheduler/src/typestate.rs) — `TypestateTicker<State: TickerState>` (TickerData: `start_time, callback, muted_elapsed`) — compile-time FSM via `Idle`/`Active`/`Muted`/`Stopped` phantom-type markers.
- Flutter has **one** `Ticker` ([ticker.dart:78](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)) with **no `Stopped` state** — Flutter uses `_future: Option<TickerFuture>` (None ⇔ inactive) + `muted: bool` flag.
- Grep confirms only `Ticker` + `TickerProvider` are consumed externally — by `flui-animation` (disabled) at [`crates/flui-animation/src/lib.rs:138`](../../crates/flui-animation/src/lib.rs). `TypestateTicker` and `ScheduledTicker`: **0 workspace consumers** outside flui-scheduler's own tests and examples.

**Why it exists:**
The crate started as a Rust-idiomatic redesign of Ticker. `TypestateTicker` was added to demonstrate the typestate pattern (lib.rs:56-65 doc example). `ScheduledTicker` was added later when the Flutter-parity behavior (auto-scheduling, returning a TickerFuture-like) was needed but `Ticker` already had a different API surface (`tick(&self, provider)`). Neither was migrated.

**Cost today:**
- ~1,400 LOC across the three Ticker shapes (Ticker 380 + ScheduledTicker 214 + TypestateTicker 306 + TickerGroup 134 = ~1,034 production LOC, + ~370 LOC tests/docs).
- API confusion — flui-animation's docs reference `Ticker`, but the Flutter-aligned auto-scheduling impl is `ScheduledTicker`; a new user can't tell which to pick.
- `TickerState` is duplicated: runtime enum (`ticker.rs:104`) vs typestate markers (`typestate.rs:41-53`). They share zero code.
- `TypestateTicker` doesn't accept callbacks that can be `Send` boundary-safely transferred between states — `data: Arc<Mutex<TickerData>>` survives transitions but the by-value typestate method signatures (`fn start(self) -> TypestateTicker<Active>`) don't compose with Slab storage or registry patterns.

**Risk of changing:**
Medium. `Ticker` + `TickerProvider` are imported by `flui-animation` (currently disabled in workspace `Cargo.toml`). Renaming/removing `Ticker` would ripple into `flui-animation/src/lib.rs:138, controller.rs, error.rs`. The right consolidation:
1. Keep `Ticker` as the single runtime FSM Ticker (the auto-scheduling behavior of `ScheduledTicker` is what Flutter actually uses).
2. Delete `ScheduledTicker` entirely — merge auto-scheduling into `Ticker::start` (gated by a `provider: Arc<dyn TickerProvider>` constructor parameter, matching Flutter's `TickerProvider.createTicker`).
3. Delete `TypestateTicker<S>` entirely — 0 consumers, can be re-introduced if a real use case emerges.
4. Drop `Stopped` state — Flutter has none, `Idle ⇔ no _future`.

**Recommendation:** **delete** `typestate.rs` (392 LOC including tests). **Merge** `ScheduledTicker`'s auto-scheduling behavior into `Ticker::start` by parameterizing on `Arc<dyn TickerProvider>` at construction (matches Flutter's `Ticker(this._onTick, ...)` + `TickerProvider.createTicker`). After merge, drop `ScheduledTicker`. Estimated net deletion: ~600 LOC.

---

### 💀 [FSM-DRIFT | CRITICAL]: `Ticker::start` silently overwrites active state; Flutter throws

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs:207-216`](../../crates/flui-scheduler/src/ticker.rs):
  ```rust
  pub fn start<F>(&mut self, callback: F) where F: FnMut(f64) + Send + 'static {
      let mut inner = self.inner.lock();
      inner.state = TickerState::Active;
      inner.start_time = Some(Instant::now());
      inner.callback = Some(Box::new(callback));
      inner.muted_elapsed = Seconds::ZERO;
  }
  ```
- No assertion on `inner.state == TickerState::Idle | Stopped`. Calling `ticker.start(...)` on an already-active ticker silently replaces the callback and resets `start_time`.
- Flutter ([`ticker.dart:186-197`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)):
  ```dart
  TickerFuture start() {
    assert(() {
      if (isActive) {
        throw FlutterError.fromParts(<DiagnosticsNode>[
          ErrorSummary('A ticker was started twice.'),
          ...
        ]);
      }
      return true;
    }());
    ...
  }
  ```
- `ScheduledTicker::start` ([ticker.rs:647-663](../../crates/flui-scheduler/src/ticker.rs)) has the same bug — no assertion against already-active state.
- `TypestateTicker` is exempt because the typestate forbids `start()` outside `<Idle>` at compile time.

**Why it exists:**
Naïve port. Flutter's assertion is debug-only (`assert(() { ... }())` pattern) and easy to miss when porting.

**Cost today:**
- Latent correctness bug — animation controllers that accidentally call `start()` twice (e.g., in a `setState` callback that re-fires from a parent rebuild) will silently drop their old TickerFuture and reset elapsed time. No error, animation jumps.
- Diverges from Flutter Ticker semantics that AnimationController and CurvedAnimation depend on.
- Flutter's "started twice" assertion is **load-bearing** — it's specifically called out in [ticker.dart:186-197](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart) with diagnostic context.

**Risk of changing:**
Low. Adding `debug_assert!(matches!(inner.state, TickerState::Idle | TickerState::Stopped), "Ticker::start called on active ticker")` is a 1-line fix with no behavior change in release.

**Recommendation:** add `debug_assert!` in both `Ticker::start` and `ScheduledTicker::start` paired with a `tracing::error!` log. If a real test depends on the silent-replace behavior, fix the test — Flutter doesn't allow it.

**Patch sketch:**
```rust
pub fn start<F>(&mut self, callback: F) where F: FnMut(f64) + Send + 'static {
    let mut inner = self.inner.lock();
    debug_assert!(
        !matches!(inner.state, TickerState::Active | TickerState::Muted),
        "Ticker::start called while ticker is {:?}; must stop() or reset() first",
        inner.state
    );
    inner.state = TickerState::Active;
    inner.start_time = Some(Instant::now());
    inner.callback = Some(Box::new(callback));
    inner.muted_elapsed = Seconds::ZERO;
}
```

---

### 💀 [LIFECYCLE-LEAK | CRITICAL]: `Ticker` has no `dispose()`, no `Drop`, no disposed-state assert (PR #84 pattern missing)

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs) — `Ticker` struct (line 176-355): no `impl Drop for Ticker`, no `dispose()` method, no `disposed: AtomicBool`. The doc-comment at line 363-367 explicitly says "Ticker intentionally does NOT implement Clone" but says nothing about disposal.
- [`crates/flui-scheduler/src/ticker.rs:610-823`](../../crates/flui-scheduler/src/ticker.rs) — `ScheduledTicker`: same. No `Drop`, no `dispose()`, no disposed assertion. The only state that gets cleared is `inner.callback = None` and `inner.scheduled = false` on `stop()` (line 678).
- Flutter ([ticker.dart:362-379](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)):
  ```dart
  @mustCallSuper
  void dispose() {
    assert(debugMaybeDispatchDisposed(this));
    if (_future != null) {
      final TickerFuture localFuture = _future!;
      _future = null;
      assert(!isActive);
      unscheduleTick();
      localFuture._cancel(this);  // ← cancel future so awaiters see TickerCanceled
    }
    assert(() {
      _startTime = Duration.zero;  // ← weak use-after-dispose marker
      return true;
    }());
  }
  ```
- PR #84 (commit `b0c914bf` per [view-tree-foundation-audit Combined Priority Order](2026-05-21-view-tree-foundation-audit.md#combined-mythos-insight)) added `ChangeNotifier::dispose()` + `disposed: AtomicBool` + `debug_assert!(!self.is_disposed())` on `add_listener`/`remove_listener`/`notify_listeners`. **The same pattern is missing in Ticker.**
- `ScheduledTicker` is worse: its `tick_and_reschedule` closure ([ticker.rs:761-763](../../crates/flui-scheduler/src/ticker.rs)) captures `Arc<Mutex<ScheduledTickerInner>>` + `Arc<Scheduler>` and schedules itself with the Scheduler. If the `ScheduledTicker` is dropped while the closure is queued in `Scheduler::transient` callbacks, the closure will still fire — and resolve its capture of `inner`, find `state != Active` (because nothing transitions on drop), return early. But the `Arc<Scheduler>` capture keeps the scheduler alive longer than needed.

**Why it exists:**
Drop was deferred. The Flutter `mustCallSuper` pattern doesn't translate to Rust's automatic Drop directly — needs a manual `Drop` impl, or a `dispose()` method, or both. None were written.

**Cost today:**
- Awaiters of `ScheduledTicker`'s future (via the missing TickerFuture integration — see next finding) cannot detect cancellation.
- A `ScheduledTicker` dropped while a frame callback is queued will silently no-op on next frame instead of cleaning up.
- Inconsistent with PR #84's recently-established workspace-wide disposal pattern (`ChangeNotifier::dispose` + disposed-state assert). The scheduler crate is the obvious next domain to apply the same pattern.
- Constitution-level concern: no use-after-dispose detection → silent bugs.

**Risk of changing:**
Medium. Adding `Drop` impl + `dispose()` + `disposed: AtomicBool` is straightforward; the ripple is into `ScheduledTicker`'s frame-callback closure, which needs to check the disposed flag and early-return without re-scheduling. Estimated change: ~80 LOC across `ticker.rs`.

**Recommendation:**
1. Add `disposed: AtomicBool` to `TickerInner` + `ScheduledTickerInner`.
2. Add `Ticker::dispose(&mut self)` and `ScheduledTicker::dispose(&mut self)` matching Flutter's semantics — clear callback, set disposed=true, set start_time=None (Flutter sets to zero as marker, but Rust `Option::None` is cleaner).
3. Add `impl Drop for Ticker` and `impl Drop for ScheduledTicker` that call `dispose()` if not already disposed.
4. Add `debug_assert!(!inner.disposed.load(Ordering::Acquire), "Ticker::start/tick/mute called after dispose")` to `start`, `tick`, `mute`, `unmute`, `reset`, `state`.
5. In `ScheduledTicker::tick_and_reschedule`, check disposed flag and early-return without re-scheduling if true.

---

### 💀 [API-SHAPE | CRITICAL]: `TickerProvider::schedule_tick` is not Flutter's `createTicker` — different shape, different lifecycle

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs:80-98`](../../crates/flui-scheduler/src/ticker.rs):
  ```rust
  pub trait TickerProvider: Send + Sync {
      fn schedule_tick(&self, callback: Box<dyn FnOnce(f64) + Send>);
      fn schedule_tick_typed(&self, callback: Box<dyn FnOnce(Seconds) + Send>) { ... }
  }
  ```
- Flutter ([ticker.dart:43-53](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)):
  ```dart
  abstract class TickerProvider {
    @factory
    Ticker createTicker(TickerCallback onTick);
  }
  ```
- FLUI's signature schedules **one one-shot callback**. Flutter's signature **vends a Ticker** that the consumer holds, starts, stops, disposes.
- Consumer in Flutter — `State` mixin (e.g., `SingleTickerProviderStateMixin`) — owns the Ticker, calls `dispose()` from its own `dispose()` method. The TickerProvider doesn't track tickers it created.
- `Scheduler` implements `TickerProvider` ([scheduler.rs:1559-1570](../../crates/flui-scheduler/src/scheduler.rs)) by forwarding `schedule_tick` to `schedule_frame_callback`. This makes Scheduler a "schedule one callback for next frame" provider, not a "Ticker factory".
- `ScheduledTicker::new` ([ticker.rs:623-635](../../crates/flui-scheduler/src/ticker.rs)) accepts `Arc<Scheduler>` directly, bypassing the `TickerProvider` trait entirely. The trait is **decoupled from the only Flutter-aligned Ticker implementation**.

**Why it exists:**
Misinterpretation of "TickerProvider" — the FLUI author read it as "provides tick callbacks" (FnOnce per frame) rather than "provides Ticker objects" (factory).

**Cost today:**
- Animation crate (disabled) imports the trait but the shape doesn't match Flutter's `TickerProvider` semantics. If `flui-animation` re-enables and tries to implement `SingleTickerProviderStateMixin`-equivalent, the trait won't fit — animations need a `Ticker`, not a single `FnOnce`.
- The trait method has no way to return a handle for cancellation — `schedule_tick` returns `()`. Flutter's `createTicker` returns the Ticker, which the consumer can stop/dispose.
- `Scheduler::schedule_tick` is currently used only by `Scheduler`'s own `impl TickerProvider` ([scheduler.rs:1560](../../crates/flui-scheduler/src/scheduler.rs)) which forwards to `schedule_frame_callback`. The trait method has effectively one implementor and one caller — both inside flui-scheduler.

**Risk of changing:**
Medium. The trait is `pub` and re-exported through both preludes. Renaming/reshaping is a breaking change. But the only external consumer is `flui-animation` (disabled), so the ripple is contained.

**Recommendation:**
Rename + reshape:
```rust
pub trait TickerProvider: Send + Sync {
    /// Vend a Ticker bound to this provider's frame schedule.
    /// Matches Flutter's `TickerProvider.createTicker(onTick)`.
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker;
}

impl TickerProvider for Scheduler {
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker {
        Ticker::new_with_provider(Arc::new(self.clone()), on_tick)
    }
}
```
Drop the current `schedule_tick`/`schedule_tick_typed`. Update `Ticker::new` to accept an optional `Arc<dyn TickerProvider>` for auto-scheduling (absorbing `ScheduledTicker`'s role per the consolidation in [Finding #1](#-duplication--critical-three-parallel-ticker-implementations--ticker--scheduledticker--typestatetickers)).

---

### 💀 [ZOMBIE | HIGH]: `prelude_advanced` — 14 exports, 0 production consumers

**Evidence:**
- [`crates/flui-scheduler/src/lib.rs:226-245`](../../crates/flui-scheduler/src/lib.rs) declares `pub mod prelude_advanced { ... }` re-exporting:
  - `AllPhaseStats`, `FrameBudgetBuilder`, `FrameTimingBuilder`, `PhaseStats`, `PriorityCount` (from prelude already)
  - `SchedulerBuilder`, `TickerGroup`, `VsyncMode`, `VsyncStats`
  - `id::{FrameHandle, Handle, Id, IdGenerator}`
  - `task::TypedTask`
  - `traits::{AnimationPriority, BuildPriority, IdlePriority, PriorityLevel, UserInputPriority}`
  - `typestate::{Active, Idle, Muted, Stopped, TickerState, TypestateTicker}`
- Grep `use flui_scheduler::prelude_advanced` workspace-wide: **0 hits** outside flui-scheduler's own examples (`flui-scheduler/examples/animation_ticker.rs`, etc.).
- Even `flui-animation` (disabled) at [`crates/flui-animation/src/lib.rs:138, :166`](../../crates/flui-animation/src/lib.rs) only imports from the basic `prelude` + direct symbols, not from `prelude_advanced`.

**Why it exists:**
Showcase prelude — bundle "advanced" features (typestate, type-safe IDs, typed tasks) for power users. None of the included symbols are required at the prelude level; they have specific use cases that warrant direct imports.

**Cost today:**
- API surface inflation — `prelude_advanced` advertises that these features are core enough to warrant a prelude, but no production consumer uses any of them.
- Doc maintenance burden — every change to one of the included types requires considering the prelude_advanced export.
- Encourages users to import 14 unused symbols (autocomplete, IDE noise).

**Risk of changing:**
Low. Removing `prelude_advanced` (or merging its contents into `prelude` selectively) is non-breaking for any current workspace consumer (because nobody imports it). External users who rely on it would need to switch to direct imports — same ergonomics, less namespace pollution.

**Recommendation:**
**Delete** `prelude_advanced`. Merge `SchedulerBuilder`, `FrameBudgetBuilder`, `FrameTimingBuilder` (the builders genuinely paired with their primary types) into `prelude`. Drop the typestate exports + Handle exports + extension trait ZSTs — those should be direct imports per use case.

---

### 💀 [ZOMBIE | HIGH]: `TypestateTicker<Idle/Active/Muted/Stopped>` (392 LOC) — 0 production consumers, conflicts with Slab storage

**Evidence:**
- [`crates/flui-scheduler/src/typestate.rs`](../../crates/flui-scheduler/src/typestate.rs) — 392 LOC. `Idle`/`Active`/`Muted`/`Stopped` ZSTs, sealed `TickerState` trait, `TypestateTicker<S>` with by-value FSM transitions (`start(self) -> TypestateTicker<Active>`, `stop(self) -> TypestateTicker<Idle>`).
- Grep `TypestateTicker|typestate::` workspace-wide outside flui-scheduler/src: **0 hits**. The only usages are flui-scheduler's own test (`crates/flui-scheduler/src/typestate.rs:326-392`) and the lib.rs doc-example (`lib.rs:56-65`).
- `flui-animation` (the natural consumer) at [`crates/flui-animation/src/lib.rs`](../../crates/flui-animation/src/lib.rs): imports `Ticker`, not `TypestateTicker`.
- The by-value typestate signature `fn start(self) -> TypestateTicker<Active>` is **incompatible with Slab arena storage** — you can't move a Ticker out of a Slab to transition its state without invalidating the slot. Same pattern that killed `flui-tree::Mountable/Unmountable` in [view-tree-foundation-audit Finding #4](2026-05-21-view-tree-foundation-audit.md#-zombie--critical-flui-tree-typestate-machinery-mountableunmountablenodestate--zero-consumers-outside-lib).

**Why it exists:**
Demonstration of the typestate pattern — listed as "Advanced Type System Features" in lib.rs doc ([:52-65](../../crates/flui-scheduler/src/lib.rs)). Pure pedagogy in a production crate.

**Cost today:**
- 392 LOC of code + tests + doc-examples for unused machinery.
- Public API freeze — `TypestateTicker`, `Idle`, `Active`, `Muted`, `Stopped`, `TickerState` (the trait) are re-exported from `lib.rs:206-208` AND from `prelude_advanced`. Any change is breaking.
- Two `TickerState` types — runtime enum at [`ticker.rs:104`](../../crates/flui-scheduler/src/ticker.rs) and typestate sealed trait at [`typestate.rs:79`](../../crates/flui-scheduler/src/typestate.rs). The lib.rs re-export at [:207](../../crates/flui-scheduler/src/lib.rs) renames the typestate one to `TypestateTickerState` to avoid collision — clear sign two types are doing the same conceptual job.

**Risk of changing:**
Low. Zero workspace consumers. The doc-example in `lib.rs:56-65` can be rewritten to demonstrate the runtime FSM instead.

**Recommendation:**
**Delete** `typestate.rs` entirely. Remove all 6 exports from `lib.rs`. Remove the typestate doc-block from `lib.rs:52-65`. Estimated deletion: 392 LOC (production) + 60 LOC (lib.rs docs).

---

### 💀 [ZOMBIE | HIGH]: `Handle<M>`/`FrameHandle`/`TaskHandle` (~120 LOC) — 0 production consumers, slot-map machinery without a slot-map

**Evidence:**
- [`crates/flui-scheduler/src/id.rs:139-247`](../../crates/flui-scheduler/src/id.rs) — `Handle<M: Marker>` (index: u32, generation: u32) for slot-map references. `next_generation`/`pack`/`unpack`/Hash/Eq/Display/Debug impls.
- `FrameHandle = Handle<markers::Frame>` and `TaskHandle = Handle<markers::Task>` aliases.
- Grep `FrameHandle|TaskHandle|Handle::<` workspace-wide outside flui-scheduler/src: **0 hits**.
- **No slot-map exists in flui-scheduler.** The Scheduler stores callbacks in `Vec<CancellableTransientCallback>` (line 357), `Vec<CancellablePersistentCallback>` (line 365), `Vec<CancellablePostFrameCallback>` (line 367) — plain Vec with linear search via `retain()`. The `Handle<M>` pattern's whole purpose (detecting ABA via generation counter) is **for slot-map slot reuse**, which doesn't happen here.
- `CallbackId = FrameCallbackId` (`id.rs:48`) which is `flui_foundation::Id<markers::FrameCallback>` — already a NonZeroUsize ID. This is the actual cancellation identifier. `Handle<M>` provides nothing on top.

**Why it exists:**
Scaffold for a future slot-map-backed callback registry. Never materialized; current registries are Vec + linear retain.

**Cost today:**
- 120 LOC of trait impls + tests for unused machinery.
- Public API liability — `Handle`, `FrameHandle`, `TaskHandle` all in prelude_advanced.

**Risk of changing:**
Low. Zero consumers.

**Recommendation:**
**Delete** the entire Handle pattern section in `id.rs` (lines 131-247) including `Handle<M>` + `FrameHandle` + `TaskHandle` + their tests. Keep `IdGenerator<M>` (used internally for `CallbackId`) and the `flui_foundation` re-exports. Remove from `lib.rs:193, :207, :237`.

---

### 💀 [ZOMBIE | HIGH]: Four ZST priority types + `PriorityLevel` sealed trait — typed-task pattern unused

**Evidence:**
- [`crates/flui-scheduler/src/traits.rs:39-101`](../../crates/flui-scheduler/src/traits.rs) — `UserInputPriority`, `AnimationPriority`, `BuildPriority`, `IdlePriority` ZSTs + `PriorityLevel` sealed trait + `impl PriorityLevel for ...` (4 impls). ~70 LOC.
- [`crates/flui-scheduler/src/task.rs:227-286`](../../crates/flui-scheduler/src/task.rs) — `TypedTask<P: PriorityLevel>` struct + impls — 75 LOC.
- [`crates/flui-scheduler/src/task.rs:367-374`](../../crates/flui-scheduler/src/task.rs) — `TaskQueue::add_typed<P>()` + `add_typed_task<P>()` methods.
- Grep `UserInputPriority|AnimationPriority|BuildPriority|IdlePriority` workspace outside flui-scheduler/src + flui-scheduler/examples: **0 hits**.
- `TypedTask` usage outside flui-scheduler: **0 hits**.
- The runtime `Priority` enum is used by `flui-app` ([`flui-app/src/embedder/embedder_scheduler.rs:12`](../../crates/flui-app/src/embedder/embedder_scheduler.rs)) — the compile-time type-level priority is not.

**Why it exists:**
Compile-time priority checking demonstration — same "advanced type system" pedagogy as TypestateTicker. The internal Scheduler uses runtime `Priority` enum everywhere; `TypedTask<P>` converts to `Task` via `into_task()` before queueing.

**Cost today:**
- ~145 LOC across `traits.rs` (ZSTs + sealed trait + impls) + `task.rs` (TypedTask + add_typed methods) — all unused.
- Two parallel priority systems (runtime enum + type-level ZSTs) where one suffices.
- `lib.rs` re-exports all 4 ZSTs at top-level + in prelude_advanced.

**Risk of changing:**
Low. Zero production consumers. Internal `TaskQueue::add_typed` callers would migrate to `add(Priority::X, ...)` — equivalent behavior.

**Recommendation:**
**Delete** the 4 ZST priority types + `PriorityLevel` sealed trait + `TypedTask<P>` + `TaskQueue::add_typed`/`add_typed_task`. Keep the runtime `Priority` enum.

---

### 💀 [ZOMBIE | HIGH]: `Scheduler::arc_instance()` parallel singleton — dual-state risk

**Evidence:**
- [`crates/flui-scheduler/src/scheduler.rs:1539-1557`](../../crates/flui-scheduler/src/scheduler.rs):
  ```rust
  static ARC_INSTANCE: std::sync::OnceLock<Arc<Scheduler>> = std::sync::OnceLock::new();

  impl Scheduler {
      pub fn arc_instance() -> Arc<Scheduler> {
          ARC_INSTANCE
              .get_or_init(|| Arc::new(Scheduler::new()))
              .clone()
      }
  }
  ```
- Doc-comment ([:1542-1551](../../crates/flui-scheduler/src/scheduler.rs)) explicitly admits: "The Arc wraps a new Scheduler instance that is **separate from the static singleton**, but both will be ticked by the same event loop since they share the same thread."
- Also at line 1529-1536 — `impl BindingBase for Scheduler` + `impl_binding_singleton!(Scheduler)` already provides `Scheduler::instance() -> &'static Scheduler` via the BindingBase pattern (verified at `flui-app/src/bindings/renderer_binding.rs:214`).
- Grep `arc_instance` workspace-wide: only `flui-scheduler/src/scheduler.rs:1539, :1552` (declaration). **0 callers** outside the impl block.
- The doc-comment notes: "Important: The caller must ensure this scheduler is ticked by calling `handle_begin_frame()` and `handle_draw_frame()` in the event loop." — but there is no consumer doing this.

**Why it exists:**
"Arc-compatible singleton for AnimationController compatibility" — comment line 1538. The animation crate apparently needed `Arc<Scheduler>` rather than `&'static Scheduler`. Since `BindingBase::instance()` returns `&'static`, an Arc wrapper was bolted on. The implementation is wrong — it creates a **second** Scheduler instance instead of wrapping the singleton.

**Cost today:**
- **Silent dual-state risk**: any caller invoking `arc_instance()` gets a *different* scheduler than `Scheduler::instance()`. Frame counts, dirty flags, callbacks, performance mode requests, vsync state — all diverge silently.
- The comment "both will be ticked by the same event loop" is **aspirational** — the event loop in `flui-app` ticks only `Scheduler::instance()`, not the `arc_instance`.
- 20 LOC of dead-but-actively-misleading code.

**Risk of changing:**
Low. Zero callers. Either delete entirely, or fix to return `Arc::new(BindingBase::instance().clone())` — but `Scheduler` derives `Clone` and the inner Arcs are shared, so cloning the singleton returns a proper handle to it.

**Recommendation:** **delete** `ARC_INSTANCE` static + `arc_instance()` method. If `Arc<Scheduler>` is later needed, fix shape: `Scheduler::instance_arc() -> Arc<Scheduler>` via a single `OnceLock<Arc<Scheduler>>` that wraps the singleton (or — cleaner — change `impl_binding_singleton!` to also yield an Arc-handle).

---

### 💀 [ZOMBIE | HIGH]: `VsyncDrivenScheduler` (134 LOC) — 0 production consumers

**Evidence:**
- [`crates/flui-scheduler/src/vsync.rs:409-543`](../../crates/flui-scheduler/src/vsync.rs) — `VsyncDrivenScheduler` wrapping `VsyncScheduler` + `Arc<Scheduler>` + auto-execute Mutex<bool>. 134 LOC.
- Grep `VsyncDrivenScheduler` workspace outside flui-scheduler/src: **0 hits**.
- `Scheduler::set_vsync(VsyncScheduler)` ([scheduler.rs:899](../../crates/flui-scheduler/src/scheduler.rs)) already provides the integration — Scheduler holds an `Option<VsyncScheduler>` and platform code calls `handle_begin_frame`/`handle_draw_frame` from the event loop directly (see [`flui-app/src/bindings/renderer_binding.rs`](../../crates/flui-app/src/bindings/renderer_binding.rs)).
- The doc-comment ([vsync.rs:384-407](../../crates/flui-scheduler/src/vsync.rs)) says "This provides Flutter-like integration where vsync signals automatically drive the frame execution pipeline." Flutter doesn't have an analogue — Flutter's binding directly attaches to `platformDispatcher.onBeginFrame`/`onDrawFrame` ([binding.dart:889-890](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)). The "auto-execute frame on vsync" wrapper is FLUI-specific and unused.

**Why it exists:**
Convenience wrapper for tests/examples. The examples (`vsync_scheduling.rs`) demonstrate it; production code uses `Scheduler::set_vsync` + direct invocation instead.

**Cost today:**
- 134 LOC of code + tests for unused machinery.
- API confusion — two ways to "integrate VsyncScheduler with Scheduler" (set_vsync method vs VsyncDrivenScheduler wrapper).

**Risk of changing:**
Low. Zero workspace consumers. The example file `examples/vsync_scheduling.rs` would need updating — small change.

**Recommendation:**
**Delete** `VsyncDrivenScheduler` (134 LOC). Update the example to use `Scheduler::set_vsync` + direct `handle_begin_frame`/`handle_draw_frame` calls — closer to how the production event loop in `flui-app` does it.

---

### 💀 [ZOMBIE | MEDIUM]: Extension traits `PriorityExt`/`FrameBudgetExt`/`FrameTimingExt` (~150 LOC) — 0 external consumers

**Evidence:**
- [`crates/flui-scheduler/src/traits.rs:108-232`](../../crates/flui-scheduler/src/traits.rs) — `PriorityExt`, `FrameBudgetExt`, `FrameTimingExt` traits with impls for `Priority`, `FrameBudget`, `FrameTiming`. ~150 LOC.
- Grep `PriorityExt|FrameBudgetExt|FrameTimingExt` workspace outside flui-scheduler/src + flui-scheduler/examples: **0 hits**.
- Most of the methods provided by these extensions are also available as inherent methods on the underlying types: `FrameTiming::elapsed()`, `FrameTiming::remaining()`, `FrameTiming::utilization()` — all already exist on `FrameTiming` itself at [frame.rs:608-665](../../crates/flui-scheduler/src/frame.rs). The extension traits add `_seconds` variants or type-safe `Milliseconds` returns where the inherent versions return raw `f64`. The newtype wrappers exist on the inherent methods too (e.g., `FrameTiming::elapsed()` returns `Milliseconds`).
- `PriorityExt::is_higher_than`/`is_interactive` — both could be inherent methods. `PriorityExt::should_skip(policy)`/`skip_threshold` — both could be on Priority directly.

**Why it exists:**
Extension trait pattern demonstration — "similar to Kotlin extension functions" per the doc comment at [traits.rs:14-16](../../crates/flui-scheduler/src/traits.rs). Pedagogy.

**Cost today:**
- 150 LOC of trait machinery that doesn't earn its keep.
- Duplicates inherent methods.
- Re-exported through both preludes.

**Risk of changing:**
Low. Zero external consumers. Internal callers (if any — there don't appear to be any) would migrate to inherent methods.

**Recommendation:**
**Delete** the 3 extension traits. Promote any genuinely-new methods (`Priority::should_skip(policy)`, `Priority::skip_threshold()`, `Priority::is_interactive()`) to inherent methods on `Priority`. Drop from preludes.

---

### 💀 [ZOMBIE | MEDIUM]: `ToMilliseconds`/`ToSeconds` conversion traits — 0 external consumers

**Evidence:**
- [`crates/flui-scheduler/src/traits.rs:237-272`](../../crates/flui-scheduler/src/traits.rs) — `ToMilliseconds`/`ToSeconds` traits + impls for `std::time::Duration` and `f64`.
- Grep `ToMilliseconds|ToSeconds|\.to_ms\(\)|\.to_secs\(\)` workspace outside flui-scheduler/src: **0 hits**.
- The duration types already have `Milliseconds::from(Duration)` / `Seconds::from(Duration)` `From` impls at [duration.rs:213-217, :354-359](../../crates/flui-scheduler/src/duration.rs). `let ms: Milliseconds = my_duration.into()` works identically.
- The `.to_ms()` / `.to_secs()` method-call form is convenient but no production code uses it.

**Recommendation:**
**Delete** `ToMilliseconds` + `ToSeconds` traits. Use the existing `From`/`Into` impls.

---

### 💀 [PARITY-DRIFT | HIGH]: `Priority` enum diverges from Flutter — values, names, and shape mismatch

**Evidence:**
- [`crates/flui-scheduler/src/task.rs:52-66`](../../crates/flui-scheduler/src/task.rs):
  ```rust
  #[repr(u8)]
  pub enum Priority { Idle = 0, Build = 1, Animation = 2, UserInput = 3 }
  ```
- Flutter ([priority.dart:11-54](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/priority.dart)):
  ```dart
  class Priority {
    static const Priority idle = Priority._(0);
    static const Priority animation = Priority._(100000);
    static const Priority touch = Priority._(200000);
    static const int kMaxOffset = 10000;
    Priority operator +(int offset) { ... clamp ... }
    Priority operator -(int offset) => this + (-offset);
  }
  ```
- **Differences:**
  1. Flutter has **3** named priorities (idle, animation, touch). FLUI has **4** (Idle, Build, Animation, UserInput). FLUI inserted `Build` between Idle/Animation — Flutter doesn't have it.
  2. Flutter has integer values with a `kMaxOffset = 10000` gap (touch=200000, animation=100000, idle=0) — explicitly to allow relative offsets via `+`/`-` operators. FLUI's values 0/1/2/3 have no room for relative priorities.
  3. Flutter's `touch` is FLUI's `UserInput` — rename without explanation.
  4. Flutter's class is open-ended (you can construct `Priority._(50000)` to get a custom priority between idle and animation). FLUI's enum is closed — no extensibility.

**Why it exists:**
Rust-idiomatic refactor — closed enum is more type-safe than Flutter's open class. The new `Build` variant matches FLUI's internal pipeline phase. The rename `touch` → `UserInput` is more general (keyboard, mouse, touch all count).

**Cost today:**
- **Migration friction** — any documentation referencing Flutter's `Priority.touch` doesn't match FLUI's `Priority::UserInput`.
- **Loss of expressiveness** — Flutter code commonly uses `Priority.animation - 1` to express "slightly lower than animation". FLUI can't.
- **Behavioral divergence** — `default_scheduling_strategy` ([config.rs:39-47](../../crates/flui-scheduler/src/config.rs)) runs `priority >= Priority::Animation` ⇒ always; lower ⇒ check budget. Flutter's strategy uses `Priority.idle`/`Priority.animation` thresholds with the 100,000 gap. The semantic alignment depends on whether `Build` should be runnable under budget pressure — FLUI says yes (Build < Animation), but Flutter has no Build category.

**Risk of changing:**
Medium. Changing this would ripple into `default_scheduling_strategy`, `BudgetPolicy::SkipIdleAndBuild` variant, `TaskQueue` priority ordering, all 4 ZST priority types (already in [zombie finding](#-zombie--high-four-zst-priority-types--prioritylevel-sealed-trait--typed-task-pattern-unused)).

**Recommendation:**
This is a **deliberate Rust-native divergence**, not an accidental drift. Acknowledge it in `Priority` doc-comment with explicit Flutter mapping:
```rust
/// Task priority levels (higher value = higher priority).
///
/// # Flutter mapping
///
/// Flutter uses an open `Priority` class with named constants at value 0
/// ([`Priority.idle`]), 100000 (`Priority.animation`), 200000 (`Priority.touch`)
/// plus `+`/`-` operator overloading for offsets ([priority.dart](...)).
///
/// FLUI uses a closed 4-variant enum:
/// - `Idle` ↔ `Priority.idle`
/// - `Build` — FLUI-specific (between Idle and Animation). Has no Flutter analogue.
/// - `Animation` ↔ `Priority.animation`
/// - `UserInput` ↔ `Priority.touch` (broader scope: touch, mouse, keyboard, gamepad).
///
/// Relative priorities via offsets are not supported — promote/demote via [`higher`]/[`lower`].
```
Do **not** revert to Flutter's open-class pattern — closed enum + sealed extension is the right Rust shape. **Document the divergence**.

---

### 💀 [PARITY-DRIFT | MEDIUM]: `addPersistentFrameCallback` / `addPostFrameCallback` are unremovable in Flutter; FLUI adds removability — extra API surface

**Evidence:**
- [`crates/flui-scheduler/src/scheduler.rs:776-793`](../../crates/flui-scheduler/src/scheduler.rs) — `add_persistent_frame_callback` returns `CallbackId`; `remove_persistent_frame_callback(id)` returns `bool`.
- [`crates/flui-scheduler/src/scheduler.rs:800-823`](../../crates/flui-scheduler/src/scheduler.rs) — `add_post_frame_callback` returns `CallbackId`; `cancel_post_frame_callback(id)` returns `bool`.
- Flutter ([binding.dart:773-783](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)):
  > "Persistent frame callbacks cannot be unregistered. Once registered, they are called for every frame for the lifetime of the application."
- Flutter ([binding.dart:802](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)):
  > "Post-frame callbacks cannot be unregistered. They are called exactly once."
- The CancellableTransientCallback wrapper ([scheduler.rs:91-107](../../crates/flui-scheduler/src/scheduler.rs)) adds `id: CallbackId` and stores callbacks in `Vec<CancellableTransientCallback>` so `retain(|c| c.id != id)` can drop them. **Extra storage** vs Flutter's plain `List<FrameCallback>`.

**Why it exists:**
Rust-native ergonomics — explicit `Drop`-like cleanup for callbacks attached during, e.g., a Widget's lifetime. The pattern matches `flui-foundation::ChangeNotifier::add_listener` returning a `ListenerId` for removal.

**Cost today:**
- API divergence — Flutter docs explicitly forbid removal, FLUI explicitly enables it. Users may carry incorrect expectations either direction.
- Extra storage — `Vec<CancellablePersistentCallback>` with id field per callback, plus DashMap `cancelled` for in-flight cancellations ([scheduler.rs:359](../../crates/flui-scheduler/src/scheduler.rs)).
- Persistent callbacks in Flutter are the rendering pipeline anchor — they MUST run every frame. Allowing their removal is a footgun.

**Risk of changing:**
Medium. Reverting to Flutter parity would remove `remove_persistent_frame_callback` + `cancel_post_frame_callback`. Could be a breaking change if external users depend on it (currently 0 do).

**Recommendation:**
This is a **deliberate ergonomic improvement** over Flutter's lifetime-of-app pattern. **Document the divergence** in the doc-comments of `add_persistent_frame_callback` + `add_post_frame_callback`:
```rust
/// # Divergence from Flutter
///
/// Flutter's `addPersistentFrameCallback` cannot be unregistered (callbacks
/// run for the lifetime of the application; [binding.dart:773](...)).
/// FLUI returns a [`CallbackId`] for explicit removal via
/// [`remove_persistent_frame_callback`] — useful when a widget subtree wants
/// to register/unregister rendering hooks tied to its mount lifetime.
```
**Decide policy**: keep the divergence (recommended — solves real lifecycle problems) OR remove and match Flutter (cleaner port). Bias toward keep + document.

---

### 💀 [PARITY-DRIFT | MEDIUM]: Lifecycle listener registry doesn't exist in Flutter's scheduler — `WidgetsBindingObserver` is the proper layer

**Evidence:**
- [`crates/flui-scheduler/src/scheduler.rs:1183-1203`](../../crates/flui-scheduler/src/scheduler.rs) — `add_lifecycle_state_listener` / `remove_lifecycle_state_listener` + `handle_app_lifecycle_state_change` notifies all listeners.
- Flutter ([binding.dart:414-441](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)):
  ```dart
  void handleAppLifecycleStateChanged(AppLifecycleState state) {
    if (lifecycleState == state) return;
    _lifecycleState = state;
    switch (state) {
      case AppLifecycleState.resumed:
      case AppLifecycleState.inactive:
        _setFramesEnabledState(true);
      case AppLifecycleState.hidden:
      case AppLifecycleState.paused:
      case AppLifecycleState.detached:
        _setFramesEnabledState(false);
    }
  }
  ```
  No listener notification in the scheduler. Lifecycle observers live at the **widgets layer** via `WidgetsBindingObserver.didChangeAppLifecycleState` (widgets/binding.dart).
- FLUI's listener notification ([scheduler.rs:1147-1160](../../crates/flui-scheduler/src/scheduler.rs)) clones the listener Arc list under one lock then fires outside the lock — pattern is correct, but the *layer* is wrong.

**Why it exists:**
Convenience — FLUI doesn't have a full WidgetsBindingObserver chain yet, so the scheduler handles lifecycle listeners directly.

**Cost today:**
- Layer violation — Scheduler is a frame-loop concern. App lifecycle is a binding concern. Mixing them couples the scheduler to listener registration that should live at a higher layer.
- When `flui-view`'s WidgetsBinding matures (see [view-tree-foundation-audit](2026-05-21-view-tree-foundation-audit.md)), there will be two lifecycle observer paths (scheduler-level + binding-level) competing for the same notification.
- Also: FLUI's scheduler doesn't auto-set `frames_enabled = false` on lifecycle changes (Flutter does at binding.dart:420-426). The lifecycle state listeners can change `framesEnabled` indirectly, but the automatic linkage is missing.

**Risk of changing:**
Medium. Moving the listener registry to `flui-app`'s `AppBinding` or `flui-view`'s `WidgetsBinding` is the correct fix, but requires the higher binding to exist. Until then, the scheduler-level listeners may be a necessary scaffold.

**Recommendation:**
1. Add the Flutter-aligned auto-update of `frames_enabled` on lifecycle state change — when state is `Hidden`/`Paused`/`Detached`, set frames_enabled=false; when `Resumed`/`Inactive`, set frames_enabled=true.
2. Mark `add_lifecycle_state_listener`/`remove_lifecycle_state_listener` with `// REVISIT_AFTER: WidgetsBinding observer chain materializes (see flui-view view-tree-foundation-audit)`. Plan to migrate listeners to the binding layer when WidgetsBindingObserver matures.

---

### 💀 [PRINCIPLE-6 | HIGH]: `from_u8` panic paths called from production atomic-load round-trips

**Evidence:**
- `SchedulerPhase::from_u8` ([frame.rs:112-117](../../crates/flui-scheduler/src/frame.rs)):
  ```rust
  pub const fn from_u8(value: u8) -> Self {
      match Self::try_from_u8(value) {
          Some(v) => v,
          None => panic!("Invalid SchedulerPhase value"),
      }
  }
  ```
- Same panic-style `from_u8` in `AppLifecycleState` ([frame.rs:308-313](../../crates/flui-scheduler/src/frame.rs)) and `FrameSkipPolicy` ([scheduler.rs:257-262](../../crates/flui-scheduler/src/scheduler.rs)).
- Production callers of these panicking `from_u8`:
  - `Scheduler::phase()` at [scheduler.rs:496](../../crates/flui-scheduler/src/scheduler.rs) — `SchedulerPhase::from_u8(atomic.load())`
  - `Scheduler::set_scheduler_phase` validation at [scheduler.rs:501](../../crates/flui-scheduler/src/scheduler.rs)
  - `Scheduler::frame_skip_policy()` at [scheduler.rs:1035](../../crates/flui-scheduler/src/scheduler.rs)
  - `Scheduler::should_skip_frames()` at [scheduler.rs:1065](../../crates/flui-scheduler/src/scheduler.rs)
  - `Scheduler::lifecycle_state()` at [scheduler.rs:1111](../../crates/flui-scheduler/src/scheduler.rs)
  - `Scheduler::handle_app_lifecycle_state_change()` at [scheduler.rs:1140](../../crates/flui-scheduler/src/scheduler.rs)
- All these load from `AtomicU8` fields that are written via `as u8` from valid discriminants in safe paths — so under normal operation the panic is unreachable. But:
  - Memory corruption / undefined behavior in unsafe sister-crates could write invalid values.
  - A failed `compare_exchange` race on a future variant insertion could observe an intermediate.
  - **Constitution Principle 6** ("No `unwrap()`/`println!`/`dbg!`/`unimplemented!`/`todo!` in production paths") is explicit about `panic!` in production code.
- Additionally:
  - `VsyncScheduler::new(0)` panics ([vsync.rs:167](../../crates/flui-scheduler/src/vsync.rs)) — `assert!(refresh_rate > 0)`.
  - `FrameDuration::from_fps(0)` panics ([duration.rs:513](../../crates/flui-scheduler/src/duration.rs)) — `assert!(fps > 0)`.
  - `set_time_dilation(non_positive)` panics ([config.rs:97](../../crates/flui-scheduler/src/config.rs)) — `assert!(value > 0.0)`.
  - `Microseconds::to_std_duration` panics on negative ([duration.rs:432-438](../../crates/flui-scheduler/src/duration.rs)) — the underlying `i64` field allows negative values.

**Why it exists:**
"Atomic round-trip is safe" assumption. Writers always write valid discriminants via `as u8`, so reads should always observe valid values. The panic is a fail-safe for "should never happen" scenarios. The constructors `new(0)` style asserts are input-validation panics.

**Cost today:**
- Constitution Principle 6 violation — `panic!` in production paths.
- Brittle against future refactors that introduce intermediate states or unsafe sister-crate corruption.
- Asymmetric API — `try_from_u8` exists but `from_u8` is used everywhere internally.

**Risk of changing:**
Low. Replace each `from_u8(atomic.load())` site with `try_from_u8(atomic.load()).unwrap_or(default)`. Default for `SchedulerPhase` is `Idle`; for `AppLifecycleState` is `Resumed`; for `FrameSkipPolicy` is `CatchUp`. For constructors (`VsyncScheduler::new(0)`, `FrameDuration::from_fps(0)`, `set_time_dilation(non_positive)`), introduce `try_new` / `try_from_fps` / `try_set_time_dilation` variants returning `Result<Self, FpsError>` or `Result<(), TimeDilationError>` and deprecate the panicking ones. For `Microseconds`, change inner type to `u64` (or `NonZeroU64` for non-zero invariants — actually frame-interval microseconds can be 0 conceptually so `u64`).

**Recommendation:**
1. Replace all 6 production `from_u8` calls in `scheduler.rs` with `try_from_u8(…).unwrap_or(Default::default())` — the round-trip default is safe.
2. Add `VsyncScheduler::try_new(u32) -> Result<Self, VsyncError>` and `FrameDuration::try_from_fps(u32) -> Result<Self, FpsError>` and `try_set_time_dilation(f64) -> Result<(), TimeDilationError>` with `#[thiserror::Error]` enums. Mark existing panicking versions `#[deprecated]`.
3. Change `Microseconds(i64)` → `Microseconds(u64)`. The `i64` was over-permissive; every production producer writes non-negative values via `as i64` from u128. Audit + change to `u64`. Removes the `to_std_duration` panic entirely.

---

### 💀 [SYNC-CONTENTION | MEDIUM]: `Ticker::state()`/`is_muted()`/`elapsed()` lock for single-field reads — should be atomics

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs:307-340`](../../crates/flui-scheduler/src/ticker.rs):
  ```rust
  pub fn state(&self) -> TickerState { self.inner.lock().state }
  pub fn is_active(&self) -> bool { self.state().can_tick() }
  pub fn is_muted(&self) -> bool { self.inner.lock().state == TickerState::Muted }
  pub fn is_running(&self) -> bool { self.state().is_running() }
  pub fn elapsed(&self) -> Seconds { /* takes lock to read start_time */ }
  ```
- Same pattern in `ScheduledTicker` ([ticker.rs:712-744](../../crates/flui-scheduler/src/ticker.rs)) and `VsyncScheduler::mode()`/`is_active()`/`time_since_vsync()` ([vsync.rs:198, :246, :325](../../crates/flui-scheduler/src/vsync.rs)).
- These are getter-style API surfaces called from hot paths (per-frame `should_run_animations` checks) — every call acquires `parking_lot::Mutex`.

**Why it exists:**
Inner consolidation refactor — early in the crate's life, multiple fields were behind separate `Arc<Mutex<>>`. The recent consolidation put them all behind one `Arc<Mutex<TickerInner>>` (per the comment at [ticker.rs:139-140](../../crates/flui-scheduler/src/ticker.rs)). That's better for the *mutate-multiple-fields* paths, but worse for *read-one-field* paths.

**Cost today:**
- Per-frame lock acquisition for trivial state reads.
- Contention if multiple threads observe ticker state during a frame (e.g., animation pipeline + UI thread).
- Compared to Flutter, which is single-threaded — Rust's potential for multi-threaded observation is wasted.

**Risk of changing:**
Medium. Splitting `state: TickerState` into `state: AtomicU8` requires either (a) keeping the Mutex for `start_time`/`callback`/`muted_elapsed` and adding the atomic alongside (best — atomic for state, mutex for everything else), or (b) splitting all fields into separate atomics+arcs (regression of the consolidation).

**Recommendation:**
Add `state: AtomicU8` outside the `Inner` struct — accessed via `TickerState::from_u8` round-trip (with try_from_u8 + unwrap_or per the [Principle 6 finding](#-principle-6--high-from_u8-panic-paths-called-from-production-atomic-load-round-trips)). The Mutex retains `start_time`, `callback`, `muted_elapsed` for the small number of write-multiple-fields code paths. Same pattern for `VsyncScheduler::active: AtomicBool` and `VsyncScheduler::mode: AtomicU8`.

**Patch sketch (Ticker):**
```rust
pub struct Ticker {
    id: TickerId,
    state: Arc<AtomicU8>,                  // ← new, lock-free
    inner: Arc<Mutex<TickerInner>>,         // ← unchanged
}

struct TickerInner {
    // state removed — moved to AtomicU8
    start_time: Option<Instant>,
    callback: Option<TickerCallback>,
    muted_elapsed: Seconds,
}

pub fn state(&self) -> TickerState {
    TickerState::try_from_u8(self.state.load(Ordering::Acquire))
        .unwrap_or(TickerState::Idle)
}
pub fn is_muted(&self) -> bool {
    self.state() == TickerState::Muted
}
```

---

### 💀 [SYNC-CONTENTION | MEDIUM]: `TaskQueue::is_empty()`/`len()`/`peek_priority()` lock for trivial reads

**Evidence:**
- [`crates/flui-scheduler/src/task.rs:387-394`](../../crates/flui-scheduler/src/task.rs):
  ```rust
  pub fn len(&self) -> usize { self.queue.lock().len() }
  pub fn is_empty(&self) -> bool { self.queue.lock().is_empty() }
  pub fn peek_priority(&self) -> Option<Priority> { self.queue.lock().peek().map(|pt| pt.0.priority) }
  ```
- Called from `Scheduler::execute_idle_callbacks` ([scheduler.rs:1484](../../crates/flui-scheduler/src/scheduler.rs)) per check — once per event-loop tick.
- `count_by_priority` ([task.rs:473-487](../../crates/flui-scheduler/src/task.rs)) iterates under lock — O(N) under lock.

**Why it exists:**
BinaryHeap requires &mut for pop/push, so Mutex is needed. The is_empty/len getters could be approximate via atomic counter, but BinaryHeap doesn't expose that.

**Cost today:**
- Per-tick lock for is_empty check.
- Multi-thread contention if task adds happen from worker threads while the event loop polls.

**Risk of changing:**
Medium. Could add `count: AtomicUsize` alongside the BinaryHeap, incremented on `add_task` and decremented on `pop`. The is_empty/len getters become lock-free.

**Recommendation:**
Add `count: AtomicUsize` to `TaskQueue` — `is_empty()` becomes `self.count.load(Acquire) == 0`. `len()` becomes `self.count.load(Acquire)`. Mutate on add/pop/clear/drain. Keep peek_priority + count_by_priority locked since they actually need the heap contents.

---

### 💀 [LOCK-CONTENTION | MEDIUM]: `ScheduledTicker::tick_and_reschedule` re-locks the inner Mutex 3× per tick + allocates closure per frame

**Evidence:**
- [`crates/flui-scheduler/src/ticker.rs:772-822`](../../crates/flui-scheduler/src/ticker.rs):
  ```rust
  fn tick_and_reschedule(inner: Arc<Mutex<ScheduledTickerInner>>, scheduler: Arc<Scheduler>) {
      let (elapsed, mut callback) = {
          let mut guard = inner.lock();              // ← lock #1
          guard.scheduled = false;
          if guard.state != TickerState::Active { return; }
          let Some(start) = guard.start_time else { return; };
          (start.elapsed().as_secs_f64(), guard.callback.take())
      };

      if let Some(ref mut cb) = callback { cb(elapsed); }

      let should_reschedule = {
          let mut guard = inner.lock();              // ← lock #2
          if guard.state == TickerState::Active {
              guard.callback = callback;
              guard.scheduled = true;
              true
          } else { false }
      };

      if should_reschedule {
          let inner = Arc::clone(&inner);
          let scheduler_inner = Arc::clone(&scheduler);
          scheduler.schedule_frame_callback(Box::new(move |_vsync| {
              Self::tick_and_reschedule(inner, scheduler_inner);
          }));                                       // ← Box allocation + Arc clones every tick
      }
  }
  ```
- Plus the original `start()` path locks once (line 653-657 — lock #3 conceptually counted at the boundary).
- The `Box::new` closure capture (line 761, 818) **allocates per frame** during animations. At 60fps a single ticker triggers 60 Box allocations/sec; at 144fps it's 144.

**Why it exists:**
The "take-invoke-restore" pattern was added to avoid holding the lock during callback invocation (good — prevents deadlock if callback re-enters the scheduler). The closure-per-frame is the standard async-recursion pattern in Rust without proper async/await.

**Cost today:**
- 60-144 Box allocations per second per active ticker. For an app with 10 concurrent animations, that's 600-1440 allocations/sec.
- 3 lock acquisitions per tick × N tickers per frame.
- Allocator pressure on the frame budget — Rust's default allocator is fast but not zero.

**Risk of changing:**
Medium-high. The "take-invoke-restore + reschedule" pattern is correct but expensive. Alternatives:
1. **Pre-allocate the closure once** at `start()`, store it as `Arc<dyn Fn(Instant) + Send + Sync>` in ScheduledTickerInner, register it with the scheduler ONCE (move to persistent callbacks). The scheduler's existing `add_persistent_frame_callback` returns a `CallbackId` so unregistration works. Persistent callbacks already iterate via Arc clone — no per-frame Box alloc.
2. **Switch to a registry pattern** — Scheduler holds a `DashMap<TickerId, Arc<dyn Fn(Instant)>>` of active tickers; `handle_begin_frame` iterates the map; ScheduledTicker registers/unregisters from the map on start/stop. No closure-per-frame.
3. **Keep closure but reuse Box** — only re-Box on tick when the prior box was consumed; if it's reusable (e.g., the callback captures don't change), retain the same boxed closure. Hard to express in safe Rust without `dyn FnMut` shared via Arc<Mutex<>>.

**Recommendation:**
Apply option (1) — register the ticker callback as a persistent frame callback in `ScheduledTicker::start`, store the returned `CallbackId` in inner, remove via `remove_persistent_frame_callback` in `stop`/`dispose`. Eliminates per-frame Box allocation entirely. Estimated change: ~30 LOC rewrite of `tick_and_reschedule` + `start` + `stop`.

---

### 💀 [BLOCKING-ON-EDGE | LOW]: `VsyncScheduler::wait_for_vsync` uses `std::thread::sleep` — contradicts STRATEGY "Sync hot path, async на краях"

**Evidence:**
- [`crates/flui-scheduler/src/vsync.rs:257-287`](../../crates/flui-scheduler/src/vsync.rs):
  ```rust
  pub fn wait_for_vsync(&self) -> Instant {
      ...
      if elapsed < interval {
          let wait_time = interval - elapsed;
          std::thread::sleep(wait_time);  // ← blocking
          Instant::now()
      } ...
  }
  ```
- `STRATEGY.md` "Sync hot path, async на краях" — render pipeline strictly sync; scheduler is "on the edges" so async is OK there, but **blocking thread::sleep** is neither sync-fast nor async-friendly.
- The doc-comment at [vsync.rs:7-9](../../crates/flui-scheduler/src/vsync.rs) acknowledges: "`wait_for_vsync()` currently uses `thread::sleep` for timing simulation. Real platform VSync integration comes from `flui-platform` via `VsyncDrivenScheduler::on_vsync()`."

**Cost today:**
- Blocks the calling thread for up to one frame interval.
- Would deadlock if called from the same thread as the scheduler's event loop.
- Tests using it sleep wall-clock time, slowing CI.

**Risk of changing:**
Low. The fallback path is acknowledged temporary. Real implementations come from `flui-platform`. Mark with `#[deprecated(note = "Use VsyncDrivenScheduler::on_vsync from platform integration instead")]` until removed.

**Recommendation:**
1. Mark `wait_for_vsync` as `#[deprecated]` with the noted alternative.
2. Long-term: remove entirely when `flui-platform`'s vsync integration covers all targets (Win32 `DwmFlush`, macOS `CADisplayLink`, Linux DRM/GLX, web `requestAnimationFrame`).

---

### 💀 [DEPENDENCY-BLOAT | LOW]: `crossbeam` workspace dep — 0 usages in flui-scheduler

**Evidence:**
- [`crates/flui-scheduler/Cargo.toml:25`](../../crates/flui-scheduler/Cargo.toml):
  ```toml
  crossbeam.workspace = true
  ```
- Grep `crossbeam` in flui-scheduler/src: **0 hits**.
- The crate uses `parking_lot::Mutex` and `dashmap::DashMap` for concurrent primitives. `crossbeam` is imported but not used.

**Cost today:**
- Unused workspace dep — compile time + binary size hit for nothing.
- Confusing — readers see crossbeam in Cargo.toml and look for it in src.

**Recommendation:** **drop** `crossbeam` from `crates/flui-scheduler/Cargo.toml`.

---

### 💀 [PARITY-DRIFT | LOW]: `Microseconds(i64)` over-permissive — every producer writes non-negative values

**Evidence:**
- [`crates/flui-scheduler/src/duration.rs:382`](../../crates/flui-scheduler/src/duration.rs) — `pub struct Microseconds(i64);`
- Producers grep results:
  - [vsync.rs:168](../../crates/flui-scheduler/src/vsync.rs): `Microseconds::new(1_000_000 / refresh_rate as i64)` — positive (refresh_rate > 0 asserted)
  - [vsync.rs:299](../../crates/flui-scheduler/src/vsync.rs): `Microseconds::new(now.duration_since(last).as_micros() as i64)` — non-negative (Duration is unsigned)
  - [vsync.rs:309](../../crates/flui-scheduler/src/vsync.rs): `Microseconds::new(sum / inner.interval_history.len() as i64)` — non-negative (sum is sum of non-negative)
  - [duration.rs:101](../../crates/flui-scheduler/src/duration.rs): `Microseconds::new((self.0 * 1000.0) as i64)` from `Milliseconds(f64).to_micros()` — could be negative if `Milliseconds` is negative; but Milliseconds itself has no positivity invariant.
- The `to_std_duration` panic ([duration.rs:432-438](../../crates/flui-scheduler/src/duration.rs)) is only reachable if someone constructs negative Microseconds.

**Why it exists:**
Symmetry with Flutter's `int microseconds` (Dart `int` is signed). Rust idiom would prefer `u64` for non-negative durations.

**Cost today:**
- The `to_std_duration` panic exists because of the over-permissive type.
- API confusion — does negative microseconds mean "rewind"? No use case.

**Risk of changing:**
Low. Change `Microseconds(i64)` → `Microseconds(u64)`. The producers all write non-negative values via `u128 as i64` (which is non-negative) or positive constants. The `Milliseconds::to_micros` could underflow `u64::MAX` — needs clamp.

**Recommendation:**
Change inner type to `u64`. Remove the negative-check in `to_std_duration` — type system enforces non-negativity. Remove `try_to_std_duration` (no longer fallible). Simplifies the API.

---

---

## Dead Code Table

| Item | Location | Evidence | Hidden-use risk | Verdict | Action |
|------|----------|----------|-----------------|---------|--------|
| `TypestateTicker<S>` + `Idle`/`Active`/`Muted`/`Stopped` + `TickerState` sealed trait | [typestate.rs](../../crates/flui-scheduler/src/typestate.rs) | 392 LOC; 0 production callers; incompatible with Slab storage | None | **Architecture theater** | **delete entire typestate.rs** |
| `ScheduledTicker` | [ticker.rs:610-823](../../crates/flui-scheduler/src/ticker.rs) | 214 LOC; can be absorbed into `Ticker` with optional `Arc<dyn TickerProvider>` | None (only flui-scheduler tests + examples) | **Duplicate Ticker shape** | **merge into Ticker** |
| `Handle<M>` + `FrameHandle` + `TaskHandle` | [id.rs:139-247](../../crates/flui-scheduler/src/id.rs) | 120 LOC; 0 production callers; no slot-map exists | None | **Speculative slot-map machinery** | **delete Handle pattern section** |
| `UserInputPriority`/`AnimationPriority`/`BuildPriority`/`IdlePriority` ZSTs + `PriorityLevel` sealed trait | [traits.rs:39-101](../../crates/flui-scheduler/src/traits.rs) | 70 LOC; 0 production callers; runtime enum suffices | None | **Pedagogical type-level priorities** | **delete 4 ZSTs + sealed trait** |
| `TypedTask<P>` + `TaskQueue::add_typed`/`add_typed_task` | [task.rs:227-286, :367-374](../../crates/flui-scheduler/src/task.rs) | 90 LOC; 0 production callers | None | **Type-level pedagogy** | **delete typed-task pattern** |
| `PriorityExt`, `FrameBudgetExt`, `FrameTimingExt` extension traits | [traits.rs:108-232](../../crates/flui-scheduler/src/traits.rs) | 150 LOC; 0 external callers; duplicates inherent methods | None | **Kotlin-style extension pedagogy** | **delete 3 ext traits, promote useful methods inherent** |
| `ToMilliseconds`, `ToSeconds` conversion traits | [traits.rs:237-272](../../crates/flui-scheduler/src/traits.rs) | 30 LOC; 0 external callers; `From`/`Into` already cover | None | **Duplicate API** | **delete** |
| `prelude_advanced` module | [lib.rs:226-245](../../crates/flui-scheduler/src/lib.rs) | 14 exports; 0 external callers | None | **API surface inflation** | **delete prelude_advanced; selectively merge into prelude** |
| `Scheduler::arc_instance()` + `ARC_INSTANCE: OnceLock<Arc<Scheduler>>` | [scheduler.rs:1539-1557](../../crates/flui-scheduler/src/scheduler.rs) | 20 LOC; 0 callers; creates *second* singleton parallel to BindingBase | None | **Silent dual-state risk** | **delete** |
| `VsyncDrivenScheduler` | [vsync.rs:409-543](../../crates/flui-scheduler/src/vsync.rs) | 134 LOC; 0 production callers; duplicates `Scheduler::set_vsync` | None (only flui-scheduler examples) | **Redundant integration wrapper** | **delete; update example** |
| `crossbeam` workspace dep | [Cargo.toml:25](../../crates/flui-scheduler/Cargo.toml) | 0 usages in flui-scheduler/src | None | **Unused dep** | **drop from Cargo.toml** |
| `VsyncScheduler::wait_for_vsync` | [vsync.rs:257-287](../../crates/flui-scheduler/src/vsync.rs) | uses `std::thread::sleep`; production vsync comes from platform layer | Real-ish (tests/examples use it) | **Temporary fallback** | **mark `#[deprecated]` until platform vsync integration ships** |
| `TickerProvider::schedule_tick_typed` default impl | [ticker.rs:93-97](../../crates/flui-scheduler/src/ticker.rs) | 0 callers; can be derived via `From<f64> for Seconds` | None | **Convenience default never used** | **delete after TickerProvider reshape** |
| `TickerFuture::set_complete`/`set_canceled` | [ticker.rs:923-951](../../crates/flui-scheduler/src/ticker.rs) | `#[allow(dead_code)]`; doc-comment "Reserved for future use when ScheduledTicker integrates with TickerFuture" | High — the integration is planned but stalled | **Reserved-for-future** | **wire to Ticker dispose/stop OR delete** |

---

---

## Restructuring Plan

### Step 1 — Ticker consolidation (largest semantic win)

1. **Add `Ticker::dispose(&mut self)` + `disposed: AtomicBool` + `impl Drop for Ticker`** matching the PR #84 `ChangeNotifier::dispose` pattern. Add `debug_assert!(!disposed)` to `start`/`tick`/`mute`/`unmute`/`reset`/`state`. Wire `Drop` to call `dispose()` if not already disposed.
2. **Add `Ticker::start` "started twice" assertion** — `debug_assert!(!matches!(state, Active | Muted))` matching Flutter's `assert(!isActive)`.
3. **Reshape `TickerProvider`** — change `schedule_tick(callback)` → `create_ticker(on_tick) -> Ticker` matching Flutter's `TickerProvider.createTicker(TickerCallback) -> Ticker`. Update `Scheduler::impl TickerProvider`. Drop `schedule_tick_typed` default method.
4. **Merge `ScheduledTicker`'s auto-scheduling into `Ticker`** — add optional `provider: Option<Arc<dyn TickerProvider>>` field to `Ticker`; when present, `start()` registers a persistent callback with the provider; `stop()`/`dispose()` removes it. After merge, **delete `ScheduledTicker`**.
5. **Delete `TypestateTicker` entirely** — 392 LOC, 0 consumers. Remove from `lib.rs:206-208`, `prelude_advanced`. Update lib-doc "Advanced Type System Features → Typestate Pattern" section to refer to `flui-tree`'s typestate (if at all).
6. **Wire `TickerFuture` to `Ticker` lifecycle** — Flutter's `Ticker.start()` returns `TickerFuture`. After consolidation, return `TickerFuture` from `Ticker::start()`. `stop({canceled: false})` calls `_complete()` on the future; `stop({canceled: true})` and `dispose()` call `_cancel()`. The `#[allow(dead_code)]` `TickerFuture::set_complete`/`set_canceled` ([ticker.rs:923-951](../../crates/flui-scheduler/src/ticker.rs)) get wired here.
7. **Document the Flutter divergence** in `Ticker` doc-comment: no `Stopped` state vs Flutter's lack of state enum, but matched start/stop/mute/unmute/dispose semantics.

### Step 2 — API surface compression

8. **Delete `prelude_advanced`** ([lib.rs:226-245](../../crates/flui-scheduler/src/lib.rs)). Merge `SchedulerBuilder`, `FrameBudgetBuilder`, `FrameTimingBuilder` into the basic `prelude`. Drop typestate/Handle/ZST exports from any prelude.
9. **Delete `Handle<M>` + `FrameHandle` + `TaskHandle`** ([id.rs:131-247](../../crates/flui-scheduler/src/id.rs)). Keep `IdGenerator<M>` (used internally for `CallbackId`).
10. **Delete the 4 ZST priority types + `PriorityLevel` sealed trait** ([traits.rs:39-101](../../crates/flui-scheduler/src/traits.rs)). Update `TaskQueue::add_typed` callers to use `add(Priority::X, ...)`.
11. **Delete `TypedTask<P>` + `TaskQueue::add_typed`/`add_typed_task`** ([task.rs:227-286, :367-374](../../crates/flui-scheduler/src/task.rs)).
12. **Delete extension traits `PriorityExt`/`FrameBudgetExt`/`FrameTimingExt`** ([traits.rs:108-232](../../crates/flui-scheduler/src/traits.rs)). Promote `Priority::should_skip(policy)`/`Priority::skip_threshold()`/`Priority::is_interactive()` to inherent methods on `Priority`. Drop the rest.
13. **Delete `ToMilliseconds`/`ToSeconds` conversion traits** ([traits.rs:237-272](../../crates/flui-scheduler/src/traits.rs)). The existing `From`/`Into` impls between Duration types and the newtypes are sufficient.
14. **Delete `Scheduler::arc_instance()` + `ARC_INSTANCE` static** ([scheduler.rs:1539-1557](../../crates/flui-scheduler/src/scheduler.rs)). If `Arc<Scheduler>` is later needed, add a proper `instance_arc()` that wraps the BindingBase singleton.
15. **Delete `VsyncDrivenScheduler`** ([vsync.rs:409-543](../../crates/flui-scheduler/src/vsync.rs)). Update `crates/flui-scheduler/examples/vsync_scheduling.rs` to use `Scheduler::set_vsync` + direct `handle_begin_frame`/`handle_draw_frame` calls (mirrors `flui-app` event loop).

### Step 3 — Principle 6 + lifecycle compliance

16. **Replace all 6 production `from_u8` calls** in `scheduler.rs` (`phase`, `set_scheduler_phase`, `frame_skip_policy`, `should_skip_frames`, `lifecycle_state`, `handle_app_lifecycle_state_change`) with `try_from_u8(…).unwrap_or(Default::default())`.
17. **Introduce `try_*` constructors** for input-validating types: `VsyncScheduler::try_new(refresh_rate) -> Result<Self, VsyncError>`, `FrameDuration::try_from_fps(fps) -> Result<Self, FpsError>`, `try_set_time_dilation(value) -> Result<(), TimeDilationError>`. Mark panicking versions `#[deprecated]` with note pointing to the try-variants.
18. **Change `Microseconds(i64)` → `Microseconds(u64)`**. Remove `try_to_std_duration`/`to_std_duration` panic — type system now prevents negative values. Add `Milliseconds::to_micros` clamp for `f64 < 0` defensiveness.
19. **Add Flutter-aligned automatic `frames_enabled` update on lifecycle change** ([scheduler.rs:1138-1161](../../crates/flui-scheduler/src/scheduler.rs)). When transitioning to `Hidden`/`Paused`/`Detached`, set `frames_enabled = false`. When transitioning to `Resumed`/`Inactive`, set `frames_enabled = true`. Matches Flutter's `binding.dart:420-426`.
20. **Mark `add_lifecycle_state_listener`/`remove_lifecycle_state_listener` with `// REVISIT_AFTER: WidgetsBinding observer chain materializes`**. Plan migration to `flui-view::WidgetsBindingObserver` per [view-tree-foundation-audit](2026-05-21-view-tree-foundation-audit.md).

### Step 4 — Performance hot-path improvements

21. **Lock-free `state: AtomicU8` for `Ticker`** — split `state` out of `TickerInner` Mutex; keep the Mutex for `start_time`/`callback`/`muted_elapsed`. Same for `VsyncScheduler::mode: AtomicU8`, `VsyncScheduler::active: AtomicBool`.
22. **Lock-free `count: AtomicUsize` for `TaskQueue`** — `is_empty()`/`len()` become atomic loads. Mutate on add/pop/clear/drain.
23. **Eliminate per-tick Box allocation in `ScheduledTicker`** — after consolidation into `Ticker` (Step 1), register the ticker callback as a **persistent frame callback** in `Ticker::start` (not a one-shot transient callback). Store the returned `CallbackId` in `inner`, remove via `remove_persistent_frame_callback` in `stop`/`dispose`. Eliminates 60-144 Box allocations/sec per active ticker.

### Step 5 — Dependency cleanup

24. **Drop `crossbeam` workspace dep from `Cargo.toml:25`** — 0 usages.
25. **Mark `VsyncScheduler::wait_for_vsync` `#[deprecated]`** with note pointing to platform-driven vsync. Plan removal when `flui-platform` covers Win32/macOS/Linux/web vsync.

### Step 6 — Tests + regression protection

26. **Add tests for `Ticker::dispose` + use-after-dispose assert** — verify `debug_assert!` fires on `tick`/`start`/`mute` after `dispose`.
27. **Add test for `Ticker::start` "started twice" assert** — verify `debug_assert!` fires when calling `start` on Active/Muted ticker.
28. **Add test for `TickerFuture` cancellation propagation** — Ticker dispose → orCancel future resolves with `TickerCanceled`.
29. **Add test for `frames_enabled` auto-toggle on lifecycle change**.
30. **Re-run `cargo clippy -p flui-scheduler --all-targets -- -D warnings`** after each step.

### Migration path summary

- **Phase 1** (Steps 1, 16-20) lands first — Ticker is the most architecturally weak component; Principle-6 fixes unblock other audits.
- **Phase 2** (Steps 2, 8-15) is pure deletion — ≈ 1,000 LOC removed, no behavior change for current consumers (all zero).
- **Phase 3** (Steps 4, 21-23) is performance work — needs criterion benchmarks first to confirm gains.
- **Phase 4** (Steps 5, 24-25) is housekeeping.
- **Phase 5** (Steps 6, 26-30) is regression protection.

Per [`no-quick-wins-vanyastaff`](../../.claude/agent-memory/architect/) — execute the full ripple. The flui-animation imports (currently disabled but planned) will need to update for the consolidated Ticker API; that ripple is bounded and expected.

---

## Optimization Plan

| Area | Current cost | Proposed change | Expected gain | Risk | Benchmark/test |
|------|--------------|-----------------|---------------|------|----------------|
| `ScheduledTicker::tick_and_reschedule` per-frame Box allocation | 1 Box closure + 2 Arc clones + 3 lock acquisitions per tick per ticker | After Ticker consolidation, register callback as persistent frame callback ONCE in `start()`; no re-registration per frame | Eliminate 60-144 Box allocs/sec/ticker; for 10 concurrent animations, save 600-1440 allocs/sec | Medium — persistent callback path needs to filter by `state == Active` skipping muted/idle tickers | Criterion bench: 10 tickers × 1000 frames vs current impl |
| `Ticker::state()`/`is_muted()`/`is_active()` lock-on-read | parking_lot::Mutex acquire per call, called from per-frame `should_run_animations` check | Split `state: AtomicU8` out of TickerInner; keep Mutex for `start_time`/`callback`/`muted_elapsed` | Lock-free reads; minimal write overhead on transitions | Low — Mutex still owns write-many-fields paths | Criterion bench: 1M state() calls vs current |
| `TaskQueue::is_empty()`/`len()` lock-on-read | parking_lot::Mutex acquire per call, called from `Scheduler::execute_idle_callbacks` per event-loop tick | Add `count: AtomicUsize` alongside BinaryHeap; mutate on add/pop | Lock-free getter; sub-ns vs ~10ns lock | Low — atomic counter staleness only if observed mid-pop, which is harmless for is_empty | Criterion bench: 1M is_empty() under contention |
| `Scheduler::handle_begin_frame` mutex acquisitions | ~6 Mutex locks per frame (current_vsync_time, current_frame, frame_duration, scheduler_phase via atomic, transient callbacks drain, frame callbacks drain) | Coalesce frame state into a single `FrameMutableState` Mutex; lock once at entry, hold guard through phase transitions | Reduce 6 lock acquisitions to 1 per begin_frame | Medium — lock held longer, contention risk if other threads want frame_state read access | Criterion bench: 10k frames |
| `handle_draw_frame` persistent callback dispatch | Clones `RecurringFrameCallback = Arc<dyn Fn>` Vec under lock, iterates outside | Replace `Mutex<Vec<CancellablePersistentCallback>>` with `RwLock<Vec<...>>` — read-lock during dispatch, write-lock for add/remove. Many readers in concurrent scenarios, few writers | Less write-lock contention | Medium — RwLock has higher single-thread overhead than Mutex; need to confirm via bench | Criterion bench under contention |
| `Microseconds(i64)` runtime branch in `to_std_duration` | Conditional `>= 0` check + Option wrapping | Change inner type to `u64`; type system enforces non-negativity | Branch elimination; smaller binary | Low — every producer is non-negative | Compile-time |
| `from_u8(value).panic` calls | Branch + panic info per call | Replace with `try_from_u8(value).unwrap_or(default)` | Slightly faster (no panic format setup); Principle 6 compliance | Low | Existing tests cover round-trip |

---

## What to Preserve

Do not touch these. They earn their place:

- **`Scheduler` triple-Arc consolidation** (`Arc<FrameState>` + `Arc<CallbackState>` + `Arc<BindingState>` at [scheduler.rs:421-431](../../crates/flui-scheduler/src/scheduler.rs)) — three Arc allocations vs Flutter's 30+ scattered fields on the binding mixin. Clean Rust-native shape.
- **`SchedulerPhase` enum + `can_transition_to` validation** ([frame.rs:66-172](../../crates/flui-scheduler/src/frame.rs)) — direct Flutter parity (Idle / TransientCallbacks / MidFrameMicrotasks / PersistentCallbacks / PostFrameCallbacks). The `can_transition_to` debug-assert is a Rust-native improvement over Flutter's scattered `assert(_schedulerPhase == ...)`.
- **`AppLifecycleState` enum + `should_animate`/`should_render`/`is_visible`/`is_focused`** ([frame.rs:245-414](../../crates/flui-scheduler/src/frame.rs)) — matches Flutter's `AppLifecycleState` 5 variants (Resumed / Inactive / Hidden / Paused / Detached) + adds Rust-native query methods.
- **`handle_begin_frame` + `handle_draw_frame` phase orchestration** ([scheduler.rs:527-673](../../crates/flui-scheduler/src/scheduler.rs)) — correct Flutter parity for transient → mid-frame microtasks → persistent → post-frame → idle ordering. The phase guard `set_scheduler_phase` enforces transition validity in debug.
- **`BindingBase` integration via `impl_binding_singleton!`** ([scheduler.rs:1529-1536](../../crates/flui-scheduler/src/scheduler.rs)) — clean composition pattern matching Flutter's mixin-based bindings. Verified working with `flui-app/src/bindings/renderer_binding.rs`.
- **`PerformanceModeRequestHandle` with `impl Drop`** ([config.rs:170-198](../../crates/flui-scheduler/src/config.rs)) — RAII pattern matching Flutter's `PerformanceModeRequestHandle.dispose` semantics. The fetch_add/fetch_sub on `performance_mode_requests` counter is the correct atomic reference-count pattern.
- **`FrameCompletionFuture` (Future + waker storage)** ([scheduler.rs:115-179](../../crates/flui-scheduler/src/scheduler.rs)) — correct `Future` impl with `Arc<Mutex<state>>` shared state, waker stored on first poll, woken on frame complete. Matches Flutter's `Completer<void>? _nextFrameCompleter` semantics with proper Rust async handling.
- **Type-safe duration newtypes** (`Milliseconds`, `Seconds`, `Microseconds`, `Percentage`, `FrameDuration` in [duration.rs](../../crates/flui-scheduler/src/duration.rs)) — Constitution Principle 4 win. `FrameDuration::FPS_60`/`FPS_120`/`FPS_144` const aliases + `from_fps`/`as_ms`/`is_over_budget`/`utilization`/`is_deadline_near`/`is_janky` methods are the right API surface. **Change `Microseconds(i64)` → `Microseconds(u64)` per Step 18 but keep everything else.**
- **`FrameBudget` rolling 60-frame window + running_sum** ([budget.rs:339-353](../../crates/flui-scheduler/src/budget.rs)) — correct O(1) average frame time via running sum. `BudgetPolicy::more_restrictive`/`less_restrictive` navigation gives the right adjacency API.
- **`Priority` enum 4-variant shape** (with Build between Idle and Animation) — deliberate Rust-native divergence from Flutter's 3-priority class. Documented divergence (Step 12 in restructure).
- **`SchedulingStrategy` callback type alias + `default_scheduling_strategy`** ([config.rs:36-47](../../crates/flui-scheduler/src/config.rs)) — matches Flutter's `defaultSchedulingStrategy`. Clean callback shape `Fn(Priority, &Scheduler) -> bool`.
- **`TaskQueue::execute_until(min_priority)` drain-then-execute pattern** ([task.rs:402-421](../../crates/flui-scheduler/src/task.rs)) — drains under one lock acquisition, executes outside the lock. Correct contention pattern.
- **`time_dilation` atomic via `AtomicU64` storing `f64::to_bits`** ([config.rs:60-112](../../crates/flui-scheduler/src/config.rs)) — clean lock-free f64 + epoch reset on change. Matches Flutter's `_timeDilation` semantics with proper Rust atomic handling.
- **`FrameSkipPolicy` + `frames_to_skip` calculation** ([scheduler.rs:200-318](../../crates/flui-scheduler/src/scheduler.rs)) — FLUI-specific perf addition with no Flutter analogue. The 4 policies (Never / CatchUp / SkipToLatest / LimitedSkip) cover the design space well. Tests at [scheduler.rs:1826-1904](../../crates/flui-scheduler/src/scheduler.rs) confirm correctness.
- **`CancellableTransientCallback` + cancellation via `DashMap<CallbackId, ()>`** ([scheduler.rs:91-95, :359](../../crates/flui-scheduler/src/scheduler.rs)) — lock-free in-flight cancellation check during transient callback execution. Correct Rust-native concurrency.
- **`report_timings` batched performance reporting** ([scheduler.rs:1382-1400](../../crates/flui-scheduler/src/scheduler.rs)) — matches Flutter's `_executeTimingsCallbacks` semantics with the right drain-clone-fire pattern.

---

## Priority Order (initial)

1. **Ticker lifecycle integrity** — add `dispose()` + `Drop` + `disposed: AtomicBool` + use-after-dispose asserts (Steps 1-2) + `start` "started twice" debug-assert. Matches PR #84 ChangeNotifier pattern across the workspace.
2. **TickerProvider reshape + ScheduledTicker absorption** (Steps 3-4, 6) — restore Flutter's `createTicker(callback) -> Ticker` signature; eliminate `ScheduledTicker` as separate type; wire `TickerFuture` to Ticker lifecycle.
3. **Delete `TypestateTicker`** (Step 5) — 392 LOC, 0 consumers, blocks doc clarity.
4. **API surface compression** (Steps 8-15) — delete `prelude_advanced`, `Handle<M>`, the 4 ZST priorities, `TypedTask`, 3 ext traits, `ToMilliseconds`/`ToSeconds`, `arc_instance`, `VsyncDrivenScheduler`. ≈ 800 LOC removed.
5. **Principle 6 compliance** (Steps 16-18) — replace 6 `from_u8` panic paths with `try_from_u8(...).unwrap_or(default)`; add `try_*` constructors; change `Microseconds(i64) -> u64`.
6. **Lifecycle parity** (Steps 19-20) — auto-set `frames_enabled` from lifecycle state; mark listener registry for migration.
7. **Performance hot paths** (Steps 21-23) — lock-free `state: AtomicU8`, lock-free `count: AtomicUsize`, persistent-callback registration for Ticker (eliminates per-tick Box alloc).
8. **Dependency cleanup** (Steps 24-25) — drop `crossbeam`, deprecate `wait_for_vsync`.
9. **Regression protection** (Steps 26-30) — tests for new asserts, lifecycle auto-toggle, future cancellation.

---

# Part II — Flutter Cross-Reference

## Section 2 — `flui-scheduler` vs Flutter `scheduler/`

Flutter `scheduler/` is **2,192 LOC across 5 files** (verified via `wc -l`):
- `binding.dart` — 1,470 LOC. `SchedulerBinding` mixin, `SchedulerPhase` enum (5 variants), `PerformanceModeRequestHandle`, `scheduleTask<T>` + `_TaskEntry<T>` priority queue, transient/persistent/post-frame callback registries, `handleBeginFrame`/`handleDrawFrame` orchestration, `scheduleFrame`/`scheduleForcedFrame`/`scheduleWarmUpFrame`, `endOfFrame` Completer, `currentFrameTimeStamp` + `_adjustForEpoch` time-dilation, `addTimingsCallback`/`_executeTimingsCallbacks`, `handleAppLifecycleStateChanged`, `defaultSchedulingStrategy`, `framesEnabled` toggle.
- `ticker.dart` — 554 LOC. `Ticker` (with `_future`/`_muted`/`_animationId`/`_startTime`), `TickerProvider` abstract class (`createTicker(TickerCallback) -> Ticker`), `TickerFuture` (with `_primaryCompleter`/`_secondaryCompleter`/`_completed` tri-state + `orCancel` getter + `whenCompleteOrCancel`), `TickerCanceled` exception, `absorbTicker` state-transfer.
- `priority.dart` — 54 LOC. Open `Priority` class with `idle=0` / `animation=100000` / `touch=200000` constants + `+`/`-` operator overloading clamped to `kMaxOffset=10000`.
- `debug.dart` — 87 LOC. Debug flags `debugPrintBeginFrameBanner`/`debugPrintEndFrameBanner`/`debugPrintScheduleFrameStacks`/`debugTracePostFrameCallbacks` + `debugAssertAllSchedulerVarsUnset`.
- `service_extensions.dart` — 27 LOC. `SchedulerServiceExtensions.timeDilation` enum.

### Coverage table (sampled by symbol)

| Flutter symbol | Location | FLUI equivalent | Status |
|---------------|----------|-----------------|--------|
| `SchedulerPhase` enum (5 variants) | binding.dart:160-199 | `SchedulerPhase` enum (5 variants — same) ([frame.rs:66](../../crates/flui-scheduler/src/frame.rs)) | ✓ direct parity + `can_transition_to` validation (Rust improvement) |
| `SchedulerBinding` mixin | binding.dart:250 | `Scheduler` struct + `BindingBase` impl ([scheduler.rs:421, :1529](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted via `impl_binding_singleton!` macro |
| `SchedulerBinding.instance` | binding.dart:268 | `Scheduler::instance()` via BindingBase | ✓ working — verified at `flui-app/src/bindings/renderer_binding.rs:214` |
| `handleBeginFrame(Duration?)` | binding.dart:1226 | `handle_begin_frame(vsync_time: Instant) -> FrameId` ([scheduler.rs:528](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity; returns FrameId (Rust improvement) |
| `handleDrawFrame()` | binding.dart:1338 | `handle_draw_frame()` ([scheduler.rs:594](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `scheduleFrame()` | binding.dart:946 | `request_frame()` ([scheduler.rs:766](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted (renamed) |
| `scheduleForcedFrame()` | binding.dart:981 | `schedule_forced_frame()` ([scheduler.rs:1312](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `scheduleWarmUpFrame()` | binding.dart:1037 | `schedule_warm_up_frame()` ([scheduler.rs:695](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted — synchronous execution vs Flutter's `PlatformDispatcher.scheduleWarmUpFrame` |
| `scheduleFrameCallback(callback, {rescheduling, scheduleNewFrame})` | binding.dart:608 | `schedule_frame_callback(callback) -> CallbackId` ([scheduler.rs:717](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted; FLUI returns `CallbackId` (Rust improvement) |
| `cancelFrameCallbackWithId(id)` | binding.dart:631 | `cancel_frame_callback(id) -> bool` ([scheduler.rs:744](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `addPersistentFrameCallback(callback)` (un-removable) | binding.dart:781 | `add_persistent_frame_callback(callback) -> CallbackId` + `remove_persistent_frame_callback(id)` ([scheduler.rs:776](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ Removal is **FLUI divergence** ([finding](#-parity-drift--medium-addpersistentframecallback--addpostframecallback-are-unremovable-in-flutter-flui-adds-removability--extra-api-surface)) |
| `addPostFrameCallback(callback, {debugLabel})` (one-shot, un-cancellable) | binding.dart:818 | `add_post_frame_callback(callback) -> CallbackId` + `cancel_post_frame_callback(id)` ([scheduler.rs:800](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ Cancellation is **FLUI divergence** ([finding](#-parity-drift--medium-addpersistentframecallback--addpostframecallback-are-unremovable-in-flutter-flui-adds-removability--extra-api-surface)) |
| `scheduleTask<T>(task, priority, {debugLabel, flow})` | binding.dart:466 | `add_task(priority, callback)` + `Scheduler::task_queue()` ([scheduler.rs:853](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ partial — no Future<T> return, no debugLabel/flow |
| `transientCallbackCount` | binding.dart:574 | `transient_callback_count()` ([scheduler.rs:926](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `debugAssertNoTransientCallbacks(reason)` | binding.dart:657 | `debug_assert_no_transient_callbacks(reason) -> bool` ([scheduler.rs:1425](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted |
| `debugAssertNoPendingPerformanceModeRequests(reason)` | binding.dart:700 | `debug_assert_no_pending_performance_mode_requests(reason)` ([scheduler.rs:1432](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `debugAssertNoTimeDilation(reason)` | binding.dart:714 | `debug_assert_no_time_dilation(reason)` ([scheduler.rs:1442](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `endOfFrame` Future getter | binding.dart:847 | `end_of_frame() -> FrameCompletionFuture` ([scheduler.rs:1251](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted to Rust Future |
| `hasScheduledFrame` | binding.dart:862 | `has_scheduled_frame()` ([scheduler.rs:1286](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `schedulerPhase` getter | binding.dart:866 | `scheduler_phase() -> SchedulerPhase` ([scheduler.rs:1281](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `framesEnabled` getter | binding.dart:872 | `frames_enabled()` ([scheduler.rs:1291](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `ensureVisualUpdate()` | binding.dart:906 | `ensure_visual_update()` ([scheduler.rs:1320](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ FLUI version doesn't check phase to avoid double-scheduling (Flutter does at binding.dart:907-916) |
| `resetEpoch()` | binding.dart:1103 | `reset_epoch()` ([scheduler.rs:1327](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ FLUI doesn't track `_firstRawTimeStampInEpoch` |
| `currentFrameTimeStamp` getter | binding.dart:1132 | `current_frame_time_stamp() -> Duration` ([scheduler.rs:1332](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ partial — FLUI doesn't have raw timestamp tracking |
| `currentSystemFrameTimeStamp` getter | binding.dart:1151 | `current_system_frame_time_stamp() -> Instant` ([scheduler.rs:1338](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted |
| `timeDilation` global + setter | binding.dart:37-53 | `time_dilation() / set_time_dilation(value)` ([config.rs:74, :96](../../crates/flui-scheduler/src/config.rs)) | ✓ adapted via `AtomicU64` of `f64::to_bits` |
| `lifecycleState` getter | binding.dart:395 | `lifecycle_state()` ([scheduler.rs:1110](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted |
| `handleAppLifecycleStateChanged` | binding.dart:414 | `handle_app_lifecycle_state_change` ([scheduler.rs:1138](../../crates/flui-scheduler/src/scheduler.rs)) | ⚠ FLUI adds listener registry (no Flutter analogue at this layer; see [finding](#-parity-drift--medium-lifecycle-listener-registry-doesnt-exist-in-flutters-scheduler--widgetsbindingobserver-is-the-proper-layer)); also missing auto-`framesEnabled` toggle |
| `requestPerformanceMode(mode) -> PerformanceModeRequestHandle?` | binding.dart:1287 | `request_performance_mode(mode) -> PerformanceModeRequestHandle` ([scheduler.rs:1351](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ adapted; uses Drop instead of explicit dispose |
| `PerformanceModeRequestHandle.dispose()` | binding.dart:223 | `impl Drop for PerformanceModeRequestHandle` + explicit `dispose()` ([config.rs:185-198](../../crates/flui-scheduler/src/config.rs)) | ✓ improved — RAII via Drop |
| `addTimingsCallback(cb) / removeTimingsCallback(cb)` | binding.dart:321 / :331 | `add_timings_callback(cb) / remove_timings_callback(cb)` ([scheduler.rs:1365](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ direct parity |
| `initServiceExtensions()` + `timeDilation` service ext | binding.dart:372 | `SERVICE_EXT_TIME_DILATION` const ([config.rs:215](../../crates/flui-scheduler/src/config.rs)) | ⚠ partial — const exists; service registration framework not wired |
| `defaultSchedulingStrategy` | binding.dart | `default_scheduling_strategy` ([config.rs:39](../../crates/flui-scheduler/src/config.rs)) | ✓ adapted |
| `schedulingStrategy` field (overridable) | binding.dart | `SchedulingStrategy` type alias ([config.rs:36](../../crates/flui-scheduler/src/config.rs)) | ⚠ type exists; no setter on Scheduler |
| `lockState(callback)` | (not in current scheduler.dart range — at higher widgets layer) | — | ✗ not in FLUI scheduler (correctly at higher layer) |
| `Ticker` class | ticker.dart:78-408 | `Ticker` + `ScheduledTicker` + `TypestateTicker<S>` ([ticker.rs:176, :610](../../crates/flui-scheduler/src/ticker.rs); [typestate.rs](../../crates/flui-scheduler/src/typestate.rs)) | ⚠⚠ **3 parallel impls** ([finding](#-duplication--critical-three-parallel-ticker-implementations--ticker--scheduledticker--typestatetickers)) |
| `Ticker.start() -> TickerFuture` | ticker.dart:185 | `Ticker::start<F>(callback)` returns `()` ([ticker.rs:207](../../crates/flui-scheduler/src/ticker.rs)) | ⚠⚠ **FLUI doesn't return future**; Flutter assertion against double-start missing |
| `Ticker.stop({canceled})` | ticker.dart:231 | `Ticker::stop()` ([ticker.rs:229](../../crates/flui-scheduler/src/ticker.rs)) | ⚠ FLUI's stop is always-canceled-equivalent; no canceled param; no future propagation |
| `Ticker.dispose()` (mustCallSuper) | ticker.dart:363 | — | ✗✗ **missing** ([finding](#-lifecycle-leak--critical-ticker-has-no-dispose-no-drop-no-disposed-state-assert-pr-84-pattern-missing)) |
| `Ticker.muted` getter/setter | ticker.dart:106 / :119 | `Ticker::mute()/unmute()/is_muted()/toggle_mute()` ([ticker.rs:239-271](../../crates/flui-scheduler/src/ticker.rs)) | ✓ adapted to methods |
| `Ticker.isActive` / `isTicking` getters | ticker.dart:163 / :141 | `Ticker::is_active() / is_running()` ([ticker.rs:313-326](../../crates/flui-scheduler/src/ticker.rs)) | ⚠ partial — `is_ticking` (Flutter: depends on framesEnabled + lifecycle) not implemented |
| `Ticker.shouldScheduleTick` / `scheduleTick` / `unscheduleTick` protected | ticker.dart:270 / :291 / :313 | — (handled internally in `ScheduledTicker::tick_and_reschedule`) | ⚠ internal, no protected hooks |
| `Ticker.absorbTicker(other)` | ticker.dart:330 | — | ✗ missing (needed for `TickerProvider` re-parenting) |
| `TickerProvider` abstract class with `createTicker(callback) -> Ticker` | ticker.dart:43-53 | `TickerProvider` trait with `schedule_tick(Box<dyn FnOnce(f64) + Send>)` ([ticker.rs:80-98](../../crates/flui-scheduler/src/ticker.rs)) | ⚠⚠ **wrong signature** ([finding](#-api-shape--critical-tickerproviderschedule_tick-is-not-flutters-createticker--different-shape-different-lifecycle)) |
| `TickerFuture` with `orCancel`, `whenCompleteOrCancel` | ticker.dart:434-533 | `TickerFuture` + `TickerFutureOrCancel` ([ticker.rs:889, :1101](../../crates/flui-scheduler/src/ticker.rs)) | ✓ adapted via `event_listener::Event`; `set_complete`/`set_canceled` `#[allow(dead_code)]` (not wired to Ticker yet) |
| `TickerCanceled` exception | ticker.dart:537 | `TickerCanceled` struct + Display + Error impls ([ticker.rs:1161](../../crates/flui-scheduler/src/ticker.rs)) | ✓ direct parity |
| `Priority` class (open, idle/animation/touch + `+/-` op) | priority.dart:11-54 | `Priority` enum (Idle/Build/Animation/UserInput) ([task.rs:52](../../crates/flui-scheduler/src/task.rs)) | ⚠ deliberate Rust-native divergence ([finding](#-parity-drift--high-priority-enum-diverges-from-flutter--values-names-and-shape-mismatch)) |
| `SchedulerServiceExtensions.timeDilation` enum | service_extensions.dart:16-27 | `SERVICE_EXT_TIME_DILATION: &str = "timeDilation"` ([config.rs:215](../../crates/flui-scheduler/src/config.rs)) | ✓ adapted as const string |
| `debugPrintBeginFrameBanner` / `debugPrintEndFrameBanner` | debug.dart:39, :45 | — | ✗ no FLUI analogue (debug tracing covers similar) |
| `debugPrintScheduleFrameStacks` | debug.dart:56 | — | ✗ no FLUI analogue |
| `debugTracePostFrameCallbacks` | debug.dart:70 | — | ✗ no FLUI analogue |
| `debugAssertAllSchedulerVarsUnset(reason)` | debug.dart:79 | — | ✗ missing (used by Flutter test framework) |
| — | — | `FrameSkipPolicy` enum ([scheduler.rs:200](../../crates/flui-scheduler/src/scheduler.rs)) | ✓ EARNED ADDITION (no Flutter equivalent; FLUI perf addition) |
| — | — | `VsyncScheduler` + `VsyncMode` + `VsyncStats` ([vsync.rs](../../crates/flui-scheduler/src/vsync.rs)) | ✓ EARNED ADDITION (Flutter integrates via PlatformDispatcher; FLUI wraps via dedicated type) |
| — | — | `FrameBudget` + `PhaseStats` + `BudgetPolicy` ([budget.rs](../../crates/flui-scheduler/src/budget.rs)) | ✓ EARNED ADDITION (Flutter uses TimelineTask; FLUI has explicit budget tracking) |

### Scheduler findings (cross-reference reinforces self-audit)

#### 💀 [PARITY | CRITICAL]: Three Ticker impls where Flutter has one

Cross-reference reinforces [Finding #1](#-duplication--critical-three-parallel-ticker-implementations--ticker--scheduledticker--typestatetickers). Flutter's `Ticker` ([ticker.dart:78-408](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)) is the canonical single Ticker abstraction with:
- Constructor takes the callback (`Ticker(this._onTick, {this.debugLabel})`) — FLUI's `Ticker::new()` takes nothing; callback comes on `start()`.
- `start()` returns `TickerFuture` — FLUI returns `()`.
- `stop({bool canceled = false})` — FLUI's `stop()` has no canceled param.
- `dispose()` is `@mustCallSuper` — FLUI doesn't have dispose.
- `absorbTicker(other)` for re-parenting — FLUI doesn't have it.
- One state representation: `_future: TickerFuture? + _muted: bool` — FLUI has 4-variant runtime enum + parallel typestate.

**Updated verdict:** Consolidate to single Ticker with the Flutter-aligned API: constructor takes callback + optional provider, `start() -> TickerFuture` returns future + asserts not active, `stop({canceled})` propagates cancel state to future, `dispose()` is mandatory cleanup.

#### 💀 [PARITY | HIGH]: `TickerProvider` shape inverts vending model

Cross-reference reinforces [Finding #4](#-api-shape--critical-tickerproviderschedule_tick-is-not-flutters-createticker--different-shape-different-lifecycle). Flutter's `TickerProvider.createTicker(TickerCallback) -> Ticker` is a **factory** — provider vends Ticker, consumer holds and disposes. FLUI's `TickerProvider::schedule_tick(callback)` schedules one callback (closer to "PlatformDispatcher.scheduleFrame" semantics). The mismatch blocks `flui-animation`'s `SingleTickerProviderStateMixin` equivalent from being implementable.

**Updated verdict:** Reshape `TickerProvider` trait per [Step 3](#step-1--ticker-consolidation-largest-semantic-win) of restructure plan.

#### 💀 [PARITY | HIGH]: `Ticker.dispose` mandatory in Flutter; absent in FLUI

Cross-reference reinforces [Finding #3](#-lifecycle-leak--critical-ticker-has-no-dispose-no-drop-no-disposed-state-assert-pr-84-pattern-missing). Flutter's `@mustCallSuper void dispose()` ([ticker.dart:362-379](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)) explicitly cancels the TickerFuture and unschedules pending tick callbacks. The PR #84 ChangeNotifier disposal pattern is the workspace-wide template — Ticker is the obvious next adoption site.

#### 💀 [PARITY | HIGH]: Persistent + post-frame callbacks: removability divergence is deliberate but undocumented

Cross-reference confirms FLUI's [removability divergence](#-parity-drift--medium-addpersistentframecallback--addpostframecallback-are-unremovable-in-flutter-flui-adds-removability--extra-api-surface). Flutter ([binding.dart:773 + :802](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)) is explicit: callbacks cannot be unregistered. FLUI returns `CallbackId` for both. **This is a deliberate Rust ergonomic improvement** — recommended to **keep + document** rather than revert.

#### 💀 [PARITY | HIGH]: `scheduleTask` returns Future<T> in Flutter; FLUI loses return value

Flutter ([binding.dart:466-479](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)):
```dart
Future<T> scheduleTask<T>(
  TaskCallback<T> task,
  Priority priority, {
  String? debugLabel,
  Flow? flow,
}) {
  ...
  return entry.completer.future;
}
```

FLUI ([scheduler.rs:853-855](../../crates/flui-scheduler/src/scheduler.rs)):
```rust
pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
    self.task_queue.add(priority, callback);
}
```

**Gap:** FLUI's `add_task` is fire-and-forget. Flutter's `scheduleTask<T>` returns a `Future<T>` that resolves when the task completes, with the task's return value. The Rust analogue would be `add_task<T>(priority, callback) -> impl Future<Output = T>` — wrapping the callback in a oneshot channel.

**Recommendation:** Add `add_task_returning<T>(priority, callback: impl FnOnce() -> T) -> impl Future<Output = T>` matching Flutter's signature. Keep the fire-and-forget `add_task` for the common case.

#### 💀 [PARITY | MEDIUM]: `ensureVisualUpdate` phase-aware logic missing

Flutter ([binding.dart:906-917](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)) explicitly checks current phase:
```dart
void ensureVisualUpdate() {
  switch (schedulerPhase) {
    case SchedulerPhase.idle:
    case SchedulerPhase.postFrameCallbacks:
      scheduleFrame();
      return;
    case SchedulerPhase.transientCallbacks:
    case SchedulerPhase.midFrameMicrotasks:
    case SchedulerPhase.persistentCallbacks:
      return;  // Frame is already in progress
  }
}
```

FLUI ([scheduler.rs:1320-1322](../../crates/flui-scheduler/src/scheduler.rs)):
```rust
pub fn ensure_visual_update(&self) {
    self.schedule_frame_if_enabled();
}
```

FLUI unconditionally calls `schedule_frame_if_enabled` regardless of current phase. If called during `TransientCallbacks`/`MidFrameMicrotasks`/`PersistentCallbacks`, FLUI sets `frame_scheduled=true` even though a frame is in progress. The next frame after the current one will fire — possibly with stale work.

**Recommendation:** Port Flutter's phase-check pattern.

#### 💀 [PARITY | MEDIUM]: `resetEpoch` doesn't track `_firstRawTimeStampInEpoch`

Flutter ([binding.dart:1082-1106](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)):
```dart
Duration? _firstRawTimeStampInEpoch;
Duration _epochStart = Duration.zero;
Duration _lastRawTimeStamp = Duration.zero;

void resetEpoch() {
  _epochStart = _adjustForEpoch(_lastRawTimeStamp);
  _firstRawTimeStampInEpoch = null;
}
```

FLUI ([scheduler.rs:1327-1329](../../crates/flui-scheduler/src/scheduler.rs)):
```rust
pub fn reset_epoch(&self) {
    *self.binding.epoch_start.lock() = Duration::ZERO;
}
```

FLUI sets `epoch_start` to ZERO unconditionally — Flutter computes `_adjustForEpoch(_lastRawTimeStamp)` to preserve continuity. The `_firstRawTimeStampInEpoch` tracking ensures monotonic adjusted timestamps across `timeDilation` changes — without it, `set_time_dilation(2.0)` followed by a frame produces a non-monotonic timestamp jump.

**Recommendation:** Add `last_raw_timestamp: Mutex<Duration>` + `first_raw_in_epoch: Mutex<Option<Duration>>` fields to `BindingState`. Update `handle_begin_frame` to track these. Reimplement `reset_epoch` per Flutter's formula.

#### 💀 [PARITY | LOW]: `scheduleWarmUpFrame` doesn't lock events / capture timeline task

Flutter ([binding.dart:1037-1080](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart)) calls `PlatformDispatcher.instance.scheduleWarmUpFrame` to lock event dispatching during warm-up and uses `TimelineTask` for profiling. FLUI's `schedule_warm_up_frame` ([scheduler.rs:695-703](../../crates/flui-scheduler/src/scheduler.rs)) just synchronously calls `execute_frame()`. This is acceptable for the current MVP — platform-level event-lock comes when flui-platform's vsync integration matures.

#### 💀 [PARITY | LOW]: Debug flags missing — `debugPrintBeginFrameBanner` / `debugPrintScheduleFrameStacks` / `debugTracePostFrameCallbacks`

Flutter's debug.dart 4 boolean flags have no FLUI analogue. `tracing` crate provides similar instrumentation via `#[tracing::instrument]` (used at [scheduler.rs:527-680](../../crates/flui-scheduler/src/scheduler.rs)), but the targeted "print at begin/end frame" pattern Flutter uses for visual debug is missing.

**Recommendation:** Defer until devtools materializes. The tracing instrumentation covers structured logging; the Flutter-style print banners are visual-debug-specific.

#### ✓ EARNED ADDITION: `SchedulerPhase::can_transition_to` validation

Flutter has scattered `assert(schedulerPhase == X)` checks (e.g., binding.dart:1253, :1339). FLUI's [`can_transition_to`](../../crates/flui-scheduler/src/frame.rs) consolidates these into a structured FSM transition table validated by `set_scheduler_phase`. Rust-native improvement.

#### ✓ EARNED ADDITION: Three-Arc state consolidation

Flutter's `SchedulerBinding` mixin scatters fields across the binding (~30+ direct fields). FLUI's `FrameState` / `CallbackState` / `BindingState` triplet ([scheduler.rs:321-394](../../crates/flui-scheduler/src/scheduler.rs)) groups related state into 3 Arc-wrapped structs. Reduces allocation count from 6+ separate `Arc<Mutex>`/`Arc<AtomicX>` to 3 Arcs. Clean Rust-native shape.

#### ✓ EARNED ADDITION: Type-safe duration newtypes

Flutter uses `Duration` everywhere. FLUI splits into `Milliseconds(f64)` / `Seconds(f64)` / `Microseconds(i64)` / `Percentage(f64)` / `FrameDuration` ([duration.rs](../../crates/flui-scheduler/src/duration.rs)). Constitution Principle 4 win — prevents unit-mixing bugs at the type level. (Subject to `Microseconds(i64) -> u64` fix per [Step 18](#step-3--principle-6--lifecycle-compliance).)

#### ✓ EARNED ADDITION: `FrameSkipPolicy` enum

Flutter's frame-skip behavior is implicit in the engine. FLUI's `FrameSkipPolicy::{Never, CatchUp, SkipToLatest, LimitedSkip}` ([scheduler.rs:200-318](../../crates/flui-scheduler/src/scheduler.rs)) gives explicit control over catch-up behavior when behind on vsync. No Flutter analogue. Keep.

#### ✓ EARNED ADDITION: `FrameBudget` rolling 60-frame average via running sum

Flutter's `TimelineTask`-based reporting is for devtools. FLUI's `FrameBudget` ([budget.rs:339-353](../../crates/flui-scheduler/src/budget.rs)) maintains a rolling 60-frame window with O(1) average via `running_sum += new - evicted`. Genuinely useful runtime metric. Keep.

#### Coverage summary

**~85%** of Flutter scheduler API present in FLUI. The 15% gap concentrates in:
- **Ticker lifecycle**: missing `dispose`, missing `absorbTicker`, missing future-return from `start`, missing canceled-stop param (CRITICAL — Finding #1+#3+#4).
- **TickerProvider shape**: wrong signature (CRITICAL — Finding #4).
- **scheduleTask Future return**: missing — fire-and-forget only (HIGH — Section 2 finding).
- **resetEpoch monotonicity**: missing `_firstRawTimeStampInEpoch` (MEDIUM — Section 2 finding).
- **ensureVisualUpdate phase-check**: missing (MEDIUM — Section 2 finding).
- **Debug print flags**: missing 4 booleans (LOW).

The 15% **divergence** (deliberate Rust-native shape) concentrates in:
- Closed `Priority` enum vs Flutter's open class (HIGH — documented divergence).
- Persistent/post-frame callback removability (MEDIUM — documented divergence).
- Lifecycle listener registry on scheduler vs binding layer (MEDIUM — temporary, plan migration).

---

# Appendix A — Investigation Trail

## Tool dispatches

- **Initial structural pass** (sequential Read on `lib.rs`, `Cargo.toml`, then each src file in order):
  - `scheduler.rs` (2,274 LOC) read in two passes (1-1621, 1622-2273) due to 30k token cap.
  - `ticker.rs` (1,386 LOC) read in single pass.
  - `frame.rs` (879 LOC), `vsync.rs` (723 LOC), `budget.rs` (764 LOC), `task.rs` (638 LOC), `config.rs` (337 LOC), `duration.rs` (758 LOC), `id.rs` (334 LOC), `traits.rs` (335 LOC), `typestate.rs` (392 LOC).
  - `Cargo.toml` (46 LOC).

- **Flutter cross-reference** (absolute paths to gitignored `.flutter/`):
  - `flutter/lib/src/scheduler/priority.dart` (54 LOC) — full read.
  - `flutter/lib/src/scheduler/service_extensions.dart` (27 LOC) — full read.
  - `flutter/lib/src/scheduler/debug.dart` (87 LOC) — full read.
  - `flutter/lib/src/scheduler/ticker.dart` (554 LOC) — full read.
  - `flutter/lib/src/scheduler/binding.dart` (1,470 LOC) — targeted reads at lines 160-230, 443-670, 700-1050, 1050-1400 + grep `SchedulerPhase|handleBeginFrame|handleDrawFrame|scheduleFrame|scheduleTask|addPostFrameCallback|addPersistentFrameCallback|...`.

- **Targeted grep passes** to verify findings:
  - Workspace consumer scan: `rg -l "flui_scheduler::|flui-scheduler" --type rust --glob '!crates/flui-scheduler/**'` → list of 8 files in flui-animation (disabled) + 6 files in flui-app + 2 files in flui-foundation.
  - Ticker consumer scan: `rg -l "Ticker\b|TickerProvider|TickerFuture|TypestateTicker|ScheduledTicker" --type rust --glob '!crates/flui-scheduler/**'` → 4 files in flui-animation (disabled), 1 file in flui-foundation (re-export).
  - Zombie type scan: `rg -n "FrameHandle|TaskHandle|FrameSkipPolicy|MergedListenable|VsyncDrivenScheduler|TypestateTicker|UserInputPriority|AnimationPriority|BuildPriority|IdlePriority|PriorityExt|FrameBudgetExt|FrameTimingExt|ToMilliseconds|ToSeconds|prelude_advanced|PriorityLevel\b|PerformanceMode\b|TimingsCallback\b|SchedulingStrategy" --type rust --glob '!crates/flui-scheduler/**'` → **0 hits** for all listed types. Confirmed full zombie set.
  - Principle 6 violation scan: `rg -n "from_u8|try_from_u8|unimplemented!|todo!|\.unwrap\(\)" crates/flui-scheduler/src/` → 6 production `from_u8` panic call sites in `scheduler.rs`, plus tests-only unwraps.
  - `arc_instance` caller scan: `rg -n "ARC_INSTANCE|arc_instance\(\)|Scheduler::arc_instance"` → 0 callers outside flui-scheduler/src/scheduler.rs itself.
  - `crossbeam` usage scan in flui-scheduler/src/ → 0 hits.
  - `Microseconds` constructor scan: `rg -n "Microseconds::new"` → all producers write non-negative values (verified inline).
  - `flui-app::Scheduler::instance()` usage: `rg -n "Scheduler::instance"` → 8 production call sites in flui-app/src/, confirms the BindingBase singleton is the real production singleton.

- **File-size confirmation**: `wc -l crates/flui-scheduler/src/*.rs` → 9,064 total LOC across 12 source files. `wc -l .flutter/.../scheduler/*.dart` → 2,192 total LOC across 5 Flutter files.

## Workspace state at audit time (2026-05-21)

- Worktree: `determined-proskuriakova-d2eccf`.
- `crates/flui-scheduler` **ACTIVE** in workspace `Cargo.toml` members.
- Rust edition: 2024; minimum rust-version: 1.94.
- Sister audit in flight: `flui-interaction` (separate agent — out of scope here).
- flui-scheduler dependencies: `flui-foundation` (only flui-* dep), `parking_lot`, `dashmap`, `crossbeam` (unused), `event-listener` 5.3, `tracing`, `web-time` 1.1, `serde` (optional).
- Reverse dependencies in workspace (active crates):
  - `flui-app` ([`bindings/`, `embedder/`, `app/`](../../crates/flui-app/src/)) — primary consumer; uses `Scheduler::instance()` via BindingBase.
  - `flui-foundation` ([`src/id.rs`, `src/lib.rs`](../../crates/flui-foundation/src/)) — re-exports `Ticker` markers (FrameId, TaskId, TickerId, FrameCallbackId).
- Reverse dependencies in workspace (disabled crates):
  - `flui-animation` ([entire crate](../../crates/flui-animation/src/)) — imports `Ticker`, `TickerProvider`, `TickerFuture`, `TickerCallback`, `TaskQueue`, `VsyncCallback`, `VsyncScheduler`. Currently `# "crates/flui-animation"` in workspace Cargo.toml.

## Files referenced

Repo-relative paths (clickable in markdown viewers):

- [`Cargo.toml`](../../Cargo.toml) — workspace root
- [`CLAUDE.md`](../../CLAUDE.md) — engineering standards
- [`STRATEGY.md`](../../STRATEGY.md) — "Behavior loyal, structure Rust-native" + "Sync hot path, async на краях"
- [`docs/research/2026-05-21-view-tree-foundation-audit.md`](2026-05-21-view-tree-foundation-audit.md) — shape reference (frontmatter, finding format, Project Map ASCII, Dead Code Table, Restructuring Plan, What to Preserve, Priority Order)
- [`crates/flui-scheduler/Cargo.toml`](../../crates/flui-scheduler/Cargo.toml)
- [`crates/flui-scheduler/src/lib.rs`](../../crates/flui-scheduler/src/lib.rs)
- [`crates/flui-scheduler/src/scheduler.rs`](../../crates/flui-scheduler/src/scheduler.rs)
- [`crates/flui-scheduler/src/ticker.rs`](../../crates/flui-scheduler/src/ticker.rs)
- [`crates/flui-scheduler/src/frame.rs`](../../crates/flui-scheduler/src/frame.rs)
- [`crates/flui-scheduler/src/vsync.rs`](../../crates/flui-scheduler/src/vsync.rs)
- [`crates/flui-scheduler/src/budget.rs`](../../crates/flui-scheduler/src/budget.rs)
- [`crates/flui-scheduler/src/task.rs`](../../crates/flui-scheduler/src/task.rs)
- [`crates/flui-scheduler/src/config.rs`](../../crates/flui-scheduler/src/config.rs)
- [`crates/flui-scheduler/src/duration.rs`](../../crates/flui-scheduler/src/duration.rs)
- [`crates/flui-scheduler/src/id.rs`](../../crates/flui-scheduler/src/id.rs)
- [`crates/flui-scheduler/src/traits.rs`](../../crates/flui-scheduler/src/traits.rs)
- [`crates/flui-scheduler/src/typestate.rs`](../../crates/flui-scheduler/src/typestate.rs)
- [`crates/flui-app/src/bindings/renderer_binding.rs`](../../crates/flui-app/src/bindings/renderer_binding.rs) — primary Scheduler consumer
- [`crates/flui-app/src/embedder/embedder_scheduler.rs`](../../crates/flui-app/src/embedder/embedder_scheduler.rs)
- [`crates/flui-app/src/app/runner.rs`](../../crates/flui-app/src/app/runner.rs)
- [`crates/flui-app/src/app/binding.rs`](../../crates/flui-app/src/app/binding.rs)
- [`crates/flui-foundation/src/id.rs`](../../crates/flui-foundation/src/id.rs) — re-exported markers
- [`crates/flui-animation/src/lib.rs`](../../crates/flui-animation/src/lib.rs) — disabled, primary Ticker consumer

Flutter reference (absolute paths — outside the worktree, at main repo root, gitignored):

- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart` — 1,470 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart` — 554 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/scheduler/priority.dart` — 54 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/scheduler/debug.dart` — 87 LOC
- `C:/Users/vanya/RustroverProjects/flui/.flutter/flutter-master/packages/flutter/lib/src/scheduler/service_extensions.dart` — 27 LOC

---
