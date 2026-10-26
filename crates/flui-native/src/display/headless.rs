//! The headless platform's injectable displays.
//!
//! A test names the displays it wants ([`DisplaySpec`]), changes one the way
//! an operating system would ([`DisplayChange`], through
//! [`HeadlessDisplayControls`]) and moves a window between them; everything
//! flows through the same [`DisplayRegistry`] and [`DisplaySnapshot`] a native
//! source uses, so identity, revision and delivery behave as they do on a real
//! platform. The loop holds the snapshot itself: it is all the double keeps.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplayRevisionExhausted, DisplaySnapshot,
};
use parking_lot::Mutex;

use crate::host::Host;
use crate::owner::headless::{HeadlessLoop, HeadlessState};
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;

/// One display a [`HeadlessPlatform`](crate::HeadlessPlatform) is built with.
///
/// Displays are laid out left to right in the order given; the first is the
/// primary display.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySpec {
    name: String,
    size: Size<i32>,
    scale_factor: f64,
    refresh_period: Option<Duration>,
    persistent_key: Option<String>,
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
            persistent_key: None,
        }
    }

    /// The key that names this display again in another run, as
    /// [`DisplayInfo::persistent_key`] reports it.
    #[must_use]
    pub fn with_persistent_key(mut self, key: impl Into<String>) -> Self {
        self.persistent_key = Some(key.into());
        self
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
                DisplayInfo::new(id, spec.name, bounds, spec.scale_factor).with_primary(index == 0);
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
        DisplayChange::ScaleFactor(scale_factor) => display.scale_factor = scale_factor,
        DisplayChange::Name(name) => display.name = name,
    }
    *snapshot = snapshot.successor(displays)?;
    Ok(())
}

/// What the headless platform offers for its displays beyond [`Platform`](crate::Platform).
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Replace the displays this platform reports with `displays`, laid out
    /// left to right; the first is primary. Windows opened afterwards start on
    /// the primary display.
    #[must_use]
    pub fn with_displays(self, displays: Vec<DisplaySpec>) -> Self {
        self.owner()
            .with_state(|state| state.displays = laid_out(displays));
        self
    }

    /// Obtain the display controls before `run` consumes this platform: the
    /// handle changes a display the way an operating system would, from any
    /// thread. See [`HeadlessDisplayControls`].
    #[must_use]
    pub fn display_controls(&self) -> HeadlessDisplayControls {
        HeadlessDisplayControls {
            signal: Arc::downgrade(&self.owner().signal),
            state: Arc::downgrade(&self.owner().state),
        }
    }
}

/// Changes the headless platform's displays after `run` has consumed it —
/// obtained from [`HeadlessPlatform::display_controls`](crate::HeadlessPlatform::display_controls) before `run`, the same
/// take-a-handle-first shape as [`HeadlessPlatform::owner_turns`](crate::HeadlessPlatform::owner_turns).
///
/// A change commits to the platform's snapshot and then wakes the owner, as a
/// native source does when the OS reports a mode switch, and so works from any
/// thread: the owner re-samples `Platform::display_snapshot` on its next turn.
#[derive(Clone)]
pub struct HeadlessDisplayControls {
    signal: Weak<OwnerSignal>,
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessDisplayControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDisplayControls")
            .finish_non_exhaustive()
    }
}

impl HeadlessDisplayControls {
    /// The platform's current snapshot, `None` once the platform is gone.
    #[must_use]
    pub fn snapshot(&self) -> Option<DisplaySnapshot> {
        let state = self.state.upgrade()?;
        let snapshot = state.lock().displays.clone();
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
        let Some(state) = self.state.upgrade() else {
            return Ok(());
        };
        apply(&mut state.lock().displays, id, change)?;
        // A failed wake leaves the signal's pending debt for the next turn.
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
        Ok(())
    }
}
