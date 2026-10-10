# Performance

Performance characteristics of `flui_animation`.

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup.

## Motion policy and idle frames

`Vsync::has_running` describes continuous animation activity. A parked Normal
repeat under Reduce contributes no continuous demand, while a Preserve
indicator still does. Pending settlement and Full resumption can require a
single frame even when continuous demand is false, including with paused
playback. The registry retains that obligation across gates and rebinding.

Property owners keep independent timelines: retargeting one property does not
restart unchanged properties. Render-owned opacity, rotation, Drawer and
Dismissible transitions invalidate their retained render path instead of
rebuilding content on every sample. The workspace `motion_lab` example makes
these paths interactive; its activity indicator intentionally keeps frames
running, so it is not an idle-frame benchmark.

The historical timing tables below do not measure the current policy path.

## Registry workload validation

The registry benchmark retains one `MotionClock` and advances a 60 Hz timestamp
on every iteration. Active and stopped populations both cover 100, 1,000, 5,000
and 10,000 owners. Active runs last 136 years of virtual time; untimed checks
verify progress on successive frames and that the measured loop leaves them
running. The mixed population retains exactly 100 active owners out of 1,000.
Active results include sampling and delivery, not just registry lookup.

Validate the workloads without collecting performance measurements with:

```bash
cargo bench --profile dev -p flui-animation --bench vsync_registry -- --test
```

This smoke run is not a timing result. Earlier migrated benchmark code created
a new clock and repeated one timestamp, so it did not measure sustained active
frames. Comparisons need baseline and final runs with the same corrected
workload, release profile and host conditions.

### Same-host registry comparison, 2026-10-10

The corrected workload ran sequentially on Windows x86_64 MSVC, an Intel
i9-13900K (24 cores, 32 logical processors), with Rust 1.99.0, Criterion 0.8.2
and the workspace's optimized bench profile. Each row used 10 samples, a
500 ms warm-up and a one-second measurement. Builds used six jobs; each
checkout had its own target directory. Other host activity and CPU frequency
were not controlled.

The baseline was `c297b6c89bc93941e28c738c9dfac3d623a2dae2` from `origin/main`.
Only its benchmark was changed to use the same advancing-clock workload as
the candidate, `b241a4f54a7fc2d8b3eb07a24bc2fed417555738`. The two workload
files were byte-identical. Baseline production code was unchanged. This
compares against current main, which already has owner-local controllers;
it does not attribute the performance of the whole owner-local migration.

Criterion median estimates, in microseconds:

| Population / operation | Owners | Baseline | Candidate |
|---|---:|---:|---:|
| Stopped frame | 100 | 1.445 | 1.429 |
| Stopped frame | 1,000 | 28.432 | 27.289 |
| Stopped frame | 5,000 | 212.185 | 213.512 |
| Stopped frame | 10,000 | 431.753 | 443.140 |
| Active frame | 100 | 5.971 | 5.983 |
| Active frame | 1,000 | 75.052 | 74.843 |
| Active frame | 5,000 | 495.902 | 485.488 |
| Active frame | 10,000 | 992.824 | 1,013.251 |
| Frame with 10% active | 1,000 | 33.283 | 31.135 |
| Retire all owners | 1,000 | 95.275 | 99.776 |
| Retire all owners | 10,000 | 1,346.989 | 1,447.577 |

A filtered repeat exposed a benchmark assertion on unmeasured populations.
Both copies now check post-measurement progress only when the measured loop
advanced time; their untimed progress and activity checks remain. This changes
no timed operation. The repeated workload files were again byte-identical.
Repeat with:

```bash
cargo bench -p flui-animation --bench vsync_registry --locked -- --noplot 'unregister_all|running_vsync_registry/10000|stopped_vsync_registry/10000'
```

| Repeated operation | Owners | Baseline | Candidate |
|---|---:|---:|---:|
| Stopped frame | 10,000 | 451.371 | 440.448 |
| Active frame | 10,000 | 1,058.414 | 1,069.997 |
| Retire all owners | 1,000 | 95.360 | 106.933 |
| Retire all owners | 10,000 | 1,331.156 | 1,463.162 |

