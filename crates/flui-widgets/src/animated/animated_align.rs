//! [`AnimatedAlign`] — animates its child's alignment when the target changes.

use std::{rc::Rc, time::Duration};

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    AnimatedValue, Animation, MotionSpec, MotionUpdate, SpringDescription, TwoWayConverter, Vsync,
    VsyncUpdate,
};
use flui_foundation::ChangeNotifier;
use flui_painting::Alignment;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, default_curve};
use crate::animated::property_motion::{PropertyMotion, observed_property};
use crate::animated::vsync_scope::VsyncScope;
use crate::{Align, AnimatedBuilder};

/// Animates the [`Alignment`] of its child within itself whenever a new
/// alignment is given.
///
/// The first build rests at its configured values. Later targets retain the
/// displayed alignment and factor velocities, using curve or spring motion.
/// Unchanged properties keep their existing trajectories and deadlines.
/// Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedAlign {
    alignment: Alignment,
    width_factor: Option<f64>,
    height_factor: Option<f64>,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedAlign {
    /// Size the box to `factor` x the child's width, animating the factor when
    /// it changes.
    ///
    /// A present factor retains its own velocity when changed. Appearing or
    /// disappearing factors snap. Leaving it unset is not the same as
    /// setting `1.0`: an unset factor makes the box fill its constraints on
    /// that axis. Negative motion overshoot is clamped to zero at layout.
    #[must_use]
    pub fn width_factor(mut self, factor: f64) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size the box to `factor` x the child's height, animating the factor when
    /// it changes. See [`width_factor`](Self::width_factor).
    #[must_use]
    pub fn height_factor(mut self, factor: f64) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Animate `child` toward `alignment`, with the 200 ms default duration and
    /// an ease-in-out curve.
    pub fn new(alignment: Alignment, child: impl IntoView) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            motion: MotionSpec::Curve {
                duration: DEFAULT_DURATION,
                curve: default_curve(),
            },
            child: child.into_view().boxed(),
        }
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

    /// Select spring motion, retaining alignment and factor velocities.
    #[must_use]
    pub fn spring(mut self, spring: SpringDescription) -> Self {
        self.motion = MotionSpec::Spring(spring);
        self
    }
}

impl std::fmt::Debug for AnimatedAlign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedAlign")
            .field("alignment", &self.alignment)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedAlign`] — owns the persistent alignment animation.
#[derive(Debug)]
pub struct AnimatedAlignState {
    alignment: AnimatedValue<Alignment>,
    width_factor: PropertyMotion<f64>,
    height_factor: PropertyMotion<f64>,
    notifications: Rc<ChangeNotifier>,
    vsync: Option<Vsync>,
    child: BoxedView,
}

impl StatefulView for AnimatedAlign {
    type State = AnimatedAlignState;

    fn create_state(&self) -> Self::State {
        let notifications = Rc::new(ChangeNotifier::new());
        let alignment = if self.alignment.to_vector().iter().all(|v| v.is_finite()) {
            self.alignment
        } else {
            Alignment::CENTER
        };
        AnimatedAlignState {
            alignment: observed_property(alignment, self.motion.clone(), &notifications, None)
                .expect("BUG: initial alignment was made finite"),
            width_factor: PropertyMotion::new(
                self.width_factor.filter(|v| v.is_finite()),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial width factor was made finite"),
            height_factor: PropertyMotion::new(
                self.height_factor.filter(|v| v.is_finite()),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: initial height factor was made finite"),
            notifications,
            vsync: None,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedAlign> for AnimatedAlignState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        if let Err(error) = VsyncUpdate::run(|update| {
            update.rebind(&mut self.alignment, self.vsync.as_ref());
            self.width_factor.stage_binding(update, self.vsync.as_ref());
            self.height_factor
                .stage_binding(update, self.vsync.as_ref());
        }) {
            tracing::error!(%error, "alignment animation has no clock");
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.init_state(ctx);
    }

    fn build(&self, _view: &AnimatedAlign, _ctx: &dyn BuildContext) -> impl IntoView {
        let alignment = self.alignment.animation();
        let width_factor = self.width_factor.animation();
        let height_factor = self.height_factor.animation();
        let child = self.child.clone();
        AnimatedBuilder::new(self.notifications.clone(), move || {
            // An unset factor leaves the axis filling its constraints.
            let mut align = Align::new(alignment.value());
            if let Some(factor) = &width_factor {
                align = align.width_factor(factor.value().max(0.0));
            }
            if let Some(factor) = &height_factor {
                align = align.height_factor(factor.value().max(0.0));
            }
            align.child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedAlign, new_view: &AnimatedAlign) {
        self.child = new_view.child.clone();
        if !new_view.alignment.to_vector().iter().all(|v| v.is_finite())
            || new_view.width_factor.is_some_and(|v| !v.is_finite())
            || new_view.height_factor.is_some_and(|v| !v.is_finite())
        {
            tracing::warn!("alignment targets refused; retaining the published motion");
            return;
        }
        let result = MotionUpdate::run(|update| {
            update.retarget(
                &mut self.alignment,
                new_view.alignment,
                new_view.motion.clone(),
            )?;
            self.width_factor.stage(
                update,
                new_view.width_factor,
                new_view.motion.clone(),
                &self.notifications,
                self.vsync.as_ref(),
            )?;
            self.height_factor.stage(
                update,
                new_view.height_factor,
                new_view.motion.clone(),
                &self.notifications,
                self.vsync.as_ref(),
            )
        });
        if let Err(error) = result {
            tracing::warn!(%error, "alignment motion refused; retaining its admitted properties");
        }
    }

    fn dispose(&mut self) {
        MotionUpdate::run(|update| {
            update.dispose(&mut self.alignment);
            self.width_factor.stage_disposal(update);
            self.height_factor.stage_disposal(update);
            Ok(())
        })
        .expect("BUG: retiring motion owners has no preparation refusal");
    }
}
