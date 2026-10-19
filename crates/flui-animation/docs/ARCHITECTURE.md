# Animation Architecture

Internal architecture of `flui_animation`.

## System Overview

`flui_animation` supplies animation values, controllers, curves and simulations.
The widget layer consumes them through `flui-widgets`' `animated` and
`transitions` modules: implicit animations own controllers, while transition
widgets and `AnimatedBuilder` observe existing animations. `DrivenController`
owns registration on the widget's ambient Vsync registry.
`build_on` is the public controller admission path; observer clones cannot
register or remove a seat, or dispose its kernel. `DrivenController::rebind` migrates the owned seat,
and disposal or drop unregisters before canceling its run (ADR-0179).

```text
flui-widgets: animated / transitions
                   │
                   ▼
flui-animation: Animation<T> / AnimationController / Curve / Simulation
               DrivenController / Vsync / MotionClock
```

## Module Structure

```
src/
├── lib.rs            # Public API, re-exports
│
├── animation.rs      # Animation<T> trait, AnimationDirection
├── controller.rs     # AnimationController (main driver)
├── controller/run.rs # ActiveRun: one run mode, its completer and settle policy
├── controller/commands.rs # Run commands: forward, animate_to, repeat, fling, simulations
├── controller/listeners.rs # Value and status listener registration
├── controller/diagnostics.rs # Committed scalar transition receipts (diagnostics feature)
├── observability.rs  # Contained tracing callouts and CPU span custody
├── controller_tests.rs # controller failure-path matrix (cfg(test))
├── builder.rs        # AnimationControllerBuilder
├── vsync.rs          # Vsync registry: drives many controllers per frame
│
├── channels.rs       # Channels: the value and status channels every combinator owns
├── curved.rs         # CurvedAnimation (applies curve)
├── tween.rs          # TweenAnimation<A> (maps through Animatable A)
├── reverse.rs        # ReverseAnimation (inverts value)
├── proxy.rs          # ProxyAnimation (hot-swappable parent)
├── constant.rs       # ConstantAnimation (fixed value)
├── switch.rs         # AnimationSwitch (crossover switching)
│
├── curve.rs          # Curve trait, shared input policy, Builtin value equality
├── curve/            # interval, cubic, split, penner (elastic/bounce/decelerate),
│                     # steps, erased (Flipped, ArcCurve), piecewise, spring
├── tween_types.rs    # Animatable, Tween, all tween types
├── status.rs         # AnimationStatus, AnimationBehavior
├── simulation.rs     # Simulation trait, Spring, Friction, bounded and bouncing scroll
├── value_conversion.rs # TwoWayConverter, fixed component arrays
├── animated_value.rs # AnimatedValue owner, AnimatedValueView, vector sample commits
├── animated_value/motion_update.rs # Coordinated changes to value motion
├── vsync_update.rs   # Coordinated registry migration of values and controllers
├── retarget.rs       # Interruptible scalar motion segments
│
├── keyframes.rs      # Keyframes, KeyframesBuilder, KeyframesError
├── stagger.rs        # Stagger, StaggerOrigin
├── prelude.rs        # common motion and observation imports
├── error.rs          # AnimationError
└── test_cases.rs     # table-test runner (cfg(test))
```

## Core Abstractions

### Optional observation

The `diagnostics` feature emits transition receipts and CPU spans through
`tracing` (ADR-0202). Immutable scalar receipts join the existing controller
FIFO; subscriber callbacks run after the controller borrow is released.
Span identity and run generation correlate lifetime and completion/cancellation.
Receipts use the controller span's owning dispatcher; subscriber-local IDs are
never sent through a different current dispatcher. Value listener snapshots
and registry admission fences precede optional span creation/entry, so subscriber
registrations join the next notification or tick rather than the current one.
The framework does not depend on a profiler and introduces no observer protocol.
`animation_observation_contract` pins run correlation, filtered/disabled capture,
subscriber reentry, first-failure custody and subsequent healthy delivery.
`diagnostic_subscriber_retirement_contract` exercises failing span closure and
last-subscriber retirement in an isolated process.

### Nested value conversion

`TwoWayConverter` is the fixed-width extension contract for custom animation
values. Its derive concatenates concrete field vectors in declaration order;
`AnimationVector::COMPONENTS` exposes the sealed array width. One `AnimatedValue`
owns the entire vector, so nested geometry and color share admission, time,
retargeting and cancellation. Derived `Lerp` delegates to each field, preserving
its interpolation contract instead of interpreting scalar color components.
Generic-dependent widths require a manual representation because stable Rust
cannot express their sum as an array length. Empty derives are refused.
`two_way_converter_derive_contract` pins nested conversion, perceptual fading
and velocity-preserving interruption through an actual Vsync registration.

### Controller delivery

Controller status transitions and run deliveries share a FIFO committed under
the controller state guard and drained outside it (ADR-0177). Reentrant changes
append to the outermost drain. Each status snapshots subscription membership;
removed listeners and disposed controllers are skipped before invocation. New
subscriptions participate in subsequent commits without implicit catch-up.
Callback custody remains alive through the round and retires in registration
order under the existing first-failure policy. Vsync continues its admitted frame
peers after a controller or child-registry failure, then resumes that failure.

Owning controller and typed-value teardown closes admission before user cleanup.
Outgoing registry, motion and exact-target custody joins this FIFO, including
release inside a continuation or waiter whose failure has not yet returned to
the controller. Healthy cleanup retires resources individually; a caught failure
retains their opaque custody. The public rows
`owner_retirement_joins_every_delivery_channel`,
`owner_retirement_keeps_its_first_destructor_failure` and
`reentrant_owner_retirement_joins_registry_custody` pin closure, failure priority,
healthy tails and subsequent recovery (ADR-0178).

`retired_owner_cleanup_passes_a_stale_settlement_fence` verifies that a stale
instant-settlement request cannot defer a retired owner's cancellation and
healthy cleanup. Only an eligible new settlement is limited to the next round.

`status_delivery_contract` pins ordering, A to B to A, late subscription, published
outcomes and subsequent frames. `status_delivery_failure_custody` covers hostile
captures and competing payloads in a bounded child process. The current ownership
and foundation value-notifier policies retain their separate contracts.

### Published samples and run anchors

Retarget continuation carries the source run generation and the raw elapsed
time of its published seam. A recent registry dispatch also records its generation
and elapsed time. Matching both coordinates lets the next tick advance from the
published seam even when an intervening dispatch failed. Replacing a run several
times before another sample preserves those source coordinates. An idle run
anchors on its first observed frame instead.

The registry frame counter saturates. At its terminal value, continuation
eligibility is disabled and replacement runs use a fresh anchor; admitted runs
still advance on subsequent ticks. `vsync_nesting_and_reentrancy` includes the
private terminal-counter case. The public `controller_retarget_frame_boundaries`
table covers repeated replacements, completion callbacks, idle intervals,
rejected frames and both cross-controller visit orders.

Retarget preparation retries one invalidated seam. Repeated reentry refuses
with `AnimationError::ReentrantMotion`, preserving the latest installed run;
silently replacing its velocity with zero would violate the seam contract.
Invalid results from displaced sources are discarded before another callout.
Curve motion corrects both departure and arrival velocity: it inherits the seam
velocity and approaches zero before the controller stops. A curve's nonzero
endpoint derivative would otherwise be cut off at completion.
`controller_retarget_is_c0_and_c1_at_the_seam` covers retry, refusal and the last
position interval before rest.

