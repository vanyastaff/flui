# Animation Architecture

The invariants of `flui_animation`. For use of the API see [GUIDE.md](GUIDE.md); for what
costs what see [PERFORMANCE.md](PERFORMANCE.md).

## Position

`flui-animation` sits below `flui-widgets`. Implicit animations, transition widgets and
`AnimatedBuilder` there own or observe its controllers; application authors reach the crate as
`flui::animation` (the SDK exposes it as `flui_sdk::animation`). It has no scheduler, host or
GPU edge: time arrives as a typed `FrameTick` from a presentation's `MotionClock`, or as a
`Duration` passed to `AnimationController::tick_at`.

```text
flui-widgets: animated / transitions
        │
        ▼
flui-animation: Animation<T> / AnimationController / DrivenController
                Curve / Tween / Keyframes / Simulation / AnimatedValue
                Vsync (registry handle) / MotionClock
        ▲
flui-runtime: owns one VsyncDriver per presentation (ADR-0382)
```

The only hidden item is `__runtime` (`VsyncDriver`, `VsyncRetirement`): the seam through
which a frame driver ticks, binds and closes a registry. Application code uses `Vsync`.

The root is the one path to every public item and every implementation module is private;
`curves` is the one public module, the namespace of the easing presets. There is no prelude.

## Module map

Every file under `src/`:

```text
src/
├── lib.rs                  public re-exports, doctest includes
├── __runtime.rs            hidden frame-driver seam
│
├── animation.rs            Animation<T>, StatusCallback/StatusObserver, AnimationDirection,
│                           ParentLinks (parent and channels shared by combinator clones)
├── channels.rs             Channels: the value and status notifiers every combinator owns
├── curved.rs               CurvedAnimation
├── tween.rs                TweenAnimation
├── reverse.rs              ReverseAnimation
├── proxy.rs                ProxyAnimation
├── constant.rs             ConstantAnimation, UNIT_START_ANIMATION, UNIT_END_ANIMATION
├── switch.rs               AnimationSwitch
│
├── builder.rs              AnimationControllerBuilder, ValueRange
├── controller.rs           AnimationController, its state, sampling and delivery entry points
├── controller/
│   ├── admission.rs        run admission and typed value-space refusals
│   ├── commands.rs         forward, reverse, animate_*, repeat, fling*, stop, reset
│   ├── completion.rs       finish
│   ├── delivery.rs         the committed delivery FIFO and its reentrant drain
│   ├── diagnostics.rs      transition receipts (feature `diagnostics`)
│   ├── listeners.rs        value and status listener registration
│   ├── motion.rs           interruptible value-motion runs
│   ├── policy.rs           per-run participation in presentation motion policy
│   ├── run.rs              ActiveRun: run mode, completer and settle policy as one value
│   └── sample.rs           timing proposals that commit with the position they describe
├── driven.rs               DrivenController: owner of a controller's registry seat
├── repeat.rs               RepeatSpec, RepeatMode, Iterations
├── run_future.rs           AnimationRunFuture, RunCanceled, completer and waiter custody
├── run_future_recovery.rs  tests of continuation ownership and waiter recovery (cfg(test))
├── run_future_unwind.rs    tests of run_future under unwinding (cfg(test))
│
├── vsync.rs                Vsync registry, VsyncRegistration, VsyncRegistrationError
├── vsync/
│   ├── driver.rs           VsyncDriver
│   ├── retirement.rs       VsyncRetirement: terminal close
│   └── retirement_tests.rs private registration-identity tests (cfg(test))
├── vsync_update.rs         VsyncUpdate, VsyncPublication: grouped registry migration
├── motion.rs               MotionClock, FrameTick, AnimationTime, PlaybackRate,
│                           MotionOverride, MotionPolicy
├── status.rs               AnimationStatus, AnimationBehavior
│
├── animated_value.rs       AnimatedValue, AnimatedValueView
├── animated_value/
│   ├── builder.rs          AnimatedValueBuilder
│   └── motion_update.rs    MotionUpdate: grouped motion changes
├── motion_value.rs         MotionValue, AnimationVector
├── retarget.rs             MotionSpec and its segments
│
├── curve.rs                Curve, shared input policy, builtin value equality
├── curve/
│   ├── cubic.rs            Cubic, ThreePointCubic
│   ├── erased.rs           Flipped, SharedCurve
│   ├── interval.rs         Interval
│   ├── penner.rs           Elastic, Bounce, Decelerate
│   ├── piecewise.rs        PiecewiseLinear, LinearStop
│   ├── spring.rs           SpringCurve
│   ├── split.rs            Split
│   └── steps.rs            Steps, JumpAt
├── curves.rs               the easing presets (the public `curves` module)
├── tween_types.rs          Animatable, Tween, IntTween, Rounding
├── keyframes.rs            Keyframes, KeyframesBuilder, KeyframesError
├── stagger.rs              Stagger, StaggerOrigin
├── simulation.rs           Simulation, spring, friction, bounded and bouncing scroll flings
│
├── error.rs                AnimationError, AnimationInput, RangeDefect
├── refusal.rs              refusal and contained-panic diagnostics (ADR-0220)
├── observability.rs        tracing callouts and CPU span custody (ADR-0202)
├── origin.rs               a controller's debug label and creation site
├── controller_tests.rs     controller failure-path matrix (cfg(test))
└── test_cases.rs           table-test runner (cfg(test))
```

