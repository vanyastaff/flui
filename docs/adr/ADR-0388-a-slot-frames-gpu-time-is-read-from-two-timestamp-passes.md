# ADR-0388: A slot frame's GPU time is read from two timestamp passes

- **Status:** Accepted (2026-11-19)
- **Date:** 2026-11-19
- **Related:** [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) (the raster executor, its
  slots and `RasterBackend`), [ADR-0328](ADR-0328-a-frame-s-scratch-memory-comes-from-the-owner-s-frame-arena.md) (no allocation on an idle
  frame), [ADR-0242](ADR-0242-typed-inspect-and-tracing-conventions.md) (the span stream the profiler
  fed)

## Context and problem statement

A frame's report could say how long the UI thread built, laid out and painted it, and how long
the raster thread recorded and submitted it. It could not say how long the GPU ran it. The one
GPU measurement was `wgpu-profiler`, behind the `gpu-profiler` feature, and it measured only
the single-surface `Renderer` (`Renderer::latest_gpu_frame_profile`). An application's windows
render through `GpuContext` slots, so nothing measured them. The feature also refused to build
for wasm32 (a `compile_error!` in `lib.rs`), added a dependency, and handed its result to no
frame record. A frame that is slow on the GPU therefore looked like an idle UI thread with a
late present.

## Decision drivers

- The time is the frame's: the owner matches it to the frame it rendered by the frame's epoch,
  never by "the latest".
- No device loses a feature or fails to start because of it. Where the adapter has no timestamp
  queries (WebGL2, some mobile GPUs) the time is `None`, with no error.
- An idle frame allocates nothing (ADR-0328), and a rendered frame allocates nothing that grows
  with the number of frames timed.
- Nothing on the frame path waits for the GPU to answer.
- Ticks become time through the queue's own period (`Queue::get_timestamp_period`), never an
  assumed nanosecond.

## Considered options

1. **Keep `wgpu-profiler`** behind its feature, and port it to the slots.
2. **A timestamp pair on every render pass**, using `RenderPassDescriptor::timestamp_writes`
   across the painter's passes.
3. **Two empty compute passes bracketing the frame**, each writing a timestamp at its start,
   on a small per-slot ring of query sets read back after the GPU finished.
4. **CPU time from submission to `Queue::on_submitted_work_done`.**

## Decision

Chosen option: 3. It needs only `Features::TIMESTAMP_QUERY`, not
`TIMESTAMP_QUERY_INSIDE_ENCODERS`, so it works wherever timestamps work. It costs two queries
per frame whatever the frame draws, and it touches no pass of the painter. Option 1 keeps the
dependency and the wasm32 refusal, and its scopes allocate per frame. Option 2 gives a
breakdown by pass, but no reader needs one yet, and it threads timestamp writes through every
pass the painter opens. Option 4 measures queue latency and the driver's wake-up as well as GPU
work.

```rust
// flui_foundation
#[non_exhaustive]
pub struct GpuFrameTime {
    pub epoch: FrameEpoch,
    pub gpu: Duration,
}

// flui_engine
pub trait RasterBackend {
    // …
    /// The oldest GPU time of `slot`'s frames read back and not taken yet.
    fn take_gpu_time(&mut self, slot: SlotId) -> Option<GpuFrameTime>;
}

impl RasterSender {
    /// Moves the GPU times the executor holds for `slot` into `out`, oldest first.
    pub fn drain_gpu_times(&self, slot: SlotId, out: &mut Vec<GpuFrameTime>) -> usize;
}

// flui_runtime
pub trait FrameSink {
    // …
    /// Moves the GPU times of this presentation's frames that arrived since the last call.
    fn take_gpu_times(&mut self, into: &mut Vec<GpuFrameTime>) {}
}

// flui_scheduler
pub struct FrameSnapshot {
    // …
    pub epoch: FrameEpoch,
    pub gpu_time: Option<Duration>,
}

impl FrameClock {
    /// Fills the GPU time of the retained frame stamped `epoch`, once.
    pub fn record_gpu_time(&self, epoch: FrameEpoch, gpu: Duration) -> Option<FrameSnapshot>;
}
```

