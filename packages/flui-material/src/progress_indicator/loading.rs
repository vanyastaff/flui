//! [`LoadingIndicator`] and [`ContainedLoadingIndicator`]: the Material 3 Expressive loading
//! indicators, a shape that morphs through seven Material shapes as it turns.
//!
//! Indeterminate, a new morph starts every 650 ms and follows a spring (damping ratio 0.6,
//! stiffness 200); each morph also turns the shape a quarter turn, on top of a steady full turn
//! every 4666 ms. Determinate, the circle morphs into the soft burst as the work completes.

use std::any::Any;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::rc::Rc;
use std::time::Duration;

use flui_sdk::animation::{
    Animation, AnimationController, DrivenController, RepeatMode, Simulation, SimulationError,
    SpringDescription, SpringSimulation, Tolerance, Vsync,
};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{Matrix4, Point, Size};
use flui_sdk::painting::{Canvas, Color, Morph, Paint, Path, PathCommand, material_shapes};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{CustomPaint, CustomPainter};

use super::{Fraction, moves, progress_semantics};
use crate::ColorScheme;
use crate::theme::Theme;

/// The indicator's side, and its container's diameter: 48 dp.
const CONTAINER: f64 = 48.0;
/// The side of the square the turning shape stays inside: 38 dp.
const SHAPE: f64 = 38.0;
/// A new morph starts this often.
const MORPH_INTERVAL: Duration = Duration::from_millis(650);
/// The steady turn takes this long for a full revolution.
const ROTATION_PERIOD: Duration = Duration::from_millis(4666);
/// The spring each morph follows.
const SPRING_STIFFNESS: f64 = 200.0;
const SPRING_DAMPING_RATIO: f64 = 0.6;
/// How many morphs the morph cycle runs before it repeats: four rounds of the seven shapes,
/// so the quarter turn per morph adds up to whole revolutions (28 × 90° = 7 × 360°) and the
/// shape does not jump when the cycle restarts.
const CYCLE_MORPHS: u32 = 28;

/// The shapes an indeterminate indicator morphs through, in order, back to the first.
const SEQUENCE: [fn() -> flui_sdk::painting::RoundedPolygon; 7] = [
    material_shapes::soft_burst,
    material_shapes::cookie_9,
    material_shapes::pentagon,
    material_shapes::pill,
    material_shapes::sunny,
    material_shapes::cookie_4,
    material_shapes::oval,
];

/// The morphs an indicator draws, built once per indicator: each step of the sequence and the
/// determinate circle-to-burst, all in the shapes' unit square.
#[derive(Debug)]
struct Shapes {
    steps: Vec<Morph>,
    determinate: Morph,
    /// Scales the unit square so that any of the shapes, turned any way about the square's
    /// centre, stays inside [`SHAPE`].
    scale: f64,
}

impl Shapes {
    fn new() -> Self {
        let polygons: Vec<_> = SEQUENCE.iter().map(|shape| shape()).collect();
        let steps = (0..polygons.len())
            .map(|i| Morph::new(&polygons[i], &polygons[(i + 1) % polygons.len()]))
            .collect();
        let circle = material_shapes::circle();
        let burst = &polygons[0];
        let reach = polygons
            .iter()
            .chain([&circle])
            .map(|polygon| reach(&polygon.path()))
            .fold(0.5, f64::max);
        Self {
            steps,
            determinate: Morph::new(&circle, burst),
            scale: SHAPE / (2.0 * reach),
        }
    }
}

/// The farthest any point of `path`, control points included, lies from the unit square's
/// centre: a bound on the outline's reach however it is turned.
fn reach(path: &Path) -> f64 {
    path.commands()
        .flat_map(|command| match command {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => vec![p],
            PathCommand::QuadraticTo(c, p) => vec![c, p],
            PathCommand::CubicTo(c1, c2, p) => vec![c1, c2, p],
            PathCommand::Close => Vec::new(),
        })
        .map(|p| (p.x - 0.5).hypot(p.y - 0.5))
        .fold(0.0, f64::max)
}

/// The spring a morph follows from 0 to 1 over [`MORPH_INTERVAL`].
fn morph_spring() -> Result<SpringSimulation, SimulationError> {
    let spring =
        SpringDescription::with_damping_ratio(1.0, SPRING_STIFFNESS, SPRING_DAMPING_RATIO)?;
    SpringSimulation::try_new(spring, 0.0, 1.0, 0.0, Tolerance::DEFAULT)
}