The active 10,000-owner repeat has overlapping 95% median confidence intervals
(baseline 1,050–1,081 µs, candidate 1,046–1,100 µs). Same-version runs vary
about 6% on this shared host. Owner retirement was consistently slower in
the candidate: roughly 5–12% at 1,000 owners and 7–10% at 10,000. The latter
has broad confidence intervals. These results require profiling retirement;
they do not establish an improvement for every row. Admission, retarget
latency and a historical baseline remain separate measurements.

The retirement workload explicitly calls `dispose`, then drops the owner.
The candidate's repeated `dispose` still constructed recovery and published
an empty retirement receipt. An already retired owner now returns immediately
from the public `dispose` entry; first retirement and grouped preparation
retain their existing closure and delivery paths. The public ownership table's
`dispose_then_drop_is_one_retirement` checks repeated disposal and exactly one
run outcome.

Additional sequential Criterion slope estimates (µs) from the same filtered
workload:

| Owner retirement | 1,000 | 10,000 |
|---|---:|---:|
| With the retired-owner return | 92.216 | 1,223.9 |
| Return removed for control | 94.965 | 1,295.4 |
| Return restored | 89.912 | 1,151.8 |

The restored run's Criterion comparison with the control reported improvement
in both rows. The first control comparison had overlapping intervals, however,
and this shared-host experiment does not establish a fixed speedup percentage.
The structural result is removal of redundant recovery on repeated disposal;
these numbers do not replace the remaining admission/retarget or full-frame
measurements.

## Measured benchmarks

The benchmark tables below are historical measurements from before the
owner-local controller migration. They are not measurements of the current
implementation. The committed Criterion
targets `benches/animation_bench.rs` and `benches/vsync_registry.rs`. The later
per-controller analysis also records historical scratch measurements and a
standalone prototype; those experiments are not committed, and the command
below does not reproduce their attribution percentages or the 15,000–30,000
controller samples. Absolute numbers are machine-relative and both hosts below
were shared with other builds, so read them as orders of magnitude and as a
historical context, not as a current regression baseline or hardware promise.
Run the committed targets with:

```bash
cargo bench -p flui-animation --bench animation_bench --bench vsync_registry
```

### Controller frame path

Windows development host, shared. Every `tick_at` row measures a run that
cannot finish during the bench, so it times a real frame advance rather than
the early return of a settled controller.

| Bench | Median |
|-------|-------:|
| `controller/tick_at/linear` | 41.6 ns |
| `controller/tick_at/ease_in_out` | 52.0 ns |
| `controller/tick_at/1_value_1_status_listeners` | 62.0 ns |
| `controller/tick_at/4_value_1_status_listeners` | 130.3 ns |
| `controller/tick_at/simulation_friction` | 64.3 ns |
| `controller/tick_at/simulation_spring` | 93.6 ns |
| `controller/curved_value` (mid-run parent) | 24.0 ns |
| `controller/status_fan_out/1` | 315 ns |
| `controller/status_fan_out/4` | 355 ns |
| `controller/status_fan_out/8` | 531 ns |
| `controller/forward` | 206 ns |

A counting allocator in the standalone `tick_allocation` target measures
10,000 advancing frames on one presentation clock. The registry holds a live
scalar controller, four-component insets and a seven-component derived value
containing geometry, color and a scalar, each with four value listeners.
Reading both value observers and evaluating a scalar cubic keyframe track are
included: the measured window makes zero allocating calls and allocates zero
bytes. Progress and listener-delivery assertions prevent an idle workload from
satisfying this claim; starting another run provides the allocator's negative
control. Relay depths of 1, 5 and 32 also allocate nothing while delivering
each measured frame.

Admission, retargeting and teardown are outside this steady-state measurement.
It does not promise allocation-free user converters or listener callbacks.
The notifier's listener snapshot holds four callbacks inline, so a fifth value
listener spills it to the heap on every notification.

### Curves, tweens and simulations

Same kind of host, loaded by concurrent builds, so these are slower than an
idle machine would show.

| Bench | Median |
|-------|-------:|
| `tween_transform/f64` | 1.42 ns |
| `tween_transform/offset` | 3.95 ns |
| `curve_eval/linear` | 1.09 ns |
| `curve_eval/elastic_out` | 24.6 ns |
| `curve_eval/ease_in_out` | 31.0 ns |
| `curve_eval/three_point_cubic_emphasized` | 84.8 ns |
| `spring/simulation_x_dx` | 54.7 ns |

