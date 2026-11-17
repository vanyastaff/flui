//! The device a render pipeline is created on, with the driver's pipeline
//! cache it is created through.
//!
//! Every pipeline constructor takes a [`PipelineDevice`] in place of a bare
//! `&wgpu::Device`, so no pipeline is created outside the cache by
//! forgetting to pass it. A structure that creates pipelines on first use
//! keeps the cache it was built with ([`PipelineDevice::owned_cache`]) and
//! pairs it again with the device its caller hands it.
//!
//! The cache is a `wgpu::PipelineCache` (Vulkan only): the driver keeps the
//! compiled pipelines in it, and `crate::pipeline_store` saves its data
//! between runs, so a driver without a disk cache of its own does not compile
//! every pipeline again at each launch. Without one (another backend, a
//! device without `PIPELINE_CACHE`) pipelines are created exactly as before.

/// A device and the pipeline cache its pipelines are created through.
#[derive(Clone, Copy)]
pub(crate) struct PipelineDevice<'a> {
    device: &'a wgpu::Device,
    cache: Option<&'a wgpu::PipelineCache>,
}

impl<'a> PipelineDevice<'a> {
    /// `device`, creating its pipelines through `cache`, which must be one
    /// `device` created.
    pub(crate) fn new(device: &'a wgpu::Device, cache: Option<&'a wgpu::PipelineCache>) -> Self {
        Self { device, cache }
    }

    /// `device` without a pipeline cache.
    pub(crate) fn uncached(device: &'a wgpu::Device) -> Self {
        Self::new(device, None)
    }

    /// The device the pipelines are created on.
    pub(crate) fn device(self) -> &'a wgpu::Device {
        self.device
    }

    /// The `cache` of a `wgpu::RenderPipelineDescriptor`.
    pub(crate) fn cache(self) -> Option<&'a wgpu::PipelineCache> {
        self.cache
    }

    /// The cache, for a structure that creates pipelines on first use and
    /// keeps it until then.
    pub(crate) fn owned_cache(self) -> Option<wgpu::PipelineCache> {
        self.cache.cloned()
    }
}