Interruptible spring motion retains the native analytic trajectory until the
physical simulation's distance and speed rest threshold. Stopping there would
freeze an arbitrary near-target sample and cut off its residual velocity.
A cubic Hermite transition instead inherits that position and velocity, then
arrives exactly at the target with zero velocity. Its duration is
`min(1 / omega, distance_tolerance / abs(velocity))`, with the second term
omitted for zero velocity. The remaining displacement is bounded by
`distance_tolerance * 31 / 27`; both joins preserve position and velocity.
Completion occurs at the computed threshold plus that duration, independently
of the observing frame. Standalone physics simulations retain their own rest
contract. `a_controller_spring_arrives_at_rest_independently_of_frames` uses
the registry and different frame partitions; the public
`a_controller_spring_enters_and_leaves_rest_continuously` probes both joins
with position differences and an independent early-trajectory reference.

Retargeting a scalar controller to the same target with the same spring replaces
the run future while continuing the existing physical trajectory. A flat elapsed
offset carries its analytic and exact-rest suffix; repeated replacements do not
nest simulations or recompute the rest deadline. If bounds changed the published
value, the new segment instead starts from that actual value.
`same_scalar_spring_target_keeps_the_rest_trajectory` compares uninterrupted and
repeatedly retargeted public traces, velocities and completion for underdamped,
critical and overdamped springs across their rest intervals. Rebuilding the
segment at every identical target fails that row.

### Owning vector motion

`TwoWayConverter::rest_thresholds` specifies one positive finite distance per
component in its vector's own units ([ADR-0186](../../../docs/adr/ADR-0186-typed-animation-rest-thresholds.md)).
Geometry uses 0.01 logical pixels; scalar, alignment and premultiplied Oklab
components use 0.001. Speed limits derive from each spring's natural rate.
Derived values concatenate field thresholds without converting error values
through nonlinear value conversion. Curve motion ignores spring thresholds.
Preparation validates thresholds before admission, so invalid distances retain
the previous run and refuse a coordinated update without admitting a prefix.
`owning_animated_value_contract` covers distinct geometry/scalar completion and
single/grouped refusal; `two_way_converter_derive_contract` proves that nested
geometry retains its own completion timing inside a nested vector. A vector's
spring components share the latest native rest start and the shortest permitted
rest interval. This keeps premultiplied color relationships intact while
respecting every component's distance bound. The color fade case in
`tolerance_constructors_validate_and_scale_with_dpr` verifies its visible hue.

`AnimatedValue<T>` owns one `DrivenController` for all its scalar components.
`VsyncUpdate` stages property owners and plain controller owners together. It
commits every new seat before delivering any clock transition. `prepare` returns
an owned `VsyncPublication` without controller borrows, so a widget can restore
temporarily extracted owner storage before a reentrant callback observes it.
Explicit publication and ordinary drop complete the accepted tail. Drop during
unwind preserves the incoming failure while still delivering settlement.
`grouped_registry_migration_commits_before_delivery` in the public owning-value
table verifies immediate delivery, ordinary drop and unwind, with live and absent
clocks and competing callback failures.

The controller's generated value-motion branch evaluates components outside
its borrow, stages one vector sample and publishes that sample together with
elapsed time only after its sample identity survives. The commit copies fixed
arrays: `AnimationVector` is sealed to `[f64; N]`, so user implementations of
`AsMut` cannot run inside the controller borrow. A failed later component leaves
the whole previous sample and clock authoritative. The sample identity is checked
between each component's position and velocity callouts. Owner release, replacement
or a nested frame stops the displaced sample before the next framework callout.

Target and motion preparation invoke converters and curves outside borrows.
Admission commits the exact target, component sample, motion and reversal state
before status, cancellation or value callbacks. Repeating a motion-equivalent
target retains the run's future and deadline, while notifying observers of its
exact target representation (transparent color channels can differ). A curve-only
update uses the current segment's remaining deadline; explicitly changing its
duration configures a new segment duration. `retarget(target, motion)` admits
both changes in one transaction.

`AnimatedValueView<T>` observes the published sample and controller; it owns no
registry seat or moving source. Dropping or disposing the owner unregisters and
cancels, even when views survive. Views keep the last sample; at rest they keep
the exact target, including components not recoverable from its motion vector.
Reading an exact target clones its `Rc` under the target borrow, then invokes
`T::clone` after releasing that borrow. Conversion from a sampled vector likewise
runs without state guards. Outgoing targets and curves retire separately under
the controller's enclosing first-failure custody.

`owning_animated_value_contract` covers component publication before cancellation,
repeated configuration deadlines, unbound settlement, surviving observers,
converter and target-clone reentry, component failure, owner release inside a curve
and exact-target delivery.
The existing physical properties use replayed public Vsync traces for independent
derivative probes; there is no production `advance(dt)` or owner clone.
`a_steady_state_frame_allocates_nothing` measures running scalar, four-component
and nested geometry/color owners, observer reads, keyframe sampling and listener
relays. It also advances and queries a warmed root/child/grandchild registry with
an actual running controller and listener delivery. Every measured component
must advance; the run-start negative control verifies that the counter is live.
This steady-state contract does not exclude allocations during topology growth
or overlapping reentrant walks.

### Source-bound status removal

`StatusSubscription` holds weak source ownership, a `Copy` registration token and
a removal function pointer. Dropping it withdraws that registration; `detach`
disables removal without touching the source's callback. Custom sources construct
the same guard and receive the borrowed recovery context during withdrawal. The
remover returns outgoing callback custody after committing membership changes.
The guard releases its temporary source reference before retiring that custody
through the context. Capture destruction can therefore release the last owner
and reenter only after its channels close. Deferred delivery custody stays in
the source's existing queue. The private guard envelope contains no user captures.

`Animation<T>::observe_status` exposes that ownership contract through the
common observation surface and borrows the current delivery recovery context;
`subscribe_status` is its provided adapter for callbacks that ignore it. Parent links and switch hops retain status guards;
they cancel them through that same context after releasing state borrows.
Framework owners in higher layers use `cancel_with_recovery` when status cleanup
belongs to an already-started retirement round. This preserves its first failure
and opaque capture policy rather than starting independent cleanup.

Controller withdrawal commits removal under its existing state borrow. During
delivery, callback retirement joins its existing FIFO; outside delivery it runs
after releasing the borrow. The guard neither owns a second channel nor keeps the
controller owner alive. Scrollable activity and AnimatedSize completion callbacks
retain the owning subscription beside their driven controller.

Disposed controller and switch status admission returns an inert guard and
retires rejected captures outside state borrows, using any established delivery
failure. The controller refuses before minting an identity. A live exhausted
counter refuses permanently after releasing the borrow; panic hooks can read
the public source. `exhausted_status_identities_refuse_outside_the_state_borrow`
pins hook reentry, permanent refusal and delivery to the last admitted callback.

Reverse, curved and tween subscriptions identify their shared `ParentLinks`
owner, while proxy and switch subscriptions identify their respective owners.
An owning guard survives a parent replacement or a switch hop without delegating
its removal token to the active parent. It does not extend wrapper lifetime:
the last wrapper clone closes both channels even if a guard survives. Withdrawal
inherits both channel failure histories before retiring captures; proxy removal
during delivery joins the existing deferred retirement queue.

