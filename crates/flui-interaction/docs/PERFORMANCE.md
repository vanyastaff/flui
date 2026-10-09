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
Transform classification also borrows its optional matrix until admission.
Passing that large optional payload by value can materialize a matrix copy
before checking an absent transform; only an admitted nonidentity route needs
to own the matrix. This ownership choice does not remove any geometry checks.

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

Six Criterion benches in `benches/` (`harness = false`, stable toolchain).
Each needs the `testing` feature, which the dev-dependency enables.

| Bench | Cases |
|---|---|
| `velocity_tracker_bench` | `VelocityTracker::estimate_at` with 20 and 3 samples and 4 repeated queries; construction plus 20 `add_position` calls; selected Ios and Impulse estimates; standalone `OneEuroFilter2D::filter` |
| `gesture_arena_bench` | `add` into an empty and a busy (4-member) arena; `sweep` of one member; add + accept with a competitor; add + close + sweep |
| `tap_detector_bench` | live tap sequences without callbacks and with primary/secondary callbacks; fresh fixtures keep setup and retirement outside measured invocation; `add_pointer` |
| `pointer_resampler_bench` | owned source-time admission; complete 60/240 Hz source traces sampled at 60 Hz with Up/Cancel flush; measured plus interpolated sample delivery; separate Up/Cancel tail flush; overflow with scalar and saturated history |
| `pointer_route_bench` | `InteractionLane::resolve_pointer_route` plus route release, scalar cached-route Move invocation, and direct scalar Down `HitTestResult::dispatch`, each for 1, 4 and 16 targets |
| `interaction_delivery_bench` | registered `PointerRouter` Move with 1/4 callbacks; two physical or ambient cursor changes; two attached focus transitions |

