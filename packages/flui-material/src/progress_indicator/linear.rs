//! [`LinearProgressIndicator`]: a Material 3 linear progress indicator.
//!
//! Determinate (`value: Some(fraction)`), it paints the fraction of its track
//! and runs no animation. Indeterminate (`None`), two bars sweep across the
//! track on one repeating 1.75 s controller while motion is permitted: each bar's head and tail is a
//! keyframe track whose leading `hold` is that end's delay (0, 250, 650 and
//! 900 ms) followed by a 1000, 1000, 850 or 850 ms run to the far end, eased
//! by Material 3's emphasized-accelerate curve (Compose `ProgressIndicator.kt`
//! constants). The painter samples the tracks at the controller's progress,
//! so the bars move by repaint, never by rebuild.

use std::any::Any;
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::animation::{
    Animatable, Animation, AnimationController, Cubic, DrivenController, Keyframes, RepeatMode,
    Vsync,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint, TextDirection};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{CustomPaint, CustomPainter, Directionality};

use super::{Fraction, STROKE, moves, paint_linear_track, progress_semantics};
use crate::theme::Theme;

/// One indeterminate cycle.
pub(super) const CYCLE: Duration = Duration::from_millis(1750);
/// The indicator's default size: Material 3's 240 × 4 dp.
pub(super) const WIDTH: f64 = 240.0;
/// Material 3 `emphasized accelerate` easing.
const EMPHASIZED_ACCELERATE: Cubic = Cubic::new(0.3, 0.0, 0.8, 0.15);

/// `(delay, duration)` in milliseconds of each bar end's run across the
/// track: first bar head and tail, then second bar head and tail.
const BAR_ENDS: [(u64, u64); 4] = [(0, 1000), (250, 1000), (650, 850), (900, 850)];

/// The segment a still indeterminate indicator shows: centred, so it reads as activity and
/// never as a fraction done.
pub(super) const STILL_SPAN: (f64, f64) = (0.375, 0.625);

/// A Material 3 linear progress indicator.
///
/// With a [`value`](Self::value) it shows that fraction of the work as done:
/// the active bar, a 4 dp gap, the rest of the track, and a stop dot at the
/// track's end. Without one it is indeterminate and animates while a
/// [`VsyncScope`] ticks it. Without a bound scope, under a disabled
/// `TickerMode`, or with reduced motion, a static centered segment denotes
/// unknown-total work; it never denotes a percentage. Reduced motion parks the
/// repeating run without continuation frames. Needs a [`Theme`] ancestor for
/// its colors.
///
/// Under a right-to-left [`Directionality`] progress starts at the right
/// edge: the fill grows leftward and the indeterminate bars travel right to
/// left.
///
/// Assistive technology sees a progress bar with the percentage as its
/// value, or a loading spinner while indeterminate, labelled with
/// [`label`](Self::label).
///
/// # Examples
///
/// ```rust,ignore
/// LinearProgressIndicator::new().value(Some(0.4)).label("Upload");
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct LinearProgressIndicator {
    value: Option<f64>,
    label: String,
}

impl Default for LinearProgressIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl LinearProgressIndicator {
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

/// The four bar-end tracks of one indeterminate cycle, as fractions of the
/// track's width.
#[derive(Debug)]
pub(super) struct Bars {
    ends: [Keyframes<f64>; 4],
}

impl Bars {
    pub(super) fn new() -> Self {
        Self {
            ends: BAR_ENDS.map(|(delay, duration)| {
                Keyframes::builder(0.0, CYCLE)
                    .hold(Duration::from_millis(delay))
                    .to(1.0, Duration::from_millis(duration), EMPHASIZED_ACCELERATE)
                    .build()
                    .expect("BUG: every bar end's delay and run fit in the 1.75 s cycle")
            }),
        }
    }

