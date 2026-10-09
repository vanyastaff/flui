//! An indeterminate circular activity indicator driven by keyframe tracks.

use std::any::Any;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{
    Animatable, Animation, AnimationController, Cubic, DrivenController, Keyframes, Linear,
};
use flui_foundation::Listenable;
use flui_foundation::geometry::{Rect, Size};
use flui_painting::{Canvas, Paint, StrokeCap, styling::Color};
use flui_rendering::{delegates::CustomPainter, semantics::SemanticsRole};
use flui_view::prelude::*;

use crate::animated::VsyncScope;
use crate::{CustomPaint, Semantics};

/// One cycle of the indicator's motion.
const CYCLE: Duration = Duration::from_secs(6);
/// The indicator's side, in logical pixels.
const SIDE: f64 = 40.0;
/// The arc's stroke width, in logical pixels.
const STROKE: f64 = 4.0;
/// The arc's length as a fraction of the circle, at rest and at full sweep.
const MIN_SWEEP: f64 = 0.1;
const MAX_SWEEP: f64 = 0.87;
/// The whole arc turns this far, linearly, every cycle.
const GLOBAL_TURN_DEGREES: f64 = 1080.0;
/// Four extra quarter turns per cycle: each turns over `STEP_TURN`, then
/// holds for `STEP_HOLD`, so one starts every 1.5 s.
const STEP_TURN: Duration = Duration::from_millis(300);
const STEP_HOLD: Duration = Duration::from_millis(1200);

/// Material 3 easing: `standard` and `emphasized decelerate`.
const STANDARD: Cubic = Cubic::new(0.2, 0.0, 0.0, 1.0);
const EMPHASIZED_DECELERATE: Cubic = Cubic::new(0.05, 0.7, 0.1, 1.0);

const DEFAULT_COLOR: Color = Color::rgb(38, 102, 192);

/// An indeterminate circular activity indicator: an arc that grows, shrinks
/// and turns, repeating every six seconds.
///
/// The motion is three keyframe tracks on one repeating controller — a
/// linear turn of three revolutions, four eased quarter-turn steps, and a
/// sweep from 10 % to 87 % of the circle and back — after Material 3's
/// indeterminate circular indicator. The arc is repainted from the
/// controller each frame; the indicator never rebuilds to animate.
///
/// The controller is registered with the ambient [`VsyncScope`]; without
/// one, or under a disabled [`TickerMode`](crate::TickerMode), the indicator
/// paints the cycle's first frame. It is exposed to assistive technology as a
/// [`SemanticsRole::LoadingSpinner`] with its [`label`](Self::label).
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_widgets::ActivityIndicator;
/// let _ = ActivityIndicator::new().label("Loading messages");
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct ActivityIndicator {
    color: Color,
    label: String,
}

impl Default for ActivityIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityIndicator {
    /// An indicator in the default color, labelled "Loading".
    #[must_use]
    pub fn new() -> Self {
        Self {
            color: DEFAULT_COLOR,
            label: "Loading".to_owned(),
        }
    }

    /// The arc's color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// What assistive technology announces for the indicator.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

/// The three tracks of one cycle, shared by the painter.
#[derive(Debug)]
struct Motion {
    /// The linear turn, in degrees.
    turn: Keyframes<f64>,
    /// The stepped quarter turns, in degrees.
    steps: Keyframes<f64>,
    /// The arc's length as a fraction of the circle.
    sweep: Keyframes<f64>,
}

impl Motion {
    fn new() -> Self {
        let turn = Keyframes::builder(0.0, CYCLE)
            .to(GLOBAL_TURN_DEGREES, CYCLE, Linear)
            .build()
            .expect("BUG: one segment of the cycle's length fits the cycle");
        let mut steps = Keyframes::builder(0.0, CYCLE);
        for quarter in 1..=4_u32 {
            steps = steps
                .to(f64::from(quarter) * 90.0, STEP_TURN, EMPHASIZED_DECELERATE)
                .hold(STEP_HOLD);
        }
        let steps = steps
            .build()
            .expect("BUG: four 1.5 s steps fill the 6 s cycle exactly");
        let sweep = Keyframes::builder(MIN_SWEEP, CYCLE)
            .to(MAX_SWEEP, CYCLE / 2, STANDARD)
            .to(MIN_SWEEP, CYCLE / 2, STANDARD)
            .build()
            .expect("BUG: two half cycles fill the cycle");
        Self { turn, steps, sweep }
    }
}

/// Persistent state for [`ActivityIndicator`]: the repeating controller and
/// its registration with the ambient [`Vsync`](flui_animation::Vsync).
pub struct ActivityIndicatorState {
    controller: DrivenController,
    motion: Arc<Motion>,
}

impl std::fmt::Debug for ActivityIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivityIndicatorState")
            .field("registered", &self.controller.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for ActivityIndicator {
    type State = ActivityIndicatorState;

    fn create_state(&self) -> Self::State {
        ActivityIndicatorState {
            controller: AnimationController::builder(CYCLE).build_on(None),
            motion: Arc::new(Motion::new()),
        }
    }
}

impl ViewState<ActivityIndicator> for ActivityIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "activity indicator lost its frame registry");
        }
        if !self.controller.controller().is_animating() {
            let _ = self.controller.controller().repeat(false);
        }
    }

    fn build(&self, view: &ActivityIndicator, _ctx: &dyn BuildContext) -> impl IntoView {
        let painter = ArcPainter {
            controller: self.controller.controller().clone(),
            motion: Arc::clone(&self.motion),
            color: view.color,
        };
        Semantics::new()
            .role(SemanticsRole::LoadingSpinner)
            .label(view.label.clone())
            .child(
                CustomPaint::new()
                    .size(Size::new(SIDE, SIDE))
                    .painter(std::rc::Rc::new(painter)),
            )
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}

/// Paints the arc at the controller's current progress.
#[derive(Debug)]
struct ArcPainter {
    controller: AnimationController,
    motion: Arc<Motion>,
    color: Color,
}

impl CustomPainter for ArcPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        if !side.is_finite() || side <= STROKE {
            return;
        }
        let progress = self.controller.value();
        let degrees = self.motion.turn.transform(progress) + self.motion.steps.transform(progress);
        let sweep = self.motion.sweep.transform(progress) * TAU;
        let inset = STROKE / 2.0;
        let rect = Rect::from_ltwh(
            (size.width - side) / 2.0 + inset,
            (size.height - side) / 2.0 + inset,
            side - STROKE,
            side - STROKE,
        );
        // Angles start at 12 o'clock and run clockwise.
        let start = (degrees.rem_euclid(360.0)).to_radians() - FRAC_PI_2;
        let paint = Paint::stroke(self.color, STROKE).with_stroke_cap(StrokeCap::Round);
        canvas.draw_arc(rect, start, sweep, false, &paint);
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.color != self.color || !Arc::ptr_eq(&old.motion, &self.motion))
    }

    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        Some(std::rc::Rc::new(self.controller.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
