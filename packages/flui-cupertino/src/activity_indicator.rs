//! [`CupertinoActivityIndicator`]: the iOS spinner of eight fading ticks.
//!
//! One repeating one-second controller drives every tick. A single keyframe
//! track steps through the cycle's eight phases (`Steps(8, JumpAt::End)`);
//! tick `i` reads it delayed by `Stagger(125 ms, First)`, so each tick is one
//! phase behind the tick before it. A tick's phase picks its alpha from
//! `[47, 47, 47, 47, 72, 97, 122, 147]` (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/activity_indicator.dart:139`),
//! which makes the brightest tick walk around the circle once a second. The
//! painter samples the track at the controller's progress, so the ticks move
//! by repaint, never by rebuild.

use std::any::Any;
use std::f64::consts::TAU;
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::animation::{
    Animation, AnimationController, DrivenController, JumpAt, Keyframes, RepeatMode, Stagger,
    StaggerOrigin, Steps,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{RRect, Rect, Size};
use flui_sdk::painting::paint::Paint;
use flui_sdk::painting::styling::Color;
use flui_sdk::painting::{CustomPaintContext, CustomPainter};
use flui_sdk::semantics::{NumericRange, SemanticsRole};
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{CustomPaint, Semantics};

use crate::{CupertinoColor, CupertinoDynamicColor};

/// One revolution of the brightest tick.
const CYCLE: Duration = Duration::from_secs(1);
/// The number of ticks around the circle.
const TICKS: u32 = 8;
/// Tick alpha by phase: phase 0 is the brightest tick's predecessor, phases
/// 4–7 are the fading tail.
const ALPHAS: [u8; TICKS as usize] = [47, 47, 47, 47, 72, 97, 122, 147];
/// The alpha of every tick a partially revealed indicator shows
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/activity_indicator.dart:142`).
const REVEALED_ALPHA: u8 = 147;
/// The default radius, in logical pixels.
const RADIUS: f64 = 10.0;
/// The ticks' color in light and dark mode.
const TICK_COLOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::rgb(0x3C, 0x3C, 0x44),
    Color::rgb(0xEB, 0xEB, 0xF5),
);

/// The iOS activity indicator: eight ticks around a circle, the brightest
/// one stepping clockwise once per eighth of a second.
///
/// The indicator is a square of twice its [`radius`](Self::radius), 10 by
/// default. Its ticks take [`color`](Self::color), by default the iOS tick
/// gray resolved against the ambient brightness.
///
/// Its controller is registered with the ambient [`VsyncScope`]; without one,
/// under a disabled `TickerMode`, with reduced motion, or while
/// [`animating`](Self::animating) is `false`, it paints the eight ticks at
/// rest, as at the start of a cycle, and requests no frames. Reduced motion
/// parks the repeating run without continuation frames.
///
/// [`partially_revealed`](Self::partially_revealed) shows the first fraction
/// of the ticks, clockwise from 12 o'clock, at one fixed alpha and never
/// animates: the pull-to-refresh indicator before the pull completes.
///
/// Assistive technology sees a [`SemanticsRole::ProgressBar`] labelled
/// "Loading"; a partially revealed indicator also publishes its progress as a
/// percentage.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/activity_indicator.dart:36`.
///
/// # Examples
///
/// ```rust,ignore
/// CupertinoActivityIndicator::new().radius(14.0).color(CupertinoColors::SYSTEM_RED)
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct CupertinoActivityIndicator {
    color: Option<CupertinoColor>,
    /// Finite, positive, and with a finite diameter.
    radius: f64,
    reveal: Reveal,
}

/// What the indicator shows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reveal {
    /// Every tick, cycling while `animating`.
    Spinning { animating: bool },
    /// The first `ceil(8 · progress)` ticks, still; `progress` is in `0..=1`.
    Partial { progress: f64 },
}

impl Default for CupertinoActivityIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl CupertinoActivityIndicator {
    /// A spinning indicator of the default radius and color.
    #[must_use]
    pub fn new() -> Self {
        Self {
            color: None,
            radius: RADIUS,
            reveal: Reveal::Spinning { animating: true },
        }
    }

