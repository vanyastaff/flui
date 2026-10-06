# Animation Architecture

Internal architecture of `flui_animation`.

## System Overview

`flui_animation` supplies animation values, controllers, curves and simulations.
The widget layer consumes them through `flui-widgets`' `animated` and
`transitions` modules: implicit animations own controllers, while transition
widgets and `AnimatedBuilder` observe existing animations. Ticker ownership
comes from the scheduler and the widget's vsync scope.

```text
flui-widgets: animated / transitions
                   │
                   ▼
flui-animation: Animation<T> / AnimationController / Curve / Simulation
                   │
                   ▼
flui-scheduler: Ticker / Scheduler
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
- **Generic over T** — any value type (f64, Color, Size)
- **Extends Listenable** — integrates with change notification system
- **Send + Sync** — thread-safe by default
- **Debug required** — all animations inspectable

### Curve Trait

Maps unit interval to unit interval:

```rust
pub trait Curve {
    /// Transform t ∈ [0,1] → output ∈ [0,1]
    /// Contract: transform(0) = 0, transform(1) = 1
    fn transform(&self, t: f64) -> f64;
    
    fn flipped(self) -> FlippedCurve<Self>;
    fn reversed(self) -> ReverseCurve<Self>;
}
```

### Animatable<T> and Tween<T> Traits

```rust
/// Maps t ∈ [0,1] → value of type T
pub trait Animatable<T>: Clone + Send + Sync + Debug {
    fn transform(&self, t: f64) -> T;
}

/// Animatable with explicit begin/end
pub trait Tween<T>: Animatable<T> {
    fn begin(&self) -> &T;
    fn end(&self) -> &T;
    fn lerp(&self, t: f64) -> T;
}
```

### Simulation Trait

Physics-based value generation:

```rust
pub trait Simulation: Send + Sync {
    /// Position at time t
    fn x(&self, time: f64) -> f64;
    
    /// Velocity at time t
    fn dx(&self, time: f64) -> f64;
    
    /// Has simulation settled?
    fn is_done(&self, time: f64) -> bool;
    
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
    value: f64,
    status: AnimationStatus,
    direction: AnimationDirection,
    
    // Configuration
    duration: Duration,
    reverse_duration: Option<Duration>,
    lower_bound: f64,
    upper_bound: f64,
    
    // Animation state. Time comes from the ticker's elapsed seconds (scaled by
    // time_dilation), not wall-clock Instants: `restart_ticker` always begins a
    // fresh run's Ticker at elapsed zero, so that elapsed time IS the elapsed
    // time since the run started — no per-run epoch to subtract.
    run_duration: Option<Duration>, // per-run override (animate_to), never clobbers `duration`
    start_value: f64,
    target_value: f64,
    
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
    min: f64,
    max: f64,
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

### Terminal owners retire outside their guards

Controllers, proxies, curved animations and switches withdraw their callbacks,
simulations, curves and subscriptions from shared state before dropping them, so
no destructor runs under an internal lock. User parent removal and queries also
run outside those locks. A registration is owned by the animation as soon as it
is accepted, so a constructor that fails later still detaches it. After the
first destructor failure in a retirement, or while the thread is already
panicking, the remaining owned values are retained rather than dropped
([ADR-0127](../../../docs/adr/ADR-0127-exceptional-path-retention.md)); the
first failure propagates. Callbacks are thread-shared `Arc`s, so a snapshot
clone cannot be proven non-last and is retained too.
Tested by `controller_sources_allow_reentry_and_preserve_run_ownership`.

`Split` keeps both curves in private fields, behind checked construction and
the borrowed accessors `split()`, `begin_curve()` and `end_curve()`. It is
`Clone` but no longer `Copy`, and keeps its serde field names. Deserialization
commits decoded curves only once the whole value is valid, so a failing partial
value cannot replace the decoding or range error.

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
fact).

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
that sets `status`, before listeners are notified; only *delivery*
(continuations, wakers) is deferred past the unlock. This is what makes a panicking status listener
leave the run `Ok`: the unwind drops the `TickerDelivery` `finish` was mid-way
through handing off, which delivers the already-published outcome instead of
losing it.

