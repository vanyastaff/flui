# Animation Architecture

Internal architecture of `flui_animation`.

## System Overview

`flui_animation` is the framework-agnostic animation **engine**. The widget
layer that consumes it (`AnimatedFoo`, `AnimatedBuilder`, implicit animations,
`TickerProvider` ownership) is **planned, not yet implemented** — shown dashed
below.

```
┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐
   widget layer (planned, not yet built)
│  AnimatedFoo, AnimatedBuilder, ImplicitAnimations        │
└ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┬ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
                       │ will use
┌──────────────────────▼──────────────────────────────────┐
│                 flui_animation                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Stateful: Animation<T>, AnimationController,       │ │
│  │           CurvedAnimation, TweenAnimation          │ │
│  ├────────────────────────────────────────────────────┤ │
│  │ Data: Curve, Tween, AnimationStatus, Simulation    │ │
│  └────────────────────────────────────────────────────┘ │
└──────────────────────┬──────────────────────────────────┘
                       │ uses
┌──────────────────────▼──────────────────────────────────┐
│              flui-scheduler                             │
│  UpdateScheduler, Ticker, FrameBudget, Priority         │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

```
src/
├── lib.rs            # Public API, re-exports
│
├── animation.rs      # Animation<T> trait, AnimationDirection
├── controller.rs     # AnimationController (main driver)
├── builder.rs        # AnimationControllerBuilder
│
├── curved.rs         # CurvedAnimation (applies curve)
├── tween.rs          # TweenAnimation<T> (maps to type T)
├── reverse.rs        # ReverseAnimation (inverts value)
├── proxy.rs          # ProxyAnimation (hot-swappable parent)
├── compound.rs       # CompoundAnimation (combine with operators)
├── constant.rs       # ConstantAnimation (fixed value)
├── switch.rs         # AnimationSwitch (crossover switching)
│
├── curve.rs          # Curve trait, Curves, implementations
├── tween_types.rs    # Animatable, Tween, all tween types
├── status.rs         # AnimationStatus, AnimationBehavior
├── simulation.rs     # Simulation trait, Spring, Friction, Gravity
│
├── ext.rs            # AnimatableExt, AnimationExt, CurveExt
└── error.rs          # AnimationError
```

## Core Abstractions

### Animation<T> Trait

The central abstraction:

```rust
pub trait Animation<T>: Listenable + Send + Sync + Debug
where
    T: Clone + Send + Sync + 'static,
{
    /// Current value
    fn value(&self) -> T;
    
    /// Current status (Forward, Reverse, Completed, Dismissed)
    fn status(&self) -> AnimationStatus;
    
    /// Listen to status changes
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;
    fn remove_status_listener(&self, id: ListenerId);
}
```

Design decisions:
- **Generic over T** — any value type (f32, Color, Size)
- **Extends Listenable** — integrates with change notification system
- **Send + Sync** — thread-safe by default
- **Debug required** — all animations inspectable

### Curve Trait

Maps unit interval to unit interval:

```rust
pub trait Curve {
    /// Transform t ∈ [0,1] → output ∈ [0,1]
    /// Contract: transform(0) = 0, transform(1) = 1
    fn transform(&self, t: f32) -> f32;
    
    fn flipped(self) -> FlippedCurve<Self>;
    fn reversed(self) -> ReverseCurve<Self>;
}
```

### Animatable<T> and Tween<T> Traits

```rust
/// Maps t ∈ [0,1] → value of type T
pub trait Animatable<T>: Clone + Send + Sync + Debug {
    fn transform(&self, t: f32) -> T;
}

/// Animatable with explicit begin/end
pub trait Tween<T>: Animatable<T> {
    fn begin(&self) -> &T;
    fn end(&self) -> &T;
    fn lerp(&self, t: f32) -> T;
}
```

### Simulation Trait

Physics-based value generation:

```rust
pub trait Simulation: Send + Sync {
    /// Position at time t
    fn x(&self, time: f32) -> f32;
    
    /// Velocity at time t
    fn dx(&self, time: f32) -> f32;
    
    /// Has simulation settled?
    fn is_done(&self, time: f32) -> bool;
    
    fn tolerance(&self) -> Tolerance;
}
```

## AnimationController Internals

### State

```rust
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationControllerInner {
    // Current state
    value: f32,
    status: AnimationStatus,
    direction: AnimationDirection,
    
    // Configuration
    duration: Duration,
    reverse_duration: Option<Duration>,
    lower_bound: f32,
    upper_bound: f32,
    
    // Animation state. Time comes from the ticker's elapsed seconds (scaled by
    // time_dilation), not wall-clock Instants: `restart_ticker` always begins a
    // fresh run's Ticker at elapsed zero, so that elapsed time IS the elapsed
    // time since the run started — no per-run epoch to subtract.
    run_duration: Option<Duration>, // per-run override (animate_to), never clobbers `duration`
    start_value: f32,
    target_value: f32,
    
    // Physics
    simulation: Option<Box<dyn Simulation>>,
    
    // Repeat: `None` outside a repeat run, so "repeating with no
    // configuration" is unrepresentable. value/status/direction
    // are a pure function of elapsed time since the run started
    // (`RepeatRun::initial_ns`, the phase the run started at, plus that
    // elapsed time, reduced modulo `RepeatRun::period_ns` in integer
    // nanoseconds) — no incremental per-cycle bookkeeping.
    repeat: Option<RepeatRun>,
    
    // Ticker
    ticker: Option<Ticker>,
    
    // Listeners
    status_listeners: Vec<(ListenerId, StatusCallback)>,
    
    // Lifecycle
    disposed: bool,
}

