//! The test kit's renderer: what a headless run's windows render with, since
//! the headless platform has no surface a GPU could present into.

use std::sync::Arc;

use flui_engine::{EngineError, FrameLatency, PresentDisposition, RasterBackend};
use flui_platform::WindowId;
use parking_lot::Mutex;

/// The renderer that stands in for the GPU in one window of a headless run,
/// as the test sees it: what it presented, and the faults the test injects.
///
/// It presents every frame it is handed. After
/// [`lose_device`](Self::lose_device) it fails every frame as a lost device
/// does, until the runner rebuilds the device.
#[derive(Clone)]
pub struct StandInRenderer {
    state: Arc<Mutex<StandInState>>,
}

#[derive(Default)]
struct StandInState {
    presented: u64,
    lost: bool,
    failing_recoveries: u32,
    recovery_attempts: u32,
    released: bool,
}

impl StandInRenderer {
    /// How many frames the window presented.
    #[must_use]
    pub fn frames_presented(&self) -> u64 {
        self.state.lock().presented
    }

    /// Lose the device, as a driver reset does: the next frame fails, and the
    /// runner rebuilds the device.
    pub fn lose_device(&self) {
        self.state.lock().lost = true;
    }

    /// Fail the next `attempts` rebuilds of a lost device.
    pub fn fail_recoveries(&self, attempts: u32) {
        self.state.lock().failing_recoveries = attempts;
    }

    /// How many times the runner tried to rebuild a lost device.
    #[must_use]
    pub fn recovery_attempts(&self) -> u32 {
        self.state.lock().recovery_attempts
    }

    /// Whether the window's raster lane dropped the renderer, as it does
    /// when the window closes.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.state.lock().released
    }
}

impl std::fmt::Debug for StandInRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("StandInRenderer")
            .field("presented", &state.presented)
            .field("lost", &state.lost)
            .field("released", &state.released)
            .finish_non_exhaustive()
    }
}

/// The stand-ins of one run, each with the window it renders.
#[derive(Clone, Default)]
pub(crate) struct StandInRenderers {
    opened: Arc<Mutex<Vec<(WindowId, StandInRenderer)>>>,
}

impl StandInRenderers {
    /// The renderer of a window that is opening.
    pub(super) fn open(&self, window: WindowId) -> StandInBackend {
        let renderer = StandInRenderer {
            state: Arc::default(),
        };
        let backend = StandInBackend {
            state: Arc::clone(&renderer.state),
            size: (0, 0),
        };
        self.opened.lock().push((window, renderer));
        backend
    }

    /// The renderer `window` was opened with, the latest when its id was
    /// used again.
    pub(crate) fn of(&self, window: WindowId) -> Option<StandInRenderer> {
        self.opened
            .lock()
            .iter()
            .rev()
            .find(|(id, _)| *id == window)
            .map(|(_, renderer)| renderer.clone())
    }
}

/// The stand-in as the raster lane owns it.
pub(super) struct StandInBackend {
    state: Arc<Mutex<StandInState>>,
    size: (u32, u32),
}

impl Drop for StandInBackend {
    fn drop(&mut self) {
        self.state.lock().released = true;
    }
}

impl RasterBackend for StandInBackend {
    fn render_scene(
        &mut self,
        _scene: &flui_layer::Scene,
    ) -> Result<PresentDisposition, EngineError> {
        let mut state = self.state.lock();
        if state.lost {
            return Err(EngineError::DeviceLost);
        }
        state.presented = state
            .presented
            .checked_add(1)
            .expect("BUG: a stand-in renderer presented u64::MAX frames");
        Ok(PresentDisposition::Presented)
    }
    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
    }
    fn is_device_lost(&self) -> bool {
        self.state.lock().lost
    }
    fn mark_dirty(&mut self, _rect: flui_foundation::geometry::Rect<f64>) {}
    fn mark_full_repaint(&mut self) {}
    fn has_damage(&self) -> bool {
        true
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

impl super::device_recovery::DeviceRecovery for StandInBackend {
    fn try_recover_device(&mut self) -> Result<(), EngineError> {
        let mut state = self.state.lock();
        state.recovery_attempts = state.recovery_attempts.saturating_add(1);
        if state.failing_recoveries > 0 {
            state.failing_recoveries -= 1;
            return Err(EngineError::DeviceLost);
        }
        state.lost = false;
        Ok(())
    }
}

impl super::surface_lifecycle::DisplayColorFollow for StandInBackend {
    fn follow_display_color(
        &mut self,
        _display: Option<flui_platform::DisplayColor>,
    ) -> Result<bool, EngineError> {
        Ok(false)
    }
}

impl super::window_renderer::WindowRenderer for StandInBackend {
    fn set_frame_latency(&mut self, _latency: FrameLatency) {}
}
