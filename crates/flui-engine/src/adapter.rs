//! Shared adapter/device acquisition policy for the production GPU stacks.
//!
//! Every production `request_adapter`/`request_device` in this crate follows
//! the same policy; before this module, each site restated it inline and each
//! wgpu API migration (most recently wgpu 30 adding `apply_limit_buckets`)
//! had to touch all of them. The policy now lives here once:
//!
//! - `trusted_adapter_options` — the `RequestAdapterOptions` used everywhere.
//! - `adapter_for`, `device_on` and `adapter_and_device` — the adapter a
//!   probed backend attempt asks for and its capability-negotiated device,
//!   which the GPU probe and the context's device rebuild both take.
//! - `required_features` and `required_limits` — what that device asks
//!   for, read from the adapter's [`GpuCapabilities`].
//!
//! `HeadlessRenderer` and the offscreen contexts deliberately do not use the
//! negotiated device: capture wants wgpu's default (downlevel-friendly)
//! device instead.

use crate::error::{EngineError, EngineResult};

/// What the GPU adapter reports, and what this engine asked for.
///
/// Holds the adapter's own `Features`/`Limits` tables rather than one `bool`
/// per question: a query is a method (so a capability this engine starts
/// using needs no struct change and no `struct_field_names` suppression), and
/// the tables are what a caller needs when it wants to ask something this
/// type does not.
///
/// `#[non_exhaustive]`: wgpu adds features and limit buckets, and this struct
/// tracks them. Construction is [`Self::detect`] — an embedder reads the
/// fields, it does not build one.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// The backend the adapter belongs to: the preferred one, or the
    /// fallback the probe took when the preferred one gave no device
    /// (ADR-0244 §1).
    pub backend: crate::GpuBackend,
    /// Adapter name, vendor, and device type as wgpu reports them.
    pub adapter_name: String,
    /// Human-readable vendor name resolved from the PCI vendor id.
    pub vendor: String,
    /// The features the adapter exposes.
    pub features: wgpu::Features,
    /// The limits the adapter reports.
    pub limits: wgpu::Limits,
}

impl GpuCapabilities {
    /// Detect GPU capabilities from an adapter.
    #[must_use]
    pub fn detect(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        Self {
            backend: crate::GpuBackend::of_adapter(&info),
            adapter_name: info.name,
            vendor: Self::vendor_name(info.vendor),
            features: adapter.features(),
            limits: adapter.limits(),
        }
    }

    /// Whether the adapter accepts immediate constants ("push constants").
    #[must_use]
    pub fn supports_push_constants(&self) -> bool {
        self.features.contains(wgpu::Features::IMMEDIATES)
    }

    /// Whether the adapter can write pass timestamps, which time a slot's
    /// frames on the GPU (ADR-0388). Pass timestamps need no
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
    #[must_use]
    pub fn supports_timestamp_queries(&self) -> bool {
        self.features.contains(wgpu::Features::TIMESTAMP_QUERY)
    }

    /// Whether the adapter exposes a second blend source (`@blend_src(1)`).
    ///
    /// The fast path serves the `Replacing` and `Masking` classes of
    /// `BlendMode::coverage_class` (`Clear`, `Src`, `SrcIn`, `SrcOut`,
    /// `Modulate`, whose destination factor is `Zero`, and `DstIn`, `DstATop`,
    /// whose destination factor is `SrcAlpha`): their partial coverage cannot
    /// be folded into source alpha, so the second source carries it. Without
    /// it, those modes use independent source/coverage isolation and
    /// destination sampling for exact partial coverage.
    pub fn supports_dual_source_blending(&self) -> bool {
        self.features.contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    }

    fn vendor_name(vendor_id: u32) -> String {
        match vendor_id {
            0x1002 => "AMD".to_string(),
            0x10DE => "NVIDIA".to_string(),
            0x8086 => "Intel".to_string(),
            0x106B => "Apple".to_string(),
            0x1414 => "Microsoft (WARP)".to_string(),
            0x5143 => "Qualcomm".to_string(),
            _ => format!("Unknown (0x{vendor_id:04X})"),
        }
    }
}