1. **The device asks for timestamps where offered.** The window device (`request_flui_device`)
   and `GpuContext`'s offscreen device add `TIMESTAMP_QUERY` when the adapter has it. Without it, no slot builds a timer and every
   frame's time is `None`.
2. **One timer per slot.** `GpuContext` builds a slot's `FrameGpuTimer` at its first timed
   frame, drops it with the slot, and drops every timer when the device is rebuilt, since the
   query sets were the old device's. Frames lent outside the slot's frames are not timed.
3. **The frame is bracketed in its own encoder.** The first timestamp pass comes after the
   encoder is created and before the frame's clear. The second comes after the frame's last
   pass, before submission. Resolve, copy and `map_buffer_on_submit` are recorded in the same
   encoder, so a frame that never submits takes its mapping with it.
4. **A ring of three, nothing waits.** An entry is free, encoded into a frame, or submitted
   with one. A frame that finds every entry in flight goes untimed. An entry still encoded when
   the slot's next frame begins belonged to a frame that failed or unwound before submission,
   and gets a new readback, since a panic after the queue took the frame may have left its
   mapping live. A readback that fails to map (the device was lost) frees its entry with no
   time.
5. **Collected at the slot's next frame.** After each frame the timer reads back every entry
   whose mapping completed and publishes its time under its epoch, oldest first, three at
   most. So the last frame before an idle period has its time only once the slot renders
   again. Two equal stamps, or two never written, are no time.
6. **Bounded on the way to the owner.** After each frame it renders, the executor takes the
   backend's times for the slot outside the inbox lock, contained as every backend call is,
   and holds them in the slot's inbox. It takes at most 32 per frame, and the inbox keeps the
   eight newest undrained times and drops the oldest. The owner drains them with
   `drain_gpu_times` into a buffer it keeps.
7. **Allocation.** A slot allocates its ring and its inbox capacity once. A frame the plan
   skips is not timed and allocates nothing. A timed frame adds two passes, a resolve, a copy
   and a mapping to its encoder, whose callback wgpu boxes: a constant per frame, which grows
   with nothing. An entry reclaimed from a frame that never submitted allocates a new readback.
8. **Tile GPUs give an approximation.** On a tile-based GPU (Apple, Mali) the passes between
   the two timestamps are not guaranteed to execute between them, so the time is an estimate
   of the frame's GPU work, not a bound. A run on such a device (the `gpu-frame-time` row of
   `crates/flui-native/platform-parity.toml`) is owed before the number is
   used as a gate.
9. **The profiler is gone.** The `gpu-profiler` feature, the `wgpu-profiler` dependency,
   `Renderer::latest_gpu_frame_profile`, `GpuFrameProfile` and `PassTiming` are removed.
10. **The time joins its frame's record.** Every frame a presentation submits is stamped with
    its epoch, inline or queued, and its `FrameSnapshot` carries that epoch and a `gpu_time`
    of `None`. After each frame it sends, the UI runtime takes the sink's times
    (`FrameSink::take_gpu_times`, which a window's sink answers from its slots'
    `drain_gpu_times`, into a buffer the runtime keeps) and hands each to the presentation
    clock's `record_gpu_time`, which fills the snapshot whose epoch matches, once. A second
    time for one frame, and a time for an unstamped frame, are dropped. A time no retained
    snapshot matches is held in a fixed list of four, the oldest overwritten: a queued frame's
    snapshot is recorded when its settlement is drained, and the raster thread can hand back
    the time first. The snapshot recorded next under that epoch takes it. A time whose frame
    left the ring, or never records (replaced while queued, not shown), stays until four later
    holds push it out, and a burst of them can push out a real one; no later snapshot shares
    its epoch, so none takes it wrongly. The times are recorded on the clock of the
    presentation whose frame was just sent, so a sink serves one presentation. A time usually
    arrives after the frame's `frame_telemetry`
    event, so a filled snapshot emits its own `flui.frame` event,
    `frame_gpu_time`, with `frame_id`, `epoch` and `gpu_us`, and a reader joins the two by
    `frame_id`; a snapshot that took a held time emits it right after its `frame_telemetry`.
    The sink's call and every event run contained, so a panic in either is held for the
    frame's tail, which still settles. `GpuFrameTime` lives in `flui-foundation`, below both the
    engine and the runtime, so the runtime names it without an engine edge (ADR-0083). The
    devtools Timeline puts `gpu_us` in a timed frame's arguments. The agent protocol carries
    no `FrameSnapshot`, so its version is unchanged.

