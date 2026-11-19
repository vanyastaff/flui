### Added

- `flui-engine`: a window or offscreen slot's frame is timed on the GPU where the adapter offers timestamp queries (ADR-0388). `RasterBackend::take_gpu_time` and `RasterSender::drain_gpu_times` hand the owner a `GpuFrameTime` under the frame's epoch; without timestamp queries no time is reported, with no error.

### Changed

- `flui-engine`: `GpuCapabilities::supports_timestamp_queries` is true wherever the adapter writes pass timestamps (`TIMESTAMP_QUERY`), no longer only where it also writes them inside encoders.

### Removed

- `flui-engine`: the `gpu-profiler` feature, its `wgpu-profiler` dependency, `Renderer::latest_gpu_frame_profile`, `GpuFrameProfile` and `PassTiming`. Use `RasterSender::drain_gpu_times`.