/// The one `RequestAdapterOptions` policy for this engine.
///
/// `apply_limit_buckets` is wgpu 30's anti-fingerprinting knob, for embedders
/// that expose a GPU to untrusted content (a browser). `false` — this engine's
/// callers ARE the trusted application, and bucketing would round the
/// adapter's real limits down to a coarse tier the pipelines would then have
/// to fit. Same answer at every other `request_adapter` in this workspace.
///
/// `force_fallback_adapter` is likewise `false` here: a software fallback is
/// a host-configuration decision (WARP in CI, lavapipe locally), except where
/// a probed attempt names the software adapter itself ([`adapter_for`]).
pub(crate) fn trusted_adapter_options<'a, 'b>(
    power_preference: wgpu::PowerPreference,
    compatible_surface: Option<&'a wgpu::Surface<'b>>,
) -> wgpu::RequestAdapterOptions<'a, 'b> {
    wgpu::RequestAdapterOptions {
        power_preference,
        compatible_surface,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }
}

/// The adapter `attempt` asks `instance` for, one that can present to
/// `surface` when one is given, and its device.
pub(crate) async fn adapter_and_device(
    instance: &wgpu::Instance,
    attempt: crate::gpu_backend::Attempt,
    surface: Option<&wgpu::Surface<'static>>,
) -> EngineResult<(wgpu::Adapter, GpuCapabilities, wgpu::Device, wgpu::Queue)> {
    let adapter = adapter_for(instance, attempt, surface).await?;
    let (capabilities, device, queue) = device_on(&adapter).await?;
    Ok((adapter, capabilities, device, queue))
}

/// The adapter `attempt` asks `instance` for, one that can present to
/// `surface` when one is given.
pub(crate) async fn adapter_for(
    instance: &wgpu::Instance,
    attempt: crate::gpu_backend::Attempt,
    surface: Option<&wgpu::Surface<'static>>,
) -> EngineResult<wgpu::Adapter> {
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            // WARP is asked for by name: the software adapter is this
            // attempt, not a host's configuration.
            force_fallback_adapter: attempt.software,
            ..trusted_adapter_options(wgpu::PowerPreference::HighPerformance, surface)
        })
        .await
        .map_err(EngineError::adapter_request)
}

/// The capability-negotiated device on `adapter`, and the capabilities
/// it was negotiated from.
pub(crate) async fn device_on(
    adapter: &wgpu::Adapter,
) -> EngineResult<(GpuCapabilities, wgpu::Device, wgpu::Queue)> {
    let capabilities = GpuCapabilities::detect(adapter);
    let (device, queue) = request_flui_device(adapter, &capabilities, "FLUI GPU Device").await?;
    Ok((capabilities, device, queue))
}

/// Requests the capability-negotiated device + queue a probed backend
/// attempt draws with.
///
/// # Errors
///
/// Returns [`EngineError::DeviceCreation`] when the driver rejects the
/// requested device.
pub(crate) async fn request_flui_device(
    adapter: &wgpu::Adapter,
    capabilities: &GpuCapabilities,
    label: &str,
) -> EngineResult<(wgpu::Device, wgpu::Queue)> {
    let adapter_limits = adapter.limits();
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(label),
            required_features: required_features(capabilities),
            required_limits: required_limits(capabilities, &adapter_limits),
            // Desktop UI: trade VRAM for faster per-frame GPU allocations.
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(EngineError::device_creation)
}