// `period_ns` is always `> 0` — constructed only after `repeat_with`'s
// zero-period and zero-count degenerate cases have already settled
// synchronously and returned. See `AnimationControllerInner::repeat_sample`.
struct RepeatRun {
    reverse: bool,
    min: f32,
    max: f32,
    period_ns: u128,
    count: Option<u32>,
    initial_ns: u128,
}
```

### State Machine

```
                forward()
    ┌─────────────────────────────────────┐
    │                                     ▼
┌───────────┐                       ┌───────────┐
│ Dismissed │◄──────────────────────│  Forward  │
│  (0.0)    │      reaches 0.0      │ (running) │
└───────────┘                       └─────┬─────┘
    ▲                                     │
    │               reaches 1.0           │
    │         ┌───────────────────────────┘
    │         ▼
    │   ┌───────────┐
    │   │ Completed │
    │   │   (1.0)   │
    │   └─────┬─────┘
    │         │ reverse()
    │         ▼
    │   ┌───────────┐
    └───│  Reverse  │
        │ (running) │
        └───────────┘
```

### Tick Cycle

Each frame (via the scheduler-driven `Ticker`):

1. Lock `inner`
2. Calculate elapsed time
3. Update `value` based on duration/simulation
4. Check for completion, update `status`
5. Unlock `inner`
6. Notify value listeners (via ChangeNotifier)
7. Notify status listeners (if status changed)

```rust
fn tick(&self, delta: Duration) {
    let (new_status, should_notify_status) = {
        let mut inner = self.inner.lock();
        // Update value and status
        // ...
        (inner.status, status_changed)
    };
    // Lock released
    
    self.notifier.notify_listeners();
    
    if should_notify_status {
        self.notify_status_listeners(new_status);
    }
}
```

## Mapping decisions

### `AnimationController` owns the one future each run resolves

**Rule:** every run-starting method (`forward`, `forward_from`, `reverse`,
`reverse_from`, `animate_to`, `animate_back`, `animate_to_curved`,
`animate_back_curved`, `repeat`, `repeat_with`, `fling`, `fling_with`,
`animate_with`, `animate_back_with`) returns
`Result<TickerFuture, AnimationError>`: `Err` means the run could not start
(the controller is disposed); the `TickerFuture` is how the run ends,
`Ok(())` on a normal finish and `Err(TickerCanceled)` when it is superseded
or torn down. `AnimationControllerInner.active_run: Option<TickerCompleter>`
is the one completer this controller ever holds; every run-ending or
run-starting site funnels through `AnimationController::finish`, the single
chokepoint that owns `drop(inner)` then delivers — see
`docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md`
for why resolution moved here rather than staying on `Ticker` (a lock-order
fact, not a Flutter divergence).

**Per-site table** (guard held → what happens → delivered after unlock):

| Site | Publishes | Displaces |
|---|---|---|
| `forward_from`/`reverse_from`/`drive_to`/`repeat_with`/`fling_with`/`drive_simulation` (non-settling path) | nothing (installs a fresh completer) | previous `active_run`, canceled |
| `forward_from`/`reverse_from`/`drive_to`'s zero-distance-or-zero-duration settle (`settle_at_target`, see its own entry below) | the trivial run, complete; value notified only if it actually moved | previous `active_run`, canceled |
| `tick_time_based` (non-repeating end) | the finishing run, complete | — |
| `tick_repeat` (exhausted end) | the finishing run, complete | — |
| `tick_simulation` (`is_done`) | the finishing run, complete | — |
| `stop`/`set_value` (`stop_running`) | — | `active_run`, canceled |
| `reset` (`stop_running`) | — | `active_run`, canceled |
| `dispose` | — | `active_run`, canceled |
| last `Arc<Mutex<Inner>>` drop (no explicit `dispose()`) | — | `active_run`'s own `Drop`, canceled — reachable only for `without_ticker`(`_bounds`) controllers; `new` **and** `with_detached_ticker` both install a real `Ticker`, and once a run starts `restart_ticker` gives it a callback capturing `self.clone()` regardless of whether that ticker is scheduler-driven or detached, so both hold `inner.ticker → callback → controller clone → inner` — a cycle that never reaches zero strong references without `dispose()` |

**Publish-before-listeners.** A natural end (`tick_time_based`,
`tick_simulation`) takes `active_run` and calls
`TickerCompleter::complete()` **before** `drop(inner)` — the same guard scope
that sets `status`. Flutter's `_tick` completes its `Completer` before
`notifyListeners()`/`_checkStatusChanged()` too
(`animation_controller.dart:951`); only *delivery* (continuations, wakers) is
deferred past the unlock. This is what makes a panicking status listener
leave the run `Ok`: the unwind drops the `TickerDelivery` `finish` was mid-way
through handing off, which delivers the already-published outcome instead of
losing it.

**Status-before-cancel.** A run-starting site displaces `active_run` under
the lock, but `finish` fires the run's own (new) status listeners **before**
delivering the displaced run's cancellation — the observable order a caller
sees is "the new run started" then "the old one was canceled", matching
Flutter's own sync-status/microtask-cancel split.

### Direction is chosen by the method; a run ends in its direction's settled status

**Rule:** `animate_to`/`animate_to_curved` always run `Forward`, and
`animate_back`/`animate_back_curved` always run `Reverse`, regardless of
whether `target` is above or below the current value. Flutter documents
this exactly: `animateTo`'s status "is reported as forward regardless of
whether target > value or not", completed at the end; `animateBack` is the
reverse/dismissed mirror (`AnimationController.animateTo`/`animateBack`,
`animation_controller.dart` @ 3.44.0). Every run ends in its direction's
settled status with no bound check
(`AnimationDirection::settled_status`: `Forward` → `Completed`, `Reverse` →
`Dismissed`), at every run end: `tick_time_based`'s non-repeat end,
`tick_repeat`'s exhaustion end, `tick_simulation`'s `is_done`, and
`settle_at_target`. Flutter's `_tick` applies the identical rule
unconditionally, so `animate_to(lower_bound)` from mid-range ends
`Completed`, not `Dismissed` — and the same holds through
`tick_simulation`: `animate_with(sim)` landing on a BOUNDED controller's
lower bound also ends `Completed`. A fling's own end is unaffected:
`fling`/`fling_with` pick `direction` from the sign of `velocity`, not from
where the simulation happens to land.

**Divergence removed.** `drive_to` previously derived direction from
`target >= value` (travel, not the method) — an unrecorded divergence that
made `animate_to`/`animate_back` differ only in default duration and
discarded the one bit of information the caller's choice of method carries
(flutter#158233's complaint). Two real consumers call `animate_to_curved`/
`animate_back_curved` toward a value that can land on either side of the
current one: `scroll_controller.rs`'s ballistic fling (`animate_to_curved`
toward an arbitrary pixel offset) and `flui-cupertino`'s `CupertinoButton`
(`animate_to_curved` for both the press-in fade toward `1.0` and, kept for
oracle parity, the release fade toward `0.0`). The scroll controller's
status listener matches `Completed | Dismissed` identically (it only ends
the scroll activity), so its `animate_to` toward a SMALLER pixel value
reporting `Forward`/`Completed` instead of `Reverse`/`Dismissed` changes
nothing observable. The button's release fade is the consumer the rule
reached: its start was chained off a status listener watching `Completed`,
which stops distinguishing "the press just landed" from "the release just
landed" once both report `Completed`, so the listener re-triggered a
redundant zero-distance settle when the release it started reached its own
end (no observable trace — same-status writes are deduplicated — but the
wrong shape). It now chains the release on the press fade's own
`TickerFuture` (`chain_release_fade`, `crates/flui-cupertino/src/button.rs`),
`Ok`-only and one-shot: Flutter's own `ticker.then(...)` shape.

**`stop()`/`set_value` keep the bounds-first rule.**
`AnimationControllerInner::settled_status_directed`/`settled_status_keep_direction`
still check `is_at_upper_bound`/`is_at_lower_bound` first, falling back to
direction only for a non-bound stop. This is FLUI's own frame-driver
contract, not a Flutter one: a scroll gesture's every-frame `set_value` must
report the bound it actually reached, not the gesture's nominal direction,
so a driver polling `status().is_running()` sees a real settle. Flutter's
own `stop()` changes no status at all.

### `settle_at_target` also covers zero-duration runs, and gates value notification on real movement

**Rule (extends the per-site table above):** `settle_at_target` is the single
settle chokepoint for BOTH zero-DISTANCE runs (`forward()` already at the
upper bound) and zero-DURATION runs (`forward(..., Some(Duration::ZERO))`, or
a zero base `duration`): `forward_from`/`reverse_from`/`drive_to` compute the
run duration before their settle check and gate on
`distance < BOUND_EPSILON || run_duration.is_zero()`, Flutter's own
`simulationDuration == Duration.zero` test
(`AnimationController._animateToInternal`, `animation_controller.dart` @
3.44.0, covering both causes identically).

It also notifies value listeners only when the value actually moved,
measured against the value at METHOD ENTRY, before
`forward_from(Some(x))`/`reverse_from(Some(x))` apply `from`. A settle-time
comparison against the post-`from` value would miss a jump:
`forward_from(Some(1.0))` from `0.3` lands exactly on the target it was told
to jump to, so comparing the post-`from` value to the target always reads
"unchanged" there. Flutter: `if (value != target) { …; notifyListeners(); }`
(`_animateToInternal`, same file).

**The same entry-value rule extends to the NON-settling path.** A real run
that still applies `from` (`forward_from(Some(x))`/`reverse_from(Some(x))`
when the resulting distance and duration are both nonzero) notifies iff
`from` actually moved the value, narrower than Flutter's `forward`/
`reverse`, whose `if (from != null) { value = from; }` goes through the
`value=` setter and notifies UNCONDITIONALLY. One case is worth naming
because it looks surprising at first: `forward_from(Some(0.0))` from `1.0`
with a Duration::ZERO base ends at `1.0` (a Forward run settles at the
UPPER bound, never at `from`) and fires NO value notification at all — net
unchanged, even though Flutter's own path fires the `value=` setter's
notification twice (once for the jump to `0.0`, once for `_animateToInternal`
snapping back to `1.0`); both observers read `1.0` at the end either way.

### `dispose` does not settle the status; `Vsync` polls for an installed run, not a running status

**`dispose` does not settle the status.** `dispose()` disposes the ticker,
cancels the active run, and clears listeners; it never touches `status`,
Flutter parity — `AnimationController.dispose` disposes the ticker and
clears listeners only (`animation_controller.dart` @ 3.44.0). A controller
disposed mid-run therefore keeps whatever status it had (e.g. `Forward`):
`hero_flight.rs`'s deferred replay reads `proxy.status()` after a flight's
controller may already be disposed, and a manufactured settled status would
be visible there.

**`status().is_running()` is not proof a run is installed; `active_run.is_some()`
is.** Two independent paths leave `status` reporting a RUNNING value
(`Forward`/`Reverse`) with no run actually installed: a mid-run `dispose()`
(above), and `set_value` at an interior value. `set_value` calls
`stop_running()`, clearing `active_run`, but still reports a directional
running status (Flutter parity, `AnimationControllerInner::settled_status_keep_direction`).

A `Vsync`-driven controller polled on `status().is_running()` after a
mid-run `set_value` would therefore look like it still has a run to
advance. The NEXT `tick_all` would then recompute its value from the
stale, already-stopped run's `start_value`/`target_value`, silently
overwriting what `set_value` had just set.

`AnimationController::walk_probe`'s `live_running` is
`!disposed && active_run.is_some()`, the two facts that are always
consistent with "is there a run to advance". Both
`AnimationController::tick_at`'s own guard and `Vsync::has_running`/
`tick_all` read it instead of bare `status().is_running()`.

### Repeat sampling is a pure function of elapsed time

**The FLUI defect fixed (#1078).** A repeat's `value`/`status`/`direction`
used to be advanced incrementally, one retired cycle at a time
(`repeat_done: u32`, `run_epoch_secs` re-zeroed per cycle). That
bookkeeping was not wrong about WHICH cycle a boundary-crossing tick
retired, or about how many cycles a long frame spanned — a dropped-frame
tick correctly walked forward by whole cycles, and exhaustion had its own
separately-maintained parity correction that landed on the right leg. The
bug was narrower: a boundary-crossing tick REPORTED only the cycle
boundary it landed on and deferred the fractional remainder past that
boundary to the NEXT tick, instead of interpolating through it in the same
call. `tick_at(1.25)` as a single call (no prior ticks, 1s period, restart
mode) reset to the cycle-restart value `0.0`, discarding the `0.25` of
elapsed time past the boundary; `tick_at(1.0); tick_at(1.25)` — the
identical total elapsed time, split across two calls — correctly
interpolated to `0.25` on the second call. Same elapsed time, different
answer depending on the caller's frame partition: #1078's own reproduction
probes. `AnimationController::tick_repeat` replaces the incremental model
with one computation: `total_ns` (elapsed nanoseconds since the run
started, plus `RepeatRun::initial_ns`) reduced modulo `RepeatRun::period_ns` gives
the cycle index and phase directly, so `tick_at(1.25)` gives the identical
answer regardless of whether an intervening `tick_at(1.0)` happened.
`AnimationControllerInner::repeat_leg`/`repeat_landing`/`repeat_sample` are
the one place the leg/landing/phase parity lives now; every call site
(`repeat_with`'s value-at-the-call sample, `tick_repeat`'s running leg and
exhaustion landing) defers to them instead of re-deriving it.

**Initial phase = Flutter parity.** A repeat starts from the CURRENT value
clamped into `[min, max]`, not from `min` — matching
`AnimationController._startSimulation`'s `_value = x(0.0)`
(`animation_controller.dart` @ 3.44.0). This is not merely oracle fidelity:
a `repeat()` issued fresh on every widget build (a common pattern for a
looping indicator built imperatively rather than held across rebuilds)
progresses from wherever the animation currently is instead of snapping
back to the start every time it is called.

**IMPROVEMENT over Flutter: exhaustion lands on the last cycle's endpoint,
with that leg's own settled status.** Flutter's `_RepeatingSimulation.x`
wraps with `% 1.0`, so a 1-count restart repeat's exit value is exactly
`min` at `completed` (`animation_controller_test.dart`'s "calling repeat by
setting count as valid with reverse as false" expects `0` at 100ms even
though the run is reported `completed`, not `dismissed`), and a bounce
exhaustion reports the direction of the NEXT leg it never runs (`count: 1`
bounce ends `dismissed` at value `1.0`). FLUI lands on the endpoint its own
final retired leg actually reached, with that leg's own direction settled
(`Completed` at `max`, `Dismissed` at `min`) — matching the Web Animations
spec's fill behavior ("holding the endpoint of the final iteration rather
than the start of the next"), Android's `ValueAnimator`, and Compose's
`VectorizedRepeatableSpec`, all of which land on the actual endpoint. The
replaced oracle is `repeat_restart_finite_count_exhausts_from_the_phase_origin`
and `repeat_bounce_flutter_oracle_finite_count_and_absolute_time_rewind`'s
exhaustion assertion (`crates/flui-animation/src/controller.rs`), which pin
the new landing/status instead of Flutter's wrap.

**`min == max` stays rejected.** Flutter permits the degenerate range
(`assert(max >= min)`); FLUI does not, the same contract `with_bounds`
already applies to an empty range (`repeat_with_rejects_equal_min_and_max`
pins it) — a repeat that can structurally never change value is a caller
error that would hold the frame loop open forever doing nothing, and
failing fast beats a silent no-op animation.

**Zero effective period, or an explicit zero count, both settle
synchronously at the call — two DISTINCT degenerate cases.** A zero
effective period (any `count`): Android's `ValueAnimator.animateBasedOnTime`
explicitly "ignores the repeat count and skips to the end" for a
0-duration animator; Compose's `InfiniteRepeatableSpec` throws
("Animation to be infinitely repeated cannot have a 0-duration",
`AnimationSpec.kt`'s `init` block); Flutter asserts. FLUI repairs rather
than rejects (the house rule — see `with_bounds`'s own `InvalidBounds`
contract for the cases that DO reject): an infinite zero-period repeat
that ticked once per frame would hold the frame loop open forever doing
nothing, so `repeat_with` settles it synchronously through the same
`settle_at_target` chokepoint a zero-distance/zero-duration
`forward`/`reverse`/`animate_to` uses, landing on `repeat_landing`'s
endpoint for the finite count (or cycle 0's end for an infinite one) — a
documented exception to "an infinite repeat's future resolves only by
cancellation". `count: Some(0)` (any period), separately: zero CYCLES run
at all, so there is no cycle to land on — the settle value is the plain
CLAMPED CURRENT value, not a landing jump, status `Completed` (Web
Animations semantics for an empty active interval; Flutter asserts
`count > 0`, Compose throws for `iterations < 1`).

**One period for both legs of a bounce; `set_duration` does not retime an
active repeat.** `RepeatRun::period` is resolved ONCE at the call
(`period.unwrap_or(duration)`), not read live on every tick — Flutter
parity, `AnimationController.repeat`'s `period ??= duration` captured by
the simulation. The previous behavior fell through to
`current_duration()`'s live `duration`/`reverse_duration` lookup whenever
`period` defaulted, so the reverse leg of a bounce with a defaulted period
could pick up `reverse_duration` instead of the forward leg's period
(contradicting `repeat_with`'s own "defaults to the forward duration"
doc), and a live `set_duration` mid-repeat changed the running period out
from under it. Resolving once fixes both, and keeps the modular-nanosecond
arithmetic in `tick_repeat` exact (one divisor for every leg).

**`velocity()` on a reverse leg is SIGNED** — a deliberate divergence from
Flutter's `_RepeatingSimulation.dx`, which is always positive
(`(max-min)/period`). FLUI's `velocity()` reports `range / duration` off
the leg's own `start_value`/`target_value`, which are swapped on a reverse
leg, so it comes out negative — matching FLUI's non-repeat velocity
contract rather than introducing a repeat-specific special case. Pinned by
`repeat_reverse_leg_velocity_is_negative`.

### Unbounded is a constructor fact; bound-targeting runs on it are refused; no path reads NaN

**Issue #1183; flutter/flutter#76014** (open: Flutter itself calls
`unbounded`'s behavior under `forward`/`reverse`/`repeat` "simply not
defined"). `AnimationController::unbounded`/`unbounded_without_ticker`/
`unbounded_with_detached_ticker` fix bounds at `(f32::NEG_INFINITY,
f32::INFINITY)` and are infallible: unboundedness is a constructor fact,
never a bound VALUE.

`with_bounds`/`without_ticker_bounds`/`with_detached_ticker_bounds` (and
`AnimationControllerBuilder::bounds`, which duplicates the same check) now
REJECT any bound that is not finite, including a wide-open
`(NEG_INFINITY, INFINITY)` pair, AND reject a pair whose finite endpoints
still overflow `f32` as a RANGE (`(-f32::MAX, f32::MAX)`; a bounded run's
`target - value`/`target - start` arithmetic needs the SPAN to be finite,
not just each endpoint).

**FLUI's own rule.** Flutter's `AnimationController` constructor asserts
only `upperBound >= lowerBound`: it accepts a non-finite (infinite) pair
and an EQUAL pair, and rejects an inverted pair only in debug (the assert
is compiled out in release, so an inverted pair is silently accepted there
too); nothing in the constructor rejects `NaN` specifically, though a NaN
bound makes that same `>=` comparison false and so trips the assert in
debug the same way an inverted pair does. Only the dedicated `unbounded`
factory fixes `+-inf`.

A half-open pair (one bound finite, one infinite) is rejected the same way
as a wide-open one: nothing in this workspace needs it, and allowing it
would make the "bounded means finite" rule incidental complexity with no
consumer.

**Initial value and status: parity, then a recorded cost.** `value = 0.0`
(never `lower_bound`, which is `-inf`) is Flutter's own `unbounded` doc
(`animation_controller.dart` @ 3.44.0). The initial `status` is computed by
the SAME rule `set_value` applies,
`AnimationControllerInner::settled_status_keep_direction`, applied once at
construction for every constructor, not hard-coded `Dismissed`: a bounded
controller at `lower_bound` still reports `Dismissed` (unchanged), but an
unbounded one at `0.0`, with `AnimationDirection` defaulted `Forward` and
neither infinite bound ever "at", reports **`Forward`**.

This IS Flutter parity: `AnimationController._internalSetValue`
(`animation_controller.dart` @ 3.44.0) falls to the same directional
`switch` FLUI's `settled_status_keep_direction` mirrors at `value == 0.0`,
since that value is neither the lower nor upper bound in the general case.

The recorded cost: a never-run unbounded controller reports
`status().is_running() == true` from the moment it is constructed. The
real "is a run installed" fact stays `is_animating()`/the internal
`active_run`, never `status`, exactly as
[`AnimationController::walk_probe`]'s own doc already documented before
this change (`set_value`'s interior-value status already had the identical
property).

One direct, load-bearing consequence: **the first `stop()` on a fresh
unbounded controller emits `Forward → Completed`.**
`settled_status_directed`'s bound checks are both false on an infinite
range, so it falls to `match direction { Forward => Completed }`. Every
`jump_to` on a fresh `Scrollable`/`RefreshIndicator` (both migrated to
`unbounded_without_ticker` below) triggers exactly this event through the
fling controller's `stop_hook`.

**Refusals: `NonFiniteTarget`, additive `#[non_exhaustive]` variant.**
Derived from `!lower_bound.is_finite()` (both bounds are `+-inf` together
by construction, so either alone detects it): `forward`/`forward_from`,
`reverse`/`reverse_from`, `fling`/`fling_with`, and `repeat`/`repeat_with`
whose EFFECTIVE range (`min`/`max` defaulted against this controller's own
bounds) is still non-finite are refused on an unbounded controller. There
is no finite bound/range to run to. `repeat_with(Some(0.0), Some(1.0), ..)`
on an unbounded controller WORKS: an explicit finite range is not
"unbounded" in the relevant sense.