```bash
cargo bench -p flui-interaction                                 # all six
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
`add_position (push)` measures construction plus 20 pushes, a `black_box`
observation of the full tracker, and its sample count. Its result is not an
individual push latency.

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

## Complete pinned measurement

The matched measurement on 2026-10-08 used Windows 11 Pro 10.0.26200,
an Intel Core i9-13900K and rustc 1.99.0 (b940084d7). Each benchmark process
ran sequentially at normal priority, pinned to logical processor 0 (affinity
mask 1). Builds used six jobs, incremental compilation disabled, development
debug information disabled, and the default release profile. Criterion used
20 samples, a 1 s warmup, a 2 s measurement and no plots. All ten BEFORE/AFTER
executables completed successfully. These are host-specific microbenchmarks,
not a frame-time guarantee or CI performance threshold.

BEFORE production is `6f60614b8f3764fb393de6c60f59e6f8ca67eca3`, with
benchmark-only adapters at `ee5894f1493d270cb8d64401dc2c991dc1d533da`.
AFTER production is `3d6fae0deaba653b6cb4d6185981dfd97412c59e`.
Saved Criterion names are `interaction_pinned_before` and
`interaction_pinned_after`. Read `mean.point_estimate` and the mean's
confidence interval from each case's `estimates.json`, identified by
`benchmark.json`'s `full_id`, under the respective checkout's
`target/criterion/`. The table reports arithmetic means in nanoseconds with
95% confidence intervals; negative changes mean a lower elapsed time.

The adapters keep strong owners alive, callback delivery witnesses and
setup/retirement boundaries equivalent. Historical `resolve/strong` accepts
an owned candidate and includes its required `Arc` clone; current
`resolve/weak` borrows the candidate. This compares complete public call
costs, not an isolated `Rc` versus `Arc` operation. Both push fixtures observe
the full tracker before its sample count. Earlier count-only timings are
excluded because they did not make the stored sample payload observable.

| Criterion row | BEFORE ns [95% CI] | AFTER ns [95% CI] | Change |
|---|---:|---:|---:|
| `add_pointer/static` | 265.075 [238.352, 294.630] | 234.163 [218.173, 252.064] | -11.66% |
| `GestureArena::add (busy, 4 prior members)` | 164.377 [162.537, 166.417] | 47.906 [47.426, 48.427] | -70.86% |
| `GestureArena::add (empty, 1 member)` | 215.761 [212.787, 218.818] | 81.213 [80.091, 83.074] | -62.36% |
| `GestureArena::add + accept (eager vs competitor)` | 2931.462 [2907.653, 2955.112] | 363.810 [359.116, 370.192] | -87.59% |
| `GestureArena::add+close+sweep (full lifecycle)` | 953.814 [944.059, 964.574] | 336.160 [331.649, 341.604] | -64.76% |
| `GestureArena::sweep (1-member arena)` | 211.864 [209.871, 213.963] | 80.858 [80.190, 81.531] | -61.83% |
| `handle_event/static/no_callbacks` | 541.147 [518.322, 565.377] | 406.970 [399.616, 414.240] | -24.79% |
| `handle_event/static/primary_callbacks` | 542.538 [516.762, 570.910] | 362.696 [353.489, 371.837] | -33.15% |
| `handle_event/static/secondary_callbacks` | 544.770 [515.653, 580.118] | 349.896 [341.700, 359.391] | -35.77% |
| `HitTestResult::dispatch/direct/1` | 228.757 [226.012, 231.830] | 201.734 [198.002, 206.291] | -11.81% |
| `HitTestResult::dispatch/direct/16` | 1454.591 [1420.368, 1495.502] | 967.648 [957.662, 978.728] | -33.48% |
| `HitTestResult::dispatch/direct/4` | 420.737 [412.497, 431.197] | 313.319 [309.050, 317.997] | -25.53% |
| `InteractionLane::invoke_pointer_route/common_move/1` | 45.806 [44.319, 47.430] | 37.528 [36.934, 38.237] | -18.07% |
| `InteractionLane::invoke_pointer_route/common_move/16` | 151.625 [149.659, 153.987] | 157.801 [153.227, 164.654] | 4.07% |
| `InteractionLane::invoke_pointer_route/common_move/4` | 64.193 [63.174, 65.300] | 63.683 [60.475, 67.502] | -0.79% |
| `InteractionLane::resolve_pointer_route/1` | 155.726 [153.629, 158.009] | 160.657 [157.971, 163.502] | 3.17% |
| `InteractionLane::resolve_pointer_route/16` | 724.871 [709.498, 743.237] | 753.410 [747.155, 759.816] | 3.94% |
| `InteractionLane::resolve_pointer_route/4` | 243.359 [236.023, 252.444] | 245.216 [242.106, 248.702] | 0.76% |
| `OneEuroFilter2D::filter (per move)` | 19.802 [18.837, 20.869] | 20.238 [18.902, 21.789] | 2.20% |
| `resampler/add_event/source_time` | 60.250 [59.247, 61.326] | 68.816 [65.865, 71.852] | 14.22% |
| `resampler/frame_trace/240_to_60/up` | 14809.454 [14361.202, 15321.959] | 17105.050 [15925.354, 18739.919] | 15.50% |
| `resampler/frame_trace/60_to_60/up` | 5121.684 [5055.770, 5200.670] | 5816.661 [5449.997, 6196.260] | 13.57% |
| `resampler/overflow/saturated_history` | 676.232 [601.023, 744.097] | 419.892 [367.480, 479.848] | -37.91% |
| `resampler/overflow/scalar_history` | 188.707 [167.775, 208.947] | 162.373 [138.915, 187.227] | -13.96% |
| `resampler/sample/measured_and_interpolated` | 1762.125 [1666.990, 1867.619] | 2024.738 [1842.580, 2214.212] | 14.90% |
| `resampler/stop/up` | 188.169 [165.829, 210.236] | 204.880 [177.380, 233.589] | 8.88% |
| `resolve/weak` | 2262.689 [2185.514, 2337.439] | 94.680 [89.394, 101.413] | -95.82% |
| `VelocityTracker::add_position (push)` | 472.099 [459.812, 486.774] | 46.100 [41.999, 51.171] | -90.24% |
| `VelocityTracker::estimate (LSQ, 20 samples)` | 539.493 [521.181, 562.036] | 709.167 [676.914, 746.009] | 31.45% |
| `VelocityTracker::estimate (LSQ, 3 samples)` | 227.090 [219.561, 235.666] | 247.879 [237.673, 259.397] | 9.15% |
| `VelocityTracker::estimate (LSQ, 4 repeated queries)` | 675.655 [623.867, 729.476] | 598.096 [589.552, 606.882] | -11.48% |
| `VelocityTracker::estimate Impulse (20 samples)` | 562.506 [544.437, 582.147] | 496.490 [481.291, 517.766] | -11.74% |
| `VelocityTracker::estimate Ios (20 samples)` | 124.102 [120.630, 128.183] | 464.678 [436.070, 497.311] | 274.43% |

Historical resampler Cancel cannot represent the delivered source timestamp
and cancellation reason. Its full semantic preflight rejects that missing
contract; all seven comparable admission, Up, sample and overflow preflights
pass with their exact assertions. Cancel has no valid BEFORE timing.
The current Cancel cases and dyn attachment sequences are AFTER-only:

| AFTER-only Criterion row | Mean ns [95% CI] |
|---|---:|
| `handle_event/dyn/primary_callbacks` | 434.383 [412.201, 460.896] |
| `handle_event/dyn/no_callbacks` | 402.501 [388.377, 418.158] |
| `resampler/stop/cancel` | 172.450 [150.211, 194.372] |
| `handle_event/dyn/secondary_callbacks` | 416.443 [399.740, 433.509] |
| `resampler/frame_trace/60_to_60/cancel` | 6240.751 [5930.837, 6606.724] |
| `resampler/frame_trace/240_to_60/cancel` | 18216.734 [17194.642, 19319.719] |

Six matched means increased by more than 10%. Source admission and the two
complete Up traces have separate mean confidence intervals. The measured
sample row's intervals overlap; this observation alone does not establish a
statistical significance test. The trace timings include event admission and
cloning, sampling, checked owned output construction, callback consumption and
terminal flush for a whole second of source events. Current output preserves
valid timestamp zero, timed cancellation, sensor readings and measured
history. Historical output cloned scalar state and cleared synthetic fields.
These are concrete workload-contract differences, not a per-method profile
or proof of the fraction caused by validation; historical code also performed
finite checks.

Ios estimation now walks the eligible continuous history (100 ms horizon,
40 ms adjacent gap) before limiting weighted pairs to that window. Historical
weighted estimation read raw ring neighbours and the oldest nonempty slot.
The required bounded scan changes the measured cost; restoring stale samples
would change the release-velocity contract. LSQ20 also increased by 31.45%
in this run. Its bounded walk and least-squares mathematics remain the same;
authored estimator selection and cache representation changed, but their
individual timing shares have not been established. The measured LSQ increase
remains in this source snapshot; the separate paired follow-up below measures
the remedy without overwriting this evidence.

### Paired least-squares follow-up

The LSQ increase prompted an isolated change at
`c1dc178815c05ba7646f201fd0cbbcf9ff36adf5`: prevent inlining of the existing
private `compute_estimate` kernel into the four-estimator dispatcher. The
bounded walk, numerical solver, public types and query-clock policy are
unchanged. Release-code inspection found that the original dispatcher shared
a 1752-byte scratch frame across estimator branches, versus 1144 bytes in the
historical fit path; the numerical solver's 585 normalized instructions were
identical. Static inspection does not establish the timing share of that
layout difference.

A fresh paired run on logical processor 0 used the same host, toolchain and
Criterion settings, saved as `interaction_pinned_before_lsq_pair` and
`interaction_pinned_after_lsq_candidate`. Both five-estimate executables
completed successfully. These independently paired means retain their own
BEFORE values:

| Criterion estimate row | Paired BEFORE ns [95% CI] | Current AFTER ns [95% CI] | Change |
|---|---:|---:|---:|
| `VelocityTracker::estimate (LSQ, 20 samples)` | 566.027 [550.809, 585.972] | 557.446 [551.604, 563.479] | -1.52% |
| `VelocityTracker::estimate (LSQ, 3 samples)` | 209.198 [202.932, 218.703] | 210.300 [208.101, 212.562] | 0.53% |
| `VelocityTracker::estimate (LSQ, 4 repeated queries)` | 538.125 [528.131, 553.246] | 582.440 [576.523, 588.652] | 8.24% |
| `VelocityTracker::estimate Impulse (20 samples)` | 457.849 [449.000, 471.144] | 457.603 [450.309, 466.967] | -0.05% |
| `VelocityTracker::estimate Ios (20 samples)` | 111.519 [109.147, 114.430] | 376.382 [373.397, 379.545] | 237.51% |

LSQ20 is now 1.52% below its fresh paired BEFORE mean, with overlapping mean
confidence intervals. LSQ3, four cached queries and Impulse remain within
10% of their paired historical means. The original LSQ20 increase is retained
above with its source revision. Ios remains slower because its corrected
weighted estimate examines eligible continuous history; the follow-up does
not relax that contract. These measurements support keeping the private
kernel separation without claiming that all of its gain comes from a
particular register, stack or instruction-layout effect.

Tap direct and dyn paths complete the same live three-event sequence:

| Tap AFTER sequence | Direct static ns [95% CI] | RecognizerSet dyn ns [95% CI] | Change |
|---|---:|---:|---:|
| primary_callbacks | 362.696 [353.489, 371.837] | 434.383 [412.201, 460.896] | 19.77% |
| no_callbacks | 406.970 [399.616, 414.240] | 402.501 [388.377, 418.158] | -1.10% |
| secondary_callbacks | 349.896 [341.700, 359.391] | 416.443 [399.740, 433.509] | 19.02% |

`RecognizerSet::dispatch` upgrades each weak attachment, classifies Down
admission, contains the recognizer call, preserves the first failure and
separately contains retirement of its promoted owner. Direct static calls
bypass that attachment membrane. The primary and secondary callback cases
measure about 20% more for the whole set sequence, with separate mean
confidence intervals; this is not an isolated virtual-call overhead.
No-callback intervals overlap.

To reproduce the fixture settings, build and run one target at a time, keeping
each checkout's target directory separate. The commands below do not pin
process affinity; apply the same process-local affinity to both executables
when reproducing the pinned experiment. Do not mix pinned and unrestricted
baselines as a matched comparison.

```bash
cargo bench --locked -p flui-interaction --bench tap_detector_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
cargo bench --locked -p flui-interaction --bench gesture_arena_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
cargo bench --locked -p flui-interaction --bench velocity_tracker_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
cargo bench --locked -p flui-interaction --bench pointer_resampler_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
cargo bench --locked -p flui-interaction --bench pointer_route_bench -- --save-baseline owned-input --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
```

The historical resampler timing filter is
`up|source_time|measured_and_interpolated|overflow`; current timing includes all
ten rows. Preserve the unsupported historical Cancel preflight failure rather
than weakening its timestamp and reason assertions to manufacture a baseline.

## Recovery delivery measurement

Measured on 2026-10-09 against production baseline
`c97217c0a2998dcefa2bef077622ff7dba2ca97d` and changed production
`babf737a5b7853fc4578e54a233a704b551fcd9b` in separate checkouts with
separate target directories. The five existing benchmark sources are identical
between these commits. The additional `interaction_delivery_bench` source and
its manifest registration were copied unchanged into the baseline checkout;
baseline production source was unchanged. Its source SHA-256 in both checkouts
is `F3CC6FEF4A59E5EF4384FDAEA8CFB35BE05F8B98288516905F50F5E217613590`.

Host: Windows 11 Pro 10.0.26200, Intel Core i9-13900K, 32 logical processors;
rustc 1.99.0 (`b940084d7`), LLVM 23.1.1. Both builds use the default release
profile: optimization level 3, thin LTO, one codegen unit. Build parallelism
was six jobs. Measurements run sequentially under the repository's host-wide
build lock, pinned to logical CPU 0 with normal process priority. No tracing
subscriber is installed by the fixtures.

The initial run alternates baseline then changed executable for each of the
six targets, with 20 samples, one second of warmup and two seconds of measured
time per case. All 44 cases on each side pass their functional preflights.
The table uses Criterion arithmetic means and their 95% bootstrap confidence
intervals from `estimates.json`, in nanoseconds; change is
`100 * (after / before - 1)`. These are independent estimates, not a paired
significance test. Overlap or separation of their intervals alone does not
establish significance. The saved baseline names are
`recovery_main_c97217c0a_20261009` and
`recovery_pr_babf737a5_20261009`.

| Case | Before mean [95% CI], ns | After mean [95% CI], ns | Change |
|---|---:|---:|---:|
| `GestureArena::add (busy, 4 prior members)` | 49.09 [48.08, 50.22] | 47.94 [47.17, 48.88] | -2.34% |
| `GestureArena::add (empty, 1 member)` | 81.98 [81.13, 83.01] | 80.38 [79.84, 81.02] | -1.95% |
| `GestureArena::add + accept (eager vs competitor)` | 365.67 [359.25, 373.44] | 359.88 [356.19, 364.73] | -1.58% |
| `GestureArena::add+close+sweep (full lifecycle)` | 336.50 [332.33, 342.24] | 333.72 [328.89, 341.40] | -0.82% |
| `GestureArena::sweep (1-member arena)` | 81.53 [80.21, 83.23] | 80.17 [79.36, 81.15] | -1.67% |
| `HitTestResult::dispatch/direct/1` | 195.05 [193.37, 196.67] | 187.50 [186.62, 188.35] | -3.87% |
| `HitTestResult::dispatch/direct/16` | 1010.18 [947.33, 1090.05] | 948.76 [943.33, 954.53] | -6.08% |
| `HitTestResult::dispatch/direct/4` | 315.72 [311.71, 320.62] | 310.63 [306.63, 315.43] | -1.61% |
| `InteractionLane::invoke_pointer_route/common_move/1` | 36.35 [35.85, 37.00] | 35.96 [35.54, 36.47] | -1.06% |
| `InteractionLane::invoke_pointer_route/common_move/16` | 134.73 [133.89, 135.58] | 139.01 [136.72, 142.07] | +3.18% |
| `InteractionLane::invoke_pointer_route/common_move/4` | 54.94 [54.49, 55.46] | 54.92 [54.57, 55.39] | -0.03% |
| `InteractionLane::resolve_pointer_route/1` | 144.91 [143.92, 145.80] | 153.56 [148.75, 161.42] | +5.97% |
| `InteractionLane::resolve_pointer_route/16` | 747.19 [742.53, 752.21] | 759.73 [749.08, 771.93] | +1.68% |
| `InteractionLane::resolve_pointer_route/4` | 239.13 [237.10, 241.59] | 239.53 [237.16, 242.81] | +0.17% |
| `OneEuroFilter2D::filter (per move)` | 15.28 [15.19, 15.38] | 15.17 [15.09, 15.25] | -0.76% |
| `VelocityTracker::add_position (push)` | 33.17 [32.85, 33.59] | 32.49 [32.37, 32.64] | -2.05% |
| `VelocityTracker::estimate (LSQ, 20 samples)` | 522.21 [518.27, 526.50] | 528.61 [519.52, 541.13] | +1.23% |
| `VelocityTracker::estimate (LSQ, 3 samples)` | 202.68 [199.99, 205.31] | 203.54 [200.27, 206.74] | +0.42% |
| `VelocityTracker::estimate (LSQ, 4 repeated queries)` | 542.56 [536.04, 549.13] | 533.73 [528.41, 539.63] | -1.63% |
| `VelocityTracker::estimate Impulse (20 samples)` | 395.69 [387.62, 408.05] | 382.75 [381.50, 383.98] | -3.27% |
| `VelocityTracker::estimate Ios (20 samples)` | 330.83 [325.20, 337.31] | 324.53 [320.77, 328.91] | -1.90% |
| `add_pointer/static` | 231.92 [218.39, 246.16] | 215.66 [204.09, 227.55] | -7.01% |
| `delivery/FocusManager/two_attached_focus_transitions` | 534.38 [531.54, 537.41] | 524.09 [520.67, 528.57] | -1.92% |
| `delivery/MouseTracker/ambient_two_cursor_changes_owned_hit_paths` | 307.12 [300.48, 314.99] | 359.20 [355.63, 364.44] | +16.96% |
| `delivery/MouseTracker/physical_two_cursor_changes` | 82.66 [82.16, 83.26] | 100.64 [100.09, 101.23] | +21.75% |
| `delivery/PointerRouter/registered_move/1` | 25.33 [25.05, 25.69] | 29.63 [29.43, 29.86] | +16.96% |
| `delivery/PointerRouter/registered_move/4` | 57.30 [56.79, 57.89] | 64.29 [63.87, 64.75] | +12.21% |
| `handle_event/dyn/no_callbacks` | 381.92 [372.88, 391.40] | 401.12 [389.27, 412.84] | +5.03% |
| `handle_event/dyn/primary_callbacks` | 399.91 [391.83, 408.50] | 418.11 [399.21, 441.66] | +4.55% |
| `handle_event/dyn/secondary_callbacks` | 402.85 [393.61, 413.15] | 418.41 [404.37, 432.44] | +3.86% |
| `handle_event/static/no_callbacks` | 409.41 [394.81, 424.12] | 396.45 [384.63, 409.06] | -3.17% |
| `handle_event/static/primary_callbacks` | 366.66 [359.54, 374.17] | 374.26 [362.29, 386.68] | +2.07% |
| `handle_event/static/secondary_callbacks` | 382.17 [368.11, 396.34] | 380.18 [369.08, 391.02] | -0.52% |
| `resampler/add_event/source_time` | 54.82 [52.86, 56.86] | 57.65 [55.53, 60.41] | +5.15% |
| `resampler/frame_trace/240_to_60/cancel` | 12613.95 [12535.85, 12695.01] | 12868.11 [12617.11, 13265.18] | +2.01% |
| `resampler/frame_trace/240_to_60/up` | 12674.82 [12534.35, 12841.54] | 12787.37 [12606.77, 13007.50] | +0.89% |
| `resampler/frame_trace/60_to_60/cancel` | 4709.22 [4664.85, 4752.71] | 4743.09 [4713.66, 4768.36] | +0.72% |
| `resampler/frame_trace/60_to_60/up` | 4815.35 [4712.31, 4946.93] | 4860.29 [4767.61, 4970.34] | +0.93% |
| `resampler/overflow/saturated_history` | 351.02 [316.04, 387.12] | 338.65 [301.92, 376.34] | -3.52% |
| `resampler/overflow/scalar_history` | 140.02 [118.66, 163.46] | 152.08 [126.73, 178.55] | +8.62% |
| `resampler/sample/measured_and_interpolated` | 1364.93 [1278.49, 1489.91] | 1217.74 [1185.06, 1250.77] | -10.78% |
| `resampler/stop/cancel` | 87.41 [78.59, 96.06] | 103.47 [90.42, 116.69] | +18.37% |
| `resampler/stop/up` | 91.74 [82.58, 101.05] | 92.82 [83.49, 102.08] | +1.18% |
| `resolve/weak` | 82.78 [79.93, 85.64] | 81.75 [77.90, 85.87] | -1.24% |

These fixtures cover synthetic API calls rather than full frames. Cursor and
focus rows measure **two transitions**. The ambient cursor fixture includes
owned hit-path clones; physical cursor fixtures borrow prebuilt paths. Setup,
owner installation and retirement are outside the timed loops. Live callback
counts and final output are checked before timing and observed during timing.
The existing 39 cases are predominantly controls for unchanged paths; the five
additional cases exercise healthy routing, cursor and focus delivery affected
by recovery changes. Native Scale admission, failure/reentry recovery, mounted
widgets, OS preference transport and physical hardware input are not timed here.

To reproduce, use the same benchmark source on both checkouts and separate
Criterion output directories, then run each target sequentially:

```bash
cargo bench --locked -p flui-interaction --bench interaction_delivery_bench -- \
  --save-baseline recovery_main_c97217c0a_20261009 \
  --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
