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
Localizing coalesced or predicted histories may allocate their transformed
sample collections; that fixture does not establish an allocation bound for
history-bearing events.

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
| `velocity_tracker_bench` | `VelocityTracker::estimate_at` with 20 and 3 samples and 4 repeated queries; `add_position`; selected Ios and Impulse estimates; `OneEuroFilter2D::filter` |
| `gesture_arena_bench` | `add` into an empty and a busy (4-member) arena; `sweep` of one member; add + accept with a competitor; add + close + sweep |
| `tap_detector_bench` | live tap sequences without callbacks and with primary/secondary callbacks; fresh fixtures keep setup and retirement outside measured invocation; `add_pointer` |
| `pointer_resampler_bench` | `add_event` at 60 Hz and 240 Hz; `sample` draining 60 events; `add_event` at the 100-event cap |
| `pointer_route_bench` | `InteractionLane::resolve_pointer_route`, cached-route Move invocation, and direct `HitTestResult::dispatch`, each for 1, 4 and 16 targets |

```bash
cargo bench -p flui-interaction                                 # all five
cargo bench -p flui-interaction --bench gesture_arena_bench     # one
```

The per-bench time targets in each file's module docs are goals; nothing
checks them.

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

## Recognizer ownership measurements

Measured on 2026-10-07, on the same Windows x86_64 MSVC host with Rust 1.99.0,
Criterion's optimized bench profile, six build jobs, 20 samples, one second of
warmup and two seconds of measurement. The live-contact baseline was saved at
`6f60614b8`; measurements after the recognizer migration use `6dd428949`.
These measurements precede the owned pointer-wire migration and measure the
recognizer ownership change only. The existing benchmark names and timed loops
are preserved. Tap fixture setup,
cancellation and destruction are outside the measured interval; callback
witnesses assert that the sequence actually completes.

The table reports Criterion's slope point estimates, in nanoseconds:

| Existing fixture | Before | After |
|---|---:|---:|
| Tap, no callbacks | 982.14 | 377.55 |
| Tap, primary callbacks | 628.25 | 347.41 |
| Tap, secondary callbacks | 650.77 | 361.67 |
| Tap admission | 266.78 | 235.46 |
| Arena add into empty arena, including prior sweep | 231.74 | 79.34 |
| Arena add into four-member arena, including rejection | 179.32 | 46.33 |
| Arena add and single-member sweep | 236.65 | 73.01 |
| Arena construction, two admissions and eager conflict | 4163.90 | 347.69 |
| Arena construction, admission, close and sweep | 985.37 | 320.79 |

Dynamic tap delivery through `RecognizerSet` measured 363.71 ns without
callbacks, 352.80 ns with primary callbacks and 395.69 ns with secondary
callbacks. These new rows have no historical baseline: the old recognizer
trait was not dyn-compatible. Both static and dynamic fixtures deliver the
same three events and assert the same callback and settlement witnesses.

Isolated `resolve/weak` measured 93.91 ns (95% slope interval 88.80–100.90 ns).
Its two strong fixture owners and closed arena are prepared outside timing;
the witness asserts one acceptance, one rejection and an empty arena. The
older eager-conflict row includes construction and admission, so its timing
cannot serve as an isolated strong-resolution baseline. No isolated
strong-resolution measurement was saved.

Reproduce the existing-row comparison with the saved `before` data:

```bash
cargo bench -p flui-interaction --bench tap_detector_bench -- 'handle_event/static|add_pointer' --baseline before --sample-size 20 --warm-up-time 1 --measurement-time 2
cargo bench -p flui-interaction --bench gesture_arena_bench -- GestureArena --baseline before --sample-size 20 --warm-up-time 1 --measurement-time 2
```

Run `handle_event/dyn` and `resolve/weak` separately with `--save-baseline after`.
The host was shared, confidence intervals vary by row, and these observations
do not establish a portable percentage speedup or a CI performance threshold.

## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [GESTURES.md](GESTURES.md)
- [HIT_TESTING.md](HIT_TESTING.md)
