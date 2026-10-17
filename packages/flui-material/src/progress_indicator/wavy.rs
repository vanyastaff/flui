//! [`LinearWavyProgressIndicator`] and [`CircularWavyProgressIndicator`]: the Material 3
//! Expressive progress indicators whose active indicator is a travelling wave.
//!
//! The wave moves one wavelength a second. Its amplitude eases to zero over 500 ms when a
//! determinate indicator is at most 10 % or at least 95 % done, so the wave never crowds the
//! ends, and back when it leaves those bands. The indeterminate motion is the plain
//! variant's: the linear bars, or the circular arc of
//! [`ActivityArc`](flui_sdk::widgets::ActivityArc).

use std::any::Any;
use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::{Animation, AnimationController, DrivenController, RepeatMode, Vsync};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{Point, Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint, Path, StrokeCap, TextDirection};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{ActivityArc, CustomPaint, CustomPainter, Directionality, MediaQuery};

use super::circular::determinate_arcs;
use super::linear::{self, Bars, STILL_SPAN};
use super::{Fraction, STROKE, moves, paint_linear_track, progress_semantics};
use crate::theme::Theme;

/// The wave travels one wavelength in this time.
const WAVE_PERIOD: Duration = Duration::from_secs(1);
/// How long the amplitude takes to ease in or out.
const AMPLITUDE_TRANSITION: Duration = Duration::from_millis(500);
/// A determinate wave lies flat at or below this fraction, and at or above
/// [`FLAT_ABOVE`].
const FLAT_BELOW: f64 = 0.1;
const FLAT_ABOVE: f64 = 0.95;

/// The linear wave: 3 dp amplitude, 40 dp wavelength determinate and 20 dp indeterminate.
const LINEAR_AMPLITUDE: f64 = 3.0;
const LINEAR_WAVELENGTH: f64 = 40.0;
const LINEAR_INDETERMINATE_WAVELENGTH: f64 = 20.0;
/// The linear indicator's height: the stroke plus the wave either side of it.
const LINEAR_HEIGHT: f64 = STROKE + 2.0 * LINEAR_AMPLITUDE;

/// The circular indicator: 48 dp across, a 1.6 dp wave about 15 dp long, fitted so a whole
/// number of waves goes round the circle.
const CIRCULAR_SIDE: f64 = 48.0;
const CIRCULAR_AMPLITUDE: f64 = 1.6;
const CIRCULAR_WAVELENGTH: f64 = 15.0;

/// The amplitude a determinate indicator at `fraction` eases toward, or an indeterminate one
/// (`None`) holds, as a share of the full amplitude.
fn amplitude_target(fraction: Option<f64>) -> f64 {
    match fraction {
        Some(fraction) if fraction <= FLAT_BELOW || fraction >= FLAT_ABOVE => 0.0,
        _ => 1.0,
    }
}

/// A Material 3 Expressive linear progress indicator, 240 × 10 dp, whose active indicator is
/// a wave.
///
/// Determinate with a [`value`](Self::value), indeterminate without one, with the flat
/// indicator's track, 4 dp gap and stop dot. The wave is 3 dp high and 40 dp long (20 dp
/// indeterminate) and travels one wavelength a second; it lies flat at or below 10 % and at
/// or above 95 %, easing over 500 ms. With reduced motion it lies flat and still; without a
/// ticking scope or under a disabled `TickerMode` it stands still. Needs a [`Theme`]
/// ancestor for its colors; follows [`Directionality`] like the flat indicator.
#[derive(Debug, Clone, StatefulView)]
pub struct LinearWavyProgressIndicator {
    value: Option<f64>,
    label: String,
}

/// A Material 3 Expressive circular progress indicator, 48 dp across, whose active arc is a
/// wave.
///
/// Determinate with a [`value`](Self::value), indeterminate without one, with the flat
/// indicator's track and gaps. The wave is 1.6 dp high and about 15 dp long, fitted to a
/// whole number of waves round the circle, and travels one wavelength a second; it lies flat
/// at or below 10 % and at or above 95 %, easing over 500 ms. With reduced motion it lies flat
/// and still; without a ticking scope or under a disabled `TickerMode` it stands still. Needs
/// a [`Theme`] ancestor for its colors.
#[derive(Debug, Clone, StatefulView)]
pub struct CircularWavyProgressIndicator {
    value: Option<f64>,
    label: String,
}

