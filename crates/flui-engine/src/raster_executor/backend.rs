//! The executor's GPU-free seam (ADR-0305 §11).

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene, TextureUpdates};
use flui_platform::DisplayColor;

use super::{
    SlotId,
    command::{SlotConfig, SlotTarget},
};
use crate::{
    FrameLatency, UnconfiguredSurface,
    error::{EngineError, EngineResult},
    raster::PresentDisposition,
};

/// What the raster executor drives: the slots of one GPU context, or a
/// testing stand-in that needs no GPU.
///
/// Every method names the slot it acts on. The executor calls them one at a
/// time on its one mutator, so an implementation shares its caches between
/// slots without a lock. It is not `Send`: only a threaded placement asks
/// for that.
pub trait SlotBackend {
    /// Give `slot` its target, configured as `config` asks.
    ///
    /// # Errors
    /// The target cannot be made (an over-limit size, a format the backend
    /// does not render, a surface the adapter cannot present to); the slot is
    /// refused.
    fn attach(&mut self, slot: SlotId, target: SlotTarget, config: SlotConfig) -> EngineResult<()>;

    /// Give `slot` a target of `size`, a size of positive sides.
    ///
    /// # Errors
    /// The target cannot be made at `size`; the slot keeps its target. The
    /// executor retries a transient refusal on its next turn
    /// ([`Recoverability::Recoverable`](crate::error::Recoverability::Recoverable)),
    /// and holds any other until the owner sends another size.
    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()>;

    /// Drop the surface of window slot `slot`; its frames render nothing
    /// until a [`restore`](Self::restore). Nothing for an offscreen slot.
    fn release(&mut self, slot: SlotId);

    /// Give window slot `slot` a new surface, configured at its size.
    ///
    /// # Errors
    /// The surface cannot be used (the adapter cannot present to it, it
    /// cannot be configured); the slot stays released.
    fn restore(&mut self, slot: SlotId, surface: UnconfiguredSurface) -> EngineResult<()>;

    /// Window slot `slot`'s display now reports `color`.
    ///
    /// # Errors
    /// The surface offers no configuration for it; it keeps its old one.
    fn set_display_color(&mut self, slot: SlotId, color: DisplayColor) -> EngineResult<()>;

    /// Window slot `slot`'s swapchain holds `latency` frames from now on.
    fn set_latency(&mut self, slot: SlotId, latency: FrameLatency);

    /// Apply `updates` to `slot`'s external textures before its next frame:
    /// keep each frame as its texture's newest image, uploaded when the
    /// slot next draws, and release each retired texture. The textures are
    /// the slot's own, since a presentation mints its texture ids alone, and
    /// outlive a device rebuild; a detach drops them.
    ///
    /// # Errors
    /// The first frame refused (an empty image, one past the device's
    /// texture limit); the texture keeps its previous image, and every
    /// other frame and retirement is still applied.
    fn apply_textures(&mut self, slot: SlotId, updates: TextureUpdates) -> EngineResult<()>;

    /// Render `scene` into `slot`, repainting at least `damage`.
    ///
    /// # Errors
    /// The frame's render, submission or present error, which the executor
    /// classifies into the frame's verdict.
    fn render(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition>;

    /// Take `slot` down; a slot the backend does not hold is ignored.
    fn detach(&mut self, slot: SlotId);

    /// Give back memory the next frames can rebuild.
    fn reclaim(&mut self, level: MemoryPressureLevel);

    /// Whether the device is lost: every frame fails
    /// [`EngineError::DeviceLost`] until a
    /// [`rebuild_device`](Self::rebuild_device) succeeds.
    fn device_lost(&self) -> bool;

    /// Make a new device on the instance every surface was made on, keeping
    /// every slot's target, and configure each slot on it again (ADR-0305
    /// §8). The executor calls it only while [`device_lost`](Self::device_lost)
    /// answers `true`, paced by the context's backoff.
    ///
    /// Answers the slots the new device cannot serve, each with why: a window
    /// whose surface was dropped or failed to configure, for which the owner
    /// sends another in a
    /// [`RasterCommand::Restore`](super::RasterCommand::Restore), or a slot over
    /// the new device's size limit, which waits for a size that fits.
    ///
    /// # Errors
    /// No device could be made; the device stays lost.
    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>>;

    /// Whether [`rebuild_device`](Self::rebuild_device) can succeed where the
    /// executor runs. One that cannot (the web, whose device request is a
    /// future the owner spawns) is given up at the first due attempt instead
    /// of walking the backoff.
    fn rebuilds_inline(&self) -> bool;
}

/// A boxed backend is one, so a host that picks its backend at run time (a
/// GPU context, or a testing stand-in) keeps one executor type.
impl<B: SlotBackend + ?Sized> SlotBackend for Box<B> {
    fn attach(&mut self, slot: SlotId, target: SlotTarget, config: SlotConfig) -> EngineResult<()> {
        (**self).attach(slot, target, config)
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        (**self).resize(slot, size)
    }

    fn release(&mut self, slot: SlotId) {
        (**self).release(slot);
    }

    fn restore(&mut self, slot: SlotId, surface: UnconfiguredSurface) -> EngineResult<()> {
        (**self).restore(slot, surface)
    }

    fn set_display_color(&mut self, slot: SlotId, color: DisplayColor) -> EngineResult<()> {
        (**self).set_display_color(slot, color)
    }

    fn set_latency(&mut self, slot: SlotId, latency: FrameLatency) {
        (**self).set_latency(slot, latency);
    }

    fn apply_textures(&mut self, slot: SlotId, updates: TextureUpdates) -> EngineResult<()> {
        (**self).apply_textures(slot, updates)
    }

    fn render(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        (**self).render(slot, scene, damage)
    }

    fn detach(&mut self, slot: SlotId) {
        (**self).detach(slot);
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        (**self).reclaim(level);
    }

    fn device_lost(&self) -> bool {
        (**self).device_lost()
    }

    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        (**self).rebuild_device()
    }

    fn rebuilds_inline(&self) -> bool {
        (**self).rebuilds_inline()
    }
}