Integration tests are modules of one binary, `tests/main.rs` (`tests/contracts/`,
`tests/support/`), plus two targets of their own: `tests/derive_motion_value.rs` and
`tests/tick_allocation.rs` (a counting global allocator is process-wide). `tests/compile_fail/`
holds trybuild cases for the owner types.

## Ownership

`AnimationController` is a cheap handle: two `Rc`s (state in a `RefCell`, the value
`ChangeNotifier`) and a shared origin. A clone observes the same run and cannot register, remove
a seat or dispose the kernel.

`DrivenController` owns a controller's lifetime and its seat on a `Vsync`. It comes from
`AnimationControllerBuilder::build_on` (the only admission path); `rebind` moves the seat and
dropping or `dispose` unregisters before cancelling the run (ADR-0179). `build()` returns a bare
controller that acquires no registry and advances only through `tick_at`. A manual controller is
cancelled when its last handle drops; a driven one when its owner does. Compile-fail cases pin
that a bare controller cannot be registered and a driven one has no bare `dispose`.

All state is owner-local: `Rc` with `Cell`/`RefCell`, no lock in any signature (ADR-0175,
ADR-0323). Callbacks are `Rc<dyn Fn>` and run on the thread that commits the change.

`ConstantAnimation` stores no callbacks and has no notifier; its `listen*` return
`Subscription::inert()`, which carries no listener id (a listener id is minted only by a
listener list).

## Delivery and user code

Rule (ADR-0287): an owner commits its state, releases its borrows, then runs code it does not
own. In this crate user code is a value or status listener, a curve, simulation or converter, a
run continuation or waker, and the `Drop` of anything retired.

- **One FIFO per controller** (`controller/delivery.rs`, ADR-0177). A transition commits under
  the state borrow and drains outside it: value listeners, then the status change, then the
  run's outcome, then retirement of the sources the transition displaced. A reentrant change
  appends to the outermost drain. The status entry snapshots subscription tickets when it is
  committed, so a listener removed before its turn is silent, a listener added later is not
  caught up, and a disposed controller delivers nothing queued. Accepted transitions do not
  coalesce.
- **First failure wins** (ADR-0178, ADR-0281). A panic in user code is caught, the queue and
  the remaining retirements still run, each destructor under containment, and the first
  failure resumes after the queue is empty and the delivery latch is released. A destructor
  that panics during an unwind aborts, as in std. `Vsync::tick_all` does the same across
  controllers and child registries: healthy peers still tick and the first failure resumes.
- **Teardown closes admission first.** Disposing an owner commits closure, then retires its
  registry seat, motion, curves, simulations and targets through the same FIFO. A last owner's
  destructor may reenter the registry, remove the same token again or register new work.
- **Sampling releases the borrow.** A sample snapshots the source and its inputs, calls user
  code (`Curve::transform`, `Simulation::x`, converters) with no guard held, and commits only if
  the run generation and sample epoch still match. An older call can neither rewind the
  controller nor finish the run that replaced it.
- Wrappers query parents and swap them outside their own borrows (`ProxyAnimation::set_parent`
  commits the new parent before the outgoing one retires).

