//! The headless platform's injectable displays.
//!
//! A test names the displays it wants ([`DisplaySpec`]), changes one the way
//! an operating system would ([`DisplayChange`], through
//! [`HeadlessDisplayControls`]) and moves a window between them; everything
//! flows through the same [`DisplayRegistry`] and [`DisplaySnapshot`] a native
//! source uses, so identity, revision and delivery behave as they do on a real
//! platform. The loop holds the snapshot itself: it is all the double keeps.

use std::time::Duration;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{
    CapabilityAbsence, ColorGamut, DisplayColor, DisplayDescription, DisplayKey, DisplayRegistry,
    DisplaySnapshot,
};
// What a test changes the displays with, built with `scripted`.
#[cfg(feature = "scripted")]
use {
    crate::host::Host,
    crate::owner::headless::{HeadlessLoop, SimulatedDisplays},
    crate::owner::signal::OwnerSignal,
    crate::platform::HostPlatform,
    flui_platform::{DisplayId, DisplayRevisionExhausted},
    parking_lot::Mutex,
    std::sync::{Arc, Weak},
};

/// One display a [`HeadlessPlatform`](crate::testing::HeadlessPlatform) is built with.
///
/// Displays are laid out left to right in the order given; the first is the
/// primary display.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySpec {
    name: String,
    size: Size<i32>,
    scale_factor: DevicePixelRatio,
    refresh_period: Option<Duration>,
    persistent_key: Option<String>,
    color: Result<DisplayColor, CapabilityAbsence>,
}

impl DisplaySpec {
    /// A standard-range sRGB display of `width` by `height` device pixels at
    /// scale 1 whose refresh period is unknown. A side below one pixel is one
    /// pixel: every display has an area.
    #[must_use]
    pub fn new(name: impl Into<String>, width: i32, height: i32) -> Self {
        Self {
            name: name.into(),
            size: Size::new(width.max(1), height.max(1)),
            scale_factor: DevicePixelRatio::ONE,
            refresh_period: None,
            persistent_key: None,
            color: Ok(DisplayColor::Standard(ColorGamut::Srgb)),
        }
    }
}

/// What a test sets on a display it names.
#[cfg(feature = "scripted")]
impl DisplaySpec {
    /// The display's colour profile, as [`DisplayDescription::color`] reports it: a
    /// colour space and headroom, or the reason a platform would give none.
    #[must_use]
    pub fn color(mut self, color: Result<DisplayColor, CapabilityAbsence>) -> Self {
        self.color = color;
        self
    }

    /// The key that names this display again in another run, as
    /// [`DisplayDescription::persistent_key`] reports it.
    #[must_use]
    pub fn persistent_key(mut self, key: impl Into<String>) -> Self {
        self.persistent_key = Some(key.into());
        self
    }

    /// The display's device pixels per logical pixel.
    #[must_use]
    pub fn scale_factor(mut self, scale_factor: DevicePixelRatio) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// The period of the mode the display runs. One outside 1 ms to 1 s is
    /// recorded as unknown, as a native source would.
    #[must_use]
    pub fn refresh_period(mut self, period: Duration) -> Self {
        self.refresh_period = Some(period);
        self
    }
}

impl Default for DisplaySpec {
    /// The single display a platform without `with_displays` has: 1920 by
    /// 1080 at scale 1, standard-range sRGB, refresh period unknown.
    fn default() -> Self {
        Self::new("Headless Display", 1920, 1080)
    }
}

/// A change to one display, as an operating system would report it.
#[cfg(feature = "scripted")]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DisplayChange {
    /// The display switched to a mode with this refresh period; `None` is a
    /// mode the OS reports no rate for.
    RefreshPeriod(Option<Duration>),
    /// The display's scale factor changed.
    ScaleFactor(DevicePixelRatio),
    /// The display was renamed. A field no window pacing reads.
    Name(String),
    /// The display's colour profile changed: HDR switched on or off, the
    /// headroom moved, or the colour can no longer be read.
    Color(Result<DisplayColor, CapabilityAbsence>),
}

/// Why a simulated display change or move was refused.
#[cfg(feature = "scripted")]
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

/// The snapshot of `specs`, laid out left to right with the first primary: the
/// displays the headless loop holds, which a test changes through
/// [`HeadlessDisplayControls`]. The registry that issues their ids is only
/// needed while the specs are turned into displays, so it is not kept.
pub(crate) fn laid_out(specs: Vec<DisplaySpec>) -> DisplaySnapshot {
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
            let mut info =
                DisplayDescription::from_device(id, spec.name, bounds, bounds, spec.scale_factor)
                    .expect("BUG: `DisplaySpec::new` holds every side at one pixel or more")
                    .with_primary(index == 0)
                    .with_color(spec.color);
            if let Some(period) = spec.refresh_period {
                info = info.with_refresh_period(period);
            }
            if let Some(key) = spec.persistent_key {
                info = info.with_persistent_key(key);
            }
            info
        })
        .collect();
    DisplaySnapshot::empty()
        .successor(displays)
        .expect("BUG: revision 0 has a successor")
}