/// Required GPU features based on capabilities and adapter support.
///
/// Only requests optional features when the adapter actually exposes them,
/// so device creation never regresses on GPUs that lack them.
///
/// One definition for every construction path, so all of them request
/// the same feature set.
pub(crate) fn required_features(capabilities: &GpuCapabilities) -> wgpu::Features {
    let mut features = wgpu::Features::empty();

    // Native format extensions are optional, and unavailable on WebGPU.
    features |= capabilities.features & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

    // Immediates (formerly push constants): only request if adapter supports them.
    // Some mobile GPUs (especially older Android devices) don't support this.
    if capabilities.supports_push_constants() {
        features |= wgpu::Features::IMMEDIATES;
    }

    // Pass timestamps time each slot's frame on the GPU (ADR-0388), where
    // the adapter offers them: a device without them times nothing.
    if capabilities.supports_timestamp_queries() {
        features |= wgpu::Features::TIMESTAMP_QUERY;
    }

    // A second blend source, so an anti-aliased clip can feather a
    // destination-destructive blend instead of applying it at full strength
    // across the whole fringe. Requested only where the adapter exposes it;
    // `PipelineCache` falls back to the folded shader otherwise, and the
    // destination-sensitive coverage uses portable isolation there. See
    // `GpuCapabilities::supports_dual_source_blending`.
    if capabilities.supports_dual_source_blending() {
        features |= wgpu::Features::DUAL_SOURCE_BLENDING;
    }

    // The driver's pipeline cache, which `GpuContext::load_pipeline_cache`
    // keeps between runs; wgpu offers it on Vulkan alone.
    features |= capabilities.features & wgpu::Features::PIPELINE_CACHE;

    features
}

/// Required GPU limits based on capabilities and adapter support
///
/// Same one-definition rationale as [`required_features`]. Every
/// field is clamped down to what the adapter actually advertises.
/// `wgpu::Limits::default()` is the *desktop* baseline: it asks for
/// `max_inter_stage_shader_variables: 16`, which the iOS simulator's Metal
/// adapter caps at 15, so a plain `..default()` makes device creation fail
/// with `LimitsExceeded` on that platform. Requesting a limit above the
/// adapter's own can never succeed — the adapter's `Limits` are what
/// `request_device` validates against — so the engine takes the adapter's
/// value wherever the default exceeds it, as the GLES 3.0 and WebGL2
/// fallback adapters (ADR-0244 §1) do for every storage and compute
/// limit.
pub(crate) fn required_limits(
    capabilities: &GpuCapabilities,
    adapter_limits: &wgpu::Limits,
) -> wgpu::Limits {
    let mut limits = wgpu::Limits {
        max_texture_dimension_2d: capabilities
            .limits
            .max_texture_dimension_2d
            .min(flui_platform::MAX_SURFACE_EXTENT),
        ..wgpu::Limits::default()
    };

    // Immediate data size — only set if adapter supports immediates
    if capabilities.supports_push_constants() {
        limits.max_immediate_size = 128;
    }

    // Never ask for more than the adapter offers, field by field: a
    // GLES 3.0 or WebGL2 adapter has no storage buffers, storage textures
    // or compute, which the desktop default asks for, and the engine
    // uses none of them.
    limits.or_worse_values_from(adapter_limits)
}

#[cfg(test)]
mod tests {
    use super::{GpuCapabilities, required_features};

    /// The device request asks for no feature the adapter does not
    /// advertise, and for the pipeline cache wherever it is offered.
    #[test]
    fn device_request_uses_only_advertised_features() {
        for features in [
            wgpu::Features::empty(),
            wgpu::Features::DUAL_SOURCE_BLENDING,
            wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            wgpu::Features::IMMEDIATES,
            wgpu::Features::TIMESTAMP_QUERY,
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            wgpu::Features::PIPELINE_CACHE,
        ] {
            let capabilities = GpuCapabilities {
                backend: crate::GpuBackend::WebGpu,
                adapter_name: String::new(),
                vendor: String::new(),
                features,
                limits: wgpu::Limits::default(),
            };
            let requested = required_features(&capabilities);
            assert!(
                features.contains(requested),
                "device request {requested:?} exceeds adapter features {features:?}"
            );
            // The pipeline cache is asked for wherever it is offered.
            assert_eq!(
                requested.contains(wgpu::Features::PIPELINE_CACHE),
                features.contains(wgpu::Features::PIPELINE_CACHE),
                "{features:?}"
            );
        }
    }
}