## Identity

- **Run generation.** Each admitted run takes the next generation from a counter that never
  wraps; `AnimationController::run_generation` exposes it so the registry can re-anchor a
  restarted run. A settle that does not start a run (`stop`, `reset`, `set_value`, a trivial
  zero-distance or zero-duration command) leaves it untouched. When the counter is exhausted
  new runs are refused with `AnimationError::IdentityExhausted` and the last admitted run can
  still finish.
- **Sample epoch.** Each sample takes an epoch before calling its source. Exhaustion cancels
  the pending outcome without publishing a new sample; `stop` and `reset` stay available.
- **Registration.** A `VsyncRegistration` names its registry by weak identity plus one slot
  from a monotonic namespace shared by controllers and children. Removal borrows the token, so a
  foreign, expired or stale token removes nothing. Slots are never reused; once exhausted,
  admission and `attach_child` refuse with `VsyncRegistrationError::Exhausted` (ADR-0125).

## Runs and their outcome

`ActiveRun` (`controller/run.rs`) holds one run's mode (`Timed`, `Repeat` or `Simulation`), its
completer and its settle policy, installed and retired together so they cannot disagree.

- **One future per run.** Every run-starting command returns
  `Result<AnimationRunFuture, AnimationError>`: `Err` means no run started; the future resolves
  `Ok(())` on a normal finish and `Err(RunCanceled)` when superseded or torn down. Dropping the
  future does not cancel the run (ADR-0064). `stop` and `reset` return `Result<(), _>` and give
  no future.
- **Publish before listeners.** A natural end takes the run and completes its completer in the
  same guard scope that sets the status, before any listener runs; only delivery of
  continuations and wakers waits for the drain. A panicking status listener therefore leaves
  the run `Ok`.
- **Status precedes outcome.** When a command replaces a run, the new run's status is delivered
  before the displaced run's cancellation.
- **`finish`** commits a finite run's declared endpoint and a successful outcome before
  listeners, independently of frame demand and playback rate. An idle controller stays idle; a
  disposed one refuses; an infinite repeat or a custom simulation has no declared endpoint and
  is refused as `NoFiniteEndpoint` without touching the run. A listener's replacement run stays
  authoritative.
- **Zero distance or zero duration settles at the call** through one chokepoint,
  `settle_at_target`. Differences are compared exactly, never against a physical tolerance: a
  tiny distance keeps its authored duration, and value listeners are notified only if the value
  at method entry differs from the value after the settle.
- **Playback rate** changes apply after the old rate has sampled the current frame, so local
  time stays continuous.

### Status

The phase is separate from position and from run ownership.

| Event | Status |
|---|---|
| a forward run is admitted | `Forward` |
| a run ends | its direction's settled status, with no bound check |
| `stop` at a bound / at an interior value | that bound's settled status / the direction's settled status |
| `set_value` at a bound / at an interior value | that bound's settled status / the running status of the current direction |

- The method chooses the direction: `animate_to` and `animate_to_curved` run `Forward`,
  `animate_back*` run `Reverse`, wherever the target lies, and the run ends `ForwardSettled` or
  `ReverseSettled` accordingly (`animate_to(lower_bound)` ends `ForwardSettled`). A fling takes
  its direction from the sign of its velocity.
- `stop` and `set_value` check the bounds first, so a per-frame `set_value` from a gesture
  reports the bound it reached (ADR-0193).
- `status().is_directional()` is therefore not proof that a run exists. `is_animating()` is:
  it is true while a run is installed, including a paused one, and false after `dispose`,
  `set_value` or a settle. The registry reads `walk_probe().live_running`
  (`!disposed && run.is_some()`), never the status, so a mid-run `set_value` is not overwritten
  by the next tick from a stopped run.
- `dispose` leaves the status as it was. Disposed controllers refuse driving commands with
  `AnimationError::Disposed`; queries stay readable.
- A never-run unbounded controller starts at `0.0` with status `Forward`, because neither
  infinite bound is ever "at" the value; its first `stop()` reports `Forward → ForwardSettled`.
  `reset()` on an unbounded controller lands on `0.0` with `ReverseSettled`.

### Repeat

