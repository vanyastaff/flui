//! The display a presentation's window is on, and the refresh period every
//! reader paces by (ADR-0236).
//!
//! The host derives a window's [`WindowDisplay`] from the platform's display
//! snapshot and delivers it as a window observation. The presentation stores
//! the last one it applied. Everything that depends on the display's refresh
//! period reads it from here, as one value:
//!
//! - the desktop pacing fallback re-arms from [`UiRuntime::refresh_interval`];
//! - the scheduler's frame budget is set to the shortest period among the
//!   runtime's presentations before each frame begins, so jank is judged
//!   against the display being produced for (and the `frame` span carries it
//!   to the profiler);
//! - the pointer resampler's interpolation window is the presentation's period
//!   when [`PointerResampling::FrameAligned`](crate::presentation::PointerResampling)
//!   is on;
//! - [`UiRuntime::scaled_refresh_interval`] is the interval metric that
//!   haptic frame deadlines multiply (ADR-0212).
//!
//! A display that reports no period is not a 60 Hz display, but the readers
//! need some cadence: they use [`DEFAULT_REFRESH_PERIOD`], a policy of this
//! crate, never a value the snapshot carries.

use std::time::Duration;

use flui_foundation::PresentationId;
use flui_interaction::processing::SamplingClock;
use flui_platform_api::WindowDisplay;

use super::UiRuntime;
use crate::presentation::PresentationState;

/// The refresh period readers assume while the display's is unknown: one 60 Hz
/// frame. A guess that is too short costs wakes on the most common displays; one
/// that is too long costs at most one period of latency after idle.
pub const DEFAULT_REFRESH_PERIOD: Duration = Duration::from_micros(16_667);

/// The period a presentation on `display` paces by.
fn interval_of(display: Option<WindowDisplay>) -> Duration {
    display.and_then(|display| display.refresh_period).map_or(
        DEFAULT_REFRESH_PERIOD,
        flui_platform_api::RefreshPeriod::get,
    )
}

/// `period × factor`, or `None` when the product is not representable. The
/// product runs through `Duration::try_from_secs_f64`, which refuses a negative,
/// non-finite or overflowing result, so no intermediate value panics or wraps.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read by the haptics frame deadline, which is pending"
    )
)]
fn scale(period: Duration, factor: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(period.as_secs_f64() * factor).ok()
}

impl UiRuntime {
    /// The display presentation `id`'s window is on, as the host last delivered
    /// it: `None` while the window names no display, one the platform's snapshot
    /// does not hold, or the presentation is not hosted here.
    #[must_use]
    pub fn window_display(&self, id: PresentationId) -> Option<WindowDisplay> {
        self.presentations.get(id)?.window_display.get()
    }

    /// The refresh interval presentation `id` paces by: its display's period,
    /// or [`DEFAULT_REFRESH_PERIOD`] while that is unknown. It follows the
    /// display, so a window moved to a display that reports nothing returns to
    /// the default rather than keeping the previous display's period.
    #[must_use]
    pub fn refresh_interval(&self, id: PresentationId) -> Duration {
        interval_of(self.window_display(id))
    }

