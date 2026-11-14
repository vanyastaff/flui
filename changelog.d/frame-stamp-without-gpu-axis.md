### Removed

- `flui::foundation::GpuResourceGeneration` and `FrameStamp::gpu_resource_generation`:
  `FrameStamp::new` takes the address, the epoch and the surface generation. A frame carries CPU
  data only, and a device rebuild refuses the frames stamped before it by each slot's surface
  generation.
