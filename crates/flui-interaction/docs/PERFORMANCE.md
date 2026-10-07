# Performance Guide

Cost bounds and the benchmarks for `flui_interaction`. No benchmark results are
recorded in the repository, so this document states bounds and how to measure,
not timings.

## Pointer path

`GestureBinding` handles one pointer sequence like this:

1. Down: hit test, resolve the route through `InteractionLane`, call
   `add_pointer` on each recognizer along it, close the arena.
2. Move: optionally queued in a per-pointer `PointerEventResampler` (when
   `set_resampling_enabled(true)`), then delivered along the cached route;
   each recognizer's `handle_event` runs and may resolve its arena entry.
3. Up / Cancel: delivered along the cached route; Up sweeps the arena, then the
   route is released.

`InputPredictor` is not part of this path: the binding never calls it. It is a
standalone helper for callers that want extrapolated positions.

Cached-route Move delivery performs no heap allocation after setup; the
counting-allocator test `resolved_route_move_invocation_allocates_no_heap_after_setup`
(`tests/pointer_route_hot_path.rs`) asserts it.

## Bounds

| Component | Bound | Source |
|---|---|---|
| `VelocityTracker` | 20-slot ring buffer (`lsq_solver::MAX_SAMPLES`), 100 ms horizon, at least 3 samples (`MIN_SAMPLE_SIZE`) for a fit, zero after 40 ms without movement; quadratic least-squares fit, O(n) for n ≤ 20; the fit is memoized until the next `add_position` / `reset` | `processing/velocity.rs` |
| `PointerEventResampler` | 100 queued events (`MAX_BUFFERED_EVENTS`); past it, adjacent moves fold, then the oldest event that is not Down, Up, Cancel, Enter or Leave is dropped, so only those boundaries can exceed it; 1 ms minimum sample interval, 38 ms default lookback; positions are interpolated linearly between queued events | `processing/resampler.rs` |
| `InputPredictor` | prediction capped at 25 ms (default 16 ms), at least 3 samples; linear `pos + v·dt`, optional `+ ½·a·dt²`, optional exponential smoothing | `processing/prediction.rs` |
| Arena entry | members in `SmallVec<[Arc<dyn GestureArenaMember>; 4]>`, inline up to four | `arena/mod.rs` (`ArenaEntryData`) |
| Arena storage | `DashMap<PointerId, Arc<ArenaSlot>>` plus a per-slot `parking_lot::Mutex`; the arena is still `!Send + !Sync` | `arena/mod.rs` |

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
| `velocity_tracker_bench` | `VelocityTracker::estimate` with 20 and 3 samples and 4 repeated queries; `add_position`; `IosFlingVelocityTracker` and `ImpulseVelocityTracker` estimates; `OneEuroFilter2D::filter` |
| `gesture_arena_bench` | `add` into an empty and a busy (4-member) arena; `sweep` of one member; add + accept with a competitor; add + close + sweep |
| `tap_detector_bench` | `TapGestureRecognizer::handle_event` without and with callbacks, on the primary-button path; `add_pointer` |
| `pointer_resampler_bench` | `add_event` at 60 Hz and 240 Hz; `sample` draining 60 events; `add_event` at the 100-event cap |
| `pointer_route_bench` | `InteractionLane::resolve_pointer_route`, cached-route Move invocation, and direct `HitTestResult::dispatch`, each for 1, 4 and 16 targets |

```bash
cargo bench -p flui-interaction                                 # all five
cargo bench -p flui-interaction --bench gesture_arena_bench     # one
```

The per-bench time targets in each file's module docs are goals; nothing
checks them.

## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [GESTURES.md](GESTURES.md)
- [HIT_TESTING.md](HIT_TESTING.md)
