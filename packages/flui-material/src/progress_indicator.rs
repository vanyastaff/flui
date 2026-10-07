//! [`LinearProgressIndicator`]: a Material 3 linear progress indicator.
//!
//! Determinate (`value: Some(fraction)`), it paints the fraction of its track
//! and runs no animation. Indeterminate (`None`), two bars sweep across the
//! track on one repeating 1.75 s controller: each bar's head and tail is a
//! keyframe track whose leading `hold` is that end's delay (0, 250, 650 and
//! 900 ms) followed by a 1000, 1000, 850 or 850 ms run to the far end, eased
//! by Material 3's emphasized-accelerate curve (Compose `ProgressIndicator.kt`
//! constants). The painter samples the tracks at the controller's progress,
//! so the bars move by repaint, never by rebuild.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::{
    Animatable, Animation, AnimationController, Cubic, Keyframes, Vsync, VsyncRegistration,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint, TextDirection};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{CustomPaint, CustomPainter, Directionality, Semantics, SemanticsRole};

use crate::theme::Theme;

/// One indeterminate cycle.
const CYCLE: Duration = Duration::from_millis(1750);
/// The indicator's default size: Material 3's 240 × 4 dp.
const WIDTH: f64 = 240.0;
const HEIGHT: f64 = 4.0;
/// Material 3 `emphasized accelerate` easing.
const EMPHASIZED_ACCELERATE: Cubic = Cubic::new(0.3, 0.0, 0.8, 0.15);

/// `(delay, duration)` in milliseconds of each bar end's run across the
/// track: first bar head and tail, then second bar head and tail.
const BAR_ENDS: [(u64, u64); 4] = [(0, 1000), (250, 1000), (650, 850), (900, 850)];

/// A Material 3 linear progress indicator.
///
/// With a [`value`](Self::value) it shows that fraction of the work as done;
/// without one it is indeterminate and animates while a [`VsyncScope`] ticks
/// it (without one, or under a disabled `TickerMode`, it paints the cycle's
/// first frame). Needs a [`Theme`] ancestor for its colors.
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
    /// indicator. The fraction is clamped into `[0, 1]`; NaN reads as 0.
    #[must_use]
    pub fn value(mut self, value: Option<f64>) -> Self {
        self.value = value.map(|fraction| {
            // NaN and both zeros store +0, so semantics never reads "-0%".
            if fraction.is_nan() || fraction <= 0.0 {
                0.0
            } else {
                fraction.min(1.0)
            }
        });
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
struct Bars {
    ends: [Keyframes<f64>; 4],
}

impl Bars {
    fn new() -> Self {
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
}

/// Persistent state for [`LinearProgressIndicator`]: the indeterminate
/// controller, registered with the ambient [`Vsync`] only while the
/// indicator is indeterminate.
pub struct LinearProgressIndicatorState {
    controller: AnimationController,
    bars: Arc<Bars>,
    vsync: Option<Vsync>,
    registration: Option<VsyncRegistration>,
    /// The current configuration, used when the ambient registry changes.
    indeterminate: bool,
}

impl std::fmt::Debug for LinearProgressIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinearProgressIndicatorState")
            .field("running", &self.registration.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for LinearProgressIndicator {
    type State = LinearProgressIndicatorState;

    fn create_state(&self) -> Self::State {
        LinearProgressIndicatorState {
            controller: AnimationController::without_ticker(CYCLE),
            bars: Arc::new(Bars::new()),
            vsync: None,
            registration: None,
            indeterminate: self.value.is_none(),
        }
    }
}

impl LinearProgressIndicatorState {
    /// Registers and repeats the controller while `indeterminate`; stops and
    /// unregisters it otherwise.
    fn run(&mut self, indeterminate: bool) {
        self.indeterminate = indeterminate;
        match (&self.vsync, indeterminate, self.registration.is_some()) {
            (Some(vsync), true, false) => {
                self.registration = Some(vsync.register(self.controller.clone()));
                if !self.controller.is_animating() {
                    self.controller.set_value(0.0);
                }
                // An undisposed controller over [0, 1] accepts a repeat; the
                // future only reports the end of an endless run.
                if !self.controller.is_animating() {
                    let _ = self.controller.repeat(false);
                }
            }
            (_, false, true) | (None, _, _) => {
                let _ = self.controller.stop();
                if let (Some(vsync), Some(registration)) = (&self.vsync, self.registration.take()) {
                    vsync.unregister(&registration);
                }
            }
            _ => {}
        }
    }
}

impl ViewState<LinearProgressIndicator> for LinearProgressIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let next = ctx.depend_on::<VsyncScope, _>(|scope| scope.vsync().clone());
        let unchanged = match (&self.vsync, &next) {
            (Some(old), Some(new)) => old.is_same(new),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        if let (Some(vsync), Some(token)) = (&self.vsync, self.registration.take()) {
            vsync.unregister(&token);
        }
        self.vsync = next;
        self.run(self.indeterminate);
    }

    fn build(&self, view: &LinearProgressIndicator, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let painter = BarPainter {
            progress: match view.value {
                Some(fraction) => Progress::Done(fraction),
                None => Progress::Indeterminate {
                    controller: self.controller.clone(),
                    bars: Arc::clone(&self.bars),
                },
            },
            track: colors.secondary_container,
            bar: colors.primary,
            rtl: Directionality::maybe_of(ctx) == Some(TextDirection::Rtl),
        };
        let semantics = Semantics::new().label(view.label.clone());
        let semantics = match view.value {
            Some(fraction) => semantics
                .role(SemanticsRole::ProgressBar)
                .value(format!("{}%", (fraction * 100.0).round())),
            None => semantics.role(SemanticsRole::LoadingSpinner),
        };
        semantics.child(
            CustomPaint::new()
                .size(Size::new(WIDTH, HEIGHT))
                .painter(Arc::new(painter)),
        )
    }

    fn did_update_view(&mut self, _old: &LinearProgressIndicator, new: &LinearProgressIndicator) {
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
    Done(f64),
    /// Two bars at the controller's progress through the cycle.
    Indeterminate {
        controller: AnimationController,
        bars: Arc<Bars>,
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
        canvas.draw_rect(
            Rect::from_ltwh(0.0, 0.0, size.width, size.height),
            &Paint::fill(self.track),
        );
        let bar = |canvas: &mut Canvas, from: f64, to: f64| {
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
        };
        match &self.progress {
            Progress::Done(fraction) => bar(canvas, 0.0, *fraction),
            Progress::Indeterminate { controller, bars } => {
                let t = controller.value();
                let [head, tail, second_head, second_tail] =
                    bars.ends.each_ref().map(|end| end.transform(t));
                bar(canvas, tail, head);
                bar(canvas, second_tail, second_head);
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
                (Progress::Done(a), Progress::Done(b)) => a != b,
                (Progress::Indeterminate { .. }, Progress::Indeterminate { .. }) => false,
                _ => true,
            }
    }

    fn repaint(&self) -> Option<Arc<dyn Listenable>> {
        match &self.progress {
            Progress::Done(_) => None,
            Progress::Indeterminate { controller, .. } => Some(Arc::new(controller.clone())),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