macro_rules! wavy_builders {
    ($name:ident) => {
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            /// An indeterminate indicator labelled "Loading".
            #[must_use]
            pub fn new() -> Self {
                Self {
                    value: None,
                    label: "Loading".to_owned(),
                }
            }

            /// The fraction of the work done, or `None` for an indeterminate indicator. The
            /// fraction is clamped into `[0, 1]`; a non-finite value keeps showing the last
            /// finite one (0 before the first).
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
    };
}

wavy_builders!(LinearWavyProgressIndicator);
wavy_builders!(CircularWavyProgressIndicator);

/// The motion behind a wavy indicator: the travelling wave, its amplitude, and the
/// indeterminate cycle, each registered with the ambient [`Vsync`] while it runs.
struct Waves {
    /// Repeats every [`WAVE_PERIOD`]: the wave's phase.
    phase: DrivenController,
    /// The amplitude as a share of the full one, easing between 0 and 1.
    amplitude: DrivenController,
    /// The indeterminate motion's cycle, running only while indeterminate.
    cycle: DrivenController,
    vsync: Option<Vsync>,
    fraction: Fraction,
    indeterminate: bool,
    /// The amplitude last eased toward.
    target: f64,
}

impl std::fmt::Debug for Waves {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waves")
            .field("running", &self.phase.is_bound())
            .field("indeterminate", &self.indeterminate)
            .finish_non_exhaustive()
    }
}

impl Waves {
    fn new(cycle: Duration, value: Option<f64>) -> Self {
        let fraction = Fraction::new(value);
        let shown = value.map(|_| fraction.get());
        let target = amplitude_target(shown);
        let amplitude = AnimationController::builder(AMPLITUDE_TRANSITION)
            .debug_label("WavyProgressIndicator.amplitude")
            .build_on(None);
        amplitude.controller().set_value(target);
        Self {
            phase: AnimationController::builder(WAVE_PERIOD)
                .debug_label("WavyProgressIndicator.phase")
                .build_on(None),
            amplitude,
            cycle: AnimationController::builder(cycle)
                .debug_label("WavyProgressIndicator.cycle")
                .build_on(None),
            vsync: None,
            fraction,
            indeterminate: value.is_none(),
            target,
        }
    }

    /// The fraction shown, or `None` while indeterminate.
    fn shown(&self) -> Option<f64> {
        (!self.indeterminate).then(|| self.fraction.get())
    }

    fn update(&mut self, value: Option<f64>) {
        if self.indeterminate && value.is_some() {
            self.cycle.controller().set_value(0.0);
        }
        self.fraction.update(value);
        self.indeterminate = value.is_none();
        self.run();
    }

    /// Registers what should run with the current registry: the wave always, the cycle while
    /// indeterminate; and eases the amplitude toward the current target.
    fn run(&mut self) {
        let vsync = self.vsync.clone();
        for driven in [&mut self.phase, &mut self.amplitude] {
            if let Err(error) = driven.rebind(vsync.as_ref()) {
                tracing::error!(%error, "wavy progress indicator lost its frame registry");
            }
        }
        if !self.phase.controller().is_animating() {
            let _ = self.phase.controller().repeat(RepeatMode::Restart);
        }
        if self.indeterminate {
            if let Err(error) = self.cycle.rebind(vsync.as_ref()) {
                tracing::error!(%error, "wavy progress indicator lost its frame registry");
            }
            if !self.cycle.controller().is_animating() {
                let _ = self.cycle.controller().repeat(RepeatMode::Restart);
            }
        } else {
            let _ = self.cycle.controller().stop();
            let _ = self.cycle.rebind(None);
        }
        let target = amplitude_target(self.shown());
        if target != self.target || !self.amplitude.is_bound() {
            self.target = target;
            if self.amplitude.is_bound() {
                let _ = self.amplitude.controller().animate_to(target, None);
            } else {
                self.amplitude.controller().set_value(target);
            }
        }
    }

    fn bind(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.run();
    }

    /// What a painter samples: the moving controllers when the indicator may move, nothing
    /// (a still frame) when it may not, and a zero amplitude under reduced motion.
    fn sample(&self, ctx: &dyn BuildContext) -> WaveSample {
        let reduced = MediaQuery::motion_of(ctx) == Some(flui_sdk::animation::MotionPolicy::Reduce);
        let moving = self.phase.is_bound() && moves(ctx);
        WaveSample {
            phase: moving.then(|| self.phase.controller().clone()),
            amplitude: (!reduced).then(|| self.amplitude.controller().clone()),
            cycle: (moving && self.indeterminate).then(|| self.cycle.controller().clone()),
            fraction: self.shown(),
        }
    }

