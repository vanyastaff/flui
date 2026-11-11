//! The executor's GPU-free seam (ADR-0305 §11).

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, Scene};

use super::{SlotId, command::SlotTarget};
use crate::{error::EngineResult, raster::PresentDisposition};

/// What the raster executor drives: the slots of one GPU context, or a
/// testing stand-in that needs no GPU.
///
/// Every method names the slot it acts on. The executor calls them one at a
/// time on its one mutator, so an implementation shares its caches between
/// slots without a lock. It is not `Send`: only a threaded placement asks
/// for that.
pub trait SlotBackend {
    /// Give `slot` its target.
    ///
    /// # Errors
    /// The target cannot be made (an over-limit size, a format the backend
    /// does not render); the slot is refused.
    fn attach(&mut self, slot: SlotId, target: SlotTarget) -> EngineResult<()>;

    /// Give `slot` a target of `size`, a size of positive sides.
    ///
    /// # Errors
    /// The target cannot be made at `size`; the slot keeps its target.
    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()>;

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
