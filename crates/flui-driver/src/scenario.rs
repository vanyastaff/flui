//! What a scenario declares before it runs (ADR-0238 §11): the window it is written for,
//! the OS devices it takes and for about how long, and whether it is a certification run, the one
//! kind of scenario that takes the OS path by itself.
//!
//! The window rule: a scenario fixes its window's logical size and scale, and the run refuses
//! when the app's window has another, naming both. A drop that landed at 800 by 600 did not at
//! 640 by 480, so a scenario run at another size proves nothing about the one it was written for.

use std::time::Duration;

use flui_protocol::geometry::LogicalSize;
use flui_protocol::session::Placement;

use crate::error::{Error, Result};
use crate::os_input::OsDevice;

/// How far a window's logical size may be from the declared one and still be it: a size the OS
/// rounded to whole device pixels.
const TOLERANCE: f64 = 0.5;

/// How far a window's scale may be from the declared one: the OS reports scales such as 1.25 as
/// the nearest `f64`.
const SCALE_TOLERANCE: f64 = 1e-3;

/// The window a scenario is written for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSpec {
    /// The surface's size in logical pixels.
    pub size: LogicalSize,
    /// Device pixels per logical pixel.
    pub scale: f64,
}

impl WindowSpec {
    /// A `width` by `height` logical-pixel window at `scale`.
    #[must_use]
    pub const fn new(width: f64, height: f64, scale: f64) -> Self {
        Self {
            size: LogicalSize::new(width, height),
            scale,
        }
    }

    /// Whether `placement` is this window.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] naming the expected and the actual size and scale when it is not.
    pub fn check(&self, placement: &Placement) -> Result<()> {
        let same_size = (placement.size.width - self.size.width).abs() <= TOLERANCE
            && (placement.size.height - self.size.height).abs() <= TOLERANCE;
        let same_scale = (placement.scale - self.scale).abs() <= SCALE_TOLERANCE;
        if same_size && same_scale {
            return Ok(());
        }
        Err(Error::invalid_argument(format!(
            "the scenario is written for a {} x {} window at scale {}, and the app's window {} is \
             {} x {} at scale {}",
            self.size.width,
            self.size.height,
            self.scale,
            placement.window,
            placement.size.width,
            placement.size.height,
            placement.scale
        )))
    }
}

/// What a scenario declares: see the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Scenario {
    name: String,
    window: Option<WindowSpec>,
    devices: Vec<OsDevice>,
    duration: Duration,
    certification: bool,
}

impl Scenario {
    /// The scenario `name`, declaring nothing yet.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            window: None,
            devices: Vec::new(),
            duration: Duration::ZERO,
            certification: false,
        }
    }

    /// Written for `window`.
    pub fn window(mut self, window: WindowSpec) -> Self {
        self.window = Some(window);
        self
    }

    /// Taking `devices` for about `duration` when it runs on the OS path.
    pub fn takes(
        mut self,
        devices: impl IntoIterator<Item = OsDevice>,
        duration: Duration,
    ) -> Self {
        self.devices = devices.into_iter().collect();
        self.duration = duration;
        self
    }

    /// A certification run: its input takes the OS path, announced and guarded.
    pub fn certification(mut self) -> Self {
        self.certification = true;
        self
    }

    /// The scenario's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The window it is written for.
    #[must_use]
    pub fn window_spec(&self) -> Option<WindowSpec> {
        self.window
    }

    /// The OS devices it takes.
    #[must_use]
    pub fn devices(&self) -> &[OsDevice] {
        &self.devices
    }

    /// About how long it takes them.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Whether it is a certification run.
    #[must_use]
    pub fn is_certification(&self) -> bool {
        self.certification
    }
}