    fn dispose(&mut self) {
        for driven in [&mut self.phase, &mut self.amplitude, &mut self.cycle] {
            let _ = driven.controller().stop();
            let _ = driven.rebind(None);
            driven.dispose();
        }
    }
}

/// The controllers a painter reads at paint time.
#[derive(Debug)]
struct WaveSample {
    /// The wave's phase, or `None` for a still wave.
    phase: Option<AnimationController>,
    /// The amplitude share, or `None` for a flat wave.
    amplitude: Option<AnimationController>,
    /// The indeterminate cycle, or `None` for a still indeterminate frame.
    cycle: Option<AnimationController>,
    /// The determinate fraction, or `None` while indeterminate.
    fraction: Option<f64>,
}

impl WaveSample {
    fn phase(&self) -> f64 {
        self.phase.as_ref().map_or(0.0, Animation::value)
    }

    fn amplitude(&self) -> f64 {
        self.amplitude.as_ref().map_or(0.0, Animation::value)
    }

    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        // The phase runs whenever the indicator moves and ticks with the other controllers,
        // so following it repaints every moving frame.
        self.phase
            .as_ref()
            .map(|controller| std::rc::Rc::new(controller.clone()) as std::rc::Rc<dyn Listenable>)
    }
}

/// Persistent state for [`LinearWavyProgressIndicator`].
#[derive(Debug)]
pub struct LinearWavyProgressIndicatorState {
    waves: Waves,
    bars: Arc<Bars>,
}

/// Persistent state for [`CircularWavyProgressIndicator`].
#[derive(Debug)]
pub struct CircularWavyProgressIndicatorState {
    waves: Waves,
    arc: Arc<ActivityArc>,
}

impl StatefulView for LinearWavyProgressIndicator {
    type State = LinearWavyProgressIndicatorState;

    fn create_state(&self) -> Self::State {
        LinearWavyProgressIndicatorState {
            waves: Waves::new(linear::CYCLE, self.value),
            bars: Arc::new(Bars::new()),
        }
    }
}

impl StatefulView for CircularWavyProgressIndicator {
    type State = CircularWavyProgressIndicatorState;

    fn create_state(&self) -> Self::State {
        CircularWavyProgressIndicatorState {
            waves: Waves::new(ActivityArc::CYCLE, self.value),
            arc: Arc::new(ActivityArc::new()),
        }
    }
}

impl ViewState<LinearWavyProgressIndicator> for LinearWavyProgressIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.waves.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.waves.bind(ctx);
    }

    fn build(&self, view: &LinearWavyProgressIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let sample = self.waves.sample(ctx);
        let fraction = sample.fraction;
        let painter = LinearWavePainter {
            sample,
            bars: Arc::clone(&self.bars),
            active: colors.primary,
            track: colors.secondary_container,
            rtl: Directionality::maybe_of(ctx) == Some(TextDirection::Rtl),
        };
        progress_semantics(&view.label, fraction).child(
            CustomPaint::new()
                .size(Size::new(linear::WIDTH, LINEAR_HEIGHT))
                .painter(std::rc::Rc::new(painter)),
        )
    }

    fn did_update_view(
        &mut self,
        _old: &LinearWavyProgressIndicator,
        new: &LinearWavyProgressIndicator,
    ) {
        self.waves.update(new.value);
    }

    fn dispose(&mut self) {
        self.waves.dispose();
    }
}

impl ViewState<CircularWavyProgressIndicator> for CircularWavyProgressIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.waves.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.waves.bind(ctx);
    }

    fn build(&self, view: &CircularWavyProgressIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let sample = self.waves.sample(ctx);
        let fraction = sample.fraction;
        let painter = CircularWavePainter {
            sample,
            arc: Arc::clone(&self.arc),
            active: colors.primary,
            track: colors.secondary_container,
        };
        progress_semantics(&view.label, fraction).child(
            CustomPaint::new()
                .size(Size::new(CIRCULAR_SIDE, CIRCULAR_SIDE))
                .painter(std::rc::Rc::new(painter)),
        )
    }

    fn did_update_view(
        &mut self,
        _old: &CircularWavyProgressIndicator,
        new: &CircularWavyProgressIndicator,
    ) {
        self.waves.update(new.value);
    }

    fn dispose(&mut self) {
        self.waves.dispose();
    }
}

