//! The application's retry of a GPU device whose rebuilds the executor gave
//! up (ADR-0305 §8, ADR-0267).
//!
//! The backend whose rebuilds exhausted the backoff may be the cause, so the
//! retry does not rebuild on the context's instance: the owner runs ADR-0244's
//! probe again in fresh instances, over a window the platform still gives a
//! surface, and sends the device it answers in a `RetryDevice`. The executor
//! adopts it and drops every window surface, which was made on the old
//! instance; the windows' next surfaces are made with the probe's factory.

use std::sync::Arc;

use flui_engine::{
    DeviceHealth, EngineError, GpuProbe, RasterCommand, RasterSendError, SendReceipt,
};
use flui_platform::SurfaceTarget;

use super::{Placed, RasterFactory};

/// Probe for a device in fresh instances and send it to `placed`'s executor,
/// with `send`, when the executor gave its device up; answers whether it
/// sent one. From that send on, the windows' surfaces are made with the
/// probe's factory. A device that renders, or whose rebuild is still paced,
/// is left to the executor: nothing is probed.
///
/// The probe blocks the owner thread for as long as the drivers take; it
/// logs how long that was. A probe that answers no device is logged, and
/// the device stays given up for the application's next retry.
pub(super) fn retry_given_up(
    factory: &RasterFactory,
    placed: &mut Placed,
    send: impl FnOnce(&mut Placed, RasterCommand) -> Result<SendReceipt, RasterSendError>,
) -> bool {
    if !matches!(placed.sender.device_health(), DeviceHealth::GivenUp { .. }) {
        return false;
    }
    let over = placed
        .slots
        .values()
        .find(|slot| slot.available)
        .map(|slot| Arc::clone(&slot.surface_target));
    let GpuProbe {
        device,
        factory: surfaces,
        first,
        ..
    } = match probe(factory, over) {
        Ok(probe) => probe,
        Err(error) => {
            tracing::warn!(%error, "raster host: no GPU device answered the retry");
            return false;
        }
    };
    // The window's surface is made again by its restore, once the executor
    // adopted the device and dropped the old one.
    drop(first);
    match send(placed, RasterCommand::RetryDevice { device }) {
        Ok(_) => {
            placed.surfaces = surfaces;
            true
        }
        Err(error) => {
            tracing::warn!(%error, "raster host: a device retry was refused");
            false
        }
    }
}

/// ADR-0244's probe over `first` on the backend `factory` places the
/// executor on: the platform's GPU, or wgpu's no-op backend for a stand-in.
fn probe(
    factory: &RasterFactory,
    first: Option<Arc<dyn SurfaceTarget>>,
) -> Result<GpuProbe, EngineError> {
    match factory {
        RasterFactory::Gpu => probe_gpu(first),
        #[cfg(all(
            feature = "testing",
            not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
        ))]
        RasterFactory::StandIn(_) => Ok(noop_probe()),
        #[cfg(all(test, feature = "testing"))]
        RasterFactory::Scripted(_) => Ok(noop_probe()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn probe_gpu(first: Option<Arc<dyn SurfaceTarget>>) -> Result<GpuProbe, EngineError> {
    let display =
        super::super::host::with_owner_platform(flui_native::OwnerPlatform::display_connection)
            .ok_or_else(|| super::refused("no platform owns this thread"))?;
    pollster::block_on(flui_engine::GpuContext::probe(display, first))
}

/// The web cannot wait on its owner thread: its owner retires the executor
/// and awaits a new context instead.
#[cfg(target_arch = "wasm32")]
fn probe_gpu(_first: Option<Arc<dyn SurfaceTarget>>) -> Result<GpuProbe, EngineError> {
    Err(super::refused(
        "the web replaces a lost context instead of probing on its owner thread",
    ))
}

/// What the probe answers a stand-in or the scripted slots: a device on
/// wgpu's no-op backend, which their backends drop.
#[cfg(any(
    all(test, feature = "testing"),
    all(
        feature = "testing",
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    )
))]
fn noop_probe() -> GpuProbe {
    flui_engine::testing::noop_probe(Arc::new(flui_engine::testing::FakeDisplay))
}
