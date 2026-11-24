### Added

- **`flui-devtools`**: a `Capture` records each frame a presentation produced as a `ProducedFrame` (runtime, presentation, frame number, outcome, segment and phase times, produce-to-present, input count and largest input latency, and the GPU time once it arrives), read from the runtime's `flui.frame` `frame_telemetry` and `frame_gpu_time` events; `Capture::produced_frames` and `produced_frames_of` return them. A timeline over the capture shows each as a `TimelineEventKind::ProducedFrame` event, on a `Produced frames` track of its runtime's process in the Chrome export. The capture's filter admits those two `TRACE` events and the runtime's `enabled!` gate for them, and no other event.

### Changed

- **`flui-runtime`**: the `frame_telemetry` and `frame_gpu_time` events write `presentation` as the presentation id's packed `as_u64` and `frame_id` as the frame's number, both `u64` fields, instead of their display text. A reader rebuilds the id with `PresentationId::from_u64`.

### Removed

- **`flui-devtools`**: `Timeline::record_frame_snapshots`. Nothing public produced the snapshots it took; a timeline over a `Capture` shows the produced frames from the runtime's frame telemetry.
- **`flui-sdk`**: `hooks::FrameSnapshot`, which only `Timeline::record_frame_snapshots` used.
