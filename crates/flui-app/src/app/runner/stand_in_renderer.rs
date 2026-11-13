//! The test kit's renderer: the backend a headless run's raster executor is
//! placed on, since the headless platform has no surface a GPU could present
//! into. Every window is a slot on it, as every window is a slot on the one
//! GPU context of a real run (ADR-0305).

use std::sync::Arc;

use flui_engine::{
    EngineError, EngineResult, FrameLatency, PresentDisposition, SlotBackend, SlotConfig, SlotId,
    SlotTarget, UnconfiguredSurface,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::memory::MemoryPressureLevel;
use flui_layer::{DamageRegion, Scene, TextureUpdates};
use flui_platform::{DisplayColor, WindowId};
use parking_lot::Mutex;

/// The renderer that stands in for the GPU in one window of a headless run,
/// as the test sees it: what the window presented, and the faults the test
/// injects.
///
/// It presents every frame it is handed. The windows of a run share one
/// device, as they share one GPU context: after
/// [`lose_device`](Self::lose_device) every window fails its frames as a
/// lost device does, until the executor rebuilds it.
#[derive(Clone)]
pub struct StandInRenderer {
    shared: Arc<Mutex<Shared>>,
    window: WindowId,
}

#[derive(Default)]
struct Shared {
    device: Device,
    /// The window the next attached slot draws, set before its attach.
    expected: Option<WindowId>,
    windows: Vec<Window>,
}

#[derive(Default)]
struct Device {
    lost: bool,
    failing_rebuilds: u32,
    rebuild_attempts: u32,
    /// How many of the next frames, of any window, panic.
    panics_ahead: u32,
    /// How many of the next resizes, of any window, panic.
    panic_resizes: u32,
    /// Whether the next slot's attach is refused.
    refuse_attach: bool,
    /// The largest side a slot can be sized to.
    max_side: Option<i32>,
}

struct Window {
    id: WindowId,
    slot: Option<SlotId>,
    presented: u64,
    released: bool,
}

impl Shared {
    fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().rev().find(|window| window.id == id)
    }

    fn slot(&mut self, slot: SlotId) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .rev()
            .find(|window| window.slot == Some(slot))
    }
}

impl StandInRenderer {
    /// Frames this window presented.
    #[must_use]
    pub fn frames_presented(&self) -> u64 {
        self.shared
            .lock()
            .window(self.window)
            .map_or(0, |window| window.presented)
    }

    /// Lose the run's device: every window's frames fail until a rebuild.
    pub fn lose_device(&self) {
        self.shared.lock().device.lost = true;
    }

    /// Fail the next `attempts` rebuilds of the device.
    pub fn fail_recoveries(&self, attempts: u32) {
        self.shared.lock().device.failing_rebuilds = attempts;
    }

    /// Make the next `frames` frames, of any window of the run, panic in the
    /// renderer, as a driver or wgpu's error handler can.
    pub fn panic_in_next_frames(&self, frames: u32) {
        self.shared.lock().device.panics_ahead = frames;
    }

    /// Make the next `resizes` resizes, of any window of the run, panic in the
    /// renderer, as a driver reconfiguring a surface can.
    pub fn panic_in_next_resizes(&self, resizes: u32) {
        self.shared.lock().device.panic_resizes = resizes;
    }

    /// Refuse the next window's slot, as a GPU that cannot present to that
    /// window's surface does.
    pub fn refuse_next_window(&self) {
        self.shared.lock().device.refuse_attach = true;
    }

    /// Refuse sizes whose width or height passes `side` pixels, as a
    /// device's texture limit does.
    pub fn limit_size(&self, side: i32) {
        self.shared.lock().device.max_side = Some(side);
    }

    /// Rebuilds of the run's device attempted so far.
    #[must_use]
    pub fn recovery_attempts(&self) -> u32 {
        self.shared.lock().device.rebuild_attempts
    }

    /// Whether this window's slot was detached.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.shared
            .lock()
            .window(self.window)
            .is_some_and(|window| window.released)
    }
}

impl std::fmt::Debug for StandInRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let shared = self.shared.lock();
        let window = shared.window(self.window);
        f.debug_struct("StandInRenderer")
            .field("presented", &window.map_or(0, |window| window.presented))
            .field("lost", &shared.device.lost)
            .field("released", &window.is_some_and(|window| window.released))
            .finish_non_exhaustive()
    }
}

