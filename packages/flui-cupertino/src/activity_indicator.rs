//! [`CupertinoActivityIndicator`]: the iOS spinner of eight fading ticks.
//!
//! One repeating one-second controller drives every tick. A single keyframe
//! track steps through the cycle's eight phases (`Steps(8, JumpAt::End)`);
//! tick `i` reads it delayed by `Stagger(125 ms, First)`, so each tick is one
//! phase behind the tick before it. A tick's phase picks its alpha from
//! `[47, 47, 47, 47, 72, 97, 122, 147]` (Flutter's `activity_indicator.dart`),
//! which makes the brightest tick walk around the circle once a second. The
//! painter samples the track at the controller's progress, so the ticks move
//! by repaint, never by rebuild.

use std::any::Any;
use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::{
    Animation, AnimationController, DrivenController, JumpAt, Keyframes, Stagger, StaggerOrigin,
    Steps,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{RRect, Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{CustomPaint, CustomPainter, Semantics, SemanticsRole};

use crate::colors::CupertinoDynamicColor;

/// One revolution of the brightest tick.
const CYCLE: Duration = Duration::from_secs(1);
/// The number of ticks around the circle.
const TICKS: u32 = 8;
/// Tick alpha by phase: phase 0 is the brightest tick's predecessor, phases
/// 4–7 are the fading tail.
const ALPHAS: [u8; TICKS as usize] = [47, 47, 47, 47, 72, 97, 122, 147];
/// The default radius, in logical pixels.
const RADIUS: f64 = 10.0;
/// The ticks' color in light and dark mode.
const TICK_COLOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::rgb(0x3C, 0x3C, 0x44),
    Color::rgb(0xEB, 0xEB, 0xF5),
);

/// The iOS activity indicator: eight ticks around a circle of radius 10,
/// the brightest one stepping clockwise once per tick per eighth of a second.
///
/// Its controller is registered with the ambient [`VsyncScope`]; without one,
/// or under a disabled `TickerMode`, it paints the cycle's first frame.
/// Assistive technology sees a [`SemanticsRole::LoadingSpinner`] labelled
/// "Loading".
///
/// # Examples
///
/// ```rust,ignore
/// CupertinoActivityIndicator::new()
/// ```
#[derive(Debug, Clone, Default, StatefulView)]
pub struct CupertinoActivityIndicator {}

impl CupertinoActivityIndicator {
    /// A spinner of the default radius and color.
    #[must_use]
    pub fn new() -> Self {
        Self {}
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
/// controller and its registration with the ambient [`Vsync`].
pub struct CupertinoActivityIndicatorState {
    controller: DrivenController,
    phases: Arc<Phases>,
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

    fn create_state(&self) -> Self::State {
        CupertinoActivityIndicatorState {
            controller: AnimationController::builder(CYCLE).build_on(None),
            phases: Arc::new(Phases::new()),
        }
    }
}

impl ViewState<CupertinoActivityIndicator> for CupertinoActivityIndicatorState {
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

    fn build(&self, _view: &CupertinoActivityIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let painter = TickPainter {
            controller: self.controller.controller().clone(),
            phases: Arc::clone(&self.phases),
            color: TICK_COLOR.resolve_from(ctx),
        };
        Semantics::new()
            .role(SemanticsRole::LoadingSpinner)
            .label("Loading")
            .child(
                CustomPaint::new()
                    .size(Size::new(RADIUS * 2.0, RADIUS * 2.0))
                    .painter(std::rc::Rc::new(painter)),
            )
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}

#[derive(Debug)]
struct TickPainter {
    controller: AnimationController,
    phases: Arc<Phases>,
    color: Color,
}

impl CustomPainter for TickPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let radius = size.width.min(size.height) / 2.0;
        if !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let elapsed = CYCLE.mul_f64(self.controller.value().clamp(0.0, 1.0));
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
        for index in 0..TICKS {
            let alpha = self.phases.alpha(index, elapsed);
            let color = Color::rgba(self.color.r, self.color.g, self.color.b, alpha);
            canvas.draw_rrect(tick, &Paint::fill(color));
            canvas.rotate(TAU / f64::from(TICKS));
        }
        canvas.restore();
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.color != self.color)
    }

    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        Some(std::rc::Rc::new(self.controller.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
