//! [`AnimatedContainer`] — animates several [`Container`] properties at once.

use std::{rc::Rc, time::Duration};

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    AnimatedValue, Animation, MotionSpec, MotionUpdate, SpringDescription, TwoWayConverter, Vsync,
};
use flui_foundation::geometry::{EdgeInsets, Matrix4};
use flui_foundation::{ChangeNotifier, Listenable};
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, TransformTween, default_curve};
use crate::animated::property_motion::PropertyMotion;
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Container};

/// Animates [`Container`]'s alignment, padding, color, width, height, margin and
/// transform whenever any of them changes.
///
/// Numeric properties retain independent component velocities and deadlines.
/// A property animates only across a present→present change; a property
/// that appears or disappears snaps (no value to interpolate from/to). An
/// overshooting curve may carry a value past its target; padding, margin, width
/// and height are clamped at zero, the other properties extrapolate.
/// Matrix replacement preserves the displayed decomposition with C⁰ continuity;
/// its progress has separate motion. Non-finite initial properties are omitted,
/// and non-finite updates preserve all previously admitted property targets.
///
/// Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedContainer {
    alignment: Option<Alignment>,
    padding: Option<EdgeInsets>,
    color: Option<Color>,
    width: Option<f64>,
    height: Option<f64>,
    margin: Option<EdgeInsets>,
    transform: Option<Matrix4>,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedContainer {
    /// An animated container wrapping `child`, with no properties set yet, the
    /// 200 ms default duration, and an ease-in-out curve.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            alignment: None,
            padding: None,
            color: None,
            width: None,
            height: None,
            margin: None,
            transform: None,
            motion: MotionSpec::Curve {
                duration: DEFAULT_DURATION,
                curve: default_curve(),
            },
            child: child.into_view().boxed(),
        }
    }

    /// Animate toward this alignment of the child within the container.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Animate toward this inner padding.
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Animate toward this background color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Animate toward this fixed width.
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// Animate toward this fixed height.
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Animate toward this outer margin.
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Animate toward this paint transform, applied about the container's origin.
    ///
    /// The matrix interpolates by decomposition ([`Matrix4::lerp`]): rotation turns
    /// along the shorter arc, and a scale that starts or ends at zero takes the other
    /// end's rotation, so a scale-in from nothing grows without spinning.
    #[must_use]
    pub fn transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        let curve = match self.motion {
            MotionSpec::Curve { curve, .. } => curve,
            _ => default_curve(),
        };
        self.motion = MotionSpec::Curve { duration, curve };
        self
    }

    /// Override the easing curve; accepts any type implementing
    /// [`Curve`], including elastic and bounce curves.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        let duration = match self.motion {
            MotionSpec::Curve { duration, .. } => duration,
            _ => DEFAULT_DURATION,
        };
        self.motion = MotionSpec::Curve {
            duration,
            curve: ArcCurve::new(curve),
        };
        self
    }

    /// Select spring motion for properties and decomposed transform progress.
    /// Numeric properties retain velocity; matrix replacement retains position.
    #[must_use]
    pub fn spring(mut self, spring: SpringDescription) -> Self {
        self.motion = MotionSpec::Spring(spring);
        self
    }
}