/// The stand-in of one run: its device, and each window it drew.
#[derive(Clone, Default)]
pub(crate) struct StandInRenderers {
    shared: Arc<Mutex<Shared>>,
}

impl StandInRenderers {
    /// The backend the run's executor is placed on.
    pub(super) fn backend(&self) -> StandInBackend {
        StandInBackend {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Name the window the next attached slot draws.
    pub(super) fn expect_window(&self, window: WindowId) {
        self.shared.lock().expected = Some(window);
    }

    /// The stand-in of `window`, once a slot drew it.
    pub(crate) fn of(&self, window: WindowId) -> Option<StandInRenderer> {
        self.shared.lock().window(window)?;
        Some(StandInRenderer {
            shared: Arc::clone(&self.shared),
            window,
        })
    }
}

/// The stand-in as the raster executor owns it.
pub(super) struct StandInBackend {
    shared: Arc<Mutex<Shared>>,
}

impl SlotBackend for StandInBackend {
    fn attach(
        &mut self,
        slot: SlotId,
        _target: SlotTarget,
        _config: SlotConfig,
    ) -> EngineResult<()> {
        let mut shared = self.shared.lock();
        let id = shared
            .expected
            .take()
            .expect("BUG: the raster host names the window before it attaches its slot");
        if std::mem::take(&mut shared.device.refuse_attach) {
            return Err(EngineError::IncompatibleSurface);
        }
        shared.windows.push(Window {
            id,
            slot: Some(slot),
            presented: 0,
            released: false,
        });
        Ok(())
    }

    fn resize(&mut self, _slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        let mut shared = self.shared.lock();
        if shared.device.panic_resizes > 0 {
            shared.device.panic_resizes -= 1;
            drop(shared);
            panic!("injected panic in a stand-in resize");
        }
        let max = shared.device.max_side;
        drop(shared);
        if max.is_some_and(|max| size.width > max || size.height > max) {
            return Err(EngineError::InvalidTargetSize {
                width: size.width.unsigned_abs(),
                height: size.height.unsigned_abs(),
            });
        }
        Ok(())
    }

    fn release(&mut self, _slot: SlotId) {}

    fn restore(&mut self, _slot: SlotId, _surface: UnconfiguredSurface) -> EngineResult<()> {
        Ok(())
    }

    fn set_display_color(&mut self, _slot: SlotId, _color: DisplayColor) -> EngineResult<()> {
        Ok(())
    }

    fn set_latency(&mut self, _slot: SlotId, _latency: FrameLatency) {}

    fn apply_textures(&mut self, _slot: SlotId, _updates: TextureUpdates) -> EngineResult<()> {
        Ok(())
    }

    fn render(
        &mut self,
        slot: SlotId,
        _scene: &Scene,
        _damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        let mut shared = self.shared.lock();
        if shared.device.lost {
            return Err(EngineError::DeviceLost);
        }
        if shared.device.panics_ahead > 0 {
            shared.device.panics_ahead -= 1;
            drop(shared);
            panic!("injected panic in a stand-in frame");
        }
        let window = shared
            .slot(slot)
            .expect("BUG: the executor renders only an attached slot");
        window.presented = window
            .presented
            .checked_add(1)
            .expect("BUG: a stand-in window presented u64::MAX frames");
        Ok(PresentDisposition::Presented)
    }

    fn detach(&mut self, slot: SlotId) {
        if let Some(window) = self.shared.lock().slot(slot) {
            window.released = true;
            window.slot = None;
        }
    }

    fn reclaim(&mut self, _level: MemoryPressureLevel) {}

    fn device_lost(&self) -> bool {
        self.shared.lock().device.lost
    }

    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        let device = &mut self.shared.lock().device;
        device.rebuild_attempts = device.rebuild_attempts.saturating_add(1);
        if device.failing_rebuilds > 0 {
            device.failing_rebuilds -= 1;
            return Err(EngineError::DeviceLost);
        }
        device.lost = false;
        Ok(Vec::new())
    }

    fn rebuilds_inline(&self) -> bool {
        true
    }
}