**Status-before-cancel.** A run-starting site displaces `active_run` under
the lock, but `finish` fires the run's own (new) status listeners **before**
delivering the displaced run's cancellation — the observable order a caller
sees is "the new run started" then "the old one was canceled".

### Direction is chosen by the method; a run ends in its direction's settled status

**Rule:** `animate_to`/`animate_to_curved` always run `Forward`, and
`animate_back`/`animate_back_curved` always run `Reverse`, regardless of
whether `target` is above or below the current value: the caller's choice of
method is the one bit of direction information it carries. Every run ends in its direction's
settled status with no bound check
(`AnimationDirection::settled_status`: `Forward` → `Completed`, `Reverse` →
`Dismissed`), at every run end: `tick_time_based`'s non-repeat end,
`tick_repeat`'s exhaustion end, `tick_simulation`'s `is_done`, and
`settle_at_target`. The rule is unconditional, so `animate_to(lower_bound)` from mid-range ends
`Completed`, not `Dismissed` — and the same holds through
`tick_simulation`: `animate_with(sim)` landing on a BOUNDED controller's
lower bound also ends `Completed`. A fling's own end is unaffected:
`fling`/`fling_with` pick `direction` from the sign of `velocity`, not from
where the simulation happens to land.

**Why not derive direction from travel.** Deriving direction from
`target >= value` would make `animate_to`/`animate_back` differ only in
default duration and discard the one bit of information the caller's choice of
method carries. Two real consumers call `animate_to_curved`/
`animate_back_curved` toward a value that can land on either side of the
current one: `scroll_controller.rs`'s ballistic fling (`animate_to_curved`
toward an arbitrary pixel offset) and `flui-cupertino`'s `CupertinoButton`
(`animate_to_curved` for both the press-in fade toward `1.0` and the
release fade toward `0.0`). The scroll controller's
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
`TickerFuture` (`chain_release_fade`, `packages/flui-cupertino/src/button.rs`),
`Ok`-only and one-shot.

**`stop()`/`set_value` keep the bounds-first rule.**
`AnimationControllerInner::settled_status_directed`/`settled_status_keep_direction`
still check `is_at_upper_bound`/`is_at_lower_bound` first, falling back to
direction only for a non-bound stop. This is FLUI's own frame-driver
contract: a scroll gesture's every-frame `set_value` must
report the bound it actually reached, not the gesture's nominal direction,
so a driver polling `status().is_running()` sees a real settle.

### `settle_at_target` also covers zero-duration runs, and gates value notification on real movement

**Rule (extends the per-site table above):** `settle_at_target` is the single
settle chokepoint for BOTH zero-DISTANCE runs (`forward()` already at the
upper bound) and zero-DURATION runs (`forward(..., Some(Duration::ZERO))`, or
a zero base `duration`): `forward_from`/`reverse_from`/`drive_to` compute the
run duration before their settle check and gate on
`distance < BOUND_EPSILON || run_duration.is_zero()`, which covers both causes
identically.

It also notifies value listeners only when the value actually moved,
measured against the value at METHOD ENTRY, before
`forward_from(Some(x))`/`reverse_from(Some(x))` apply `from`. A settle-time
comparison against the post-`from` value would miss a jump:
`forward_from(Some(1.0))` from `0.3` lands exactly on the target it was told
to jump to, so comparing the post-`from` value to the target always reads
"unchanged" there.

**The same entry-value rule extends to the NON-settling path.** A real run
that still applies `from` (`forward_from(Some(x))`/`reverse_from(Some(x))`
when the resulting distance and duration are both nonzero) notifies iff
`from` actually moved the value, and never notifies unconditionally. One case is worth naming
because it looks surprising at first: `forward_from(Some(0.0))` from `1.0`
with a Duration::ZERO base ends at `1.0` (a Forward run settles at the
UPPER bound, never at `from`) and fires NO value notification at all — net
unchanged; an observer reads `1.0` before and after the call.

### `dispose` does not settle the status; `Vsync` polls for an installed run, not a running status

