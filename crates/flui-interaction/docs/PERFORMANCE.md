# Performance Guide

Cost bounds, benchmark fixtures and measurements for `flui_interaction`.
Microbenchmark timings describe the measured fixtures, not a frame-time guarantee.

## Pointer path

`GestureBinding` handles one pointer sequence like this:

1. Down: hit test, resolve the route through `InteractionLane`, call
   `add_pointer` on each recognizer along it, close the arena.
2. Move: optionally queued in a per-pointer `PointerEventResampler` (when
   `set_resampling_enabled(true)`), then delivered along the cached route;
   each recognizer's `handle_event` runs and may resolve its arena entry.
3. Up / Cancel: delivered along the cached route; Up sweeps the arena, then the
   route is released.

Hardware-predicted samples travel with the owned pointer event. The binding
does not synthesize future samples through a separate extrapolator.

The measured cached-route Move fixture without sample history performs no heap
allocation after setup; the
counting-allocator test `resolved_route_move_invocation_allocates_no_heap_after_setup`
(`tests/pointer_route_hot_path.rs`) asserts it.
The same public counting-allocator matrix covers measured and predicted
histories at 1, 4 and 16 cached targets. On the Windows x86_64 host it measured:

| Cached targets | Scalar Move, global or translated | Move with both histories, global | Move with both histories, translated |
|---|---:|---:|---:|
| 1 | 0 | 0 | 2 |
| 4 | 0 | 0 | 8 |
| 16 | 0 | 0 | 32 |

Setup and route resolution are excluded. Each history-bearing packet contains
two measured historical samples and one predicted sample. The test permits
at most two allocations per translated target, one for each nonempty owned
localized history, while asserting every sample field, metadata and unchanged
global readings. The previous implementation cloned both source histories
before replacing them with localized histories; its one-target case allocated
four times. Localization now constructs the required owned histories directly
from the borrowed source. These counts are allocation costs, not elapsed times.

The root hit-test path produced by `HitTestResult::add` carries a composed
identity matrix rather than an absent transform. Exact identity also borrows
the original pointer event, including both histories, without local sample
allocation. The allocation matrix covers that producer alongside translated
paths and a small nonzero translation. Approximate identity is not sufficient:
even a small authored displacement must localize every reading. Nonidentity
paths retain checked plane projection and their required owned histories.

## Bounds

| Component | Bound | Source |
|---|---|---|
| `VelocityTracker` | 20-slot ring buffer (`lsq_solver::MAX_SAMPLES`), 100 ms horizon, at least 3 samples (`MIN_SAMPLE_SIZE`) for a fit, zero after 40 ms without movement; quadratic least-squares fit, O(n) for n ≤ 20; the fit is memoized until the next `add_position` / `reset` | `processing/velocity.rs` |
| `PointerEventResampler` | soft cap of 100 queued events (`MAX_BUFFERED_EVENTS`): adjacent moves fold, then the oldest non-boundary event is dropped; Down, Up, Cancel, Enter and Leave are preserved, so an all-boundary queue may exceed the cap; 1 ms minimum sample interval, 38 ms default lookback; positions are interpolated linearly between queued events | `processing/resampler.rs` |
| Arena entry | weak members in `SmallVec<[Weak<dyn GestureArenaMember>; 4]>`, inline up to four; verdicts upgrade each live participant at invocation | `arena/mod.rs` (`ArenaEntryData`) |
| Arena storage | owner-local `Rc<RefCell<BTreeMap<PointerId, Rc<ArenaSlot>>>>`, retained generation map and deferred-resolution queue; slot state is `RefCell` | `arena/mod.rs` |

The resampler keeps its queue behind `Arc<parking_lot::Mutex<_>>`. `sample`
builds the output batch under the lock and invokes the callback after
releasing it, so the callback may re-enter input processing.

There is no wall-clock force-resolution of the arena. A lone member's default
win is queued on `close` and applied by `drain_deferred_resolutions` at the
event or frame boundary.

## Benchmarks

Five Criterion benches in `benches/` (`harness = false`, stable toolchain).
Each needs the `testing` feature, which the dev-dependency enables.