A repeat is a pure function of elapsed time: `total_ns = elapsed + initial_ns` reduced modulo
the period gives the cycle and the phase, so one `tick_at(1.25)` and `tick_at(1.0); tick_at(1.25)`
agree. The leg, landing and phase arithmetic lives in one place (`controller/run.rs`,
`RepeatRun`) and is shared by sampling, exhaustion, `finish` and policy settlement.

- The run starts from the current value clamped into the repeat range, not from its start, so a
  `repeat()` issued on every build continues instead of snapping back.
- The period is resolved once at the call (`RepeatSpec::period` or the forward duration) and
  serves both legs of an alternating repeat; `set_duration` does not retime an active repeat.
- A finite repeat lands on its last leg's endpoint with that leg's settled status.
- `repeat` settles synchronously, through `settle_at_target`, for a zero effective period and
  for `Iterations::Finite(0)`; the latter settles at the clamped current value.
- An empty range is refused with `InvalidRepeatRange`; `RepeatSpec::range` takes a
  `ValueRange`, so non-finite endpoints and overflowing spans cannot reach the controller.
- `velocity()` on a reverse leg is negative.

### Bounds and non-finite input

- `ValueRange::try_new` accepts a finite, ordered pair whose span is finite and returns a
  `RangeDefect` otherwise. Unbounded is a builder choice (`unbounded()`), never a pair of
  infinities.
- Unbounded controllers refuse `forward*`, `reverse*`, `fling*` and a `repeat` with no range
  (there is no bound to run to); an explicit finite repeat range works.
- On any controller, a non-finite `target`, `from` or velocity is refused, a target or `from`
  of `±inf` clamps to the finite bound it points at, and an overflowing `target - value` or a
  simulation whose `x(0)` is non-finite is refused. Every refusal runs before any state
  changes, so the installed run, the displayed value and the original future survive.
- `set_value` canonicalizes on a bounded controller (`NaN` to the lower bound, `±inf` to the
  bound it points at); on an unbounded one a non-finite input is a complete no-op. A running
  simulation whose sample turns non-finite ends at the last finite value. Both paths warn once
  per controller (`non_finite_warned`).
- Timed sampling (`controller/sample.rs`) reads the start and target fields directly at progress 0 and 1, so no
  `inf * 0.0` can produce a `NaN`.
- Every refusal is reported (ADR-0220): each public command returns through
  `refusal::report`, which emits one `warn!` on `REFUSAL_TARGET` with the operation, the error's
  stable `kind` and the error, after the borrow is released. `stop`/`reset` on a disposed
  controller report at `debug`.

## Registry

`Vsync` is the handle a tree receives; it registers owners, nests child registries and mutes.
`VsyncDriver` is the presentation's side: it alone ticks (`tick_all`), binds the frame requester
and build phase, and closes the registry (`prepare_close`, returning a `VsyncRetirement`)
(ADR-0382). The registry is not process-global.

- **Restart awareness.** The registry watches each controller's run generation and re-anchors a
  run's `t = 0` when it advances. A retargeted run continues from the published seam of its
  predecessor when the dispatch coordinates match; otherwise it anchors on its first observed
  frame. The frame counter saturates, which disables continuation and leaves admitted runs
  ticking.
- **Order.** Child registries admitted at entry tick before the registry's own controllers;
  controllers tick in registration order, resolved through a `BTreeMap` keyed by slot with a
  cursor, so removing an entry keeps the order of the rest. Muting is re-read between samples
  and stops the remainder of the frame. A resident controller that starts a run before its turn
  can be sampled in the same frame; one already visited waits for the next.
- **Tree.** A child registry has at most one weak parent seat. `attach_child` preflights
  closure, ancestor cycles, an existing live parent and identity capacity before committing
  either edge, and refuses with `Closed`, `WouldCycle`, `AlreadyAttached` or `Exhausted`
  (ADR-0191). A cached successful ancestor check is valid for the root's current hierarchy
  revision; attach, withdraw, mute and close advance it before user code runs, and a saturated
  revision falls back to full validation.
- **Snapshots.** A frame holds the child snapshot taken at entry until each child's turn, even
  if a callback detaches it; each visited or skipped child retires on its own outside the
  borrows. Cached snapshot storage never retains detached owners.
- **Removal drops outside the borrow.** `unregister` and `detach_child` withdraw the entry under
  the registry borrow and drop it after releasing it.
