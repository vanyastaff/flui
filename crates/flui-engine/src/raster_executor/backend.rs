//! The executor's GPU-free seam (ADR-0305 §11).

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene};
use flui_platform::DisplayColor;

use super::{
    SlotId,
    command::{SlotConfig, SlotTarget},
};
use crate::{FrameLatency, UnconfiguredSurface, error::EngineResult, raster::PresentDisposition};

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
}