/// A Material 3 Expressive loading indicator: a 38 dp shape in a 48 dp square that morphs
/// through seven Material shapes as it turns.
///
/// Without a [`value`](Self::value) it is indeterminate: soft burst, nine-sided cookie,
/// pentagon, pill, sunny, four-sided cookie and oval in turn, a new morph every 650 ms on a
/// spring, each adding a quarter turn to a steady revolution every 4666 ms. With one it morphs
/// a circle into the soft burst as the work completes; the value is clamped into `[0, 1]`, and
/// a non-finite value keeps showing the last finite one (the circle before the first).
///
/// Without a ticking [`VsyncScope`], under a disabled `TickerMode`, or with reduced motion, an
/// indeterminate indicator shows the soft burst standing still. Needs a [`Theme`] ancestor
/// for its color, the theme's primary.
///
/// Assistive technology sees a progress bar labelled with [`label`](Self::label): busy
/// without a value while indeterminate, the percentage when determinate.
///
/// # Examples
///
/// ```rust,ignore
/// LoadingIndicator::new().label("Fetching mail");
/// LoadingIndicator::new().value(Some(0.4));
/// ```
#[derive(Debug, Clone, StatefulView)]
pub struct LoadingIndicator {
    value: Option<f64>,
    label: String,
}

/// A [`LoadingIndicator`] on a 48 dp circular container: the theme's primary container
/// behind a shape in its on-primary-container color, for an indicator that sits over content.
///
/// Behaves as [`LoadingIndicator`] does in every other respect.
#[derive(Debug, Clone, StatefulView)]
pub struct ContainedLoadingIndicator {
    value: Option<f64>,
    label: String,
}

macro_rules! loading_builders {
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
            /// finite one (0, the circle, before the first).
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

loading_builders!(LoadingIndicator);
loading_builders!(ContainedLoadingIndicator);

/// The motion and shapes behind a loading indicator.
struct Loading {
    shapes: Rc<Shapes>,
    /// Runs through [`CYCLE_MORPHS`] morphs, then repeats.
    morphs: DrivenController,
    /// The steady revolution.
    spin: DrivenController,
    /// The spring of one morph; `None` only if its fixed parameters were refused, when each
    /// morph runs linearly.
    spring: Option<SpringSimulation>,
    vsync: Option<Vsync>,
    fraction: Fraction,
    indeterminate: bool,
    /// Whether the indicator may move where it is mounted.
    moving: bool,
}

impl std::fmt::Debug for Loading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loading")
            .field("running", &self.morphs.is_bound())
            .field("indeterminate", &self.indeterminate)
            .finish_non_exhaustive()
    }
}

impl Loading {
    fn new(value: Option<f64>) -> Self {
        let spring = morph_spring()
            .inspect_err(|error| tracing::error!(%error, "loading indicator spring refused"))
            .ok();
        Self {
            shapes: Rc::new(Shapes::new()),
            morphs: AnimationController::builder(MORPH_INTERVAL * CYCLE_MORPHS)
                .debug_label("LoadingIndicator.morphs")
                .build_on(None),
            spin: AnimationController::builder(ROTATION_PERIOD)
                .debug_label("LoadingIndicator.spin")
                .build_on(None),
            spring,
            vsync: None,
            fraction: Fraction::new(value),
            indeterminate: value.is_none(),
            moving: false,
        }
    }

    fn update(&mut self, value: Option<f64>) {
        self.fraction.update(value);
        self.indeterminate = value.is_none();
        self.run();
    }

    /// Runs the morphs and the revolution while an indeterminate indicator may move, and
    /// releases them otherwise: a still or determinate indicator needs no frames.
    fn run(&mut self) {
        let vsync = (self.indeterminate && self.moving)
            .then(|| self.vsync.clone())
            .flatten();
        for driven in [&mut self.morphs, &mut self.spin] {
            if vsync.is_none() {
                let _ = driven.controller().stop();
            }
            if let Err(error) = driven.rebind(vsync.as_ref()) {
                tracing::error!(%error, "loading indicator lost its frame registry");
            }
            if vsync.is_some() && !driven.controller().is_animating() {
                let _ = driven.controller().repeat(RepeatMode::Restart);
            }
        }
    }

    fn bind(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.moving = moves(ctx);
        self.run();
    }

    /// What the painter draws now.
    fn frame(&self, ctx: &dyn BuildContext) -> Frame {
        if !self.indeterminate {
            return Frame::Determinate(self.fraction.get());
        }
        if self.morphs.is_bound() && moves(ctx) {
            Frame::Moving {
                morphs: self.morphs.controller().clone(),
                spin: self.spin.controller().clone(),
                spring: self.spring.clone(),
            }
        } else {
            Frame::Still
        }
    }

    fn shown(&self) -> Option<f64> {
        (!self.indeterminate).then(|| self.fraction.get())
    }

    fn dispose(&mut self) {
        for driven in [&mut self.morphs, &mut self.spin] {
            let _ = driven.controller().stop();
            let _ = driven.rebind(None);
            driven.dispose();
        }
    }
}

/// What a loading indicator shows in one paint.
#[derive(Debug, Clone)]
enum Frame {
    /// The circle morphed this far into the soft burst.
    Determinate(f64),
    /// The first shape of the sequence, standing still.
    Still,
    /// The indeterminate motion, sampled at paint time.
    Moving {
        morphs: AnimationController,
        spin: AnimationController,
        spring: Option<SpringSimulation>,
    },
}