**`dispose` does not settle the status.** `dispose()` disposes the ticker,
cancels the active run, and clears listeners; it never touches `status`. A controller
disposed mid-run therefore keeps whatever status it had (e.g. `Forward`):
`hero_flight.rs`'s deferred replay reads `proxy.status()` after a flight's
controller may already be disposed, and a manufactured settled status would
be visible there.

**`status().is_running()` is not proof a run is installed; `active_run.is_some()`
is.** Two independent paths leave `status` reporting a RUNNING value
(`Forward`/`Reverse`) with no run actually installed: a mid-run `dispose()`
(above), and `set_value` at an interior value. `set_value` calls
`stop_running()`, clearing `active_run`, but still reports a directional
running status (`AnimationControllerInner::settled_status_keep_direction`).

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

**Initial phase.** A repeat starts from the CURRENT value
clamped into `[min, max]`, not from `min`: a `repeat()` issued fresh on every widget build (a common pattern for a
looping indicator built imperatively rather than held across rebuilds)
progresses from wherever the animation currently is instead of snapping
back to the start every time it is called.

**Exhaustion lands on the last cycle's endpoint,
with that leg's own settled status.** Wrapping with `% 1.0` would make a
1-count restart repeat's exit value exactly `min` at `completed`, and a
bounce exhaustion would report the direction of the NEXT leg it never runs
(`count: 1` bounce ends `dismissed` at value `1.0`). FLUI lands on the
endpoint its own final retired leg actually reached, with that leg's own
direction settled (`Completed` at `max`, `Dismissed` at `min`) — matching
the Web Animations spec's fill behavior ("holding the endpoint of the final iteration rather
than the start of the next"), Android's `ValueAnimator`, and Compose's
`VectorizedRepeatableSpec`, all of which land on the actual endpoint.
The tests `repeat_tick_at_infinity_exhausts_a_finite_count_instead_of_rewinding`
and `repeat_bounce_finite_count_and_absolute_time_rewind`'s
exhaustion assertion (`crates/flui-animation/src/controller_tests.rs`) pin
the landing/status.
(The `repeat_*` and `without_ticker_bounds_*` names in this document are rows of the table tests
`repeat_contract` and `controller_contract` in
`src/controller_tests.rs`; a failure names its row.)

**`min == max` stays rejected.** The degenerate range is refused, the same contract `with_bounds`
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
`AnimationSpec.kt`'s `init` block). FLUI repairs rather
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
Animations semantics for an empty active interval; Compose throws for
`iterations < 1`).

**One period for both legs of a bounce; `set_duration` does not retime an
active repeat.** `RepeatRun::period` is resolved ONCE at the call
(`period.unwrap_or(duration)`), not read live on every tick. Falling
through to `current_duration()`'s live `duration`/`reverse_duration` lookup
whenever `period` defaulted would let the reverse leg of a bounce with a defaulted period
could pick up `reverse_duration` instead of the forward leg's period
(contradicting `repeat_with`'s own "defaults to the forward duration"
doc), and a live `set_duration` mid-repeat change the running period out
from under it. Resolving once avoids both, and keeps the modular-nanosecond
arithmetic in `tick_repeat` exact (one divisor for every leg).

**`velocity()` on a reverse leg is SIGNED.** `velocity()` reports `range / duration` off
the leg's own `start_value`/`target_value`, which are swapped on a reverse
leg, so it comes out negative — matching FLUI's non-repeat velocity
contract rather than introducing a repeat-specific special case. Pinned by
`repeat_reverse_leg_velocity_is_negative`.

### Unbounded is a constructor fact; bound-targeting runs on it are refused; no path reads NaN

**Issue #1183.** `AnimationController::unbounded`/`unbounded_without_ticker`/
`unbounded_with_detached_ticker` fix bounds at `(f64::NEG_INFINITY,
f64::INFINITY)` and are infallible: unboundedness is a constructor fact,
never a bound VALUE.

`with_bounds`/`without_ticker_bounds`/`with_detached_ticker_bounds` (and
`AnimationControllerBuilder::bounds`, which duplicates the same check) now
REJECT any bound that is not finite, including a wide-open
`(NEG_INFINITY, INFINITY)` pair, AND reject a pair whose finite endpoints
still overflow `f64` as a RANGE (`(-f64::MAX, f64::MAX)`; a bounded run's
`target - value`/`target - start` arithmetic needs the SPAN to be finite,
not just each endpoint).

