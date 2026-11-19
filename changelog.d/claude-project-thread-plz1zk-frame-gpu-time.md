### Added

- `flui-scheduler`: `FrameSnapshot::epoch` names the epoch a frame was stamped with, and `FrameSnapshot::gpu_time` holds the frame's GPU time once it arrives; `FrameClock::record_gpu_time` fills it for the retained frame of that epoch, once (ADR-0388).
- `flui-runtime`: `FrameSink::take_gpu_times`, a provided method through which a sink hands the UI runtime its frames' GPU times; a filled snapshot emits a `flui.frame` `frame_gpu_time` event with `frame_id`, `epoch` and `gpu_us`. A window's sink answers it from its raster slots.
- `flui-devtools`: a timed frame's Timeline event carries `gpu_us` in its arguments.

### Changed

- `flui-scheduler`: `FrameClock::record_frame`, `record_frame_retaining_epochs` and `record_frame_with_epochs` take the frame's `FrameEpoch` after its address.