    /// `factor ×` the refresh interval of presentation `id`: the display-bound
    /// deadline metric of ADR-0212, recomputed from the current display on every
    /// call. Crate-private until its consumer (the haptics frame deadline) lands.
    ///
    /// `None` when `factor` is not finite and positive, or when the product is
    /// not representable even at [`DEFAULT_REFRESH_PERIOD`]. A product that
    /// overflows for the display's own period is treated as that period being
    /// unknown, with a diagnostic, and the default period applies instead.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by the haptics frame deadline, which is pending"
        )
    )]
    pub(crate) fn scaled_refresh_interval(
        &self,
        id: PresentationId,
        factor: f64,
    ) -> Option<Duration> {
        if !factor.is_finite() || factor <= 0.0 {
            tracing::warn!(
                factor,
                "refresh-interval factor is not a finite positive number"
            );
            return None;
        }
        let period = self.refresh_interval(id);
        scale(period, factor).or_else(|| {
            tracing::warn!(
                factor,
                ?period,
                "refresh interval times factor overflows; treating the period as unknown"
            );
            scale(DEFAULT_REFRESH_PERIOD, factor)
        })
    }

    /// The shortest refresh interval among this runtime's presentations: the
    /// strictest cadence any window being produced for runs at. A window on a
    /// slower display beside a faster one is judged against the faster, so its
    /// jank statistics are pessimistic, never optimistic.
    fn shortest_refresh_interval(&self) -> Duration {
        self.presentations
            .iter()
            .map(|presentation| interval_of(presentation.window_display.get()))
            .min()
            .unwrap_or(DEFAULT_REFRESH_PERIOD)
    }

    /// Point the scheduler's frame budget at the display being produced for.
    /// Runs before the frame begins: the frame copies the budget's target when
    /// it starts, so setting it afterwards would judge this frame by the last.
    pub(crate) fn publish_frame_budget(&self) {
        match flui_scheduler::FrameDuration::try_from_period(self.shortest_refresh_interval()) {
            Ok(duration) => self.scheduler.set_budget_frame_duration(duration),
            Err(error) => {
                tracing::warn!(%error, "no usable refresh interval; keeping the frame budget");
            }
        }
    }

    /// Apply the display the host derived for presentation `id`'s window.
    /// An unchanged value does nothing, so a host may deliver its derivation as
    /// often as it likes.
    ///
    /// A change moves everything that follows the display, then stores the value
    /// last: if any step fails part-way, the presentation still holds the
    /// previous value and the host's next delivery, even of an equal value,
    /// applies it again. The steps are idempotent.
    pub(crate) fn apply_window_display(&self, id: PresentationId, display: Option<WindowDisplay>) {
        let Some(presentation) = self.presentations.get(id) else {
            return;
        };
        if presentation.window_display.get() == display {
            return;
        }
        Self::follow_display_period(presentation, interval_of(display));
        // A new display can answer different gesture geometry: re-read it
        // through the barrier a pixel-ratio change uses.
        self.reread_gesture_geometry_for(id);
        self.request_redraw_for(presentation);
        presentation.window_display.set(display);
    }

    /// The pointer resampler's frame-aligned window is one display period, so
    /// it follows the display instead of the 60 Hz the binding starts with.
    fn follow_display_period(presentation: &PresentationState, period: Duration) {
        presentation
            .gestures()
            .set_sampling_clock(SamplingClock::Fixed { period });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform_api::{DisplayKey, DisplayRegistry, RefreshPeriod, WindowDisplay};

    use super::*;

    /// A runtime whose primary presentation is on a display of `micros`
    /// microseconds, delivered the way the host does.
    fn runtime_on(micros: u64) -> (UiRuntime, PresentationId) {
        let runtime = UiRuntime::for_test();
        let id = runtime.presentation_id();
        let display = WindowDisplay::new(
            DisplayRegistry::new()
                .mint(&DisplayKey::Injected(1))
                .expect("id"),
            Some(RefreshPeriod::try_from(Duration::from_micros(micros)).expect("period")),
        );
        runtime.apply_window_display(id, Some(display));
        (runtime, id)
    }

    #[test]
    fn refresh_interval_metric_recomputes_on_display_change() {
        let (runtime, id) = runtime_on(16_000);
        assert_eq!(
            runtime.scaled_refresh_interval(id, 2.0),
            Some(Duration::from_millis(32))
        );
        let faster = WindowDisplay::new(
            DisplayRegistry::new()
                .mint(&DisplayKey::Injected(2))
                .expect("id"),
            Some(RefreshPeriod::try_from(Duration::from_millis(8)).expect("period")),
        );
        runtime.apply_window_display(id, Some(faster));
        assert_eq!(
            runtime.scaled_refresh_interval(id, 2.0),
            Some(Duration::from_millis(16)),
            "the metric kept the previous display's interval"
        );
    }

    #[test]
    fn period_product_overflow_is_treated_as_unknown() {
        // The longest accepted period times 2e19 does not fit a `Duration`; the
        // default period times it does, so the metric falls back instead of failing.
        let (runtime, id) = runtime_on(1_000_000);
        let fallback = runtime
            .scaled_refresh_interval(id, 2.0e19)
            .expect("the default period fits");
        assert!(
            fallback > Duration::from_secs(3 * 10_u64.pow(16))
                && fallback < Duration::from_secs(4 * 10_u64.pow(17)),
            "expected 2e19 x 16.667 ms, got {fallback:?}"
        );
        // A factor nothing can hold is unknown outright.
        assert_eq!(runtime.scaled_refresh_interval(id, 1.0e300), None);
    }

    #[test]
    fn non_finite_factor_is_treated_as_unknown() {
        let (runtime, id) = runtime_on(8_333);
        for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert_eq!(
                runtime.scaled_refresh_interval(id, factor),
                None,
                "{factor}"
            );
        }
    }
}