| Bench | Cases |
|---|---|
| `velocity_tracker_bench` | `VelocityTracker::estimate_at` with 20 and 3 samples and 4 repeated queries; construction plus 20 `add_position` calls; selected Ios and Impulse estimates; standalone `OneEuroFilter2D::filter` |
| `gesture_arena_bench` | `add` into an empty and a busy (4-member) arena; `sweep` of one member; add + accept with a competitor; add + close + sweep |
| `tap_detector_bench` | live tap sequences without callbacks and with primary/secondary callbacks; fresh fixtures keep setup and retirement outside measured invocation; `add_pointer` |
| `pointer_resampler_bench` | owned source-time admission; complete 60/240 Hz source traces sampled at 60 Hz with Up/Cancel flush; measured plus interpolated sample delivery; separate Up/Cancel tail flush; overflow with scalar and saturated history |
| `pointer_route_bench` | `InteractionLane::resolve_pointer_route` plus route release, scalar cached-route Move invocation, and direct scalar Down `HitTestResult::dispatch`, each for 1, 4 and 16 targets |

```bash
cargo bench -p flui-interaction                                 # all five
cargo bench -p flui-interaction --bench gesture_arena_bench     # one
```

The per-bench time targets in each file's module docs are goals; nothing
checks them.

Velocity estimation fixtures independently check the known linear velocity
before timing: 1000 px/s for the 20-sample LSQ, Ios and Impulse cases, and
500 px/s for the 3-sample LSQ case. Ios and Impulse use `PerIteration` batching
to match the historical fixtures' immediate argument-free queries; LSQ uses
`SmallInput`. Compare each row before and after with its matching batching.
Cross-estimator timings include these batching differences. The row named
`add_position (push)` measures construction plus 20 pushes and a sample-count
observation, so its result is not an individual push latency.

Resampler traces independently witness a 1000 px/s source trajectory,
monotonic timestamps, valid timestamp zero, delivered readings and exactly one
terminal. Setup and fixture retirement stay outside the isolated sample,
stop and overflow timings. A frame-trace timing includes admission, sampling,
callback consumption and terminal flush for the entire one-second workload;
it is not a per-event number. The overflow fixture explicitly checks bounded
retained history rather than claiming lossless delivery beyond its cap.

The route timing bench uses scalar events without sample history. The
history-bearing localization allocation matrix above is a separate public
test, so scalar route timings do not price that workload.
Direct dispatch resolves and releases its route inside each measured iteration;
cached invocation excludes that setup and retirement. Its root identity entries
borrow source readings, while transformed entries still validate and localize
their samples. Neither timing includes destruction of the original hit path.

For an API or ownership change, compare saved baselines on the same host and
toolchain. Keep the fixture lifetime and callback witnesses identical:

```bash
cargo bench -p flui-interaction --bench tap_detector_bench -- --save-baseline before
cargo bench -p flui-interaction --bench gesture_arena_bench -- --save-baseline before
# After the change, using the same checkout's saved Criterion data:
cargo bench -p flui-interaction --bench tap_detector_bench -- --baseline before
cargo bench -p flui-interaction --bench gesture_arena_bench -- --baseline before
```

A new dynamic-dispatch case has no historical measurement when the old trait
was not dyn-compatible. Compare static dispatch before/after, then dynamic
against static dispatch on the new implementation. Weak-member resolution
must keep a strong fixture owner alive; measuring dead weak references would
exercise withdrawal instead of arbitration.

## Timing measurement scope

The earlier recognizer-ownership timings used the pointer representation from
before the owned input migration. They are not measurements of these final
fixtures and are omitted here. No elapsed-time or portable speedup claim is
made for the current owned input path until the complete fixtures are measured.

Run final timings sequentially on the same host and pinned toolchain, with
six build jobs and no concurrent benchmark or gate. Retain Criterion's point
estimate and confidence interval, source revision, host and fixture identity:

```bash
cargo bench --locked -p flui-interaction --bench tap_detector_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2
cargo bench --locked -p flui-interaction --bench gesture_arena_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2
cargo bench --locked -p flui-interaction --bench velocity_tracker_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2
cargo bench --locked -p flui-interaction --bench pointer_resampler_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2
cargo bench --locked -p flui-interaction --bench pointer_route_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2
```

Tap static and dynamic delivery complete the same live three-event sequence.
Strong fixture owners survive weak arena arbitration. The isolated
`resolve/weak` case prepares its two owners and closed arena outside timing and
witnesses one acceptance, one rejection and settlement; construction-inclusive
arena rows are different workloads. Compare saved baselines only when their
fixture semantics and lifetime boundaries agree. A host-shared observation
does not establish a CI threshold or a portable percentage speedup.

## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [GESTURES.md](GESTURES.md)
- [HIT_TESTING.md](HIT_TESTING.md)
