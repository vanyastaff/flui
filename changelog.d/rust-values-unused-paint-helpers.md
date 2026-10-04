### Removed
- Remove the unused `Shader::to_mask_uniform_data` serializer; the engine owns gradient and shader-mask GPU packing.
- Remove `FittedSizes::will_clip`, which mistook contained image upscaling for clipping.