The cubic-curve solve (`EaseInOut` and every other `Cubic`) inverts the
bezier x-coordinate to find the parameter, using Newton-Raphson with a
bisection fallback, the same broad inversion structure as
[WebKit UnitBezier](https://github.com/WebKit/WebKit/blob/029da7d3d074a75a24c2413e98b183a8bbfcfb62/Source/WebCore/platform/graphics/UnitBezier.h).
FLUI terminates using an output-error bound; WebKit uses an x residual.
The timings above do not measure iteration counts. Colour interpolation now uses premultiplied
Oklab throughout (ADR-0149). Earlier sRGB and separate Oklab-tween timings
are omitted because those APIs and the colour-spring representation changed;
run the current colour cases in `animation_bench` to measure the new paths.

## Vsync registry indexing (#1060)

`Vsync::tick_all` resolves each registration through a `BTreeMap` keyed by
registration id instead of a linear `Vec` scan; see `vsync.rs`'s `tick_all`
doc for the cursor-walk design. Measured with the committed Criterion bench
(`benches/vsync_registry.rs`); run `cargo bench -p flui-animation --bench
vsync_registry` to reproduce. The `unregister_all` rows below are from the
bench's current shape, which keeps one extra clone of every controller alive
per batch so a removal's `Rc` drop only decrements a refcount instead of
deallocating a whole `AnimationController` — both the "before" and "after"
`unregister_all` rows were measured under that shape so they compare like
for like. Repeat these measurements before assessing the current implementation.

Host: 13th Gen Intel Core i9-13900K, rustc 1.98.1 (48a229cea 2026-09-01),
Linux x86_64 — not CPU-isolated, so treat these as a distribution and a
regression baseline, not a hardware promise.

Acceptance is the scaling ratio between the two tables below, not either
table's wall time.

### Before: linear `Vec` scan (`iter_mut().find`)

| Bench | N | Criterion estimate (low / median / high) |
|-------|---:|---|
| `stopped_vsync_registry` | 100 | 3.1351 / 3.1489 / 3.1636 µs |
| `stopped_vsync_registry` | 1,000 | 115.93 / 115.96 / 116.00 µs |
| `stopped_vsync_registry` | 5,000 | 2.5790 / 2.5802 / 2.5822 ms |
| `stopped_vsync_registry` | 10,000 | 10.021 / 10.036 / 10.055 ms |
| `running_vsync_registry` | 100 | 6.4585 / 6.6842 / 6.8430 µs |
| `running_vsync_registry` | 1,000 | 171.39 / 204.23 / 261.10 µs |
| `mixed_vsync_registry` | 1,000 (10% running) | 123.48 / 125.84 / 129.69 µs |
| `unregister_all` | 1,000 | 298.07 / 298.27 / 298.47 µs |
| `unregister_all` | 10,000 | 31.867 / 32.497 / 33.196 ms |

### After: `BTreeMap`, cursor walk over `range_mut(cursor..fence)`

| Bench | N | Criterion estimate (low / median / high) |
|-------|---:|---|
| `stopped_vsync_registry` | 100 | 2.9094 / 2.9253 / 2.9363 µs |
| `stopped_vsync_registry` | 1,000 | 34.483 / 34.720 / 34.854 µs |
| `stopped_vsync_registry` | 5,000 | 264.20 / 265.13 / 265.88 µs |
| `stopped_vsync_registry` | 10,000 | 541.13 / 542.65 / 543.93 µs |
| `running_vsync_registry` | 100 | 6.3262 / 6.3521 / 6.4244 µs |
| `running_vsync_registry` | 1,000 | 69.581 / 69.773 / 69.948 µs |
| `mixed_vsync_registry` | 1,000 (10% running) | 37.981 / 38.077 / 38.230 µs |
| `unregister_all` | 1,000 | 30.429 / 30.793 / 31.541 µs |
| `unregister_all` | 10,000 | 332.38 / 333.95 / 335.77 µs |

### Scaling ratio, 10,000 / 1,000 (the acceptance criterion)

| Bench | Before (≈N²) | After (≈N log N) |
|-------|---:|---:|
| `stopped_vsync_registry` | ×86.6 | ×15.6 |
| `unregister_all` | ×109.0 | ×10.8 |

An N² scan scales ×100 over a 10× population growth; N log N scales
×(10,000·log₂10,000)/(1,000·log₂1,000) ≈ ×13.3. The two after-ratios above
(×15.6, ×10.8) sit roughly ×6–9 below the ×100 quadratic scale and within
~20% of the ×13.3 N log N estimate, consistent with the indexed registry
rather than the quadratic scan it replaced. `has_running` is unaffected by
this change and stays O(N); it is not part of either table.

### Where the remaining per-controller cost goes

A stopped controller costs ~54 ns per pump after the change (10,000 → ~540 µs).
A scratch decomposition bench on the same host attributed it as: the
`BTreeMap` seek (`range_mut(cursor..fence).next()` re-walks from the root on
every iteration, since the registry lock — and so the map borrow — is dropped
between steps) ≈ 55 %; the registry lock pair ≈ 17 %; the two controller-lock
pairs `run_generation()` + `status()` ≈ 28 %. The release assembly
(`cargo asm -p flui-animation --lib --release tick_all`) shows six inlined
`lock cmpxchg` fast-path operations per stopped iteration and no allocation on
that path (the children `Vec` clone is skipped when there are no children).
`has_running` adds ~8 ns per controller under a single registry lock, so a
frame driver that calls `has_running` and then `tick_all` every pump spends
~630 µs per 10,000 resident stopped controllers doing no animation work.

The "exceeds 1 ms per pump" point for stopped controllers moved from
N ≈ 2,900 (quadratic scan, fitted) to N ≈ 17,000–18,000 (measured directly:
836 µs at 15,000, 1.14 ms at 20,000, 1.84 ms at 30,000). An active-set index
is not worth building until a real tree is observed carrying that many
resident controllers; reading generation and status under one controller lock
instead of two was prototyped at ~11 ns per controller (~20 %) and is the
cheaper next step.

Criterion's regression/improvement annotation is not trustworthy at N = 100
with `sample_size(10)`: three back-to-back runs of an identical binary
reported −70 % / +3 % / +20 % "changes". Read the N ≥ 1,000 rows for
signal; the N = 100 row exists to show the small-registry cost is unchanged.

### After the single `walk_probe` (issue #1171)

The "cheaper next step" above is implemented: `AnimationController::walk_probe`
reads `run_generation` and `live_running` under one controller lock instead
of two, and both `has_running` and the `tick_all` walk now use it.
Re-measured with the same bench and host as above:

| Bench | N | Criterion estimate (low / median / high) |
|-------|---:|---|
| `stopped_vsync_registry` | 100 | 2.4181 / 2.4184 / 2.4195 µs |
| `stopped_vsync_registry` | 1,000 | 34.335 / 34.778 / 35.536 µs |
| `stopped_vsync_registry` | 5,000 | 235.70 / 237.14 / 239.16 µs |
| `stopped_vsync_registry` | 10,000 | 497.53 / 498.06 / 498.93 µs |

At N = 10,000 that is ~49.8 ns/controller/pump, down from the two-lock
table's ~54.3 ns (542.65 µs / 10,000) — a ~4.5 ns (~8 %) reduction, smaller
than the ~11 ns (~20 %) the standalone decomposition prototype estimated for
the two-controller-lock share alone. Three more back-to-back runs on this
same (not CPU-isolated) host put the N = 10,000 row anywhere from 497 µs to
560 µs, i.e. the ~54 ns baseline is sometimes matched or slightly exceeded by
noise alone — the isolated micro-benchmark that produced the ~11 ns estimate
did not carry the surrounding walk's own lock/branch overhead, which is most
of the noise floor here. Read this as "measurably not worse, and typically a
few percent better," not as a confirmed 20 % win; `has_running` folds the
same probe in and is otherwise unaffected (still O(N), not part of either
table).

## Ownership and size

Sizes are not listed here: they follow from the field types and change with
them. What matters for cost is what each type holds:

| Type | Holds |
|------|-------|
| `AnimationController` | two `Rc`s (state behind one `parking_lot::Mutex`, and the value notifier); `clone()` shares the controller |
| `CurvedAnimation<C>` | the curve(s) and one `Rc` of links: the parent `Rc<dyn Animation<f64>>`, a notifier, a curve-direction `Mutex` and two parent subscriptions (value and status) |
| `TweenAnimation<A>` | the parent `Rc<dyn Animation<f64>>`, the animatable, a notifier and a parent subscription |
| `ReverseAnimation` | the parent `Rc<dyn Animation<f64>>`, a notifier and a parent subscription |
| `ConstantAnimation<T>` | the value and a status; no notifier, since it never changes |
| `Cubic`, `ElasticOutCurve`, `Interval<C>` | plain `f64` parameters (plus the inner curve) |
| `Steps` | step count and jump placement |
| `Keyframes<T>` | starting value, total duration and an immutable boxed segment slice |

---

## UI ownership

### Controller borrow strategy

Controller state sits behind an owner-local `RefCell`, next to a
separately shared value notifier:

```text
AnimationController {
    inner: Rc<RefCell<AnimationControllerInner>>,
    notifier: Rc<ChangeNotifier>,
}
```

Benefits:
- Simple reasoning about state consistency
- Borrows end before user code (listeners, curves, simulations) runs

### Tick cycle

A manual sample uses `tick_at(Duration)`. A registered controller receives a
typed `FrameTick` from its presentation's `MotionClock` through `Vsync`.
Sampling releases the state borrow before invoking a curve or simulation,
then verifies run and sample identities before committing its result.
Value and status delivery runs outside the state borrow. Existing benchmark
rows must be measured again before claiming a performance improvement.

---

## Rc and dispatch

`AnimationController::clone()` is two reference-count increments. Composition
types hold their parent as `Rc<dyn Animation<f64>>`, so each `value()` call
crosses one dynamic dispatch per layer; `controller/curved_value` above is
that hop plus a cubic solve.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let shared = controller.clone(); // shares the controller, no copy
let animation: Rc<dyn Animation<f64>> = Rc::new(CurvedAnimation::new(
    Rc::new(shared),
    Curves::EaseInOut,
));
let value = animation.value(); // dynamic dispatch into the curved layer
# drop(controller);
```

---

## Listener overhead

Value and status listeners are `Rc<dyn Fn ...>` callbacks. Reusing one
callback across several animations shares its `Rc` and captures, but
`add_listener` still wraps each registration in a fresh `Rc` (plus any map
growth), so every registration costs at least one allocation:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_foundation::{Listenable, ListenerCallback};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
# let other = AnimationController::builder(Duration::from_millis(300)).build();

// The captures are shared; each registration still allocates its own wrapper
let callback: ListenerCallback = Rc::new(|| println!("changed"));
controller.add_listener(Rc::clone(&callback));
other.add_listener(callback);
# drop(controller);
# drop(other);
```

The per-frame cost of listeners is in the benchmark table:
`tick_at/1_value_1_status_listeners` against `tick_at/4_value_1_status_listeners`,
and `status_fan_out/{1,4,8}` for status transitions.

---

## Optimization Tips

### 1. Reuse Controllers

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# use flui_scheduler::UpdateScheduler;
# fn main() -> Result<(), AnimationError> {
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// Instead of creating a controller per animation, rewind and replay
controller.reset()?;
controller.forward()?;
# drop(controller);
# Ok(())
# }
```

### 2. Use Status Listeners Instead of Polling

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
// React to the transition once instead of reading status every frame
let _subscription = controller.subscribe_status(Rc::new(|status| {
    if status == AnimationStatus::Completed { /* ... */ }
}));
# drop(controller);
```

### 3. Batch Animations

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
// One presentation registry drives both owning controllers.
let vsync = flui_animation::Vsync::new();
let d = Duration::from_millis(300);
let mut ctrl1 = AnimationController::builder(d).build_on(Some(&vsync));
let mut ctrl2 = AnimationController::builder(d).build_on(Some(&vsync));
// Both receive the presentation's FrameTick through this registry.
# ctrl1.dispose();
# ctrl2.dispose();
```

### 4. Keep Listener Counts Small on Hot Controllers

Up to four value listeners notify without allocating; a fifth spills the
notifier's snapshot to the heap on every frame. Fan out from one listener
when a controller needs many observers.

### 5. Built-in Curves Are Plain Values

`Curves::EaseInOut` is `Cubic::new(0.42, 0.0, 0.58, 1.0)`: a named constant
costs exactly what the equivalent hand-written `Cubic` costs.

```rust
use flui_animation::{Cubic, Curves};

assert_eq!(Curves::EaseInOut, Cubic::new(0.42, 0.0, 0.58, 1.0));
```