- **Demand.** `has_running` reports continuous activity. A registry with nothing running
  remembers that until something can start a run (a run's frame request, an admission, an
  attach, an unmute or a tick), so a host asking on every input event scans once per frame.
  Pending settlement and Full-policy resumption can need one frame even when continuous demand
  is false; the registry retains that obligation across gates and rebinding.

### Motion policy and time

- `MotionClock` is a plain value, one per presentation. Raw timestamps go in; a `FrameTick`
  (the only way to obtain one) comes out. Animation time is `epoch_time + (raw − epoch_raw) ·
  rate`, never decreases (an earlier raw timestamp holds the timeline) and saturates at
  `Duration::MAX`. Changing the rate rebases the epoch, so time is continuous; `step` advances
  by an exact amount regardless of rate.
- `MotionOverride` (`FollowSystem`, `Reduce`, `Full`) is the application's choice; the runtime
  resolves it against the host preference into a `MotionPolicy` before the next frame
  (ADR-0176, ADR-0195). `AnimationBehavior::Normal` runs follow the resolved policy;
  `Preserve` keeps authored timing, while debug playback and registry muting still apply.
- Under Reduce a finite Normal run settles at its terminal value on the next registry tick and
  an infinite Normal repeat parks (no completion, no continuous frame demand); Full resumes it
  from a fresh anchor. A saturated timeline settles finite work and parks infinite work until
  the owner is rebound to a registry with time left. A policy change commits clock state without
  sampling controllers or running their listeners. Loading indicators draw static visible
  feedback under Reduce (ADR-0192).
- Rebinding preserves the last sampled elapsed time; a missing clock settles finite runs and
  parks infinite ones.

## Retargeting and `AnimatedValue`

`MotionSpec` (`Curve { duration, curve }` or `Spring(description)`) says how a value moves to a
new target. A segment starts from the position and velocity of the one it interrupts, so motion
is continuous in value and velocity however often it is retargeted (`retarget.rs` derives the
formulas).

- A **curve** segment adds two Hermite terms that vanish at both ends: it starts with the
  inherited velocity and arrives exactly at the target, at rest, after its duration. Reversing
  toward the previous start shortens the duration by the eased fraction already travelled, not
  below one millisecond; below one millisecond a segment jumps.
- A **spring** segment follows the analytic trajectory until the physical rest threshold, then a
  cubic Hermite transition of at most one inverse natural frequency (shortened to keep the
  inherited displacement within the position tolerance) reaches the exact target at rest.
  Completion time depends on the motion, not on which frame observes it. Retargeting to the same
  target with the same spring continues the existing trajectory instead of rebuilding it.
- `MotionSpec::duration` keeps the easing and `MotionSpec::curve` keeps the duration; a spring
  has neither half and takes `EASE_IN_OUT` or `MotionSpec::DEFAULT_DURATION` (200 ms).
- A retarget that is invalidated while its converters and curves run (outside all borrows) is
  prepared again once; a second invalidation is refused as `ReentrantMotion`, never admitted
  with a zeroed velocity.

`MotionValue` is the fixed-width contract for custom values: an associated `Vector` of `f64`
components (`AnimationVector`, sealed to `[f64; N]`, so user code cannot run inside the commit),
`to_vector`/`from_vector`, and `rest_thresholds`, one positive finite distance per component in
the vector's own units (ADR-0186; geometry 0.01 logical pixels, scalar, alignment and premultiplied
Oklab 0.001). `#[derive(MotionValue)]` concatenates field vectors in declaration order and
rejects empty structs; widths must be concrete. Interpolation is the separate `Lerp` derive
of `flui_foundation::geometry`, which delegates to each field.

`AnimatedValue<T>` owns one `DrivenController` for all components of a value, so geometry and
color share admission, time, retargeting and cancellation. Its spring components share the
latest rest start and the shortest permitted rest interval, which keeps premultiplied color
relationships intact. A sample is evaluated outside the borrow, staged as one vector and
published with its elapsed time only if its sample identity survives; a failing component leaves
the whole previous sample authoritative.

- Admission commits the exact target, sample, motion and reversal state before any callback.
  Retargeting to a motion-equivalent target keeps the run's future and deadline while still
  notifying observers of the exact target.