**On ANY controller** (bounded too: FLUI's declared behavior CHANGE, since
`f32::clamp` today passes `NaN` through unchanged and PANICS on a NaN
*bound* even in release), `animate_to`/`animate_back`(`_curved`) refuse a
non-finite `target`, and `forward_from`/`reverse_from` refuse a non-finite
`from`. `NaN` always refuses; `+-inf` clamps to the bound it points at
when that bound is finite (the "go to the end" idiom, unchanged) and
refuses when that bound is itself infinite.

`fling`/`fling_with` additionally refuse a non-finite `velocity` (a NaN
velocity took the `Forward` branch and built a spring whose `is_done`
never fires). `animate_to`/`animate_back` also refuse when
`target - value` overflows `f32` (an extreme `set_value` followed by an
extreme `animate_to`), and `drive_simulation`/`animate_with` refuse a
simulation whose `x(0.0)` is already non-finite.

Every refusal runs BEFORE any state mutation. `fling_with` in particular
used to write `inner.direction` before its `InvalidSpring` check,
corrupting direction on a refused fling (observable only via a LATER
`stop()`); fixed by computing `direction`/`target` into locals and
assigning only once every check passes.

Every refusal is also emitted as a `tracing::warn!`, once per call, after
the guard drops (`warn_non_finite_target`, mirroring `warn_if_no_ticker`'s
pattern): every production caller of these methods discards the `Result`
(`let _ = fling.animate_to_curved(..)`, `let _ =
fc_fling.animate_with(sim)`), and a silent no-op here would hide the
caller bug exactly the way Flutter's own `∞` jump does.

