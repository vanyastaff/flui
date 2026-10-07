# Performance

Performance characteristics of `flui_animation`.

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup.

## Measured benchmarks

The benchmark tables below record measurements from the committed Criterion
targets `benches/animation_bench.rs` and `benches/vsync_registry.rs`. The later
per-controller analysis also records historical scratch measurements and a
standalone prototype; those experiments are not committed, and the command
below does not reproduce their attribution percentages or the 15,000–30,000
controller samples. Absolute numbers are machine-relative and both hosts below
were shared with other builds, so read them as orders of magnitude and as a
regression baseline, not as a hardware promise. Run the committed targets with:

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
| `spring/animated_value_color_frame` | 100.5 ns |
| `smoothing/smooth_damp_step` | 7.6 ns |

A steady-state frame (`Vsync::tick_all` on a running controller with four
value listeners and one status listener) performs no heap allocation; the
`tick_allocation` test target pins that with a counting allocator. The
notifier's listener snapshot holds four callbacks inline, so a fifth value
listener spills it to the heap on every notification.

### Curves, tweens and simulations

Same kind of host, loaded by concurrent builds, so these are slower than an
idle machine would show.

| Bench | Median |
|-------|-------:|
| `tween_transform/f64` | 1.42 ns |
| `tween_transform/offset` | 3.95 ns |
| `tween_transform/color` | 21.6 ns |
| `tween_transform/color_oklab` | 288 ns |
| `curve_eval/linear` | 1.09 ns |
| `curve_eval/elastic_out` | 24.6 ns |
| `curve_eval/ease_in_out` | 31.0 ns |
| `curve_eval/three_point_cubic_emphasized` | 84.8 ns |
| `smoothing/exp_decay_half_life` | 10.9 ns |
| `spring/simulation_x_dx` | 54.7 ns |

The cubic-curve solve (`EaseInOut` and every other `Cubic`) inverts the
bezier x-coordinate to find the parameter, using Newton-Raphson with a
bisection fallback (the WebKit `UnitBezier` solver), which converges in 2–4
iterations on the common path. Oklab interpolation converts both endpoints
to Oklab and the result back to sRGB on every call, which is why it costs an
order of magnitude more than componentwise sRGB.

## Vsync registry indexing (#1060)

`Vsync::tick_all` resolves each registration through a `BTreeMap` keyed by
registration id instead of a linear `Vec` scan; see `vsync.rs`'s `tick_all`
doc for the cursor-walk design. Measured with the committed Criterion bench
(`benches/vsync_registry.rs`); run `cargo bench -p flui-animation --bench
vsync_registry` to reproduce. The `unregister_all` rows below are from the
bench's current shape, which keeps one extra clone of every controller alive
per batch so a removal's `Arc` drop only decrements a refcount instead of
deallocating a whole `AnimationController` — both the "before" and "after"
`unregister_all` rows were re-measured under that shape so they compare like
for like.

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
| `AnimationController` | two `Arc`s (state behind one `parking_lot::Mutex`, and the value notifier); `clone()` shares the controller |
| `CurvedAnimation<C>` | the curve(s) and one `Arc` of links: the parent `Arc<dyn Animation<f64>>`, a notifier, a curve-direction `Mutex` and two parent subscriptions (value and status) |
| `TweenAnimation<T, A>` | the parent `Arc<dyn Animation<f64>>`, the animatable, a notifier and a parent subscription |
| `ReverseAnimation` | the parent `Arc<dyn Animation<f64>>`, a notifier and a parent subscription |
| `CompoundAnimation` | two parent `Arc<dyn Animation<f64>>`s, a notifier and two parent subscriptions |
| `ConstantAnimation<T>` | the value and a status; no notifier, since it never changes |
| `Cubic`, `ElasticOutCurve`, `Interval<C>` | plain `f64` parameters (plus the inner curve) |
| `CatmullRomCurve` | a `SmallVec` of points, eight inline |

---

## Synchronization

### Controller lock strategy

All controller state sits behind a single `parking_lot::Mutex`, next to a
separately shared value notifier:

```text
AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}
```

Benefits:
- Simple reasoning about state consistency
- Batched updates in single lock acquisition
- Lock released before user code (listeners, curves, simulations) runs

### Tick cycle

A frame is driven by `tick_at(raw_elapsed_secs)`, an absolute time since the
run started. The controller samples the active run's source (curve or
simulation) outside its lock, commits the new value and status under the
lock, releases it, and only then notifies value listeners and, on a status
change, status listeners. The `tick_at` rows of the benchmark table include
that lock traffic.

---

## Arc and dispatch

`AnimationController::clone()` is two reference-count increments. Composition
types hold their parent as `Arc<dyn Animation<f64>>`, so each `value()` call
crosses one dynamic dispatch per layer; `controller/curved_value` above is
that hop plus a cubic solve.

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, Curves};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
let shared = controller.clone(); // shares the controller, no copy
let animation: Arc<dyn Animation<f64>> = Arc::new(CurvedAnimation::new(
    Arc::new(shared),
    Curves::EaseInOut,
));
let value = animation.value(); // dynamic dispatch into the curved layer
# controller.dispose();
```

---

## Listener overhead

Value and status listeners are `Arc<dyn Fn ...>` callbacks. Reusing one
callback across several animations shares its `Arc` and captures, but
`add_listener` still wraps each registration in a fresh `Arc` (plus any map
growth), so every registration costs at least one allocation:

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
use flui_foundation::{Listenable, ListenerCallback};
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
# let other = AnimationController::new(Duration::from_millis(300), &scheduler);

// The captures are shared; each registration still allocates its own wrapper
let callback: ListenerCallback = Arc::new(|| println!("changed"));
controller.add_listener(Arc::clone(&callback));
other.add_listener(callback);
# controller.dispose();
# other.dispose();
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
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
// Instead of creating a controller per animation, rewind and replay
controller.reset()?;
controller.forward()?;
# controller.dispose();
# Ok(())
# }
```

### 2. Use Status Listeners Instead of Polling

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# use flui_scheduler::UpdateScheduler;
# let scheduler = UpdateScheduler::new();
# let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
// React to the transition once instead of reading status every frame
controller.add_status_listener(Arc::new(|status| {
    if status == AnimationStatus::Completed { /* ... */ }
}));
# controller.dispose();
```

### 3. Batch Animations

```rust
# use std::sync::Arc;
# use std::time::Duration;
# use flui_animation::AnimationController;
# use flui_scheduler::UpdateScheduler;
// Good: single scheduler drives all
let scheduler = Arc::new(UpdateScheduler::new());
let d = Duration::from_millis(300);
let ctrl1 = AnimationController::new(d, &scheduler);
let ctrl2 = AnimationController::new(d, &scheduler);
// Both tick on same frame callback
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