- `AnimatedValueView` observes sample and controller without a registry seat; the owner's drop
  or disposal unregisters and cancels even if views survive, and a view at rest reports the exact
  target.
- `VsyncUpdate` and `MotionUpdate` stage several owners and commit every new seat or motion
  before delivering any clock transition. `prepare` returns an owned `VsyncPublication` without
  controller borrows, so a widget can restore extracted owner storage before a reentrant
  callback observes it; drop publishes the tail, and a drop during unwind keeps the incoming
  failure.

## Subscriptions

Value listeners are `ChangeNotifier` registrations (`Listenable::listen`). Status listeners come
from `Animation::listen_status_with_recovery` (and its provided adapter `listen_status`); both
return `flui_foundation::Subscription` (ADR-0379). A status subscription is built with
`Subscription::for_source`: a weak source, a `Copy` token and a removal function. Dropping or
detaching it withdraws the callback; `forget` gives up that right, and the source's disposal then
removes it. Withdrawal returns the outgoing callback to the source's FIFO tail, or retires it
under the inherited recovery when no delivery is running. A subscription never keeps its source
alive, and a guard that outlives a parent replacement or switch hop does not delegate its token
to the active parent. Disposed sources return an inert subscription and retire the rejected
capture outside their borrows.

## Combinators

`CurvedAnimation`, `TweenAnimation` and `ReverseAnimation` share a private `ParentLinks`: the
parent, a `Channels` pair (value and status notifiers) and one subscription to each parent
channel. Clones share the links; the last clone closes the channels, withdrawing outgoing
callbacks before detaching from the parent. Curves and mappings are owned by each value clone and
retire with it. No combinator demands `Debug` or `Clone` of a mapping, curve or value; only
cloning the observer clones the mapping.

- `CurvedAnimation` shares the run's entering direction in a `Cell`. A direction change during
  the run keeps the chosen curve; a resting status clears it so the next run picks again.
- `ReverseAnimation` mirrors value and status. `ProxyAnimation` clones its parent under the read
  guard and queries it after release, so a custom animation may replace the parent from
  `value()` or `status()`.
- `AnimationSwitch` keeps its source until the values actually cross; only exactly equal
  initial values adopt the next source immediately (no epsilon).

## Curves, tweens and simulations

- `Curve::transform` is exact at 0 and 1, clamps outside `[0, 1]` and maps `NaN` to `NaN`; a
  fallible constructor reports a `CurveParameter`, and `CurveError::RescalingOutOfRange` marks
  normalization past the solver's range. Constructors meant for constants panic instead
  (`Steps::new`, `Interval::new`, `Cubic::new`, `ThreePointCubic::new`, the elastic curves,
  `Split::with_curves`); their conditions are in their rustdoc.
- `SharedCurve` compares built-in curves by value, so recreating an equivalent configuration
  keeps an `AnimatedValue` run and future. `PiecewiseLinear` canonicalizes its stops once into
  finite ordered knots; `SpringCurve` reuses the validated `SpringSimulation`. Both are
  immutable and hold no callbacks or frame state. `Split` keeps its curves private and
  deserializes only a valid whole.
- `IntTween` converts both endpoints to `f64` before subtracting, so every `i32` pair is
  exact; `Rounding::Nearest` rounds halves toward positive infinity.
- A `Keyframes` track is a value as a pure function of elapsed time. At a boundary it returns
  the keyframe exactly (the value after a jump); a non-finite sample publishes the segment's
  start value.
- A simulation is an immutable validated value: private fields, constructors that refuse
  non-finite or non-positive input and overflow with a `SimulationError`, in debug and release.
  Each computes at construction the time from which it stays within its `Tolerance`; from then on
  `x` is the resting position, `dx` is `0.0` and `is_done` stays true, so rest does not depend
  on how frames partition time. A spring is stored as `(ω, ζ)` and evaluated by one closed form
  for every ratio, finite over `[0, ∞]`. `SpeedLimit::Derived` takes the speed limit from the
  motion's own time scale; scroll physics use `Tolerance::for_device_pixel_ratio` (ADR-0200).
  `BouncingScrollSimulation` hands friction to an edge spring with the friction's position and
  velocity. Friction uses `exp_m1` and `ln_1p`, so a drag near one keeps small motion; a
  position it never reaches answers `+inf` for `time_at_x`.
