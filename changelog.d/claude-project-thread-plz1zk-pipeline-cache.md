### Added

- **Pipeline cache kept between runs on Vulkan**: the GPU context creates its pipelines through a `wgpu::PipelineCache` wherever the adapter offers `PIPELINE_CACHE`, and the native runners load its data from the application's machine-local storage (`gpu-pipeline-cache`) before the first window attaches and save it after the pipeline warm-up and when the context retires. A driver without a disk cache of its own (Android's Vulkan drivers) no longer compiles every pipeline at each launch. A stored value that is not this adapter's cache is emptied and the context starts cold.