/// Paints a linear wavy indicator: the flat track, gaps and stop dot, and a wave over each
/// active span.
#[derive(Debug)]
struct LinearWavePainter {
    sample: WaveSample,
    bars: Arc<Bars>,
    active: Color,
    track: Color,
    rtl: bool,
}

impl CustomPainter for LinearWavePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let (width, height) = (size.width, size.height);
        if !width.is_finite() || !height.is_finite() || width <= 0.0 {
            return;
        }
        let (spans, stop, wavelength) = match (self.sample.fraction, &self.sample.cycle) {
            (Some(fraction), _) => (vec![(0.0, fraction)], Some(self.active), LINEAR_WAVELENGTH),
            (None, Some(cycle)) => (
                self.bars.spans(cycle.value()).to_vec(),
                None,
                LINEAR_INDETERMINATE_WAVELENGTH,
            ),
            (None, None) => (vec![STILL_SPAN], None, LINEAR_INDETERMINATE_WAVELENGTH),
        };
        paint_linear_track(canvas, width, height, &spans, stop, self.track, self.rtl);
        let amplitude = LINEAR_AMPLITUDE.min(height / 2.0) * self.sample.amplitude();
        let phase = self.sample.phase();
        let paint = Paint::stroke(self.active, STROKE).with_stroke_cap(StrokeCap::Butt);
        for (from, to) in spans {
            if to <= from {
                continue;
            }
            let (from, to) = (from * width, to * width);
            // About one sample a pixel, which keeps the polyline within a hundredth of a pixel
            // of the sine for these amplitudes and wavelengths.
            let steps = ((to - from).ceil() as usize).max(1);
            let mut path = Path::new();
            for i in 0..=steps {
                let x = from + (to - from) * (i as f64 / steps as f64);
                let y = height / 2.0 + amplitude * (TAU * (x / wavelength - phase)).sin();
                let point = Point::new(if self.rtl { width - x } else { x }, y);
                if i == 0 {
                    path.move_to(point);
                } else {
                    path.line_to(point);
                }
            }
            canvas.draw_path(&path, &paint);
        }
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }

    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        self.sample.repaint()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Paints a circular wavy indicator: for a determinate one the flat track with its gaps and a
/// wavy active arc, for an indeterminate one the wavy arc of the indeterminate motion.
#[derive(Debug)]
struct CircularWavePainter {
    sample: WaveSample,
    arc: Arc<ActivityArc>,
    active: Color,
    track: Color,
}

impl CustomPainter for CircularWavePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        if !side.is_finite() || side <= STROKE + 2.0 * CIRCULAR_AMPLITUDE {
            return;
        }
        let center = Point::new(size.width / 2.0, size.height / 2.0);
        let radius = (side - STROKE) / 2.0 - CIRCULAR_AMPLITUDE;
        let stroke = |color| Paint::stroke(color, STROKE).with_stroke_cap(StrokeCap::Round);
        let (start, sweep) = match (self.sample.fraction, &self.sample.cycle) {
            (Some(fraction), _) => {
                let [active, (track_start, track_sweep)] = determinate_arcs(fraction, radius);
                if track_sweep > 0.0 {
                    let rect = Rect::from_ltwh(
                        center.x - radius,
                        center.y - radius,
                        2.0 * radius,
                        2.0 * radius,
                    );
                    canvas.draw_arc(rect, track_start, track_sweep, false, &stroke(self.track));
                }
                active
            }
            (None, Some(cycle)) => self.arc.at(cycle.value()),
            (None, None) => self.arc.at(0.0),
        };
        if sweep <= 0.0 {
            return;
        }
        // A whole number of waves round the circle, so the wave is continuous where it meets
        // itself and travels without a seam.
        let waves = (TAU * radius / CIRCULAR_WAVELENGTH).round().max(1.0);
        let amplitude = CIRCULAR_AMPLITUDE * self.sample.amplitude();
        let phase = self.sample.phase();
        let steps = ((sweep * radius).ceil() as usize).max(1);
        let mut path = Path::new();
        for i in 0..=steps {
            let angle = start + sweep * (i as f64 / steps as f64);
            let r = radius + amplitude * (waves * angle - TAU * phase).sin();
            let point = Point::new(center.x + r * angle.cos(), center.y + r * angle.sin());
            if i == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }
        canvas.draw_path(&path, &stroke(self.active));
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }

    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        self.sample.repaint()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