- Fling constructs its simulation before replacing any live source, direction or future, and
  keeps the typed cause as `AnimationError::Simulation`. Policy and missing-clock settlement
  land a native fling on its declared bound.

## Errors

`error.rs` holds the variants. `ValueRange::try_new` returns the narrow `RangeDefect`;
controller and animated-value operations return `AnimationError`, with `AnimationInput`
naming a refused scalar and the numeric fields, component indices and original `SimulationError`
causes matchable without parsing a message (ADR-0198). Error enums are `#[non_exhaustive]` and
carry no serde implementation (ADR-0381). A refusal commits no change to the run.

## Observation

The `diagnostics` feature emits transition receipts and CPU spans (`animation_tick`,
`animation_sampling`, `animation_listeners` at target `flui.animation`) through `tracing`
(ADR-0202). Receipts are immutable scalars queued in the controller's FIFO and emitted after the
borrow is released, on the dispatcher that owns the controller's span; value-listener snapshots
and registry admission fences are taken before span creation, so a subscriber registered from
inside a callout joins the next notification. A subscriber is user code: its panics are
contained and never own an animation outcome. The crate depends on no profiler.

## Decisions

| Topic | ADR |
|---|---|
| Completion is one controller-resolved future | [ADR-0064](../../../docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md) |
| Registration authority, stale withdrawal, exhaustion | [ADR-0125](../../../docs/adr/ADR-0125-vsync-registration-authority.md) |
| Interpolation, premultiplied Oklab color | [ADR-0149](../../../docs/adr/ADR-0149-interpolation-contracts.md), [ADR-0194](../../../docs/adr/ADR-0194-continuous-color-and-opacity.md) |
| Owner-local state | [ADR-0175](../../../docs/adr/ADR-0175-owner-local-scheduling-and-animation.md) |
| Presentation playback and motion policy | [ADR-0176](../../../docs/adr/ADR-0176-presentation-animation-playback.md), [ADR-0195](../../../docs/adr/ADR-0195-presentation-motion-policy.md), [ADR-0192](../../../docs/adr/ADR-0192-accessible-loading-motion.md) |
| Ordered delivery and first-failure custody | [ADR-0177](../../../docs/adr/ADR-0177-controller-ordered-delivery.md), [ADR-0178](../../../docs/adr/ADR-0178-notification-first-failure-custody.md), [ADR-0281](../../../docs/adr/ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md), [ADR-0287](../../../docs/adr/ADR-0287-owner-state-is-committed-before-user-code-runs.md) |
| Controller seat lifetime | [ADR-0179](../../../docs/adr/ADR-0179-controller-registration-ownership.md) |
| Rest thresholds, derived rest velocity | [ADR-0186](../../../docs/adr/ADR-0186-typed-animation-rest-thresholds.md), [ADR-0200](../../../docs/adr/ADR-0200-dynamics-derived-rest-velocity.md) |
| Single-parent registry tree | [ADR-0191](../../../docs/adr/ADR-0191-single-parent-animation-registries.md) |
| Directional phase names | [ADR-0193](../../../docs/adr/ADR-0193-directional-phase-names.md) |
| Typed admission errors, not serialized | [ADR-0198](../../../docs/adr/ADR-0198-typed-animation-admission-errors.md), [ADR-0381](../../../docs/adr/ADR-0381-animation-errors-are-not-serialized.md) |
| Observation through tracing; misuse diagnostics | [ADR-0202](../../../docs/adr/ADR-0202-animation-observation-through-tracing.md), [ADR-0220](../../../docs/adr/ADR-0220-animation-misuse-diagnostics.md) |
| Threads: values stay on the owner thread | [ADR-0323](../../../docs/adr/ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md) |
| Status subscription is the foundation `Subscription` | [ADR-0379](../../../docs/adr/ADR-0379-an-animation-status-subscription-is-the-foundation-subscription.md) |
| A presentation drives its registry through `VsyncDriver` | [ADR-0382](../../../docs/adr/ADR-0382-a-presentation-drives-its-animation-registry-through-a-vsyncdriver.md) |

Platform status and open gaps are in [docs/gaps.md](../../../docs/gaps.md), not here.
