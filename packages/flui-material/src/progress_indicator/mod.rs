//! Material 3 progress indicators: [`LinearProgressIndicator`],
//! [`CircularProgressIndicator`], and their Material 3 Expressive wavy variants
//! [`LinearWavyProgressIndicator`] and [`CircularWavyProgressIndicator`].
//!
//! Every variant is determinate with a value and indeterminate without one, and they share
//! three rules:
//!
//! - **One value policy.** A determinate value is clamped into `[0, 1]`; a non-finite one
//!   keeps showing the last finite value (0 before the first), so the drawn arc or bar and the
//!   announced percentage are always finite.
//! - **A gap and a stop.** A 4 dp gap separates the active indicator from its track, and a
//!   linear track ends in a 4 dp stop dot that marks where the work is complete.
//! - **Motion is never the only signal.** Assistive technology sees a progress bar: with its
//!   percentage and a 0–100 range when determinate, without a value (unknown-total, busy work)
//!   when indeterminate. Without a ticking [`VsyncScope`](flui_sdk::widgets::animated::VsyncScope),
//!   under a disabled `TickerMode`, or with reduced motion, an indeterminate indicator draws a
//!   still frame that reads as activity, never as a percentage, and a wave lies flat.

mod circular;
mod linear;
mod wavy;

pub use circular::{CircularProgressIndicator, CircularProgressIndicatorState};
pub use linear::{LinearProgressIndicator, LinearProgressIndicatorState};
pub use wavy::{
    CircularWavyProgressIndicator, CircularWavyProgressIndicatorState, LinearWavyProgressIndicator,
    LinearWavyProgressIndicatorState,
};

use flui_sdk::animation::MotionPolicy;
use flui_sdk::geometry::Rect;
use flui_sdk::painting::{Canvas, Color, Paint};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, NumericRange, Semantics, SemanticsRole, TickerMode};

/// The stroke of every indicator, and the height of a flat linear one: 4 dp.
const STROKE: f64 = 4.0;
/// The space between the active indicator and its track: 4 dp.
const GAP: f64 = 4.0;
/// The diameter of a linear track's stop dot: 4 dp.
const STOP: f64 = 4.0;

/// The fraction a determinate indicator shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Fraction(f64);

impl Fraction {
    fn new(value: Option<f64>) -> Self {
        let mut fraction = Self::default();
        fraction.update(value);
        fraction
    }

    /// Takes `value` clamped into `[0, 1]`; a non-finite value or `None` keeps the last one.
    fn update(&mut self, value: Option<f64>) {
        if let Some(value) = value
            && value.is_finite()
        {
            // Both zeros store +0, so semantics never reads "-0%".
            self.0 = if value <= 0.0 { 0.0 } else { value.min(1.0) };
        }
    }

    fn get(self) -> f64 {
        self.0
    }
}

/// The progress bar assistive technology sees: the percentage and its range when the
/// indicator is determinate, no value when it is not.
fn progress_semantics(label: &str, fraction: Option<f64>) -> Semantics {
    let semantics = Semantics::new()
        .label(label.to_owned())
        .role(SemanticsRole::ProgressBar);
    let Some(fraction) = fraction else {
        return semantics;
    };
    let percent = (fraction * 100.0).round();
    let semantics = semantics.value(format!("{percent}%"));
    match NumericRange::new(percent, 0.0, 100.0, 1.0) {
        Ok(range) => semantics.numeric_range(range),
        Err(_) => semantics,
    }
}

/// Whether an indicator at `ctx` may move: its ticker is enabled and motion is not reduced.
fn moves(ctx: &dyn BuildContext) -> bool {
    TickerMode::is_enabled(ctx) && MediaQuery::motion_of(ctx) != Some(MotionPolicy::Reduce)
}

/// The track left around `spans` (fractions of the length, in order): `[0, 1]` without each
/// span and a gap of `gap` either side of it, the gap never wider than the span itself, so a
/// sliver of progress does not open a hole wider than itself.
fn track_around(spans: &[(f64, f64)], gap: f64) -> Vec<(f64, f64)> {
    let mut pieces = Vec::with_capacity(spans.len() + 1);
    let mut from = 0.0;
    for &(start, end) in spans {
        if end <= start {
            continue;
        }
        let gap = gap.min(end - start);
        if start - gap > from {
            pieces.push((from, start - gap));
        }
        from = f64::max(from, end + gap);
    }
    if from < 1.0 {
        pieces.push((from, 1.0));
    }
    pieces
}

/// A linear indicator's flat parts: the track pieces around `spans` in `track`, each `height`
/// tall and centred, and, for a determinate indicator, the stop dot at the track's end.
///
/// `rtl` mirrors every span about the centre, so progress starts at the right edge.
fn paint_linear_track(
    canvas: &mut Canvas,
    width: f64,
    height: f64,
    spans: &[(f64, f64)],
    stop: Option<Color>,
    track: Color,
    rtl: bool,
) {
    let gap = GAP / width;
    let pieces = track_around(spans, gap);
    let top = (height - STROKE.min(height)) / 2.0;
    let thickness = STROKE.min(height);
    for &(start, end) in &pieces {
        let left = if rtl { 1.0 - end } else { start };
        canvas.draw_rect(
            Rect::from_ltwh(left * width, top, (end - start) * width, thickness),
            &Paint::fill(track),
        );
    }
    // The stop marks the end of the work, on the track; once the active indicator reaches
    // it there is no track left to mark.
    if let Some(color) = stop
        && pieces
            .last()
            .is_some_and(|&(start, end)| end >= 1.0 && start * width <= width - STOP)
    {
        let x = if rtl { STOP / 2.0 } else { width - STOP / 2.0 };
        canvas.draw_circle(
            flui_sdk::geometry::Point::new(x, height / 2.0),
            STOP / 2.0,
            &Paint::fill(color),
        );
    }
}
