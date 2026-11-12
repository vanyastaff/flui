# Shared grid measurement

This application uses public widgets and the existing raw-owner `HeadlessBinding` test seam.
It measures **wall time per driver invocation**, including state mutation, building, layout and
painting when requested. It excludes the native runtime/event loop, GPU encoding/submission,
presentation, serialization, correctness inspection and process startup. `mount_wall_ns` covers
initial scene/owner construction through the initial synchronous paint, not launch-to-visible.
The virtual animation interval is not a measurement clock. Inspect `frames_produced` before
interpreting a sample: retained idle usually produces no new frame.

The grid has fixed 14×14 cells at 16 px pitch, sidebar items 244×22 at 26 px pitch, a 48 px header,
8 px outer padding and pane gap. The sidebar pane is 260 px wide. The viewport is 1600×1000 at
scale 1. Roboto Regular is bundled. Grid text is `X` at 9 px; sidebar `Item N` at 12 px; header
`Benchmark 0`/`Benchmark 1` at 18 px. Colors are RGB (20,24,32) background, (40,80,120) cells,
white text. No OS theme or system font is used. Each pane clips; offsets stay zero. A single
`RepaintBoundary` surrounds the body. This optimization is part of the workload contract.

| Items | Columns × rows | Sidebar | Visible, including partial items |
|---:|---:|---:|---:|
| 100 | 10 × 8 | 20 | 100 |
| 1,000 | 20 × 40 | 200 | 836 |
| 10,000 | 50 × 180 | 1,000 | 2,936 |
| 53,000 | 80 × 600 | 5,000 | 4,676 |

All logical items exist; there is no virtualization. Labels and RawButtons share content and
geometry; button focus/gesture infrastructure is intentionally extra work. The header is not
included in item counts. Internal render-node counts are reported, not used as a cross-framework
unit. No upstream code is copied; the proposed shared workload is described in the draft research.

Build on a pinned checkout with `CARGO_INCREMENTAL=0`:

```text
cargo heli build --locked --release -p flui --example benchmark_grid --no-default-features
target/release/examples/benchmark_grid --items 1000 --kind buttons --mode local --warmup 120 --frames 10000 --out NEW_DIRECTORY
```

Use `.exe` on Windows. Every output directory must be new and its parent must exist. Capture
correctness separately by adding `--capture`: GPU creation/readback failures are errors, not skips.
This checks fill/glyph/gap/clip pixels and that changing the header changes pixels only above the
body. Population, cell sizes and selected button hits are checked on every run. Labels have no
activation requirement. The executable emits a VALID summary only after its checks succeed;
this means workload correctness, not statistical stability or external parity.

Run five fresh processes for each size × kind × retained/local cell. Interleave cells by repeat
and reverse their order every other repeat. Do not run builds, other benchmarks or captures in
parallel. Preserve all samples, including slow outliers. Use a separate 100-item forced-rebuild
control (`--mode rebuild`, 120 warmups, 1,000 samples); it is not the ordinary retained workload.
Native input latency, process CPU time, memory, energy and GPU time are unavailable in this adapter.

Before comparing different binaries, use the identical-binary repeats as A/A. Primary descriptive
statistic: median of five process p50 values. Report their full range, each process p95/p99/max,
and the median mount time. A/A spread is `(max(p50)-min(p50))/median(p50)`. A spread above 5%
is INCONCLUSIVE for a 10% improvement claim; these are experimental noise/practical thresholds,
not release budgets. Do not pool samples as independent process replicates or subtract timer
overhead from samples. The 10,000 empty-measurement samples give a lower-bound timer overhead,
not the cost of FrameReport instrumentation. Counter/report costs remain included in every mode.

Record source SHA, Cargo.lock and binary SHA-256, compiler/target, release profile and features,
CPU/RAM/OS, power policy, process ordering, run start/end UTC, and any concurrent load. GPU captures
also record the requested backend; the existing capture API does not expose selected adapter
identity. A list of installed GPUs is not proof of which adapter was selected. This Windows
offscreen cohort cannot be ranked against native Linux/Wayland upstream results.

Raw data: `samples.jsonl` has ordered wall-clock nanoseconds and the ten original FrameReport
counters per invocation; `clock-samples.json` contains ordered timer calibration samples;
`summary.json` uses nearest-rank percentiles. A failed process with no summary is INVALID. The
baseline report supplies a manifest and compressed raw files whose hashes can be checked after
decompression. The integration tests share the scene and measurement source, and mutation evidence
is recorded in the feature task file.
