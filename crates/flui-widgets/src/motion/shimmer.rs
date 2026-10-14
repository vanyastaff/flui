//! [`Shimmer`] and [`Skeleton`]: loading placeholders that share one sweeping
//! highlight band.
//!
//! A `Shimmer` owns one repeating phase and publishes it, with its band, to
//! the subtree. Each `Skeleton` paints its own gradient: the band laid across
//! the whole shimmer, translated by the skeleton's offset from it, which the
//! paint walk reports from the shimmer's paint anchor. Every skeleton thus
//! shows its window of the same band, and no offscreen group is needed
//! around them. The shimmer is a repaint boundary, so its ticks repaint the
//! skeletons and nothing outside it.

use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, ConstantAnimation, DrivenController, MotionPolicy,
    ProxyAnimation, RepeatMode,
};
use flui_foundation::geometry::Size;
use flui_objects::{RenderPaintAnchor, RenderSkeleton, ShimmerBand};
use flui_painting::styling::Color;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_rendering::semantics::SemanticsRole;
use flui_view::prelude::*;
use flui_view::{
    InheritedView, RenderObjectContext, RenderView, impl_inherited_view, impl_render_view,
};

use crate::animated::VsyncScope;
use crate::{
    Column, CrossAxisAlignment, MainAxisSize, MediaQuery, Semantics, SizedBox, TickerMode,
};

/// A skeleton's colour when none is given: a light neutral grey.
const DEFAULT_BASE: Color = Color::rgb(224, 224, 224);
/// The band's colour when none is given: a lighter grey than the base.
const DEFAULT_HIGHLIGHT: Color = Color::rgb(245, 245, 245);

/// Sweeps one highlight band across every [`Skeleton`] below it, repeating
/// every [`period`](Self::period) (1.5 s by default).
///
/// The band crosses the shimmer's own bounds in [`direction`](Self::direction);
/// each skeleton shows the part of it that falls on the skeleton, so a column
/// of placeholders reads as one sweep. Content that is not a skeleton is
/// painted unchanged.
///
/// The band holds still, and every skeleton paints its flat base colour,
/// while [`enabled`](Self::enabled) is `false`, under a disabled
/// [`TickerMode`], under reduced motion, and without an ambient
/// [`VsyncScope`] to drive it.
///
/// Assistive technology sees one progress indicator labelled
/// [`label`](Self::label) ("Loading" by default) in place of the subtree:
/// placeholders are not content, so nothing below the shimmer is announced.
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// use flui_widgets::motion::{Shimmer, Skeleton};
///
/// let card = Shimmer::new(Column::new((
///     Skeleton::circle(40.0),
///     Skeleton::text(3),
/// )));
/// # let _ = card;
/// ```
#[derive(Clone, StatefulView)]
pub struct Shimmer {
    child: BoxedView,
    period: Duration,
    band: ShimmerBand,
    enabled: bool,
    label: String,
}

impl Shimmer {
    /// How long one sweep takes unless [`period`](Self::period) says
    /// otherwise.
    pub const DEFAULT_PERIOD: Duration = Duration::from_millis(1500);

    /// A shimmer over `child`, sweeping left to right every
    /// [`DEFAULT_PERIOD`](Self::DEFAULT_PERIOD).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            period: Self::DEFAULT_PERIOD,
            band: ShimmerBand::new(DEFAULT_HIGHLIGHT),
            enabled: true,
            label: "Loading".to_owned(),
        }
    }

    /// One sweep takes `period`.
    #[must_use]
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// The band travels in `direction`.
    #[must_use]
    pub fn direction(mut self, direction: AxisDirection) -> Self {
        self.band = self.band.with_direction(direction);
        self
    }

    /// The band's colour at its centre.
    #[must_use]
    pub fn highlight(mut self, highlight: Color) -> Self {
        self.band = ShimmerBand::new(highlight)
            .with_direction(self.band.direction())
            .with_width(self.band.width());
        self
    }

    /// The band's width as a fraction of the shimmer's extent along its
    /// direction.
    #[must_use]
    pub fn band_width(mut self, fraction: f64) -> Self {
        self.band = self.band.with_width(fraction);
        self
    }

    /// Whether the band sweeps; `false` paints every skeleton flat.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// What assistive technology announces for the loading region.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl std::fmt::Debug for Shimmer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shimmer")
            .field("period", &self.period)
            .field("band", &self.band)
            .field("enabled", &self.enabled)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// The repeating phase a [`Shimmer`] publishes, and whether it runs.
pub struct ShimmerState {
    controller: DrivenController,
    phase: Rc<dyn Animation<f64>>,
    enabled: bool,
}

impl std::fmt::Debug for ShimmerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShimmerState")
            .field("phase", &self.phase.value())
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Shimmer {
    type State = ShimmerState;

    fn create_state(&self) -> Self::State {
        let controller = AnimationController::builder(self.period)
            .debug_label("Shimmer")
            .build_on(None);
        let phase: Rc<dyn Animation<f64>> = Rc::new(controller.controller().clone());
        ShimmerState {
            controller,
            phase,
            enabled: self.enabled,
        }
    }
}