impl Frame {
    /// The outline to draw and the angle it is turned by.
    fn outline(&self, shapes: &Shapes) -> Option<(Path, f64)> {
        match self {
            Self::Determinate(fraction) => Some((shapes.determinate.path(*fraction)?, 0.0)),
            Self::Still => Some((shapes.steps[0].path(0.0)?, 0.0)),
            Self::Moving {
                morphs,
                spin,
                spring,
            } => {
                let cycle = morphs.value() * f64::from(CYCLE_MORPHS);
                // The cycle's value stays below 1 while it repeats, so the index is in range.
                let index = (cycle.floor() as usize).min(CYCLE_MORPHS as usize - 1);
                let elapsed = (cycle - index as f64) * MORPH_INTERVAL.as_secs_f64();
                let progress = match spring {
                    Some(spring) => spring.x(elapsed),
                    None => elapsed / MORPH_INTERVAL.as_secs_f64(),
                };
                let morph = &shapes.steps[index % shapes.steps.len()];
                // The spring overshoots a little; were the overshot shape ever out of range,
                // the settled one stands in.
                let path = morph.path(progress).or_else(|| morph.path(1.0))?;
                let turn = (index as f64 + progress) * FRAC_PI_2 + spin.value() * TAU;
                Some((path, turn.rem_euclid(TAU)))
            }
        }
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        match self {
            // Both controllers tick together, so following one repaints every moving frame.
            Self::Moving { morphs, .. } => Some(Rc::new(morphs.clone()) as Rc<dyn Listenable>),
            _ => None,
        }
    }
}

/// Persistent state for [`LoadingIndicator`].
#[derive(Debug)]
pub struct LoadingIndicatorState {
    loading: Loading,
}

/// Persistent state for [`ContainedLoadingIndicator`].
#[derive(Debug)]
pub struct ContainedLoadingIndicatorState {
    loading: Loading,
}

macro_rules! loading_state {
    ($view:ident, $state:ident, $colors:ident) => {
        impl StatefulView for $view {
            type State = $state;

            fn create_state(&self) -> Self::State {
                $state {
                    loading: Loading::new(self.value),
                }
            }
        }

        impl ViewState<$view> for $state {
            fn init_state(&mut self, ctx: &dyn LifecycleContext) {
                self.loading.bind(ctx);
            }

            fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
                self.loading.bind(ctx);
            }

            fn build(&self, view: &$view, ctx: &dyn BuildContext) -> impl IntoView {
                let scheme = Theme::of(ctx).color_scheme;
                let (container, color) = $colors(&scheme);
                let painter = ShapePainter {
                    shapes: Rc::clone(&self.loading.shapes),
                    frame: self.loading.frame(ctx),
                    color,
                    container,
                };
                progress_semantics(&view.label, self.loading.shown()).child(
                    CustomPaint::new()
                        .size(Size::new(CONTAINER, CONTAINER))
                        .painter(painter),
                )
            }

            fn did_update_view(&mut self, _old: &$view, new: &$view) {
                self.loading.update(new.value);
            }

            fn dispose(&mut self) {
                self.loading.dispose();
            }
        }
    };
}

/// An uncontained indicator's colors: no container, the shape in primary.
fn uncontained(scheme: &ColorScheme) -> (Option<Color>, Color) {
    (None, scheme.primary)
}

/// A contained indicator's colors: the primary container behind a shape in its on color.
fn contained(scheme: &ColorScheme) -> (Option<Color>, Color) {
    (Some(scheme.primary_container), scheme.on_primary_container)
}

loading_state!(LoadingIndicator, LoadingIndicatorState, uncontained);
loading_state!(
    ContainedLoadingIndicator,
    ContainedLoadingIndicatorState,
    contained
);

/// Paints a loading indicator: the container, if any, and the turned shape over it.
#[derive(Debug)]
struct ShapePainter {
    shapes: Rc<Shapes>,
    frame: Frame,
    color: Color,
    container: Option<Color>,
}

impl CustomPainter for ShapePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let side = size.width.min(size.height);
        if !side.is_finite() || side <= 0.0 {
            return;
        }
        let center = Point::new(size.width / 2.0, size.height / 2.0);
        if let Some(container) = self.container {
            canvas.draw_circle(center, side / 2.0, &Paint::fill(container));
        }
        let Some((outline, turn)) = self.frame.outline(&self.shapes) else {
            return;
        };
        // Into the indicator's frame: the unit square centred on the origin, scaled to the
        // shape's side (shrunk with a smaller indicator), turned, and centred in the box.
        let scale = self.shapes.scale * (side / CONTAINER).min(1.0);
        let mut placement = Matrix4::translation(-0.5, -0.5, 0.0);
        placement.scale(scale, scale, 1.0);
        placement.rotate_z(turn);
        placement.translate(center.x, center.y, 0.0);
        // A layout too large for the shape's coordinates to stay finite draws no shape.
        if let Some(shape) = outline.transform(&placement) {
            canvas.draw_path(&shape, &Paint::fill(self.color));
        }
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        true
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        self.frame.repaint()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