### Confirmation

- `a_slot_frame_is_timed_on_the_gpu_under_its_epoch` (`flui-engine`, on lavapipe) renders a
  slot's frames under epochs 1 to 4 and reads back nonzero times for epochs 1 to 3.
- `frames_that_never_submit_give_their_entries_back` checks that frames which fail after
  recording, or unwind, leave the ring able to time the frames after them.
- `ticks_convert_through_the_queues_period` checks the conversion through the queue's period.
- `a_backends_gpu_times_reach_the_owner_after_the_frame_that_read_them`
  (`tests/raster_executor.rs`) checks that the owner drains a backend's times under their
  epochs and that an undrained record keeps the eight newest.
- `the_ticks_are_the_second_stamp_less_the_first` checks the byte order, and that equal,
  unwritten or wrapped stamps are no time.
- `a_backend_that_panics_handing_out_a_gpu_time_leaves_its_frame_settled` checks that a
  backend's panic while its times are taken is held and its frame still settles.
- `a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full` draws timed frames
  across a rebuild, which fails if the old device's timers outlive it.
- `a_gpu_time_lands_on_the_snapshot_of_the_frame_its_stamp_names` (`flui-scheduler`) checks
  that a time fills its own frame's snapshot once, that an unstamped frame takes none, and
  that a time that comes before its frame rides on the snapshot recorded under its epoch.
- `a_frames_gpu_time_is_traced_once_it_arrives` (`flui-runtime`) checks that a sink's time
  reaches one `frame_gpu_time` event whose `frame_id` is its frame's, that a time under an
  epoch no frame had traces nothing, and that a time handed back before its frame was
  recorded is traced with that frame.
- `a_sink_that_panics_handing_out_gpu_times_still_gets_its_retry` (`flui-runtime`) checks
  that a sink's panic while its times are taken resumes only after the frame's tail armed the
  retry its refusal asked for.
- `a_windows_sink_hands_back_the_gpu_times_of_its_frames` (`flui-app`) checks that a window's
  sink drains its slots' times.
- `a_timed_frame_carries_its_gpu_time_into_the_trace` (`flui-devtools`) checks `gpu_us` on a
  timed frame and its absence on an untimed one.
- An idle frame's plan is a skip, which returns before the timer's `begin`;
  `perf_an_idle_frame_leaves_the_retained_scene_alone` keeps asserting zero allocations on the
  retained scene's idle frame.

## Pros and cons of the options

### 1. `wgpu-profiler`

- Good, because it already exists and names its scopes.
- Bad, because it is a dependency that does not build for wasm32, it allocates per scope and
  per frame, and its result reaches no frame record.

### 2. A timestamp pair per render pass

- Good, because it breaks the frame down by pass.
- Bad, because every pass the painter opens takes a timestamp argument, and the query count
  grows with the frame's passes.
- Bad, because no reader of a per-pass breakdown exists yet.

### 3. Two bracketing timestamp passes

- Good, because it needs only `TIMESTAMP_QUERY`, costs two queries per frame, and changes no
  pass of the painter.
- Good, because the time is matched to its frame by epoch, and a missing feature is `None`.
- Bad, because the time arrives a frame late, and on tile GPUs it is an approximation.
- Bad, because an empty compute pass is recorded twice per frame where timestamps are on.

### 4. CPU time to `on_submitted_work_done`

- Good, because it needs no device feature.
- Bad, because it measures the queue's latency and the driver's callback, not the GPU's work.
