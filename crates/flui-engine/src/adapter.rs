//! Shared adapter/device acquisition policy for the production GPU stacks.
//!
//! Every production `request_adapter`/`request_device` in this crate follows
//! the same policy; before this module, each site restated it inline and each
//! wgpu API migration (most recently wgpu 30 adding `apply_limit_buckets`)
//! had to touch all of them. The policy now lives here once:
//!
//! - `trusted_adapter_options` — the `RequestAdapterOptions` used everywhere.
//! - `request_flui_device` — the capability-derived `DeviceDescriptor`.
//!
//! `HeadlessRenderer` deliberately does not use `request_flui_device`:
//! capture wants wgpu's default (downlevel-friendly) device rather than the
//! renderer's capability-negotiated one.

use crate::error::{EngineError, EngineResult};

use crate::renderer::{GpuCapabilities, Renderer};

/// The one `RequestAdapterOptions` policy for this engine.
///
/// `apply_limit_buckets` is wgpu 30's anti-fingerprinting knob, for embedders
/// that expose a GPU to untrusted content (a browser). `false` — this engine's
/// callers ARE the trusted application, and bucketing would round the
/// adapter's real limits down to a coarse tier the pipelines would then have
/// to fit. Same answer at every other `request_adapter` in this workspace.
///
/// `force_fallback_adapter` is likewise `false` everywhere: a software
/// fallback is a host-configuration decision (WARP in CI, lavapipe locally),
/// not something the engine opts into per call.
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

/// Requests the capability-negotiated device + queue shared by the windowed
/// and offscreen renderer stacks.
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
            required_features: Renderer::required_features(capabilities),
            required_limits: Renderer::required_limits(capabilities, &adapter_limits),
            // Desktop UI: trade VRAM for faster per-frame GPU allocations.
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(EngineError::device_creation)
}
