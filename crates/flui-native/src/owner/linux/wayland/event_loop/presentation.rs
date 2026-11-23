//! `wp_presentation` and the clock the compositor stamps presentation times on.

use std::sync::Arc;
use std::time::Duration;

use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::presentation_time::client::wp_presentation::{self, WpPresentation};

use super::WaylandState;

/// `wp_presentation`, through which a surface asks when each commit is
/// shown, and the clock the compositor stamps those times on.
#[derive(Clone)]
pub(crate) struct PresentationTime {
    /// The global, which makes a feedback object per commit.
    pub(crate) manager: WpPresentation,
    /// The clock its `presented` events are stamped on.
    pub(crate) clock: PresentationClock,
}

/// The clock a compositor stamps presentation times on, as its
/// `wp_presentation.clock_id` named it (`CLOCK_MONOTONIC` on every
/// compositor known); unknown until it has.
#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationClock(Arc<std::sync::OnceLock<u32>>);

impl PresentationClock {
    /// The clock's reading now, from its epoch; `None` before the
    /// compositor named it, or for a clock this process cannot read.
    pub(crate) fn now(&self) -> Option<Duration> {
        let id = i32::try_from(*self.0.get()?).ok()?;
        let clock = rustix::time::ClockId::try_from(id).ok()?;
        let now = rustix::time::clock_gettime(clock);
        Some(Duration::new(
            u64::try_from(now.tv_sec).ok()?,
            u32::try_from(now.tv_nsec).ok()?,
        ))
    }
}

impl Dispatch<WpPresentation, PresentationClock> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpPresentation,
        event: wp_presentation::Event,
        clock: &PresentationClock,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Sent once, right after the bind.
        if let wp_presentation::Event::ClockId { clk_id } = event {
            let _ = clock.0.set(clk_id);
        }
    }
}