impl std::fmt::Debug for AnimatedContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedContainer")
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedContainer`] — owns independent property trajectories.
#[derive(Debug)]
pub struct AnimatedContainerState {
    alignment: PropertyMotion<Alignment>,
    padding: PropertyMotion<EdgeInsets>,
    color: PropertyMotion<Color>,
    width: PropertyMotion<f64>,
    height: PropertyMotion<f64>,
    margin: PropertyMotion<EdgeInsets>,
    transform_progress: AnimatedValue<f64>,
    transform: TransformTween,
    notifications: Rc<ChangeNotifier>,
    vsync: Option<Vsync>,
    child: BoxedView,
}

impl StatefulView for AnimatedContainer {
    type State = AnimatedContainerState;

    fn create_state(&self) -> Self::State {
        let notifications = Rc::new(ChangeNotifier::new());
        let transform_progress = AnimatedValue::new(0.0, self.motion.clone(), None)
            .expect("BUG: initial transform progress is finite");
        let weak = Rc::downgrade(&notifications);
        transform_progress
            .animation()
            .add_observer(Rc::new(move |recovery| {
                if let Some(notifications) = weak.upgrade() {
                    notifications.notify_listeners_with_recovery(recovery);
                }
            }));
        AnimatedContainerState {
            alignment: PropertyMotion::new(
                self.alignment.filter(finite),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial alignment was made finite"),
            padding: PropertyMotion::new(
                self.padding.filter(finite),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial padding was made finite"),
            color: PropertyMotion::new(self.color, self.motion.clone(), &notifications, None)
                .expect("BUG: color components are finite"),
            width: PropertyMotion::new(
                self.width.filter(finite),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial width was made finite"),
            height: PropertyMotion::new(
                self.height.filter(finite),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial height was made finite"),
            margin: PropertyMotion::new(
                self.margin.filter(finite),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial margin was made finite"),
            transform: TransformTween::at_rest(
                self.transform.filter(|m| m.m.iter().all(|v| v.is_finite())),
            ),
            transform_progress,
            notifications,
            vsync: None,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedContainer> for AnimatedContainerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        for result in [
            self.alignment.rebind(self.vsync.as_ref()),
            self.padding.rebind(self.vsync.as_ref()),
            self.color.rebind(self.vsync.as_ref()),
            self.width.rebind(self.vsync.as_ref()),
            self.height.rebind(self.vsync.as_ref()),
            self.margin.rebind(self.vsync.as_ref()),
            self.transform_progress.rebind(self.vsync.as_ref()),
        ] {
            if let Err(error) = result {
                tracing::error!(%error, "container animation has no clock");
            }
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.init_state(ctx);
    }

    fn build(&self, _view: &AnimatedContainer, _ctx: &dyn BuildContext) -> impl IntoView {
        let progress = self.transform_progress.animation();
        let alignment = self.alignment.animation();
        let padding = self.padding.animation();
        let color = self.color.animation();
        let width = self.width.animation();
        let height = self.height.animation();
        let margin = self.margin.animation();
        let transform = self.transform.clone();
        let child = self.child.clone();
        AnimatedBuilder::new(self.notifications.clone(), move || {
            // An overshooting curve extrapolates the tweens past their targets; the
            // insets and the size are clamped into their non-negative domain here,
            // where they meet the property (ADR-0149).
            let t = progress.value();
            let mut container = Container::new();
            if let Some(value) = &alignment {
                container = container.alignment(value.value());
            }
            if let Some(value) = &padding {
                container = container.padding(value.value().clamp_non_negative());
            }
            if let Some(value) = &color {
                container = container.color(value.value());
            }
            if let Some(value) = &width {
                container = container.width(non_negative(value.value()));
            }
            if let Some(value) = &height {
                container = container.height(non_negative(value.value()));
            }
            if let Some(value) = &margin {
                container = container.margin(value.value().clamp_non_negative());
            }
            if let Some(value) = transform.current(t) {
                container = container.transform(value);
            }
            container.child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedContainer, new_view: &AnimatedContainer) {
        self.child = new_view.child.clone();
        if new_view.alignment.as_ref().is_some_and(|v| !finite(v))
            || new_view.padding.as_ref().is_some_and(|v| !finite(v))
            || new_view.width.is_some_and(|v| !v.is_finite())
            || new_view.height.is_some_and(|v| !v.is_finite())
            || new_view.margin.as_ref().is_some_and(|v| !finite(v))
            || new_view
                .transform
                .is_some_and(|m| m.m.iter().any(|v| !v.is_finite()))
        {
            tracing::warn!("container targets refused; retaining published motion");
            return;
        }
        let restart = self.transform.animates_toward(new_view.transform.as_ref());
        let restart_at = restart.then(|| self.transform_progress.value());
        let result = MotionUpdate::run_with(
            |update| {
                self.alignment.stage(
                    update,
                    new_view.alignment,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                self.padding.stage(
                    update,
                    new_view.padding,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                self.color.stage(
                    update,
                    new_view.color,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                self.width.stage(
                    update,
                    new_view.width,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                self.height.stage(
                    update,
                    new_view.height,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                self.margin.stage(
                    update,
                    new_view.margin,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
                if restart {
                    update.restart(
                        &mut self.transform_progress,
                        0.0,
                        1.0,
                        new_view.motion.clone(),
                    )
                } else {
                    let target = *self.transform_progress.target();
                    update.retarget(
                        &mut self.transform_progress,
                        target,
                        new_view.motion.clone(),
                    )
                }
            },
            || self.transform.retarget(new_view.transform, restart_at),
        );
        if let Err(error) = result {
            tracing::warn!(%error, "container motion refused; retaining its admitted properties");
        }
    }

    fn dispose(&mut self) {
        self.alignment.dispose();
        self.padding.dispose();
        self.color.dispose();
        self.width.dispose();
        self.height.dispose();
        self.margin.dispose();
        self.transform_progress.dispose();
    }
}

/// Clamp finite motion overshoot at the layout property's boundary (ADR-0149).
fn non_negative(value: f64) -> f64 {
    if value < 0.0 { 0.0 } else { value }
}

fn finite<T: TwoWayConverter>(value: &T) -> bool {
    value.to_vector().as_ref().iter().all(|v| v.is_finite())
}