/// Apply `change` to display `id` of `snapshot`, as the snapshot's successor.
///
/// # Errors
/// [`DisplayChangeError::UnknownDisplay`] for an id the snapshot does not
/// hold; [`DisplayChangeError::RevisionExhausted`] when the revision cannot
/// advance.
#[cfg(feature = "scripted")]
fn apply(
    snapshot: &mut DisplaySnapshot,
    id: DisplayId,
    change: DisplayChange,
) -> Result<(), DisplayChangeError> {
    let mut displays = snapshot.displays().to_vec();
    let display = displays
        .iter_mut()
        .find(|display| display.id == id)
        .ok_or(DisplayChangeError::UnknownDisplay)?;
    match change {
        DisplayChange::RefreshPeriod(period) => {
            display.refresh_period =
                period.and_then(|period| flui_platform::RefreshPeriod::try_from(period).ok());
        }
        DisplayChange::ScaleFactor(scale_factor) => {
            // The panel keeps its device pixels; its logical bounds are
            // those pixels over the new scale.
            let bounds = display.bounds;
            // Two ratios between 1/32 and 32: the factor is positive and at
            // most 1024, and multiplying every edge by one positive factor
            // keeps the usable area inside the extent.
            let factor = display.scale_factor.get() / scale_factor.get();
            let rescaled = flui_platform::DisplayBounds::try_new(
                Size::new(
                    bounds.extent().width * factor,
                    bounds.extent().height * factor,
                ),
                bounds.usable().scale_from_origin(factor),
            )
            .expect("BUG: an area rescaled by a ratio of two display scales is still an area");
            display.bounds = rescaled;
            display.scale_factor = scale_factor;
        }
        DisplayChange::Name(name) => display.name = name,
        DisplayChange::Color(color) => display.color = color,
    }
    *snapshot = snapshot.successor(displays)?;
    Ok(())
}

/// What the headless platform offers for its displays beyond [`Platform`](crate::Platform).
#[cfg(feature = "scripted")]
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Replace the displays this platform reports with `displays`, laid out
    /// left to right; the first is primary. Windows opened afterwards start on
    /// the primary display.
    #[must_use]
    pub fn with_displays(self, displays: Vec<DisplaySpec>) -> Self {
        self.owner()
            .with_state(|state| state.displays.lock().snapshot = laid_out(displays));
        self
    }

    /// Obtain the display controls before `run` consumes this platform: the
    /// handle changes a display the way an operating system would, from any
    /// thread. See [`HeadlessDisplayControls`].
    #[must_use]
    pub fn display_controls(&self) -> HeadlessDisplayControls {
        HeadlessDisplayControls {
            signal: Arc::downgrade(&self.owner().signal),
            displays: self
                .owner()
                .with_state(|state| Arc::downgrade(&state.displays)),
        }
    }
}

/// Changes the headless platform's displays after `run` has consumed it —
/// obtained from [`HeadlessPlatform::display_controls`](crate::testing::HeadlessPlatform::display_controls) before `run`, the same
/// take-a-handle-first shape as [`HeadlessPlatform::owner_turns`](crate::testing::HeadlessPlatform::owner_turns).
///
/// A change commits to the platform's snapshot and then wakes the owner, as a
/// native source does when the OS reports a mode switch, and so works from any
/// thread: the owner re-samples `Platform::display_snapshot` on its next turn.
#[cfg(feature = "scripted")]
#[derive(Clone)]
pub struct HeadlessDisplayControls {
    signal: Weak<OwnerSignal>,
    displays: Weak<Mutex<SimulatedDisplays>>,
}

#[cfg(feature = "scripted")]
impl std::fmt::Debug for HeadlessDisplayControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDisplayControls")
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "scripted")]
impl HeadlessDisplayControls {
    /// The platform's current snapshot, `None` once the platform is gone.
    #[must_use]
    pub fn snapshot(&self) -> Option<DisplaySnapshot> {
        let displays = self.displays.upgrade()?;
        let snapshot = displays.lock().snapshot.clone();
        Some(snapshot)
    }

    /// Apply `change` to display `id` and wake the owner. A platform that is
    /// gone applies nothing.
    ///
    /// # Errors
    /// [`DisplayChangeError::UnknownDisplay`] for an id this platform does not
    /// hold; [`DisplayChangeError::RevisionExhausted`] when the snapshot
    /// revision cannot advance.
    pub fn simulate_display_change(
        &self,
        id: DisplayId,
        change: DisplayChange,
    ) -> Result<(), DisplayChangeError> {
        let Some(displays) = self.displays.upgrade() else {
            return Ok(());
        };
        apply(&mut displays.lock().snapshot, id, change)?;
        self.wake();
        Ok(())
    }

    /// Report `displays` as the system's displays, all at once, as the
    /// snapshot's successor, and wake the owner: a hot-plug, a rearrangement
    /// or a replayed read. A platform that is gone applies nothing.
    ///
    /// # Errors
    /// [`DisplayChangeError::RevisionExhausted`] when the snapshot revision
    /// cannot advance.
    pub fn replace(&self, displays: Vec<DisplayDescription>) -> Result<(), DisplayChangeError> {
        let Some(simulated) = self.displays.upgrade() else {
            return Ok(());
        };
        {
            let mut simulated = simulated.lock();
            simulated.snapshot = simulated.snapshot.successor(displays)?;
        }
        self.wake();
        Ok(())
    }

    /// Make the next read of the displays fail with `message`, as a native
    /// source that cannot complete a sample does, and wake the owner. The
    /// read after that reports the platform's displays again.
    pub fn fail_next_read(&self, message: impl Into<String>) {
        let Some(displays) = self.displays.upgrade() else {
            return;
        };
        displays.lock().failure = Some(message.into());
        self.wake();
    }

    fn wake(&self) {
        // A failed wake leaves the signal's pending debt for the next turn.
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }
}