    /// An indicator that shows `progress` of its ticks and never animates.
    ///
    /// `progress` is the fraction of the eight ticks shown, clockwise from 12
    /// o'clock: a tick shows once the fraction reaches into it, so `0.5`
    /// shows four ticks and `0.1` one. Below `1.0` every shown tick has alpha
    /// 147; at `1.0` the eight ticks show at rest, as an indicator that does
    /// not animate.
    ///
    /// Flutter asserts `0 <= progress <= 1`; here a progress above `1.0` is
    /// read as `1.0`, and one below `0.0` or NaN as `0.0` (no tick).
    #[must_use]
    pub fn partially_revealed(progress: f64) -> Self {
        // `progress > 0.0` is false for NaN and both zeros, so neither a NaN
        // nor a `-0.0` reaches paint or semantics.
        let progress = if progress > 0.0 {
            progress.min(1.0)
        } else {
            0.0
        };
        Self {
            reveal: Reveal::Partial { progress },
            ..Self::new()
        }
    }

    /// The radius of the circle the ticks stand on, in logical pixels; the
    /// indicator is a square of twice it. Defaults to 10.
    ///
    /// Flutter asserts a positive radius; here a radius that is not finite,
    /// not positive, or whose diameter overflows keeps the default 10.
    #[must_use]
    pub fn radius(mut self, radius: f64) -> Self {
        self.radius = if radius > 0.0 && (radius * 2.0).is_finite() {
            radius
        } else {
            RADIUS
        };
        self
    }

    /// The ticks' color, resolved against the ambient brightness, contrast
    /// and interface level. Each tick replaces its alpha. Defaults to the iOS
    /// tick gray.
    #[must_use]
    pub fn color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Whether the ticks cycle. Defaults to `true`; `false` paints them at
    /// rest and requests no frames. A mounted indicator starts or stops when
    /// a rebuild changes it.
    ///
    /// A [`partially_revealed`](Self::partially_revealed) indicator never
    /// animates, and this setting does not change that.
    #[must_use]
    pub fn animating(mut self, animating: bool) -> Self {
        if let Reveal::Spinning { animating: slot } = &mut self.reveal {
            *slot = animating;
        }
        self
    }

    /// Whether the controller should be running.
    fn animates(&self) -> bool {
        matches!(self.reveal, Reveal::Spinning { animating: true })
    }

    /// The fraction of the ticks shown.
    fn progress(&self) -> f64 {
        match self.reveal {
            Reveal::Spinning { .. } => 1.0,
            Reveal::Partial { progress } => progress,
        }
    }
}

/// The cycle's phase track and the per-tick delay.
#[derive(Debug)]
struct Phases {
    /// The cycle's phase, `0, 1, …, 7`, as a staircase over one second.
    phase: Keyframes<f64>,
    stagger: Stagger,
}

impl Phases {
    fn new() -> Self {
        let phase = Keyframes::builder(0.0, CYCLE)
            .to(f64::from(TICKS), CYCLE, Steps::new(TICKS, JumpAt::End))
            .build()
            .expect("BUG: one segment of the cycle's length fits the cycle");
        Self {
            phase,
            stagger: Stagger::new(CYCLE / TICKS, StaggerOrigin::First),
        }
    }

    /// The alpha of tick `index` at `elapsed` into the cycle.
    fn alpha(&self, index: u32, elapsed: Duration) -> u8 {
        let delay = self.stagger.delay(index as usize, TICKS as usize);
        // Shift by a whole cycle first so the delayed time never underflows.
        let shifted = (elapsed + CYCLE).saturating_sub(delay);
        let phase = self.phase.value_at_looped(shifted).round() as usize % ALPHAS.len();
        // Tick `index` is `phase` steps behind the brightest tick.
        ALPHAS[(ALPHAS.len() - phase) % ALPHAS.len()]
    }
}

/// Persistent state for [`CupertinoActivityIndicator`]: the repeating
/// controller and its registration with the ambient [`Vsync`](flui_sdk::animation::Vsync).
pub struct CupertinoActivityIndicatorState {
    controller: DrivenController,
    phases: Rc<Phases>,
}

impl std::fmt::Debug for CupertinoActivityIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoActivityIndicatorState")
            .field("registered", &self.controller.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for CupertinoActivityIndicator {
    type State = CupertinoActivityIndicatorState;

    /// The controller is born on the ambient clock; `apply_view` starts it
    /// before the first build when the view animates.
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CupertinoActivityIndicatorState {
            controller: AnimationController::builder(CYCLE)
                .debug_label("CupertinoActivityIndicator")
                .build_on(VsyncScope::maybe_of(ctx).as_ref()),
            phases: Rc::new(Phases::new()),
        }
    }
}

