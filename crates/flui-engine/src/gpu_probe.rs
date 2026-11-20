//! ADR-0244 decision 1's backend probe: each backend attempt in an instance
//! of its own, over the first window when there is one, until one answers
//! with a device (ADR-0305 §2, §8).
//!
//! The bootstrap runs it once for the context it makes; the owner runs it
//! again when the application asks to retry a device whose rebuilds were
//! given up, since the backend whose rebuilds exhausted the backoff may be
//! the cause, and the context adopts the device it answers
//! ([`RasterCommand::RetryDevice`](crate::RasterCommand::RetryDevice)).

use std::sync::Arc;

use flui_platform::{DisplayConnection, SurfaceTarget};

use crate::{
    error::{EngineError, EngineResult},
    gpu_backend::{Attempt, GpuBackend},
    gpu_context::{DeviceRecipe, GpuContext},
    surface_factory::{SurfaceFactory, UnconfiguredSurface, checked_target, instance_descriptor},
};

/// A device the probe answered, on an instance of its own, for a context to
/// be made on or to adopt in place of a device it gave up on.
///
/// `Send`, so the owner that probed hands it to a raster thread.
pub struct ProbedDevice {
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    /// How a rebuild on this instance asks for its next device.
    pub(crate) recipe: DeviceRecipe,
}

impl std::fmt::Debug for ProbedDevice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProbedDevice")
            .field("adapter", &self.adapter.get_info().name)
            .finish_non_exhaustive()
    }
}

static_assertions::assert_impl_all!(ProbedDevice: Send);

/// What the probe made: the device, the owner's factory over the device's
/// instance, and the first window's surface on that instance when there was
/// one ([`GpuContext::probe`]).
#[non_exhaustive]
pub struct GpuProbe {
    /// The device, for a new context or the context that adopts it.
    pub device: ProbedDevice,
    /// The owner's factory over the device's instance: every window's
    /// surface is made with it from now on.
    pub factory: SurfaceFactory,
    /// The first window's surface, on the device's instance.
    pub first: Option<UnconfiguredSurface>,
}

impl std::fmt::Debug for GpuProbe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GpuProbe")
            .field("device", &self.device)
            .field("first", &self.first.is_some())
            .finish_non_exhaustive()
    }
}

/// One backend attempt's instance, adapter, device and the first window's
/// surface.
struct Answered {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    first: Option<wgpu::Surface<'static>>,
    /// The backend attempt that answered, for a rebuild to ask the same.
    attempt: Attempt,
}

impl GpuContext {
    /// Run ADR-0244 decision 1's probe on the host's `display`: the
    /// preferred backend, then its fallback, each in an instance of its own,
    /// over the `first` window when there is one, so the adapter chosen
    /// presents to it; `WGPU_BACKEND` keeps only the backends it names.
    ///
    /// The probe blocks a native owner for as long as the drivers take, and
    /// logs how long that was (`gpu_probe` on the `flui.gpu` target).
    ///
    /// # Errors
    /// - [`EngineError::SurfaceTargetUnavailable`] or
    ///   [`EngineError::SurfaceOnForeignDisplay`]: `first` fails the factory's
    ///   checks, before any GPU work.
    /// - [`EngineError::NoSupportedGpu`]: no backend gave a device, with why
    ///   each one failed.
    pub async fn probe(
        display: Arc<dyn DisplayConnection>,
        first: Option<Arc<dyn SurfaceTarget>>,
    ) -> EngineResult<GpuProbe> {
        let started = web_time::Instant::now();
        let first = first
            .map(|target| checked_target(&*display, &target).map(|leased| (leased, target)))
            .transpose()?;
        let attempts = crate::gpu_backend::attempts(
            crate::gpu_backend::Family::CURRENT.order(),
            wgpu::Backends::from_env(),
        );
        let leased = first.as_ref().map(|(leased, _)| leased);
        let display_ref = &display;
        let answered = crate::gpu_backend::first_answering(&attempts, |attempt| async move {
            answer(attempt, display_ref, leased)
                .await
                .map_err(|error| error.to_string())
        })
        .await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let (backend, answered) = answered.map_err(|tried| {
            tracing::warn!(
                target: "flui.gpu",
                event = "gpu_probe",
                elapsed_ms,
                "no GPU backend gave a device"
            );
            EngineError::NoSupportedGpu { tried }
        })?;
        tracing::info!(
            target: "flui.gpu",
            event = "gpu_probe",
            %backend,
            adapter = %answered.adapter.get_info().name,
            elapsed_ms,
            "GPU backend selected"
        );
        let Answered {
            instance,
            adapter,
            device,
            queue,
            first: surface,
            attempt,
        } = answered;
        let first = match (first, surface) {
            (Some((leased, target)), Some(surface)) => {
                Some(UnconfiguredSurface::new(leased, target, surface))
            }
            _ => None,
        };
        let factory = SurfaceFactory::new(instance.clone(), display);
        Ok(GpuProbe {
            device: ProbedDevice {
                instance,
                adapter,
                device,
                queue,
                recipe: DeviceRecipe::Probed(attempt),
            },
            factory,
            first,
        })
    }
}

/// One backend attempt: an instance over `display` for `attempt` alone, the
/// first window's surface on it, an adapter that presents to it, and its
/// device.
async fn answer(
    attempt: Attempt,
    display: &Arc<dyn DisplayConnection>,
    first: Option<&Arc<dyn crate::window_target::WindowTarget>>,
) -> EngineResult<Answered> {
    #[cfg(test)]
    crate::gpu_context::INSTANCES_CREATED.with(|created| created.set(created.get() + 1));
    let instance = wgpu::Instance::new(instance_descriptor(
        display,
        attempt.backends,
        wgpu::BackendOptions {
            dx12: crate::surface_present::dx12_options(),
            ..Default::default()
        },
    ));
    let surface = |instance: &wgpu::Instance| {
        first
            .map(|target| {
                instance
                    .create_surface(Arc::clone(target))
                    .map_err(EngineError::surface_creation)
            })
            .transpose()
    };
    // A canvas keeps the first context kind it hands out, so WebGPU gets its
    // device before it claims the canvas: a browser that exposes WebGPU but
    // refuses the adapter or device leaves the canvas free for WebGL2.
    if attempt.backend == GpuBackend::WebGpu {
        let (adapter, _, device, queue) =
            crate::adapter::adapter_and_device(&instance, attempt, None).await?;
        let first = surface(&instance)?;
        return Ok(Answered {
            instance,
            adapter,
            device,
            queue,
            first,
            attempt,
        });
    }
    let first = surface(&instance)?;
    let (adapter, _, device, queue) =
        crate::adapter::adapter_and_device(&instance, attempt, first.as_ref()).await?;
    Ok(Answered {
        instance,
        adapter,
        device,
        queue,
        first,
        attempt,
    })
}