**`repeat_with`'s NaN endpoint is `InvalidBounds`, a range-SHAPE error,
distinct from the unbounded-range `NonFiniteTarget` above.** A
caller-supplied `Some(f32::NAN)` `min`/`max` is checked BEFORE defaulting
against the controller's own bounds, on ANY controller: it widens
`lo >= hi`'s inversion check to include non-finite endpoints (`!(lo < hi)
|| !lo.is_finite() || !hi.is_finite()`).

Unguarded, `repeat_with(Some(f32::NAN), ..)` reached
`inner.value.clamp(lo, hi)` with `lo = NaN` as the clamp's own `min`
PARAMETER and PANICKED (`f32::clamp` asserts `min <= max`), worse than
silently installing a broken run.

The unbounded-range case (both `min`/`max` unset, or one side unset on an
unbounded controller) is checked SEPARATELY, after the shape check, and
maps to `NonFiniteTarget` instead: a range that is not inverted but has a
side that defaulted through an infinite bound has a shape problem of a
different kind. There is no finite range to repeat inside, not an invalid
one.

**`set_value`/simulation NaN canonicalization stays infallible, latched.**
`set_value` on a BOUNDED controller keeps its existing canonicalization
(`NaN` → `lower_bound`, `+-inf` → the bound it points at): a declared PIN.
On an UNBOUNDED controller a non-finite input is a FULL no-op instead: no
`stop_running`, no notification, the value stays exactly what it was. A
poisoned gesture drag or a bad computation must not clobber a live fling
or snap a scrollable to a bound it doesn't have.

A mid-run simulation sample going non-finite (`tick_simulation`) ENDS the
run at the LAST FINITE value: settled status by direction, `active_run`
completed (`Ok`), ticker stopped, rather than "value unchanged, run
continues" (the latter would leave `active_run` installed, `Vsync`
ticking forever, and a scrollable's `is_scrolling` stuck). This is new
behavior on this path, not a preserved pin: before this change, a running
simulation's sample had no non-finite handling at all and reached `clamp`
unchanged, which returns a `NaN` self as-is.

Both paths share ONE latch, `AnimationControllerInner::non_finite_warned:
bool`, fired at most once per controller (`warn_non_finite_value`),
distinct from `NonFiniteTarget`'s per-call warn: a NaN-producing drag or a
misbehaving simulation must warn once, not every frame.

**`tick_time_based` reads `start_value`/`target_value` DIRECTLY at the
exact endpoints (`t <= 0.0` / `t >= 1.0`), never via `start + range *
eased_t` there.** Flutter's `_InterpolationSimulation.x` already
special-cases the endpoints to the exact begin/end value structurally, but
FLUI's previous shape only special-cased `eased_t` (to exactly `0.0`/`1.0`)
while still computing the product: `range` can be `+-inf` in principle
(the span-overflow case above), and `inf * 0.0 = NaN`.

Reading the field directly at the boundary removes the multiplication from
that path entirely, so no path through `tick_at` can ever compute or store
a NaN value: the issue's own stated criterion.

**Consumer migration.** `scrollable.rs`'s ballistic fling controller and
`refresh_indicator.rs`'s both migrate from
`without_ticker_bounds(1ms, NEG_INFINITY, INFINITY).expect(..)` to
`unbounded_without_ticker(1ms)`: the `.expect` (previously provably
infallible only because the literal pair was hardcoded) is gone with the
fallible constructor it guarded.

`scroll_controller.rs`'s `service_pending_command` raises
`ScrollPosition::set_is_scrolling(true)` only AFTER
`fling.animate_to_curved(..)` returns `Ok` AND the resulting status is
still running. A refused start (a non-finite target reaching this call)
must not park the scrollable in "scrolling" forever, and a start that
settles SYNCHRONOUSLY (target already equals the just-synced current
value) already reported its own end through the fling status listener
installed above; checking `is_running()` rather than firing
unconditionally on `Ok` keeps that already-correct settle from being
clobbered back to `true`.

**`reset()` on an unbounded controller lands on `0.0`, not `-inf`.** This
is the one place this change diverges from a literal reading of Flutter's
own `unbounded` doc (which implies `-inf`/`+inf` as the "ends"):
flutter#76014's reporter asks for exactly this defined beginning, and
`0.0` is already the value construction itself starts an unbounded
controller at.

`0.0` is NOT the value every non-finite-input path canonicalizes toward in
general: a BOUNDED controller's `set_value(NaN)` still canonicalizes to
`lower_bound` (whatever that is, not necessarily `0.0`), and an unbounded
controller's `set_value`/simulation-sample non-finite input is a full
no-op that leaves `value` wherever it already was, never snapping it to
`0.0`. `reset()` never fails for non-finiteness on any controller: a
reset always has a value to land on.

**Cost (2): status at `0.0` is path-dependent.** `reset()` on an unbounded
controller lands at `0.0` with status `Dismissed` (its documented
contract); construction, or a later `set_value(0.0)`, lands at the SAME
`0.0` with status `Forward` (the keep-direction rule above). Two different
routes to the identical value report two different statuses: a caller
that only inspects `value() == 0.0` cannot infer `status()` from it on an
unbounded controller the way it could reason "at the lower bound implies
Dismissed" on a bounded one.

**Tests inverted, not fixed.** `without_ticker_bounds_rejects_wide_open_ones`
and its `with_detached_ticker_bounds` twin used to assert that a wide-open
`(NEG_INFINITY, INFINITY)` pair was ACCEPTED, landing `value() ==
NEG_INFINITY`. Under this change that same input is REJECTED, and the
tests were rewritten (not merely patched) to assert the rejection plus the
unbounded constructor's own `0.0` start, each documenting why the old
assertion no longer holds.

## Composition Model

Animations compose via `Arc<dyn Animation<f32>>`:

```
AnimationController (produces 0.0 → 1.0)
        │
        ▼ Arc<dyn Animation<f32>>