`owning_status_subscription_contract` uses bounded child-process rows for source
and channel independence, detach, source teardown, custom-source construction,
self/later removal during delivery, reentrant capture retirement, first-failure
custody, recovery, disposed admission, shared wrapper lifetime, proxy replacement
and switch hops.

### Registration tokens and removal

The related decisions have independent scopes:

| Authority | Decision |
|---|---|
| Backend/slot token identity, stale withdrawal and permanent exhaustion | [ADR-0125](../../../docs/adr/ADR-0125-vsync-registration-authority.md) |
| Driven controller seat lifetime and exclusive disposal | [ADR-0179](../../../docs/adr/ADR-0179-controller-registration-ownership.md) |
| Single-parent child topology, admission and root-clock transfer | [ADR-0191](../../../docs/adr/ADR-0191-single-parent-animation-registries.md) |
| Presentation playback and host/application policy timelines | [ADR-0176](../../../docs/adr/ADR-0176-presentation-animation-playback.md), [ADR-0195](../../../docs/adr/ADR-0195-presentation-motion-policy.md) |
| Visible loading representation and application pause controls | [ADR-0192](../../../docs/adr/ADR-0192-accessible-loading-motion.md) |
| Ordered controller delivery and borrowed notification failure custody | [ADR-0177](../../../docs/adr/ADR-0177-controller-ordered-delivery.md), [ADR-0178](../../../docs/adr/ADR-0178-notification-first-failure-custody.md) |

This map locates the current authority; it does not combine independently
reversible decisions or change their recovery boundaries.

Child registries form a tree. One optional weak parent seat prevents conflicting
presentation policies and duplicate child sampling. `attach_child` preflights
closed state, ancestor cycles, existing parent and identity capacity before
committing either edge. Refusals are typed; upward cycle checking allocates no
descendant snapshot. Wake failure withdraws the new edge before recovery.
The public `vsync_admission_contract` pins these rules and reattachment; existing
closure rows retain nested first-failure and independent-owner coverage (ADR-0191).

Frame delivery retains the entry child snapshot until each child's turn, even
when an earlier callback detaches it. Each visited or skipped child retires
independently outside borrows through the walk's first-failure custody. Only
empty snapshot storage returns to the registry for reuse; cached storage never
retains detached owners. Reentrant walks own separate populated snapshots.
Activity and frame-demand queries traverse borrowed children directly because
their private probes invoke no user code or owned-value destruction.
`child_snapshot_custody_survives_reentry_and_retires_outside_borrows`, a row of
`vsync_admission_contract`, pins last-owner timing, muting, reentry, competing
callback/capture failures and the next healthy frame.

A `VsyncRegistration` names the registry that issued it (a weak identity) and
one slot from a monotonic namespace that controllers and children share
(ADR-0125). Removal borrows the token: a token from another registry, or one
whose registration is already gone, removes nothing. A registry never reuses or
wraps a slot; once the namespace is exhausted, owning-controller admission and
`attach_child` refuse every new registration (`VsyncRegistrationError`), so a
stale token can never name later work.

`unregister` and `detach_child` withdraw the registered value under the
registry borrow and drop it after releasing the borrow. The destructor of a last owner may
therefore reenter the same registry, remove the same token again or register
fresh work, and removing a child keeps the tick order of the remaining
children. A panic from that destructor propagates to the caller after the
registration is already absent (ADR-0127). Destruction of a whole registry is
not covered by this contract.

Controller registration and removal are private to this crate. Public callers
own a `DrivenController`; child callers retain the explicit attachment token.
`registration_identity_and_retirement` needs private token access to pin
foreign, expired, stale and wrong-kind removal, retirement reentry and child
order. Public owner behavior remains in integration tests.

### Animation<T> Trait

The central abstraction:

```rust
pub trait Animation<T>: Listenable + Debug
where
    T: Clone + 'static,
{
    /// Current value
    fn value(&self) -> T;
    
    /// Current status (Forward, Reverse, ForwardSettled, ReverseSettled)
    fn status(&self) -> AnimationStatus;

    /// Installed run, independent of directional status; includes paused runs.
    fn is_animating(&self) -> bool;
    
    /// Observe status changes inside the notifying delivery's recovery scope
    fn observe_status(&self, observer: StatusObserver) -> StatusSubscription;

    /// Provided: a callback that ignores the scope
    fn subscribe_status(&self, callback: StatusCallback) -> StatusSubscription { .. }
}
```

Design decisions:
- **Generic over T** — any value type (f64, Color, Size)
- **Extends Listenable** — integrates with change notification system
- **Owner-local** — UI state and callbacks stay on one owner
- **Debug required** — all animations inspectable

Run ownership is required rather than inferred from status. Owners inspect their
run, combinators forward their source's answer, and `ConstantAnimation` reports
`false` for every phase. A manually positioned controller can have a directional
phase without an installed run. `controller_robustness_contract` verifies the
constant case through proxy, reverse, curve, tween and switch combinators;
`is_animating_false_after_set_value` covers manually positioned controllers.
The live-source row `a_switch_tracks_activity_after_hopping_to_a_live_source`
also checks a tween over a switch before and after adoption, completion,
restarting the old source and switch disposal.

`AnimationSwitch` retains its current source until the values actually cross.
Only exactly equal initial values adopt the next source immediately; a fixed
epsilon would cause premature switching and discard meaningful values close
to zero. Construction selects the mode and source from one pair of samples.
`nearby_values_wait_for_their_actual_crossing` covers both directions and the
subsequent source custody; `equal_initial_values_adopt_the_next_source` preserves
the immediate-adoption case.

### Curve Trait

Maps normalized progress to a finite eased value; selected curves may overshoot:

```rust
pub trait Curve {
    /// Eased progress; exact at 0 and 1, clamps outside [0,1], NaN → NaN
    /// (the full input policy is in the `Curve` rustdoc)
    fn transform(&self, t: f64) -> f64;
    
    fn flipped(self) -> Flipped<Self>;
}
```

### Animatable mappings and Tween values

```rust
/// Maps progress to one associated value type.
pub trait Animatable {
    type Value;
    fn transform(&self, t: f64) -> Self::Value;
}

/// A concrete linear mapping between two Lerp values.
pub struct Tween<V> {
    pub begin: V,
    pub end: V,
}
```

`TweenAnimation<A>` retains the mapping and its parent subscription, exposing
`A::Value` through `Animation`. A mapping specifies its output once; consumers
such as Hero accept `Animatable<Value = Rect>`, including owner-local captures.

### Simulation Trait

Physics-based value generation:

```rust
pub trait Simulation {
    /// Position at time t
    fn x(&self, time: f64) -> f64;
    
    /// Velocity at time t
    fn dx(&self, time: f64) -> f64;
    
    /// Has simulation settled? Monotonic in `time`.
    fn is_done(&self, time: f64) -> bool;

}
```

## AnimationController Internals

### State

The controller kernel stores its value, direction, run source, local time,
listener membership and committed delivery FIFO in `Rc<RefCell<_>>`.
`DrivenController` separately owns the Vsync registration and controller lifetime.
Observer clones retain the kernel without retaining that registration.

