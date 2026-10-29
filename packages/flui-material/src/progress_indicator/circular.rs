//! [`CircularProgressIndicator`]: a Material 3 circular progress indicator.

use std::any::Any;
use std::f64::consts::{FRAC_PI_2, TAU};

use flui_sdk::geometry::{Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint, StrokeCap};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{ActivityIndicator, CustomPaint, CustomPainter};

use super::{Fraction, GAP, STROKE, progress_semantics};
use crate::theme::Theme;

/// The indicator's side: 40 dp.
const SIDE: f64 = 40.0;

/// A Material 3 circular progress indicator, 40 dp across with a 4 dp stroke.
///
/// With a [`value`](Self::value) it shows that fraction of the work as done:
/// an arc clockwise from 12 o'clock, a 4 dp gap at either end, and the rest
/// of the circle as its track. Without one it is indeterminate: an arc that
/// grows, shrinks and turns every six seconds, the motion
/// [`ActivityIndicator`] draws, here in the theme's primary color. Without a
/// ticking scope, under a disabled `TickerMode`, or with reduced motion, the
/// indeterminate arc stands still. Needs a [`Theme`] ancestor for its colors.
///
/// Assistive technology sees a progress bar with the percentage as its
/// value, or without a value while indeterminate, labelled with
/// [`label`](Self::label).
///
/// # Examples
///
/// ```rust,ignore
/// CircularProgressIndicator::new().value(Some(0.4)).label("Upload");
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct CircularProgressIndicator {
    value: Option<f64>,
    label: String,
}

impl Default for CircularProgressIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl CircularProgressIndicator {
    /// An indeterminate indicator labelled "Loading".
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: None,
            label: "Loading".to_owned(),
        }
    }

    /// The fraction of the work done, or `None` for an indeterminate
    /// indicator. The fraction is clamped into `[0, 1]`; a non-finite value
    /// keeps showing the last finite one (0 before the first).
    #[must_use]
    pub fn value(mut self, value: Option<f64>) -> Self {
        self.value = value;
        self
    }

    /// What assistive technology announces for the indicator.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

/// Persistent state for [`CircularProgressIndicator`]: the last finite fraction.
#[derive(Debug)]
pub struct CircularProgressIndicatorState {
    fraction: Fraction,
}

impl StatefulView for CircularProgressIndicator {
    type State = CircularProgressIndicatorState;

    fn create_state(&self) -> Self::State {
        CircularProgressIndicatorState {
            fraction: Fraction::new(self.value),
        }
    }
}

impl ViewState<CircularProgressIndicator> for CircularProgressIndicatorState {
    fn build(&self, view: &CircularProgressIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        if view.value.is_none() {
            return ActivityIndicator::new()
                .color(colors.primary)
                .label(view.label.clone())
                .boxed();
        }
        let fraction = self.fraction.get();
        progress_semantics(&view.label, Some(fraction))
            .child(
                CustomPaint::new()
                    .size(Size::new(SIDE, SIDE))
                    .painter(ArcPainter {
                        fraction,
                        active: colors.primary,
                        track: colors.secondary_container,
                    }),
            )
            .boxed()
    }

    fn did_update_view(
        &mut self,
        _old: &CircularProgressIndicator,
        new: &CircularProgressIndicator,
    ) {
        self.fraction.update(new.value);
    }
}

/// The arcs of a determinate circular indicator of radius `radius` (at the stroke's centre):
/// the active arc from 12 o'clock for `fraction` of the circle, and the track around the rest
/// with a gap at each end, as `(start, sweep)` angles. A sweep of zero draws nothing.
///
/// With round caps each end reaches half a stroke past its angle, so the gap is widened by a
/// stroke to keep 4 dp of space between the caps; a gap never exceeds the active arc itself.
pub(super) fn determinate_arcs(fraction: f64, radius: f64) -> [(f64, f64); 2] {
    let start = -FRAC_PI_2;
    let sweep = fraction * TAU;
    let gap = ((GAP + STROKE) / radius).min(sweep);
    let track = (TAU - sweep - 2.0 * gap).max(0.0);
    [(start, sweep), (start + sweep + gap, track)]
}

/// Paints a determinate indicator's arc and track.
#[derive(Debug)]
struct ArcPainter {
    fraction: f64,
    active: Color,
    track: Color,
}

impl CustomPainter for ArcPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        if !side.is_finite() || side <= STROKE {
            return;
        }
        let radius = (side - STROKE) / 2.0;
        let rect = Rect::from_ltwh(
            size.width / 2.0 - radius,
            size.height / 2.0 - radius,
            2.0 * radius,
            2.0 * radius,
        );
        let [(start, sweep), (track_start, track_sweep)] = determinate_arcs(self.fraction, radius);
        let stroke = |color| Paint::stroke(color, STROKE).with_stroke_cap(StrokeCap::Round);
        if track_sweep > 0.0 {
            canvas.draw_arc(rect, track_start, track_sweep, false, &stroke(self.track));
        }
        if sweep > 0.0 {
            canvas.draw_arc(rect, start, sweep, false, &stroke(self.active));
        }
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any().downcast_ref::<Self>().is_none_or(|old| {
            old.fraction != self.fraction || old.active != self.active || old.track != self.track
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
