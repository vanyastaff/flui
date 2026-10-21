//! The motion configuration every implicit widget shares, the owner lifecycle
//! of a single-value implicit widget, and matrix interpolation.

use std::time::Duration;

use flui_animation::curve::Curve;
use flui_animation::{
    Animatable, AnimatedValue, AnimatedValueView, MotionSpec, SpringDescription, Tween,
    TwoWayConverter,
};
use flui_foundation::geometry::Matrix4;
use flui_view::prelude::LifecycleContext;

use crate::animated::vsync_scope::VsyncScope;

/// How an implicit widget moves toward each new target.
///
/// Every implicit widget implements this, so `duration`, `curve` and
/// `spring` behave the same on all of them: `duration` keeps the easing,
/// `curve` keeps the duration, and `spring` keeps the velocity a target
/// change inherits. The default is [`MotionSpec::default`].
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{AnimatedOpacity, MotionBuilder};
/// use flui_animation::curves;
/// use std::time::Duration;
///
/// let fade = AnimatedOpacity::new(0.5, SizedBox::shrink())
///     .curve(curves::DECELERATE)
///     .duration(Duration::from_millis(300));
/// # let _ = fade;
/// ```
pub trait MotionBuilder: Sized {
    /// The motion this widget is configured with.
    fn motion_mut(&mut self) -> &mut MotionSpec;

    /// Curve motion over `duration`, keeping the configured easing.
    #[must_use]
    fn duration(mut self, duration: Duration) -> Self {
        let motion = self.motion_mut();
        *motion = std::mem::take(motion).with_duration(duration);
        self
    }

    /// Curve motion eased by `curve`, keeping the configured duration.
    #[must_use]
    fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        let motion = self.motion_mut();
        *motion = std::mem::take(motion).with_curve(curve);
        self
    }

    /// Spring motion, which keeps the velocity a target change inherits.
    #[must_use]
    fn spring(mut self, spring: SpringDescription) -> Self {
        *self.motion_mut() = MotionSpec::Spring(spring);
        self
    }

    /// This motion, replacing the configured one.
    #[must_use]
    fn motion(mut self, motion: MotionSpec) -> Self {
        *self.motion_mut() = motion;
        self
    }
}

/// The owner of one implicitly animated value: created at rest, bound to the
/// ambient [`VsyncScope`], retargeted on each rebuild and disposed with its
/// state. A refused command keeps the published run and is logged under the
/// widget's `label`.
#[derive(Debug)]
pub(crate) struct ImplicitMotion<T: TwoWayConverter + 'static> {
    label: &'static str,
    owner: AnimatedValue<T>,
}

impl<T: TwoWayConverter + 'static> ImplicitMotion<T> {
    /// At rest at `initial`, or at `fallback` when a component of `initial`
    /// is not finite: the motion core publishes only finite values.
    pub(crate) fn new(label: &'static str, initial: T, fallback: T, motion: &MotionSpec) -> Self {
        let finite = initial
            .to_vector()
            .as_ref()
            .iter()
            .all(|component| component.is_finite());
        let start = if finite { initial } else { fallback };
        let owner = AnimatedValue::with_debug_label(label, start, motion.clone(), None)
            .expect("BUG: an implicit widget starts from a finite value");
        Self { label, owner }
    }

    /// Follow the ambient clock: call from `init_state` and
    /// `did_change_dependencies`.
    pub(crate) fn bind(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.owner.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, label = self.label, "implicit animation has no clock");
        }
    }

    /// Move toward `target`; returns whether the motion admitted it.
    pub(crate) fn retarget(&mut self, target: T, motion: &MotionSpec) -> bool {
        match self.owner.retarget(target, motion.clone()) {
            Ok(_run) => true,
            Err(error) => {
                tracing::warn!(
                    %error,
                    label = self.label,
                    "implicit motion refused; retaining the published run"
                );
                false
            }
        }
    }

    /// Move toward `target`, running `admitted` once the motion admits it and
    /// before the new run is published, so state that layout, hit testing or
    /// semantics read alongside the value is committed even when a listener
    /// then fails; returns whether the motion admitted it.
    pub(crate) fn retarget_then(
        &mut self,
        target: T,
        motion: &MotionSpec,
        admitted: impl FnOnce(),
    ) -> bool {
        let result = flui_animation::MotionUpdate::run_with(
            |update| update.retarget(&mut self.owner, target, motion.clone()),
            admitted,
        );
        match result {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    %error,
                    label = self.label,
                    "implicit motion refused; retaining the published run"
                );
                false
            }
        }
    }

    /// Move from `origin` to `target`, for a scalar progress whose consumer
    /// re-anchors a value that is not a vector, running `admitted` once the
    /// motion admits it and before the new run is published; returns whether
    /// it was admitted. A refused or panicking preparation never runs
    /// `admitted`, so the consumer's anchors change only with the run.
    pub(crate) fn restart_then(
        &mut self,
        origin: T,
        target: T,
        motion: &MotionSpec,
        admitted: impl FnOnce(),
    ) -> bool {
        let result = flui_animation::MotionUpdate::run_with(
            |update| update.restart(&mut self.owner, origin, target, motion.clone()),
            admitted,
        );
        match result {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    %error,
                    label = self.label,
                    "implicit motion refused; retaining the published run"
                );
                false
            }
        }
    }

    /// The displayed value.
    pub(crate) fn value(&self) -> T {
        self.owner.value()
    }

    /// The target the current run moves toward.
    pub(crate) fn target(&self) -> &T {
        self.owner.target()
    }

    /// A stream of the displayed value that stays valid across retargets.
    pub(crate) fn stream(&self) -> AnimatedValueView<T> {
        self.owner.animation()
    }

    pub(crate) fn dispose(&mut self) {
        self.owner.dispose();
    }
}

/// Optional matrix interpolation, using the decomposition contract of ADR-0149.
/// Replacement re-anchors the displayed matrix; absent endpoints snap.
#[derive(Debug, Clone)]
pub(crate) struct TransformTween {
    tween: Option<Tween<Matrix4>>,
}

impl TransformTween {
    pub(crate) fn at_rest(target: Option<Matrix4>) -> Self {
        Self {
            tween: target.map(|value| Tween::new(value, value)),
        }
    }

    pub(crate) fn current(&self, progress: f64) -> Option<Matrix4> {
        self.tween.as_ref().map(|tween| tween.transform(progress))
    }

    pub(crate) fn animates_toward(&self, target: Option<&Matrix4>) -> bool {
        matches!((target, &self.tween), (Some(target), Some(existing)) if existing.end != *target)
    }

    pub(crate) fn retarget(&mut self, target: Option<Matrix4>, restart_at: Option<f64>) {
        match (target, self.tween.as_ref()) {
            (Some(target), Some(existing)) => {
                if let Some(progress) = restart_at {
                    self.tween = Some(Tween::new(existing.transform(progress), target));
                }
            }
            (Some(target), None) => {
                self.tween = Some(Tween::new(target, target));
            }
            (None, _) => self.tween = None,
        }
    }
}