### State Machine

The directional phase is separate from position and run ownership:

| Operation | Phase |
| --- | --- |
| Admit a forward run | `Forward` |
| Settle that run, or stop it at an interior value | `ForwardSettled` |
| Admit a reverse run | `Reverse` |
| Settle that run, or stop it at an interior value | `ReverseSettled` |

A stopped run retains its current position, including an interior position.
A manually assigned value also selects a phase without installing a run.
Stopping at a bound selects that bound's settled phase.
Use the controller's value for endpoint decisions, `is_animating()` for run
ownership and the run future for success or cancellation (ADR-0193).

### Tick Cycle

A presentation samples its `MotionClock` and passes a typed `FrameTick` to
`Vsync::tick_all`. Manual controllers accept elapsed `Duration` through
`tick_at`. Source sampling releases the state borrow before invoking user code;
the run generation and sample epoch must still match before its result commits.
Value, status and outcome callbacks drain outside the borrow with shared
first-failure custody. Playback-rate changes apply after the old rate has
sampled the current frame, preserving local-time continuity.

## Mapping decisions

### Public imports preserve ownership and type identity

The root is the canonical type interface. Implementation files are private;
`curve`, `curves` and `simulation` remain explicit domain namespaces. The
focused `prelude` contains common owners, observers and mapping traits, while
frame stamps, registry publication and recovery relays require explicit imports.
The SDK's whole-crate alias exposes these identical items; it adds no adapter.
`the_re_exports_are_the_facades_types` pins owner/view/run/track identity, and
the SDK's `nested_motion` compiler fixture resolves the derive through its prelude.

`Animatable::animate` supplies method composition directly. `Tween<T>` replaces
per-value aliases without changing geometry, color or matrix interpolation.
`TweenAnimation`'s diagnostic describes its parent, so an animatable value need
not implement `Debug` to be observed. `mapped_animation_accepts_values_without_debug`
reads actual running/final values through both composition constructors with a
mapping that is neither `Debug` nor `Clone`. Only the observer's `Clone` impl
requires cloning its mapping; creating and sampling it move and borrow instead.
The same scenario carries a non-Clone/non-Debug curve through forward and reverse,
then swaps the non-Debug output's proxy to a chosen-phase constant and advances
the original source. Curved, constant and proxy observations therefore preserve
the base traits' admitted types instead of imposing diagnostic bounds downstream.
`CurveParameter` identifies rejected constructor coordinates; only diagnostic
formatting selects their range prose. Three-point normalization beyond the solver's range
has its own `RescalingOutOfRange` cause. `curve_parameters_reject_invalid_input`
checks construction and numeric serde refusal without relying on parser failure.

The retained surface has these responsibilities:

| Interface | Consumer or extension contract |
|---|---|
| `Animation`, `Animatable`, `Tween`, integer mappings, curve trait/catalog and live wrappers | Transitions, Hero flights, Cupertino route/button, Material drawer and render-object size/header motion; custom mappings use the same observation trait |
| `AnimationController`, builder/range, driven owner, status/run/error types | Widget and package lifecycle owners; builder configuration is separate from the registered owner's disposal authority |
| `AnimatedValue`, its view, `TwoWayConverter`/derive and `AnimationVector` | Implicit properties and nested application values; fixed vectors keep component admission and sampling atomic |
| `MotionUpdate`, `VsyncUpdate`, `VsyncPublication` | Multi-property retargeting, ambient migration and Dismissible publication after restoring widget state |
| Clock, preference, playback, frame and registry types | Runtime frame selection, TickerMode, DevTools playback and actual timer/scroll consumers |
| Simulation, checked descriptions, bounds, tolerances and errors | Scroll/page physics and controller fling; physical parameterizations form the documented custom-motion constructor contract |
| `Keyframes`/builder/errors and `Stagger`/origin | Three indicator implementations and authored Hero tracks |
| `ConstantAnimation`, unit endpoints, subscriptions, callback/recovery relay types | Navigator idle endpoints and source-bound removal across higher-layer cleanup; hidden relays remain an intentional cross-crate seam |

`finish`, chosen-phase constants, cubic keyframes and immutable authored easing
retain their explicitly accepted application-author contracts. Pure
`ConstantTween`, `ReverseTween` and `CurveTween` had no production caller and are
removed rather than given another adapter layer. Fixed observations, reversed
linear endpoints and live curve observations already have direct interfaces;
these choices do not claim equivalence for arbitrary parent-dependent mappings.
The public API change is intentional before 1.0; its migration is recorded in
the changelog fragment rather than preserving parallel legacy spellings.

### Fixed values admit no listener identity

`ConstantAnimation` drops value callbacks and recovery relays at admission.
It returns the inert `ListenerId` at `usize::MAX`, which core notifier allocation
cannot issue because it checks a representable successor before admission.
No process counter, callback storage or wrap/reissue path is needed for a source
that cannot notify. Value IDs otherwise remain source-scoped; status removal
authority is bound to its source by `StatusSubscription`.
`constant_listener_tokens_never_withdraw_live_callbacks` observes callback
retirement and later notifications from a real `ChangeNotifier`; the previous
counter token collided with its first live listener and fails that assertion.

### Perceptual spring parameters retain physical retargeting

Duration/bounce describes a physical `SpringDescription`; it does not impose a
fixed completion deadline. `a_time_defined_spring_inherits_velocity` interrupts
that actual owning motion with a narrow target and substantial incoming momentum,
checks subsequent position and velocity against an independent underdamped ODE,
then observes exact rest, original-run cancellation and new-run completion.
Discarding only that parameterization's incoming velocity fails its position oracle.
An immutable `SpringCurve` instead describes normalized easing; when admitted as
`MotionSpec::Curve`, it uses the generic Hermite velocity correction and assigned
deadline. Direct keyframe sampling retains the curve's authored time scale.

### Changing one half of a motion spec keeps the other

`MotionSpec::with_duration` keeps the easing and `with_curve` keeps the
duration, so `.curve(..).duration(..)` and `.duration(..).curve(..)` on an
implicit widget's `MotionBuilder` agree. A spring has neither half: asked for a
duration it takes the default easing (`EASE_IN_OUT`), asked for a curve it
takes `MotionSpec::DEFAULT_DURATION` (200 ms), the same values
`MotionSpec::default()` uses. `motion_spec_changes_keep_the_other_half` pins
the four cases.

### Immutable authored easing retains the existing Curve domain

`PiecewiseLinear` canonicalizes a borrowed stop slice once, then shares the
finite ordered knots immutably. Missing endpoints become 0/1, decreasing inputs
are raised before omitted interior inputs are distributed, and an interior
equal-input group selects its last output. Endpoints remain exactly 0/1,
outside inputs clamp, and NaN propagates. This adopts CSS Easing Level 2's stop
placement without its extrapolating/source-text interface. The explicit stop
count bound keeps spacing ratios exactly representable as integer inputs.

Interpolation uses foundation's stable Lerp across opposite finite extremes.
Regular-knot slopes select the outgoing segment (incoming at 1), repeated-input
jumps select zero, and unrepresentable finite-segment rates saturate finitely.
`SpringCurve` reuses the validated physical `SpringSimulation`, including its
conservative rest estimate. It samples normalized progress times an upward-rounded
`Duration`, retaining overshoot. It is a pure duration-based authoring value;
`MotionSpec::Spring` remains the velocity-inheriting motion contract.