    /// The two bars as `(tail, head)` fractions of the width at `t` through the cycle.
    pub(super) fn spans(&self, t: f64) -> [(f64, f64); 2] {
        let [head, tail, second_head, second_tail] =
            self.ends.each_ref().map(|end| end.transform(t));
        [(tail, head), (second_tail, second_head)]
    }
}

/// Persistent state for [`LinearProgressIndicator`]: the indeterminate
/// controller, registered with the ambient [`Vsync`] only while the
/// indicator is indeterminate.
pub struct LinearProgressIndicatorState {
    controller: DrivenController,
    bars: Rc<Bars>,
    vsync: Option<Vsync>,
    /// The current configuration, used when the ambient registry changes.
    indeterminate: bool,
    fraction: Fraction,
}

impl std::fmt::Debug for LinearProgressIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinearProgressIndicatorState")
            .field("running", &self.controller.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for LinearProgressIndicator {
    type State = LinearProgressIndicatorState;

    fn create_state(&self) -> Self::State {
        LinearProgressIndicatorState {
            controller: AnimationController::builder(CYCLE)
                .debug_label("ProgressIndicator")
                .build_on(None),
            bars: Rc::new(Bars::new()),
            vsync: None,
            indeterminate: self.value.is_none(),
            fraction: Fraction::new(self.value),
        }
    }
}

impl LinearProgressIndicatorState {
    /// Registers and repeats the controller while `indeterminate`; stops and
    /// unregisters it otherwise.
    fn run(&mut self, indeterminate: bool) {
        self.indeterminate = indeterminate;
        if indeterminate {
            if let Err(error) = self.controller.rebind(self.vsync.as_ref()) {
                tracing::error!(%error, "progress indicator lost its frame registry");
            }
            if !self.controller.controller().is_animating() {
                let _ = self.controller.controller().repeat(RepeatMode::Restart);
            }
        } else {
            let _ = self.controller.controller().stop();
            let _ = self.controller.rebind(None);
        }
    }
}

impl ViewState<LinearProgressIndicator> for LinearProgressIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.run(self.indeterminate);
    }

    fn build(&self, view: &LinearProgressIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let fraction = view.value.map(|_| self.fraction.get());
        let painter = BarPainter {
            progress: match fraction {
                Some(fraction) => Progress::Determinate(fraction),
                None if !self.controller.is_bound() || !moves(ctx) => Progress::StaticIndeterminate,
                None => Progress::AnimatedIndeterminate {
                    controller: self.controller.controller().clone(),
                    bars: Rc::clone(&self.bars),
                },
            },
            track: colors.secondary_container,
            bar: colors.primary,
            rtl: Directionality::maybe_of(ctx) == Some(TextDirection::Rtl),
        };
        progress_semantics(&view.label, fraction).child(
            CustomPaint::new()
                .size(Size::new(WIDTH, STROKE))
                .painter(painter),
        )
    }

    fn did_update_view(&mut self, old: &LinearProgressIndicator, new: &LinearProgressIndicator) {
        if old.value.is_some() && new.value.is_none() {
            let _ = self.controller.controller().set_value(0.0);
        }
        self.fraction.update(new.value);
        self.run(new.value.is_none());
    }

    fn dispose(&mut self) {
        self.run(false);
        self.controller.dispose();
    }
}

/// What the painter draws.
#[derive(Debug)]
enum Progress {
    /// A fixed fraction of the track.
    Determinate(f64),
    /// Visible unknown-total feedback, distinct from a leading progress fill.
    StaticIndeterminate,
    /// Two bars at the controller's progress through the cycle.
    AnimatedIndeterminate {
        controller: AnimationController,
        bars: Rc<Bars>,
    },
}

#[derive(Debug)]
struct BarPainter {
    progress: Progress,
    track: Color,
    bar: Color,
    /// Progress runs from the right edge: the fill grows leftward and the
    /// indeterminate bars travel right to left.
    rtl: bool,
}

impl CustomPainter for BarPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !size.width.is_finite() || !size.height.is_finite() || size.width <= 0.0 {
            return;
        }
        let (spans, stop) = match &self.progress {
            Progress::Determinate(fraction) => (vec![(0.0, *fraction)], Some(self.bar)),
            // A centered segment conveys activity without suggesting a known
            // percentage. Full-mode keyframe samples keep their original shape.
            Progress::StaticIndeterminate => (vec![STILL_SPAN], None),
            Progress::AnimatedIndeterminate { controller, bars } => {
                (bars.spans(controller.value()).to_vec(), None)
            }
        };
        paint_linear_track(
            canvas,
            size.width,
            size.height,
            &spans,
            stop,
            self.track,
            self.rtl,
        );
        for (from, to) in spans {
            if to > from {
                let left = if self.rtl { 1.0 - to } else { from };
                canvas.draw_rect(
                    Rect::from_ltwh(
                        left * size.width,
                        0.0,
                        (to - from) * size.width,
                        size.height,
                    ),
                    &Paint::fill(self.bar),
                );
            }
        }
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        let Some(old) = old.as_any().downcast_ref::<Self>() else {
            return true;
        };
        old.track != self.track
            || old.rtl != self.rtl
            || old.bar != self.bar
            || match (&old.progress, &self.progress) {
                (Progress::Determinate(a), Progress::Determinate(b)) => a != b,
                (Progress::StaticIndeterminate, Progress::StaticIndeterminate) => false,
                (
                    Progress::AnimatedIndeterminate { bars: a, .. },
                    Progress::AnimatedIndeterminate { bars: b, .. },
                ) => !Rc::ptr_eq(a, b),
                _ => true,
            }
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        match &self.progress {
            Progress::Determinate(_) | Progress::StaticIndeterminate => None,
            Progress::AnimatedIndeterminate { controller, .. } => Some(Rc::new(controller.clone())),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
