//! The executor's GPU-free seam (ADR-0305 §11).

use flui_foundation::{FrameStamp, geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene, TextureUpdates};
use flui_platform::DisplayColor;

use super::{
    DeviceAnswered, DeviceTicket, SlotId,
    command::{SlotConfig, SlotTarget},
};
use crate::{
    FrameLatency, ProbedDevice, UnconfiguredSurface,
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
pub trait RasterBackend {
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
    /// [`EngineError::InvalidTargetSize`]
    /// when the slot's size is over the device's limit: the surface is kept,
    /// unconfigured, until a resize fits. Any other error: the surface cannot
    /// be used (the adapter cannot present to it, it cannot be configured),
    /// and the slot stays released.
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

    /// Render `scene`, the frame `frame` stamps, into `slot`, repainting at
    /// least `damage`. A window slot hands `frame` to its surface target with
    /// the present
    /// ([`SurfaceTarget::pre_present_notify`](flui_platform::SurfaceTarget::pre_present_notify)),
    /// so the window's report of the frame shown names it.
    ///
    /// # Errors
    /// The frame's render, submission or present error, which the executor
    /// classifies into the frame's verdict.
    fn render(
        &mut self,
        slot: SlotId,
        frame: FrameStamp,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition>;

    /// Render `scene`, which the caller lends for this call alone, into
    /// window slot `slot` in full, with the fonts it carries: the development
    /// reload plugin's scene, drawn outside the slot's frames. `reset_fonts`
    /// drops the fonts an earlier lent scene registered.
    ///
    /// # Errors
    /// As [`render`](Self::render).
    fn render_lent(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        reset_fonts: bool,
    ) -> EngineResult<PresentDisposition>;

    /// Take `slot` down; a slot the backend does not hold is ignored.
    fn detach(&mut self, slot: SlotId);

    /// The newest image of each external texture `slot` keeps, shared rather
    /// than copied: what the owner gives the slot it attaches for the same
    /// presentation on the backend that replaces this one (the web, whose
    /// owner replaces a lost context, ADR-0305 §8). Empty for a slot the
    /// backend does not hold.
    fn kept_textures(&self, slot: SlotId) -> TextureUpdates;

    /// Give back memory the next frames can rebuild.
    fn reclaim(&mut self, level: MemoryPressureLevel);

    /// Whether the device is lost: every frame fails
    /// [`EngineError::DeviceLost`] until a requested device is installed
    /// ([`install_requested_device`](Self::install_requested_device)).
    fn device_lost(&self) -> bool;

    /// Start asking for a new device on the instance every surface was made
    /// on, off the executor's thread, under `ticket` (ADR-0305 §8). The
    /// executor calls it only while [`device_lost`](Self::device_lost)
    /// answers `true`, paced by the context's backoff, and only while it
    /// waits for no other request: a request under an earlier ticket is
    /// dropped. Once the answer can be installed, the backend calls
    /// `answered`, from whichever thread answered.
    ///
    /// # Errors
    /// No request could start (no adapter presents to the slots' surfaces,
    /// the operating system refused the worker): the attempt failed.
    fn request_device(
        &mut self,
        ticket: DeviceTicket,
        answered: DeviceAnswered,
    ) -> EngineResult<()>;

    /// Install the device the request under `ticket` answered, keeping every
    /// slot's target, and configure each slot on it again; `None` while the
    /// request is still asking. An answer under another ticket is dropped.
    ///
    /// Answers the slots the new device cannot serve, each with why: a window
    /// whose surface was dropped or failed to configure, for which the owner
    /// sends another in a
    /// [`RasterCommand::Restore`](super::RasterCommand::Restore), or a slot over
    /// the new device's size limit, which waits for a size that fits.
    ///
    /// # Errors
    /// The request answered no device, or no request under `ticket` is held:
    /// the device stays lost.
    fn install_requested_device(
        &mut self,
        ticket: DeviceTicket,
    ) -> Option<EngineResult<Vec<(SlotId, EngineError)>>>;

    /// Adopt `device`, which the owner's probe answered in an instance of
    /// its own, in place of a device whose rebuilds were given up, and
    /// configure each slot on it as
    /// [`install_requested_device`](Self::install_requested_device) does
    /// (ADR-0305 §8). Every window slot's surface was made on the
    /// instance this replaces and is dropped: the owner sends each window
    /// another, made with the probe's factory, in a
    /// [`RasterCommand::Restore`](super::RasterCommand::Restore).
    ///
    /// Answers the slots the new device cannot serve otherwise, each with why.
    fn adopt_device(&mut self, device: ProbedDevice) -> Vec<(SlotId, EngineError)>;

    /// Whether [`request_device`](Self::request_device) can be answered
    /// where the executor runs. One that cannot (the web, whose device
    /// request is a future the owner spawns) is given up at the first due
    /// attempt instead of walking the backoff.
    fn requests_devices(&self) -> bool;
}

/// A boxed backend is one, so a host that picks its backend at run time (a
/// GPU context, or a testing stand-in) keeps one executor type.
impl<B: RasterBackend + ?Sized> RasterBackend for Box<B> {
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
        frame: FrameStamp,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        (**self).render(slot, frame, scene, damage)
    }

    fn render_lent(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        reset_fonts: bool,
    ) -> EngineResult<PresentDisposition> {
        (**self).render_lent(slot, scene, reset_fonts)
    }

    fn detach(&mut self, slot: SlotId) {
        (**self).detach(slot);
    }

    fn kept_textures(&self, slot: SlotId) -> TextureUpdates {
        (**self).kept_textures(slot)
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        (**self).reclaim(level);
    }

    fn device_lost(&self) -> bool {
        (**self).device_lost()
    }

    fn request_device(
        &mut self,
        ticket: DeviceTicket,
        answered: DeviceAnswered,
    ) -> EngineResult<()> {
        (**self).request_device(ticket, answered)
    }

    fn install_requested_device(
        &mut self,
        ticket: DeviceTicket,
    ) -> Option<EngineResult<Vec<(SlotId, EngineError)>>> {
        (**self).install_requested_device(ticket)
    }

    fn adopt_device(&mut self, device: ProbedDevice) -> Vec<(SlotId, EngineError)> {
        (**self).adopt_device(device)
    }

    fn requests_devices(&self) -> bool {
        (**self).requests_devices()
    }
}