Both concrete curves participate in `ArcCurve`'s built-in value comparison.
Equivalent recreated configuration retains the original AnimatedValue run and
future, rather than rediscovering equality through allocation identity. Neither
curve adds user callbacks, shared mutation, frame state or retirement obligations.
Admission can allocate; query-time storage is immutable. Optional piecewise serde
uses the same checked constructor and serializes canonical points.

The owner permits retaining these authoring surfaces before a stock widget
requires them. `authored_easing_contract` asserts canonical placement, finite
extremes, independent spring ODE references, input policy, recreated-run lifetime,
natural conservative duration and once-only original-future completion.
`authored_piecewise_hero_geometry_is_painted_and_retraced` and
`authored_spring_hero_geometry_is_painted_and_retraced` pass them through the real
Hero factory, compare painted bounds to independent references, reverse an
airborne flight, and verify silence after unmount. Each paint case fails under
its independent interpolation/time-scale negative control. The existing
`a_steady_state_frame_allocates_nothing` includes 10,000 advancing samples of
both eased tracks, with the allocation counter's positive control.

### Run and sample identities never wrap

Each admitted run consumes a monotonically increasing generation; each sample
consumes an epoch before calling its source. Exhausted namespaces refuse new
runs with `AnimationError::IdentityExhausted`. A final admitted run can finish;
sample exhaustion instead cancels its pending outcome without publishing a
new sample. Stop and reset remain available without reopening admission.

`exhausted_run_identities_refuse_without_displacing_the_last_run` and
`exhausted_sample_identities_cancel_without_reissuing_a_stale_sample` seed the
terminal counter boundary privately, then observe only controller behavior.

### Terminal owners retire outside their guards

Controllers, proxies, curved animations and switches withdraw their callbacks,
simulations, curves and subscriptions from shared state before dropping them, so
no destructor runs under an internal lock. User parent removal and queries also
run outside those locks. A registration is owned by the animation as soon as it
is accepted, so a constructor that fails later still detaches it. After the
first destructor failure in a retirement, or while the thread is already
panicking, the remaining owned values are retained rather than dropped
([ADR-0127](../../../docs/adr/ADR-0127-exceptional-path-retention.md)); the
first failure propagates. Callbacks are owner-shared `Rc`s, so a snapshot
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
`animate_back_curved`, `repeat`, `fling`, `fling_with`,
`animate_with`, `animate_back_with`) returns
`Result<AnimationRunFuture, AnimationError>`: `Err` means the run could not start
(the controller is disposed); the `AnimationRunFuture` is how the run ends,
`Ok(())` on a normal finish and `Err(RunCanceled)` when it is superseded
or torn down. `AnimationControllerInner.run: Option<ActiveRun>` holds the one completer
this controller ever has, together with the run's mode and settle policy; every run-ending or
run-starting site funnels through `AnimationController::deliver_transition`, the single
chokepoint that owns `drop(inner)` then delivers — see
`docs/adr/ADR-0064-animation-completion-is-one-controller-resolved-future.md`
for why resolution moved here rather than staying on `Ticker` (a lock-order
fact).

**Per-site table** (guard held → what happens → delivered after unlock):

| Site | Publishes | Displaces |
|---|---|---|
| `forward_from`/`reverse_from`/`drive_to`/`repeat`/`fling_with`/`drive_simulation` (non-settling path) | nothing (installs a fresh completer) | previous `run`, canceled |
| `forward_from`/`reverse_from`/`drive_to`'s zero-distance-or-zero-duration settle (`settle_at_target`, see its own entry below) | the trivial run, complete; value notified only if it actually moved | previous `run`, canceled |
| `tick_time_based` (non-repeating end) | the finishing run, complete | — |
| `tick_repeat` (exhausted end) | the finishing run, complete | — |
| `tick_simulation` (`is_done`) | the finishing run, complete | — |
| public `finish` (a finite run with a declared endpoint) | original run, complete at its endpoint | completed simulation and curve, retired after delivery |
| `stop`/`set_value` (`stop_running`) | — | `run`, canceled |
| `reset` (`stop_running`) | — | `run`, canceled |
| `dispose` | — | `run`, canceled |
| last controller kernel drop | — | the remaining active run is canceled; Vsync keeps only a weak controller reference |

**Publish-before-listeners.** A natural end (`tick_time_based`,
`tick_simulation`) takes `run` and calls
`RunCompleter::complete()` **before** `drop(inner)` — the same guard scope
that sets `status`, before listeners are notified; only *delivery*
(continuations, wakers) is deferred past the unlock. This is what makes a panicking status listener
leave the run `Ok`: the controller drains the admitted `RunDelivery` with
the already-published outcome, even after containing a listener failure.

**Status-before-cancel.** A run-starting site displaces `run` under
the lock, but transition delivery fires the run's own (new) status listeners **before**
delivering the displaced run's cancellation — the observable order a caller
sees is "the new run started" then "the old one was canceled".

**Explicit successful completion.** `AnimationController::finish()` is an
application-authoring command, independent of frame demand and playback rate.
It commits the original finite run's endpoint and successful outcome before
calling listeners. Idle calls are silent; disposed calls fail. Infinite repeats
and arbitrary custom simulations have no declared endpoint and are refused
without sampling or canceling their run. Finite repeat parity determines the
endpoint; native flings use their bound and property motion uses its target.
Completed sources leave the kernel before delivery and retire afterward through
the existing recovery custody. A listener's replacement run remains authoritative.
The public cases `explicit_finish_completes_the_original_finite_run` and
`explicit_finish_retires_sources_with_failure_custody` pin these guarantees;
`explicit_finish_commits_an_authored_page_and_retires_the_old_route` exercises
the command through an application-authored navigator page and committed opacity.

### Direction is chosen by the method; a run ends in its direction's settled status

**Rule:** `animate_to`/`animate_to_curved` always run `Forward`, and
`animate_back`/`animate_back_curved` always run `Reverse`, regardless of
whether `target` is above or below the current value: the caller's choice of
method is the one bit of direction information it carries. Every run ends in its direction's
settled status with no bound check
(`AnimationDirection::settled_status`: `Forward` → `ForwardSettled`, `Reverse` →
`ReverseSettled`), at every run end: `tick_time_based`'s non-repeat end,
`tick_repeat`'s exhaustion end, `tick_simulation`'s `is_done`, and
`settle_at_target`. The rule is unconditional, so `animate_to(lower_bound)` from mid-range ends
`ForwardSettled`, not `ReverseSettled` — and the same holds through
`tick_simulation`: `animate_with(sim)` landing on a BOUNDED controller's
lower bound also ends `ForwardSettled`. A fling's own end is unaffected:
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
status listener matches `ForwardSettled | ReverseSettled` identically (it only ends
the scroll activity), so its `animate_to` toward a SMALLER pixel value
reporting `Forward`/`ForwardSettled` instead of `Reverse`/`ReverseSettled` changes
nothing observable. The button's release fade is the consumer the rule
reached: its start was chained off a status listener watching `ForwardSettled`,
which stops distinguishing "the press just landed" from "the release just
landed" once both report `ForwardSettled`, so the listener re-triggered a
redundant zero-distance settle when the release it started reached its own
end (no observable trace — same-status writes are deduplicated — but the
wrong shape). It now chains the release on the press fade's own
`AnimationRunFuture` (`chain_release_fade`, `packages/flui-cupertino/src/button.rs`),
`Ok`-only and one-shot.