**The rule.** A bounded constructor accepts only a finite, non-inverted pair (NaN
included in what is refused). Only the dedicated `unbounded` factory fixes
`+-inf`.

A half-open pair (one bound finite, one infinite) is rejected the same way
as a wide-open one: nothing in this workspace needs it, and allowing it
would make the "bounded means finite" rule incidental complexity with no
consumer.

**Initial value and status, and a recorded cost.** `value = 0.0`
(never `lower_bound`, which is `-inf`). The initial `status` is computed by
the SAME rule `set_value` applies,
`AnimationControllerInner::settled_status_keep_direction`, applied once at
construction for every constructor, not hard-coded `Dismissed`: a bounded
controller at `lower_bound` still reports `Dismissed` (unchanged), but an
unbounded one at `0.0`, with `AnimationDirection` defaulted `Forward` and
neither infinite bound ever "at", reports **`Forward`**.

`settled_status_keep_direction` falls to its directional `switch` at
`value == 0.0`, since that value is neither the lower nor upper bound in the
general case.

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
`f64::clamp` today passes `NaN` through unchanged and PANICS on a NaN
*bound* even in release), `animate_to`/`animate_back`(`_curved`) refuse a
non-finite `target`, and `forward_from`/`reverse_from` refuse a non-finite
`from`. `NaN` always refuses; `+-inf` clamps to the bound it points at
when that bound is finite (the "go to the end" idiom, unchanged) and
refuses when that bound is itself infinite.

