# Performance

The cost rules of `flui_animation`, and what its benchmarks and tests check. Timings are
machine-relative and are not recorded here: measure on your own host (see
[Benchmarks](#benchmarks)).

Every `rust` block is compiled as a doctest against the current API. Lines
starting with `#` are hidden setup.

## Rules

1. **A steady-state frame allocates nothing.** Advancing a registry that holds running
   controllers, `AnimatedValue` owners, observer reads, keyframe sampling and listener relays makes
   no allocating call. Admission, retargeting, teardown, user converters and user listener
   bodies are outside that promise.
2. **Retiring idle registry owners allocates nothing.** A registry's cleanup custody has a known
   type and is queued directly, not boxed per owner. Retiring owners that have an active run, a
   callback or an arbitrary property capture can allocate.
3. **Per-tick cost is per resident controller.** `VsyncDriver::tick_all` visits every resident
   controller once per frame, running or stopped, so a registry of N controllers costs O(N) per
   frame plus the sampling of the running ones. Each step seeks the next registration in an
   ordered map, so a stopped controller costs a cursor step and one controller-state read, not a
   sample. Keep long-lived controllers that are not animating unregistered or disposed when a
   registry holds thousands.
4. **Idle frames are free of continuous demand.** `Vsync::has_running` is the frame driver's
   test for "request another frame". A scan that finds nothing running is remembered until
   something can start a run (a run's frame request, an admission, an attach, an unmute or a
   tick), and a scan that finds a run stops at the first. A parked Normal repeat under Reduce
   contributes no continuous demand; a Preserve repeat does. A pending settlement or a Full
   resumption can need a single frame while continuous demand is false.
5. **Nesting is cheap to walk.** A nested registry validates its ancestors once per hierarchy
   revision and compares one revision number between samples afterwards. Attaching, detaching,
   muting or closing advances the revision, so the next walk after one of them validates the
   ancestor chain in full.
6. **A value is one registration.** An `AnimatedValue` owns one controller for all its
   components, and tracks that share one `total` read one controller as a group. Retargeting one
   property owner does not restart the others.
7. **Registration and removal are not per-frame work.** Register when the owner is created
   and keep it for the widget's lifetime; a listener registration can allocate, and removal
   retires through the controller's delivery queue.
8. **Listeners.** A notification snapshots its listeners; up to four are held inline, so a
   fifth value listener on one controller spills the snapshot to the heap on every notification.
   Fan out from one listener when a controller needs many observers.
9. **Combinators cost one dynamic call per layer.** A `CurvedAnimation` or `TweenAnimation`
   holds its parent as `Rc<dyn Animation<f64>>`; each `value()` crosses one virtual call per
   layer plus the curve or mapping.
10. **Curves and tweens are plain values.** A named preset costs exactly what the equivalent
    hand-written curve costs. A cubic solve inverts the Bézier x-coordinate with Newton steps
    and a bisection fallback. Color interpolation converts through premultiplied Oklab on every
    sample (ADR-0149, ADR-0194), so it costs more than a numeric tween.
11. **Simulations pay at construction.** A spring or friction simulation computes its rest time
    when built; sampling afterwards is closed-form. Build one per run, not per frame.

## Ownership and size

Sizes follow from the field types and are not listed. What matters for cost is what each type
holds:

| Type | Holds |
|------|-------|
| `AnimationController` | `Rc`s to its owner-local state (a `RefCell`), its value notifier and its debug origin; `clone()` shares all of them |
| `CurvedAnimation<C>` | the curve (owned by each clone), a shared `Rc<Cell<Option<AnimationStatus>>>` for the entering direction, and shared parent links |
| `TweenAnimation<A>` | the mapping (owned by each clone) and shared parent links |
| `ReverseAnimation` | shared parent links |
| `ConstantAnimation<T>` | the value and a status; no notifier, since it never changes |
| `Cubic`, `Elastic`, `Interval<C>` | plain `f64` parameters (plus the inner curve) |
| `Steps` | step count and jump placement |
| `Keyframes<T>` | starting value, total duration and an immutable boxed segment slice |

The shared parent links hold the parent, the value and status notifiers and one subscription to
each parent channel; clones share them instead of installing another subscription pair. These are
owner-local capabilities (ADR-0175), so no animation has a per-object lock.

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, CurvedAnimation, curves};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let shared = controller.clone(); // shares the controller, no copy
let animation: Rc<dyn Animation<f64>> = Rc::new(CurvedAnimation::new(
    Rc::new(shared),
    curves::EASE_IN_OUT,
));
let value = animation.value(); // dynamic dispatch into the curved layer
# drop(controller);
```

## Practices

Reuse a controller by rewinding it instead of creating one per animation:

```rust
# use std::time::Duration;
# use flui_animation::{AnimationController, AnimationError};
# fn main() -> Result<(), AnimationError> {
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
controller.reset()?;
controller.forward()?;
# drop(controller);
# Ok(())
# }
```

React to a transition with a status listener instead of reading the status every frame:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{Animation, AnimationController, AnimationStatus};
# let controller = AnimationController::builder(Duration::from_millis(300)).build();
let _subscription = controller.listen_status(Rc::new(|status| {
    if status == AnimationStatus::ForwardSettled { /* ... */ }
}));
# drop(controller);
```

Drive many owners from one presentation registry, and share a callback's captures by cloning its
`Rc`:

```rust
# use std::rc::Rc;
# use std::time::Duration;
# use flui_animation::{AnimationController, Vsync};
use flui_foundation::{Listenable, ListenerCallback};
let vsync = Vsync::new();
let d = Duration::from_millis(300);
let mut first = AnimationController::builder(d).build_on(Some(&vsync));
let mut second = AnimationController::builder(d).build_on(Some(&vsync));

let callback: ListenerCallback = Rc::new(|| println!("changed"));
let _first = first.controller().listen(Rc::clone(&callback));
let _second = second.controller().listen(callback);
# first.dispose();
# second.dispose();
```

A built-in preset is the same value as its hand-written curve:

```rust
use flui_animation::{Cubic, curves};

assert_eq!(curves::EASE_IN_OUT, Cubic::new(0.42, 0.0, 0.58, 1.0));
```

## Benchmarks

Two Criterion targets live in `benches/`, both with `harness = false`:

| Target | Measures |
|--------|----------|
| `animation_bench` | `tween_transform` (f64, offset, color), `curve_eval` (linear, ease-in-out, elastic, three-point cubic), `spring` (`simulation_x_dx`, `simulation_new`, `animated_value_color_frame`), `controller` (`tick_at` for linear, eased, with listeners, friction and spring runs; `curved_value`; `status_fan_out`; `forward`) |
| `vsync_registry` | `tick_all` over 100 to 10,000 resident controllers, stopped (`stopped_vsync_registry`), running (`running_vsync_registry`), nested at depths 1, 8 and 32 (`nested_vsync_registry`) and mixed (`mixed_vsync_registry`), and `unregister_all` |

Every controller bench measures a controller that is genuinely mid-run: a run too long to finish
or a simulation that cannot settle in the measured span. A finished run would make `tick_at` an
early return and price a branch instead of a frame. The registry benches advance one retained
`MotionClock` by a 60 Hz step per iteration and check, outside the timed region, that the active
controllers progressed and are still running; active results include sampling and delivery, not
only registry lookup.

Check that the workloads still hold their own assertions, without collecting timings:

```bash
cargo bench --profile dev -p flui-animation --bench vsync_registry -- --test
```

Measure a change on your host, on a quiet machine, with the optimized bench profile and the same
sampling settings before and after:

```bash
cargo bench --locked -p flui-animation --bench vsync_registry -- --noplot 'running_vsync_registry/10000|nested_vsync_registry/1000'
```

For an A/B comparison use `cargo xtask bench-collect <name>` on each commit and `critcmp`
([testing](../../../docs/testing.md)). Same-version runs on a shared host vary by several
percent, and Criterion's change annotation is not reliable at small sample sizes or at the
100-controller rows; read the rows with 1,000 or more controllers for a signal.

No animation benchmark is a CI gate: CI compiles them with the workspace's targets and does not
collect timings.

## What the tests hold

| Test | Holds |
|------|-------|
| `a_steady_state_frame_allocates_nothing` (`tests/tick_allocation.rs`) | rule 1: 10,000 advancing frames of a scalar controller, four-component and nested geometry and color owners, each with four value listeners, plus keyframe and authored-easing sampling, a warmed root/child/grandchild registry and listener relays of depth 1, 5 and 32, with a positive allocation control |
| `retiring_idle_registry_owners_allocates_nothing` (same file) | rule 2: 1, 1,000 and 10,000 owners, with admission as the positive control |

`tests/tick_allocation.rs` is a target of its own because its counting `#[global_allocator]` is
process-wide; its counters are per thread, so concurrent tests cannot add to a measured window.
Both run with the crate's suite on every pull request. The widget-level frame and allocation
budgets of mounted animated widgets belong to `flui-widgets` and `flui-material`
and are described in [testing](../../../docs/testing.md).