**`stop()`/`set_value` keep the bounds-first rule.**
`AnimationControllerInner::settled_status_directed`/`settled_status_keep_direction`
still check `is_at_upper_bound`/`is_at_lower_bound` first, falling back to
direction only for a non-bound stop. This is FLUI's own frame-driver
contract: a scroll gesture's every-frame `set_value` must
report the bound it actually reached, not the gesture's nominal direction,
so a driver polling `status().is_directional()` sees a real settle.

### `settle_at_target` also covers zero-duration runs, and gates value notification on real movement

**Rule (extends the per-site table above):** `settle_at_target` is the single
settle chokepoint for BOTH zero-DISTANCE runs (`forward()` already at the
upper bound) and zero-DURATION runs (`forward(..., Some(Duration::ZERO))`, or
a zero base `duration`): `forward_from`/`reverse_from`/`drive_to` compute the
run duration before their settle check and gate on
`value == target || run_duration.is_zero()`, which covers both causes
identically.

Representable differences are preserved regardless of value units. Timed runs
retain their authored duration for distinct targets; bound phases require exact
endpoint equality, and value notifications compare exact entry and exit values.
Numerical solver accuracy and physical rest thresholds do not define equality.
`small_distances_keep_their_authored_duration`,
`small_remaining_distance_runs_until_its_scaled_deadline`,
`small_from_jumps_notify_their_exact_value`,
`small_zero_duration_settlement_notifies_the_endpoint` and
`nearby_interior_positions_keep_their_directional_phase` pin these rules through
public controller commands in both directions.

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

**`status().is_directional()` is not proof a run is installed; `run.is_some()`
is.** Two independent paths leave `status` reporting a RUNNING value
(`Forward`/`Reverse`) with no run actually installed: a mid-run `dispose()`
(above), and `set_value` at an interior value. `set_value` calls
`stop_running()`, clearing `run`, but still reports a directional
running status (`AnimationControllerInner::settled_status_keep_direction`).

A `Vsync`-driven controller polled on `status().is_directional()` after a
mid-run `set_value` would therefore look like it still has a run to
advance. The NEXT `tick_all` would then recompute its value from the
stale, already-stopped run's `start_value`/`target_value`, silently
overwriting what `set_value` had just set.

`AnimationController::walk_probe`'s `live_running` is
`!disposed && run.is_some()`, the two facts that are always
consistent with "is there a run to advance". Both
`AnimationController::tick_at`'s own guard and `Vsync::has_running`/
`tick_all` read it instead of bare `status().is_directional()`.

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
(`repeat`'s value-at-the-call sample and zero-period settle, `tick_repeat`'s
running leg, and exhaustion, `finish` and policy settlement through
`RepeatRun::landing`) defers to them instead of re-deriving it.

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
direction settled (`ForwardSettled` at `max`, `ReverseSettled` at `min`) — matching
the Web Animations spec's fill behavior ("holding the endpoint of the final iteration rather
than the start of the next"), Android's `ValueAnimator`, and Compose's
`VectorizedRepeatableSpec`, all of which land on the actual endpoint.
The tests `maximum_elapsed_time_exhausts_a_finite_repeat`
and `finite_bounce_holds_stale_samples_and_settles_on_its_final_leg`'s
exhaustion assertion (`tests/contracts/controller_commands.rs`) pin
the landing/status.
These public scenarios are rows of `repeat_contract` and `controller_contract`
in `tests/contracts/controller_commands.rs`; a failure names its row.

**An empty range stays rejected.** A repeat that can structurally never
change value is a caller error that would hold the frame loop open forever
doing nothing, and failing fast beats a silent no-op animation. The range is
a `ValueRange`, so equal endpoints never reach the controller; a range that
collapses once clamped into the controller's bounds is refused with
`InvalidRepeatRange` (`repeat_rejects_an_empty_range` pins both).