`fling`/`fling_with` additionally refuse a non-finite `velocity` (a NaN
velocity took the `Forward` branch and built a spring whose `is_done`
never fires). `animate_to`/`animate_back` also refuse when
`target - value` overflows `f64` (an extreme `set_value` followed by an
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
caller bug.

**`repeat_with`'s NaN endpoint is `InvalidBounds`, a range-SHAPE error,
distinct from the unbounded-range `NonFiniteTarget` above.** A
caller-supplied `Some(f64::NAN)` `min`/`max` is checked BEFORE defaulting
against the controller's own bounds, on ANY controller: it widens
`lo >= hi`'s inversion check to include non-finite endpoints (`!(lo < hi)
|| !lo.is_finite() || !hi.is_finite()`).

Unguarded, `repeat_with(Some(f64::NAN), ..)` reached
`inner.value.clamp(lo, hi)` with `lo = NaN` as the clamp's own `min`
PARAMETER and PANICKED (`f64::clamp` asserts `min <= max`), worse than
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
eased_t` there.** Special-casing only `eased_t` (to exactly `0.0`/`1.0`)
while still computing the product is not enough: `range` can be `+-inf` in principle
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
is a defined beginning rather than `-inf`, and
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

### Friction numerical range

Friction displacement uses `exp_m1(log_drag * time)`, and inverse arrival time
uses `ln_1p` of the relative displacement. This retains small finite movement
as drag approaches one without changing constructor validation, velocity
sampling or the existing near-origin inverse threshold. Public consumer test
`friction_preserves_small_decay_and_position_time_roundtrips` checks the
constant-velocity limit, positive and negative normal flings, position/time
round trips and unreachable/non-finite queries.

### Smoothing survives an idle tick

`SmoothDamp`'s overshoot guard places the follower exactly at its target and
sets its velocity to zero. That assignment does not divide by elapsed time:
a zero-duration tick while at rest must leave the follower usable for its
next target. `damped_motion_remains_usable_after_idle_ticks` checks positive
and negative retargeting after a zero-duration idle tick, with an ordinary
idle tick as a control, through the public smoothing API.

## Composition Model

Animations compose via `Arc<dyn Animation<f64>>`:

```
AnimationController (produces 0.0 → 1.0)
        │
        ▼ Arc<dyn Animation<f64>>
CurvedAnimation (applies easing curve)
        │
        ▼ Arc<dyn Animation<f64>>
TweenAnimation<Color> (maps to Color)
        │
        ▼ Animation<Color>
```

Each wrapper stores parent as `Arc<dyn Animation<f64>>`:

```rust
pub struct CurvedAnimation<C: Curve> {
    parent: Arc<dyn Animation<f64>>,
    curve: C,
    reverse_curve: Option<C>,
}

impl<C: Curve> Animation<f64> for CurvedAnimation<C> {
    fn value(&self) -> f64 {
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

`parking_lot::Mutex` does not poison on panic. Controller code must restore its
own invariants and invoke user callbacks outside the state lock; choosing this
primitive is not a measured throughput claim.

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
pub trait AnimationExt: Animation<f64> + Sized + 'static {
    fn curved<C: Curve>(self: Arc<Self>, curve: C) -> Arc<CurvedAnimation<C>>;
    fn reversed(self: Arc<Self>) -> Arc<ReverseAnimation>;
    fn add(self: Arc<Self>, other: Arc<dyn Animation<f64>>) -> Arc<CompoundAnimation>;
    // ...
}

impl<A: Animation<f64> + 'static> AnimationExt for A {}
```

### AnimatableExt

```rust
pub trait AnimatableExt<T>: Animatable<T> {
    fn animate<A: Animation<f64>>(self, parent: Arc<A>) -> TweenAnimation<T, Self>;
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

### Proxy queries release the parent guard before user code

A custom `Animation` may replace a proxy's parent from its `value` or `status`
query. Proxy queries clone the current parent under the read guard, then invoke
that parent after the guard is released. The query returns the sampled parent's
result; the next query observes the replacement. The old-status sample during
`set_parent` follows the same rule. This does not serialize concurrent setters
or claim that their separate subscription swaps are atomic.

`proxy_parent_queries_allow_reentrant_replacement` uses public custom animation
implementations for value, status and old-status-during-swap reentry, then checks
the next parent change still delivers notifications. Each case runs in a bounded
child process so a reverted read guard cannot hang the parent test runner.

### Integer tweens cover the full endpoint range

`IntTween` and `StepTween` convert both integer endpoints to `f64` before
subtracting them. Every `i32` endpoint is exactly representable there, whereas
subtracting `i32::MIN` from `i32::MAX` in the integer domain panics or wraps.
Existing rounding, flooring and progress clamping remain deliberate.
The public consumer family `integer_tweens_interpolate_across_the_full_range`
checks both directions across the full range and ordinary rounding.

### Weighted progress uses relative weights and exact endpoints

`TweenSequence` revalidates each item's finite positive weight after caller edits
to the public item fields. Evaluation scales weights by the largest weight, so
finite inputs whose raw sum overflows still describe usable relative durations.
The `total_weight` accessor retains the original sum and may return infinity;
it does not drive interpolation. Exact progress endpoints return the first and
last tween's endpoints. Interior progress divides by the actual relative weight,
without an arbitrary epsilon that discards short segments. A relative interval
that underflows to zero cannot be selected by representable interior progress,
but its endpoint remains reachable.

Public consumer families `weighted_sequences_preserve_endpoints_and_relative_progress`
and `weighted_sequences_reject_invalid_edited_configuration` cover overflowing
finite weights, small first and final intervals, ordinary weighted progress,
edited invalid configuration and a subsequent valid sequence.

### Controller sources execute outside the state lock

Custom `Simulation::x`, `dx`, `is_done` and `Curve::transform` implementations may
read or change their controller. Sampling snapshots the source and inputs under
the state lock, then calls user code after releasing it. A run generation and
sample epoch reject a result after replacement, stop or a newer nested tick;
the older call cannot rewind the controller or finish its replacement run.

Displaced simulations, curves and status callbacks retire after the lock is
released. Opaque envelopes keep their source alive across sampling and delivery;
if a call unwinds, its remaining envelopes are retained without invoking user
`Drop`. On ordinary return they retire normally, so an ordinary destructor panic
still propagates. This protects other envelopes during that unwind; it does not
contain competing destructors inside a user's own aggregate.

`controller_sources_allow_reentry_and_preserve_run_ownership` tests public source
queries, replacement, stop, nested ticks, ordinary retirement reentry, competing
sampling/status and destructor failures, and the next operation. Each row runs
in a bounded child process because the previous implementation calls these
sources while holding a non-reentrant mutex.