impl ShimmerState {
    /// Repeat while enabled, stop otherwise: a disabled shimmer asks for no
    /// frames.
    fn sync_run(&self) {
        let controller = self.controller.controller();
        if !self.enabled {
            if let Err(error) = controller.stop() {
                tracing::warn!(%error, "shimmer could not stop its sweep");
            }
        } else if !controller.is_animating()
            && let Err(error) = controller.repeat(RepeatMode::Restart)
        {
            tracing::warn!(%error, "shimmer refused to repeat its sweep");
        }
    }
}

impl ViewState<Shimmer> for ShimmerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.controller.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "shimmer lost its frame registry");
        }
        self.sync_run();
    }

    fn did_update_view(&mut self, old: &Shimmer, new: &Shimmer) {
        self.enabled = new.enabled;
        if old.period != new.period {
            let controller = self.controller.controller();
            controller.set_duration(new.period);
            // A running repeat keeps the period it started with.
            if let Err(error) = controller.stop() {
                tracing::warn!(%error, "shimmer could not stop its sweep");
            }
        }
        self.sync_run();
    }

    fn build(&self, view: &Shimmer, ctx: &dyn BuildContext) -> impl IntoView {
        let sweeping = self.enabled
            && self.controller.is_bound()
            && TickerMode::is_enabled(ctx)
            && MediaQuery::motion_of(ctx) != Some(MotionPolicy::Reduce);
        Semantics::new()
            .role(SemanticsRole::ProgressBar)
            .label(view.label.clone())
            .exclude_semantics(true)
            .child(ShimmerScope {
                phase: ShimmerPhase {
                    band: sweeping.then_some(view.band),
                    phase: Rc::clone(&self.phase),
                },
                child: PaintAnchorView {
                    child: view.child.clone(),
                }
                .boxed(),
            })
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}

/// What a [`Shimmer`] hands its skeletons: the band (`None` while it holds
/// still) and the phase, `0..1` once per sweep.
#[derive(Clone)]
struct ShimmerPhase {
    band: Option<ShimmerBand>,
    phase: Rc<dyn Animation<f64>>,
}

/// Publishes a [`ShimmerPhase`] to the subtree.
#[derive(Clone)]
struct ShimmerScope {
    phase: ShimmerPhase,
    child: BoxedView,
}

impl ShimmerScope {
    /// The nearest shimmer's phase, depending on it.
    fn maybe_of(ctx: &dyn LifecycleContext) -> Option<ShimmerPhase> {
        ctx.depend_on::<Self, _>(|scope| scope.phase.clone())
    }
}

impl std::fmt::Debug for ShimmerScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShimmerScope")
            .field("band", &self.phase.band)
            .finish_non_exhaustive()
    }
}

impl InheritedView for ShimmerScope {
    type Data = ShimmerPhase;

    fn data(&self) -> &Self::Data {
        &self.phase
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.phase.band != old.phase.band || !Rc::ptr_eq(&self.phase.phase, &old.phase.phase)
    }
}

impl_inherited_view!(ShimmerScope);

/// The shimmer's paint anchor: skeletons measure their offset from it.
#[derive(Clone)]
struct PaintAnchorView {
    child: BoxedView,
}

impl std::fmt::Debug for PaintAnchorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintAnchorView").finish_non_exhaustive()
    }
}

impl RenderView for PaintAnchorView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPaintAnchor;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderPaintAnchor::new()
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(PaintAnchorView);

/// The shape a [`Skeleton`] stands in for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Rect(Size),
    Line(f64),
    Circle(f64),
    Text(usize),
}

/// A loading placeholder in a flat colour, swept by the band of the nearest
/// [`Shimmer`].
///
/// Without a shimmer above it, or while the shimmer holds still, it paints
/// its flat [`color`](Self::color). A skeleton adds nothing to the semantics
/// tree; the shimmer announces the loading region.
///
/// Lines and text take their width from a bounded incoming width, as inside
/// a [`Column`]; under an unbounded width they collapse to nothing.
#[derive(Clone, Debug, StatefulView)]
pub struct Skeleton {
    shape: Shape,
    color: Color,
    corner_radius: Option<f64>,
}

impl Skeleton {
    /// The height of a [`line`](Self::line) and of each [`text`](Self::text)
    /// line.
    pub const LINE_HEIGHT: f64 = 12.0;
    /// The gap between [`text`](Self::text) lines.
    pub const LINE_GAP: f64 = 8.0;
    /// The share of the width the last of several [`text`](Self::text) lines
    /// takes, so the block ends like a paragraph.
    pub const LAST_LINE: f64 = 0.6;
    /// The corner radius of a [`rect`](Self::rect) unless set.
    pub const RECT_RADIUS: f64 = 4.0;