impl ViewState<CupertinoActivityIndicator> for CupertinoActivityIndicatorState {
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "activity indicator lost its frame registry");
        }
    }

    /// Runs the controller exactly while the view animates. This hook runs
    /// before the first build and after every view or clock change, so it is
    /// the one place that starts and stops the run: a run a missing clock
    /// refused starts once a clock arrives, and `animating(false)` stops the
    /// run, which leaves no continuation frame.
    fn apply_view(&mut self, view: &CupertinoActivityIndicator, _ctx: &dyn BuildContext) {
        let controller = self.controller.controller();
        if view.animates() {
            if !controller.is_animating() {
                let _ = controller.repeat(RepeatMode::Restart);
            }
        } else if controller.is_animating() {
            let _ = controller.stop();
        }
    }

    fn build(&self, view: &CupertinoActivityIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = flui_sdk::widgets::TickerMode::is_enabled(ctx);
        let reduced = flui_sdk::view::environment::MediaQuery::motion_of(ctx)
            == Some(flui_sdk::animation::MotionPolicy::Reduce);
        let moving = view.animates() && self.controller.is_bound() && enabled && !reduced;
        let color = view
            .color
            .map_or_else(|| TICK_COLOR.resolve_from(ctx), |color| color.resolve(ctx));
        let progress = view.progress();
        let painter = TickPainter {
            controller: moving.then(|| self.controller.controller().clone()),
            phases: Rc::clone(&self.phases),
            color,
            progress,
        };
        let diameter = view.radius * 2.0;
        let semantics = Semantics::new()
            .role(SemanticsRole::ProgressBar)
            .label("Loading");
        let semantics = match view.reveal {
            Reveal::Spinning { .. } => semantics,
            Reveal::Partial { progress } => {
                let percent = (progress * 100.0).round();
                let semantics = semantics.value(format!("{percent}%"));
                match NumericRange::try_new(percent, 0.0, 100.0, 1.0) {
                    Ok(range) => semantics.numeric_range(range),
                    Err(_) => semantics,
                }
            }
        };
        semantics.child(
            CustomPaint::new()
                .size(Size::new(diameter, diameter))
                .painter(painter),
        )
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}

#[derive(Debug)]
struct TickPainter {
    controller: Option<AnimationController>,
    phases: Rc<Phases>,
    color: Color,
    /// The fraction of the ticks shown, in `0..=1`.
    progress: f64,
}

impl CustomPainter for TickPainter {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let size = ctx.size();
        let canvas = ctx.canvas();
        let radius = size.width.min(size.height) / 2.0;
        if !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let elapsed = CYCLE.mul_f64(
            self.controller
                .as_ref()
                .map_or(0.0, Animation::value)
                .clamp(0.0, 1.0),
        );
        // Flutter draws tick `i` while `i < 8 · progress`
        // (`activity_indicator.dart:179`): the ceiling, at most eight.
        let partial = self.progress < 1.0;
        let shown = if partial {
            // `progress` is in `0..1` here, so the product is in `0..8`.
            (f64::from(TICKS) * self.progress).ceil() as u32
        } else {
            TICKS
        };
        // A tick: a rounded bar from a third of the radius to the rim, at 12
        // o'clock before rotation, `radius / 5` wide.
        let half_width = radius / RADIUS;
        let tick = RRect::from_rect_xy(
            Rect::from_ltrb(-half_width, -radius, half_width, -radius / 3.0),
            half_width,
            half_width,
        );
        canvas.save();
        canvas.translate(size.width / 2.0, size.height / 2.0);
        for index in 0..shown.min(TICKS) {
            let alpha = if partial {
                REVEALED_ALPHA
            } else {
                self.phases.alpha(index, elapsed)
            };
            canvas.draw_rrect(tick, &Paint::fill(self.color.with_alpha(alpha)));
            canvas.rotate(TAU / f64::from(TICKS));
        }
        canvas.restore();
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        (old as &dyn Any).downcast_ref::<Self>().is_none_or(|old| {
            old.color != self.color
                || old.progress.to_bits() != self.progress.to_bits()
                || old.controller.is_some() != self.controller.is_some()
                || !Rc::ptr_eq(&old.phases, &self.phases)
        })
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        self.controller
            .as_ref()
            .map(|controller| Rc::new(controller.clone()) as Rc<dyn Listenable>)
    }
}