CurvedAnimation (applies easing curve)
        │
        ▼ Arc<dyn Animation<f32>>
TweenAnimation<Color> (maps to Color)
        │
        ▼ Animation<Color>
```

Each wrapper stores parent as `Arc<dyn Animation<f32>>`:

```rust
pub struct CurvedAnimation<C: Curve> {
    parent: Arc<dyn Animation<f32>>,
    curve: C,
    reverse_curve: Option<C>,
}

impl<C: Curve> Animation<f32> for CurvedAnimation<C> {
    fn value(&self) -> f32 {
        let t = self.parent.value();
        self.curve.transform(t)
    }
    
    fn status(&self) -> AnimationStatus {
        self.parent.status()  // Pass through
    }
}
```

## Thread Safety

All types are `Send + Sync`. Synchronization strategy:

| Component | Mechanism | Rationale |
|-----------|-----------|-----------|
| Controller state | `Mutex<Inner>` | Single lock, batch updates |
| Value listeners | `ChangeNotifier` | Separate from state lock |
| Status listeners | Inside `Inner` | Updated with state |
| Disposed flag | Inside `Inner` | Checked under lock |

Using `parking_lot::Mutex`:
- 2-3x faster than std
- No poisoning on panic
- Smaller memory footprint

## Error Handling

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimationError {
    InvalidBounds,      // lower >= upper
    InvalidValue,       // value outside bounds
    InvalidDuration,    // duration <= 0
    AlreadyDisposed,    // operation on disposed controller
    AlreadyAnimating,   // conflicting animation command
    TickerError,        // scheduler/ticker failure
}
```