```

Repeat for every target listed above and use the changed baseline name in the
changed checkout. Set `CRITERION_HOME` to each checkout's `target/criterion`.
The command itself does not pin CPU affinity or acquire the host lock; those
were applied by the measurement process launcher. Match rows by
`benchmark.json`'s `full_id`, and extract `.mean.point_estimate` and
`.mean.confidence_interval` with `jq` rather than treating console slope
estimates as arithmetic means.


### Reverse-order repeat

The three targets `interaction_delivery_bench`, `pointer_resampler_bench`
and `pointer_route_bench` were repeated with the changed executable first,
then baseline, 40 samples, one second of warmup and four seconds of measured
time. All 24 matched cases completed successfully. Build, CPU affinity, priority
and production commits are unchanged. Saved names:
`recovery_pr_babf737a5_repeat_20261009` and
`recovery_main_c97217c0a_repeat_20261009`.
For reproduction, replace the initial command's sample size with 40,
measurement time with 4 and saved names with these repeat names.

| Case | Before mean [95% CI], ns | After mean [95% CI], ns | Change |
|---|---:|---:|---:|
| `HitTestResult::dispatch/direct/1` | 191.60 [188.97, 195.85] | 186.69 [185.88, 187.61] | -2.56% |
| `HitTestResult::dispatch/direct/16` | 960.91 [949.20, 974.16] | 939.17 [933.31, 946.10] | -2.26% |
| `HitTestResult::dispatch/direct/4` | 319.84 [315.34, 325.29] | 307.10 [305.02, 309.78] | -3.98% |
| `InteractionLane::invoke_pointer_route/common_move/1` | 35.86 [35.60, 36.22] | 36.07 [35.79, 36.36] | +0.57% |
| `InteractionLane::invoke_pointer_route/common_move/16` | 140.42 [138.90, 142.24] | 133.32 [132.23, 134.42] | -5.06% |
| `InteractionLane::invoke_pointer_route/common_move/4` | 56.80 [56.20, 57.41] | 54.62 [54.34, 54.93] | -3.83% |
| `InteractionLane::resolve_pointer_route/1` | 144.04 [143.09, 145.28] | 143.76 [142.65, 145.06] | -0.19% |
| `InteractionLane::resolve_pointer_route/16` | 721.08 [717.74, 724.62] | 753.81 [737.65, 773.92] | +4.54% |
| `InteractionLane::resolve_pointer_route/4` | 238.68 [234.08, 244.80] | 235.22 [233.56, 237.32] | -1.45% |
| `delivery/FocusManager/two_attached_focus_transitions` | 533.53 [531.24, 535.97] | 559.29 [548.67, 572.17] | +4.83% |
| `delivery/MouseTracker/ambient_two_cursor_changes_owned_hit_paths` | 287.07 [285.00, 289.46] | 368.47 [364.87, 372.56] | +28.35% |
| `delivery/MouseTracker/physical_two_cursor_changes` | 83.37 [82.91, 83.87] | 101.67 [101.26, 102.08] | +21.96% |
| `delivery/PointerRouter/registered_move/1` | 25.16 [25.00, 25.37] | 30.12 [29.96, 30.30] | +19.73% |
| `delivery/PointerRouter/registered_move/4` | 57.53 [57.25, 57.84] | 65.68 [65.12, 66.38] | +14.18% |
| `resampler/add_event/source_time` | 52.90 [51.65, 54.12] | 51.61 [50.71, 52.49] | -2.44% |
| `resampler/frame_trace/240_to_60/cancel` | 12690.73 [12626.90, 12757.39] | 12602.71 [12534.83, 12673.06] | -0.69% |
| `resampler/frame_trace/240_to_60/up` | 12815.27 [12709.04, 12933.92] | 12573.36 [12511.30, 12642.67] | -1.89% |
| `resampler/frame_trace/60_to_60/cancel` | 4728.41 [4685.98, 4773.75] | 4697.65 [4663.49, 4732.16] | -0.65% |
| `resampler/frame_trace/60_to_60/up` | 4720.15 [4679.21, 4760.40] | 4742.47 [4685.98, 4808.49] | +0.47% |
| `resampler/overflow/saturated_history` | 390.81 [340.87, 445.88] | 346.25 [314.78, 378.47] | -11.40% |
| `resampler/overflow/scalar_history` | 160.21 [144.53, 176.31] | 139.36 [121.61, 157.74] | -13.01% |
| `resampler/sample/measured_and_interpolated` | 1352.86 [1314.56, 1393.92] | 1221.73 [1199.66, 1243.17] | -9.69% |
| `resampler/stop/cancel` | 140.05 [123.94, 156.13] | 90.85 [84.05, 97.68] | -35.13% |
| `resampler/stop/up` | 148.14 [135.76, 159.77] | 89.39 [82.92, 95.82] | -39.66% |

Registered routing is consistently more expensive across both run orders:
one callback adds 4.30–4.96 ns per Move (17–20%); four callbacks add
7.00–8.16 ns (12–14%). Physical cursor delivery adds 17.98–18.30 ns
per two transitions (about 22%). Ambient cursor delivery adds
52.08–81.40 ns per two transitions (17–28%). These are measurable healthy-path
costs, not evidence that recovery is free. The implementation now checks exact
registration identity and tracks cursor observations, publication debt and
in-flight callbacks; these measurements do not isolate the cost of each check.

Focus changes sign between runs (−1.92% then +4.83%), so there is no consistent
speedup. Complete resampler traces stay within roughly 2% of their matching
baseline in both orders. Measured/interpolated sample delivery is about 10%
lower in both runs, while isolated stop and overflow rows vary substantially
between runs; do not interpret their individual deltas as stable improvements.
Cached route resolution with 16 targets increases in both runs (+1.68%,
+4.54%); the one-target increase and 16-target invocation increase from the
initial run do not persist in the repeat. This comparison establishes fixture
costs on this host, not a performance budget for an entire interaction layer.


## See also

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [GESTURES.md](GESTURES.md)
- [HIT_TESTING.md](HIT_TESTING.md)
