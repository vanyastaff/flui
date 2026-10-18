//! The headless platform's injectable displays.
//!
//! A test names the displays it wants ([`DisplaySpec`]), changes one the way
//! an operating system would ([`DisplayChange`]) and moves a window between
//! them; everything flows through the same [`DisplayRegistry`] and
//! [`DisplaySnapshot`] a native source uses, so identity, revision and
//! delivery behave as they do on a real platform.

use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplayRevisionExhausted, DisplaySnapshot,
};

/// One display a [`HeadlessPlatform`](super::HeadlessPlatform) is built with.
///
/// Displays are laid out left to right in the order given; the first is the
/// primary display.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySpec {
    name: String,
    size: Size<i32>,
    scale_factor: f64,
    refresh_period: Option<Duration>,
}

impl DisplaySpec {
    /// A display of `width` by `height` device pixels at scale 1 whose
    /// refresh period is unknown.
    #[must_use]
    pub fn new(name: impl Into<String>, width: i32, height: i32) -> Self {
        Self {
            name: name.into(),
            size: Size::new(width, height),
            scale_factor: 1.0,
            refresh_period: None,
        }
    }

    /// The display's device pixels per logical pixel.
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// The period of the mode the display runs. One outside 1 ms to 1 s is
    /// recorded as unknown, as a native source would.
    #[must_use]
    pub fn with_refresh_period(mut self, period: Duration) -> Self {
        self.refresh_period = Some(period);
        self
    }
}

impl Default for DisplaySpec {
    /// The single display a platform without `with_displays` has: 1920 by
    /// 1080 at scale 1, refresh period unknown.
    fn default() -> Self {
        Self::new("Headless Display", 1920, 1080)
    }
}

/// A change to one display, as an operating system would report it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DisplayChange {
    /// The display switched to a mode with this refresh period; `None` is a
    /// mode the OS reports no rate for.
    RefreshPeriod(Option<Duration>),
    /// The display's scale factor changed.
    ScaleFactor(f64),
    /// The display was renamed. A field no window pacing reads.
    Name(String),
}

/// Why a simulated display change or move was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DisplayChangeError {
    /// The platform has no display with this id.
    #[error("the platform has no display with this id")]
    UnknownDisplay,
    /// The snapshot revision space is used up and was not wrapped.
    #[error(transparent)]
    RevisionExhausted(#[from] DisplayRevisionExhausted),
}

/// The platform's registry and the snapshot it last published.
#[derive(Debug)]
pub(super) struct HeadlessDisplays {
    snapshot: DisplaySnapshot,
}

impl HeadlessDisplays {
    pub(super) fn new(specs: Vec<DisplaySpec>) -> Self {
        let registry = DisplayRegistry::new();
        let mut next_x = 0;
        let displays = specs
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let id = registry
                    .mint(&DisplayKey::Injected(index as u64))
                    .expect("BUG: a fresh registry has ids for the displays a test names");
                let bounds = Bounds::new(Point::new(next_x, 0), spec.size);
                next_x = next_x.saturating_add(spec.size.width);
                let mut info = DisplayInfo::new(id, spec.name, bounds, spec.scale_factor)
                    .with_primary(index == 0);
                if let Some(period) = spec.refresh_period {
                    info = info.with_refresh_period(period);
                }
                info
            })
            .collect();
        let snapshot = DisplaySnapshot::empty()
            .successor(displays)
            .expect("BUG: revision 0 has a successor");
        Self { snapshot }
    }

    pub(super) fn snapshot(&self) -> DisplaySnapshot {
        self.snapshot.clone()
    }

    pub(super) fn contains(&self, id: DisplayId) -> bool {
        self.snapshot.get(id).is_some()
    }

    /// The display a new window opens on.
    pub(super) fn default_window_display(&self) -> Option<DisplayId> {
        self.snapshot
            .primary()
            .or_else(|| self.snapshot.displays().first())
            .map(|display| display.id)
    }

    pub(super) fn apply(
        &mut self,
        id: DisplayId,
        change: DisplayChange,
    ) -> Result<(), DisplayChangeError> {
        let mut displays = self.snapshot.displays().to_vec();
        let display = displays
            .iter_mut()
            .find(|display| display.id == id)
            .ok_or(DisplayChangeError::UnknownDisplay)?;
        match change {
            DisplayChange::RefreshPeriod(period) => {
                display.refresh_period = period
                    .and_then(|period| flui_platform_api::RefreshPeriod::try_from(period).ok());
            }
            DisplayChange::ScaleFactor(scale_factor) => display.scale_factor = scale_factor,
            DisplayChange::Name(name) => display.name = name,
        }
        self.snapshot = self.snapshot.successor(displays)?;
        Ok(())
    }
}