Design:
- `#[non_exhaustive]` — can add variants without breaking
- `Clone` — shareable across threads
- All fallible operations return `Result<_, AnimationError>`

## Memory Management

### Arc Sharing

```rust
let controller = Arc::new(AnimationController::new(...));
let curved = Arc::new(CurvedAnimation::new(controller.clone(), curve));
let tweened = TweenAnimation::new(tween, curved.clone());
```

Benefits:
- Cheap cloning (pointer copy + atomic increment)
- Automatic cleanup on last reference drop
- Thread-safe sharing

### Explicit Disposal

Controllers require explicit disposal:

```rust
controller.dispose();
```

After disposal:
- All operations return `Err(AnimationError::AlreadyDisposed)`
- Ticker stopped and dropped
- Listeners cleared

Why not just Drop?
- `Drop` can't return errors
- `Drop` takes `&mut self`, not compatible with `Arc<Self>`
- Explicit disposal can be called safely multiple times

## Extension Traits

Add fluent APIs without cluttering core types:

### AnimationExt

```rust
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    fn curved<C: Curve>(self: Arc<Self>, curve: C) -> Arc<CurvedAnimation<C>>;
    fn reversed(self: Arc<Self>) -> Arc<ReverseAnimation>;
    fn add(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> Arc<CompoundAnimation>;
    // ...
}

impl<A: Animation<f32> + 'static> AnimationExt for A {}
```

### AnimatableExt

```rust
pub trait AnimatableExt<T>: Animatable<T> {
    fn animate<A: Animation<f32>>(self, parent: Arc<A>) -> TweenAnimation<T, Self>;
    fn chain<B: Animatable<T>>(self, next: B) -> ChainedTween<Self, B>;
    fn with_curve<C: Curve>(self, curve: C) -> ChainedTween<CurveTween<C>, Self>;
    fn reversed(self) -> ReverseTween<T, Self>;
}
```

### CurveExt

```rust
pub trait CurveExt: Curve + Sized {
    fn into_tween(self) -> CurveTween<Self>;
    fn then<C: Curve>(self, next: C) -> ChainedCurve<Self, C>;
}
```