**Zero effective period, or an explicit zero count, both settle
synchronously at the call — two DISTINCT degenerate cases.** A zero
effective period (any `count`): Android's `ValueAnimator.animateBasedOnTime`
explicitly "ignores the repeat count and skips to the end" for a
0-duration animator; Compose's `InfiniteRepeatableSpec` throws
("Animation to be infinitely repeated cannot have a 0-duration",
`AnimationSpec.kt`'s `init` block). FLUI repairs rather
than rejects (the house rule — see `ValueRange`'s validation contract for
the cases that DO reject): an infinite zero-period repeat
that ticked once per frame would hold the frame loop open forever doing
nothing, so `repeat` settles it synchronously through the same
`settle_at_target` chokepoint a zero-distance/zero-duration
`forward`/`reverse`/`animate_to` uses, landing on `repeat_landing`'s
endpoint for the finite count (or cycle 0's end for an infinite one) — a
documented exception to "an infinite repeat's future resolves only by
cancellation". `Iterations::Finite(0)` (any period), separately: zero
iterations run at all, so there is none to land on — the settle value is the
plain CLAMPED CURRENT value, not a landing jump, status `ForwardSettled`, in
either mode (Web Animations semantics for an empty active interval; Compose
throws for `iterations < 1`). `zero_iterations_settle_at_the_call_without_landing`
pins it.

**One period for both legs of a bounce; `set_duration` does not retime an
active repeat.** `RepeatRun::period` is resolved ONCE at the call
(`period.unwrap_or(duration)`), not read live on every tick. Falling
through to `current_duration()`'s live `duration`/`reverse_duration` lookup
whenever `period` defaulted would let the reverse leg of a bounce with a defaulted period
could pick up `reverse_duration` instead of the forward leg's period
(contradicting `RepeatSpec::with_period`'s "instead of the forward
duration"), and a live `set_duration` mid-repeat change the running period out
from under it. Resolving once avoids both, and keeps the modular-nanosecond
arithmetic in `tick_repeat` exact (one divisor for every leg).

**`velocity()` on a reverse leg is SIGNED.** `velocity()` reports `range / duration` off
the leg's own `start_value`/`target_value`, which are swapped on a reverse
leg, so it comes out negative — matching FLUI's non-repeat velocity
contract rather than introducing a repeat-specific special case. Pinned by
`repeat_reverse_leg_velocity_is_negative`.

### Unbounded is a constructor fact; bound-targeting runs on it are refused; no path reads NaN

`AnimationControllerBuilder::unbounded` fixes bounds at
`(f64::NEG_INFINITY, f64::INFINITY)` and is infallible: unboundedness is a
configuration fact, never a bound value.

`ValueRange::new` rejects any bound that is not finite, including a wide-open
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

**Explicit initial values share direct-assignment canonicalization.** A finite
input clamps to the configured range. For bounded controllers, NaN and negative
infinity select the lower endpoint; positive infinity selects the upper endpoint.
For unbounded controllers, non-finite inputs retain the default zero. A shared
pure helper applies this rule at construction and in `set_value`; construction
does not invoke the assignment command's notification or cancellation machinery.
The finite state and warning latch are committed before calling diagnostics,
with no controller borrow held. Non-finite initialization and subsequent direct
assignments share the once-per-controller warning. `controller_builder_contract`
pins all six bounded/unbounded non-finite cases, warning count and subsequent
successful motion.

**Initial value and status, and a recorded cost.** `value = 0.0`
(never `lower_bound`, which is `-inf`). The initial `status` is computed by
the SAME rule `set_value` applies,
`AnimationControllerInner::settled_status_keep_direction`, applied once at
construction for every constructor, not hard-coded `ReverseSettled`: a bounded
controller at `lower_bound` still reports `ReverseSettled` (unchanged), but an
unbounded one at `0.0`, with `AnimationDirection` defaulted `Forward` and
neither infinite bound ever "at", reports **`Forward`**.

`settled_status_keep_direction` falls to its directional `switch` at
`value == 0.0`, since that value is neither the lower nor upper bound in the
general case.

The recorded cost: a never-run unbounded controller reports
`status().is_directional() == true` from the moment it is constructed. The
real "is a run installed" fact stays `is_animating()`/the internal
`run`, never `status`, exactly as
[`AnimationController::walk_probe`]'s own doc already documented before
this change (`set_value`'s interior-value status already had the identical
property).

One direct, load-bearing consequence: **the first `stop()` on a fresh
unbounded controller emits `Forward → ForwardSettled`.**
`settled_status_directed`'s bound checks are both false on an infinite
range, so it falls to `match direction { Forward => ForwardSettled }`. Every
`jump_to` on a fresh `Scrollable` (it owns an unbounded controller built
with `AnimationController::builder(..).unbounded()`) triggers this event through the
fling controller's `stop_hook`.

**Refusals carry typed input, range or arithmetic causes (ADR-0198).**
Derived from `!lower_bound.is_finite()` (both bounds are `+-inf` together
by construction, so either alone detects it): `forward`/`forward_from`,
`reverse`/`reverse_from`, `fling`/`fling_with`, and a `repeat` given no
range (so it defaults to this controller's own bounds) are refused on an
unbounded controller. There is no finite bound/range to run to. A repeat
`with_range` on an unbounded controller WORKS: an explicit finite range is
not "unbounded" in the relevant sense.

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
used to write `inner.direction` before its underdamped-spring check,
corrupting direction on a refused fling (observable only via a LATER
`stop()`); fixed by computing `direction`/`target` into locals and
assigning only once every check passes.

Every refusal is also reported (ADR-0220): each public command returns
through `refusal::report`, which emits one `warn!` on `REFUSAL_TARGET`
(`flui_animation::refused`) with the command's `operation`, the error's stable
`kind` and the error, after the command has released its borrow. Built-in
drivers can discard the result, so a silent no-op would hide the caller bug.
`stop` and `reset` on a disposed controller report at `debug`: tearing down
twice is ordinary. `refusal_diagnostic_contract` holds one row per reachable
error kind.

**`repeat` takes its range as a `ValueRange` and reports `InvalidRepeatRange`
with endpoints, controller bounds and a `RangeDefect`.** A `RepeatSpec` range
is finite, ordered and of finite span by construction, so a NaN endpoint (which
would reach `inner.value.clamp(lo, hi)` as the clamp's own bound and panic) and
an overflowing span (which would make `repeat_sample` publish NaN or infinity)
are refused by `ValueRange::new`, before any controller exists to refuse them.
The controller refuses what only it can see: a range that collapses once
clamped into its bounds (`RangeDefect::Inverted`), and, on an unbounded
controller, a repeat given no range, which defaults to infinite bounds
(`RangeDefect::NonFinite`). The public `overflowing_repeat_span_preserves_the_run`
case pins the constructor's span refusal and that a refused repeat leaves the
displayed value, the original pending future and that run's later completion
unchanged.

**`set_value`/simulation NaN canonicalization stays infallible, latched.**
`set_value` on a BOUNDED controller keeps its existing canonicalization
(`NaN` → `lower_bound`, `+-inf` → the bound it points at): a declared PIN.
On an UNBOUNDED controller a non-finite input is a FULL no-op instead: no
`stop_running`, no notification, the value stays exactly what it was. A
poisoned gesture drag or a bad computation must not clobber a live fling
or snap a scrollable to a bound it doesn't have.

A mid-run simulation sample going non-finite (`tick_simulation`) ENDS the
run at the LAST FINITE value: settled status by direction, `run`
completed (`Ok`), ticker stopped, rather than "value unchanged, run
continues" (the latter would leave `run` installed, `Vsync`
ticking forever, and a scrollable's `is_scrolling` stuck). This is new
behavior on this path, not a preserved pin: before this change, a running
simulation's sample had no non-finite handling at all and reached `clamp`
unchanged, which returns a `NaN` self as-is.

Both paths share ONE latch, `AnimationControllerInner::non_finite_warned:
bool`, fired at most once per controller (`warn_non_finite_value`),
distinct from admission's per-call warn: a NaN-producing drag or a
misbehaving simulation must warn once, not every frame.

**`tick_time_based` reads `start_value`/`target_value` DIRECTLY at the
exact endpoints (`t <= 0.0` / `t >= 1.0`), never via `start + range *
eased_t` there.** Special-casing only `eased_t` (to exactly `0.0`/`1.0`)
while still computing the product is not enough: `range` can be `+-inf` in principle
(the span-overflow case above), and `inf * 0.0 = NaN`.

Reading the field directly at the boundary removes the multiplication from
that path entirely, so no path through `tick_at` can ever compute or store
a NaN value: the issue's own stated criterion.

**Consumer construction.** `Scrollable` owns its ballistic controller through `AnimationController::builder(..).unbounded().build_on(..)`.
Unbounded configuration is explicit; no literal infinite endpoint pair or
fallible range construction is needed.

`scroll_controller.rs`'s `service_pending_command` raises
`ScrollPosition::set_is_scrolling(true)` only AFTER
`fling.animate_to_curved(..)` returns `Ok` AND the resulting status is
still running. A refused start (a non-finite target reaching this call)
must not park the scrollable in "scrolling" forever, and a start that
settles SYNCHRONOUSLY (target already equals the just-synced current
value) already reported its own end through the fling status listener
installed above; checking `is_directional()` rather than firing
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
controller lands at `0.0` with status `ReverseSettled` (its documented
contract); construction, or a later `set_value(0.0)`, lands at the SAME
`0.0` with status `Forward` (the keep-direction rule above). Two different
routes to the identical value report two different statuses: a caller
that only inspects `value() == 0.0` cannot infer `status()` from it on an
unbounded controller the way it could reason "at the lower bound implies
ReverseSettled" on a bounded one.

**Bounded ranges and unbounded initialization have separate contracts.**
`bounded_ranges_refuse_infinity_and_unbounded_starts_at_zero` rejects a
wide-open `(NEG_INFINITY, INFINITY)` range and drives an explicitly unbounded
controller from zero. Infinite endpoints cannot serve as the initial value
of a bounded controller.

### Friction numerical range

Friction displacement uses `exp_m1(log_drag * time)`, and inverse arrival time
uses `ln_1p` of the relative displacement. This retains small finite movement
as drag approaches one. A position the motion never reaches (behind the
start, at or past the resting position) answers `+inf`. Public consumer test
`friction_preserves_small_decay_and_position_time_roundtrips` checks the
constant-velocity limit, positive and negative normal flings, position/time
round trips, unreachable queries and the remaining-glide rest.

### Simulations are validated values that rest at a precomputed time

- A spring is stored as `(ω, ζ)` with private fields; every constructor
  refuses NaN, infinite and non-positive inputs (an undamped spring never
  rests) and constants that overflow (`SimulationError`), the same in debug
  and release. `spring_constructors_refuse_outside_the_admitted_domain`.
- `SpringSimulation::new` is the sole constructor and takes an explicit
  tolerance. Unrepresentable coefficients return `SimulationError::Overflow`;
  admission never substitutes a motion already resting at the target. Fling
  constructs its simulation before replacing any live source, direction or
  future, and preserves the typed cause as `AnimationError::Simulation`.
  `fling_with_overflowing_spring_preserves_the_admitted_run` checks the refused
  replacement, continued original trajectory, successful original completion
  and a healthy subsequent command in `springs_at_the_edge_of_f64_stay_finite_and_rest`.
- Policy and missing-clock settlement finish native flings at their declared
  bound. The bounded probe of arbitrary simulations cannot identify a slow
  fling's endpoint. `fling_settlement_reaches_its_declared_bound` in
  `simulation_settle_grid` covers both directions under Reduce, missing clock
  and exhausted clock, endpoint publication before completion delivery, one
  transition and healthy subsequent motion.
- The motion is one closed form for every `ζ`:
  `x = e^{−at}[x0·C + (a·x0 + v0)·S]`, `v = e^{−at}[v0·C − (ω²x0 + a·v0)·S]`,
  `a = ζω`, with `C`, `S` summed as series where `|ω²(1 − ζ²)t²| < 10⁻²`
  (continuous through `ζ = 1`) and the overdamped roots computed without
  cancellation (`λs = −ω/(ζ + √(ζ² − 1))`), so a very heavy spring still
  moves and every `t ∈ [0, ∞]` is finite. `spring_matches_analytic_reference`,
  `spring_outputs_are_finite_over_the_admitted_domain`.
- Rest is a moment, computed at construction from a conservative envelope of
  the motion (`(1 + at)·e^{−rt}`, the amplitude bound below `ζ = 1`, the
  two-exponential bound above): from it on the simulation stays within its
  tolerance, `x` is exactly the target, `dx` is `0.0` and `is_done` stays
  true. Rest therefore does not depend on how frames partition time, and is at
  most `1.25·t_last + 2/ω` for the last moment `t_last` the motion leaves the
  tolerance. `rest_time_is_conservative_and_tight`,
  `spring_run_is_independent_of_frame_partition`,
  `simulation_time_domain_edges`.
- A tolerance with an infinite velocity limit derives one from the motion's
  own time scale (`distance·ω`, or the remaining glide for friction). Scroll
  physics use `Tolerance::for_device_pixel_ratio`, half a device pixel, with
  the ratio read from the presentation's pipeline at release
  (`ScrollMetrics::device_pixel_ratio`).
  `scroll_fling_rest_scales_with_device_pixel_ratio`.
- `BouncingScrollSimulation` hands friction to the edge spring at the moment
  the friction reaches the edge, with the friction's velocity, so position and
  velocity are continuous. `bouncing_simulation_hands_friction_to_spring_at_the_edge`.

## Composition Model

Animations compose via `Rc<dyn Animation<f64>>`:

```
AnimationController (produces 0.0 → 1.0)
        │
        ▼ Rc<dyn Animation<f64>>
CurvedAnimation (applies easing curve)
        │
        ▼ Rc<dyn Animation<f64>>
TweenAnimation<Tween> (maps to Color)
        │
        ▼ Animation<Color>
```

The wrappers share a private `ParentLinks` allocation containing the parent,
separate value/status notifiers and one subscription to each parent channel.
Cloning a wrapper shares these links; final retirement withdraws outgoing
callbacks before detaching the subscriptions. curves and animatables remain
owned by each value clone and follow terminal retirement when authored
destruction fails.

`CurvedAnimation` also shares the run's entering direction in an owner-local
`Cell`. A direction change during that run keeps the selected curve; resting
status clears the selection so the next run can choose its forward or reverse
curve. Value evaluation and authored destruction execute after storage borrows
end. `TweenAnimation` maps the sampled parent value, while `ReverseAnimation`
mirrors both value and status.

## UI ownership

Controllers, Vsync registries, notification channels and animation wrappers
share owner-local state through `Rc`, `Cell` and `RefCell` (ADR-0175).
Borrows end before callbacks, curves, simulations, diagnostics or outgoing
captures run. Frame wakers retain a separate cross-thread wake capability;
they do not retain UI callback storage.

## Error Handling

[`error.rs`](../src/error.rs) defines the authoritative public error variants.
`ValueRange::new` returns the narrow `RangeDefect`; controller and animated-value
operations return `AnimationError`. `AnimationInput` identifies the refused
value. Numeric fields, component indices and original `SimulationError` causes
remain matchable without parsing a message. Construction of an already-validated
`ValueRange` cannot produce a controller-lifetime error.

The error enums are `#[non_exhaustive]`; outcomes can clone their causes. Typed
refusals commit no run change, and diagnostics run after borrows end. The public
`typed_refusals_preserve_the_original_run` case checks exact input kinds and
values, bounded infinity clamping, unchanged samples and original-future delivery.
ADR-0198 records migration and optional serde limits.

## Memory Management

### Rc Sharing

```rust
// `AnimationController` is itself a handle over `Rc`-shared state:
// `clone()` shares the controller.
let controller = AnimationController::builder(duration).build();
let curved: Rc<dyn Animation<f64>> =
    Rc::new(CurvedAnimation::new(Rc::new(controller.clone()), curve));
let tweened = TweenAnimation::new(tween, Rc::clone(&curved));
```

Benefits:
- Cheap cloning (pointer copy + reference-count increment)
- Automatic cleanup on last reference drop
- Owner-local sharing

### Explicit Disposal

An owning `DrivenController` unregisters and disposes automatically on drop.
It can also end that lifetime explicitly while observer clones remain alive:

```rust
owner.dispose();
```

After disposal, driving operations (`forward`, `reverse`, `animate_*`,
`fling*`, `repeat*`, `stop`, `reset`) return `Err(AnimationError::Disposed)`
and the active run is canceled.

Clones of an `AnimationController` share its kernel; dropping an observer
does not end its owner's lifetime. Public observers cannot dispose the kernel
and leave a registry seat behind. Owning disposal is idempotent. A manually
sampled controller built with `build()` is canceled when its last handle drops.

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

`IntTween` converts both integer endpoints to `f64` before subtracting them.
Every `i32` endpoint is exactly representable there, whereas subtracting
`i32::MIN` from `i32::MAX` in the integer domain panics or wraps. Its
`Rounding` is explicit; `Nearest` rounds a half toward positive infinity, as CSS
Values 4 §5.2.1 interpolates integers. It compares the exact fraction
`x - floor(x)` with one half: `(x + 0.5).floor()` would round the largest `f64`
below one half up to 1. The public consumer family
`integer_tweens_interpolate_across_the_full_range` checks both directions across
the full range in every rounding mode, non-half fractions, negative halves and
the value just below a half.

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