    /// A rectangle of `size`, corners rounded by
    /// [`RECT_RADIUS`](Self::RECT_RADIUS).
    #[must_use]
    pub fn rect(size: Size) -> Self {
        Self::of(Shape::Rect(size))
    }

    /// One line of text, `width_fraction` of the available width wide and
    /// [`LINE_HEIGHT`](Self::LINE_HEIGHT) tall, with fully rounded ends.
    #[must_use]
    pub fn line(width_fraction: f64) -> Self {
        Self::of(Shape::Line(width_fraction))
    }

    /// A circle `diameter` across, as for an avatar.
    #[must_use]
    pub fn circle(diameter: f64) -> Self {
        Self::of(Shape::Circle(diameter))
    }

    /// A paragraph of `lines` lines: full width, the last of several
    /// [`LAST_LINE`](Self::LAST_LINE) of it.
    #[must_use]
    pub fn text(lines: usize) -> Self {
        Self::of(Shape::Text(lines))
    }

    fn of(shape: Shape) -> Self {
        Self {
            shape,
            color: DEFAULT_BASE,
            corner_radius: None,
        }
    }

    /// The flat colour, which the band fades back to on both sides.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Round the corners by `radius` instead of the shape's default. A
    /// circle stays round.
    #[must_use]
    pub fn corner_radius(mut self, radius: f64) -> Self {
        self.corner_radius = Some(radius);
        self
    }
}

/// The phase a [`Skeleton`]'s boxes listen to, following the nearest
/// shimmer, and the band it currently sweeps.
pub struct SkeletonState {
    phase: ProxyAnimation<f64>,
    source: Option<Rc<dyn Animation<f64>>>,
    band: Option<ShimmerBand>,
}

impl std::fmt::Debug for SkeletonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkeletonState")
            .field("band", &self.band)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Skeleton {
    type State = SkeletonState;

    fn create_state(&self) -> Self::State {
        SkeletonState {
            phase: ProxyAnimation::new(still()),
            source: None,
            band: None,
        }
    }
}

/// The phase of a skeleton no shimmer drives.
fn still() -> Rc<dyn Animation<f64>> {
    Rc::new(ConstantAnimation::new(0.0))
}

impl ViewState<Skeleton> for SkeletonState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    /// Follow the nearest shimmer: its band, and its phase through the one
    /// proxy every box of this skeleton keeps listening to.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let shimmer = ShimmerScope::maybe_of(ctx);
        self.band = shimmer.as_ref().and_then(|shimmer| shimmer.band);
        let source = shimmer.map(|shimmer| shimmer.phase);
        let same = match (&self.source, &source) {
            (Some(held), Some(next)) => Rc::ptr_eq(held, next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.phase
                .set_parent(source.as_ref().map_or_else(still, Rc::clone));
            self.source = source;
        }
    }

    fn build(&self, view: &Skeleton, _ctx: &dyn BuildContext) -> impl IntoView {
        let bone = |width_factor: f64, corner_radius: f64| SkeletonBox {
            base: view.color,
            corner_radius,
            width_factor,
            band: self.band,
            phase: self.phase.clone(),
        };
        let line = |width_factor: f64| {
            SizedBox::height(Skeleton::LINE_HEIGHT).child(bone(
                width_factor,
                view.corner_radius.unwrap_or(Skeleton::LINE_HEIGHT / 2.0),
            ))
        };
        match view.shape {
            Shape::Rect(size) => SizedBox::new(size.width, size.height)
                .child(bone(
                    1.0,
                    view.corner_radius.unwrap_or(Skeleton::RECT_RADIUS),
                ))
                .boxed(),
            Shape::Line(width_factor) => line(width_factor).boxed(),
            Shape::Circle(diameter) => SizedBox::square(diameter)
                .child(bone(1.0, f64::INFINITY))
                .boxed(),
            Shape::Text(lines) => Column::new(
                (0..lines)
                    .map(|index| {
                        let last = lines > 1 && index + 1 == lines;
                        line(if last { Skeleton::LAST_LINE } else { 1.0 }).boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min)
            .spacing(Skeleton::LINE_GAP)
            .boxed(),
        }
    }
}

/// One painted placeholder box.
#[derive(Clone)]
struct SkeletonBox {
    base: Color,
    corner_radius: f64,
    width_factor: f64,
    band: Option<ShimmerBand>,
    phase: ProxyAnimation<f64>,
}

impl std::fmt::Debug for SkeletonBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkeletonBox")
            .field("base", &self.base)
            .field("band", &self.band)
            .finish_non_exhaustive()
    }
}

impl RenderView for SkeletonBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSkeleton;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut node = RenderSkeleton::new(self.base, self.band, self.phase.clone());
        let _ = node.set_corner_radius(self.corner_radius);
        let _ = node.set_width_factor(self.width_factor);
        node
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        render_object.set_base(self.base)
            | render_object.set_corner_radius(self.corner_radius)
            | render_object.set_width_factor(self.width_factor)
            | render_object.set_band(self.band)
    }
}

impl_render_view!(SkeletonBox);
