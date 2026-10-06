//! [`AnimatedRotation`] — rotates its child, animating to each new [`Angle`].

use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_foundation::geometry::Angle;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::Transform;
use crate::animated::implicitly_animated::{DEFAULT_DURATION, default_curve};

/// Which way an [`AnimatedRotation`] turns toward a new angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RotationPath {
    /// Turn through the numeric difference: from 0° to 270° is three quarters of a turn
    /// forward, and from 0° to 720° is two full turns.
    #[default]
    Numeric,
    /// Turn along the shorter arc to the same orientation: from 0° to 270° is a quarter
    /// turn back. An exact half turn goes toward the increasing angle.
    Shorter,
}

/// Rotates its child about its center, animating to each new angle it is given.
///
/// The first build shows the child at `angle` with no motion; a later build with a
/// different angle turns from the angle currently shown to the new one over `duration`
/// along `curve`, the way [`path`](Self::path) says. Rotation is paint-only: the child is
/// laid out unrotated.
///
/// Driven by a binding under a [`VsyncScope`](crate::VsyncScope).
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_foundation::geometry::Angle;
/// use flui_widgets::{AnimatedRotation, RotationPath, SizedBox};
///
/// let dial = AnimatedRotation::new(Angle::from_degrees(270.0), SizedBox::new(40.0, 40.0))
///     .path(RotationPath::Shorter)
///     .duration(Duration::from_millis(300));
/// # let _ = dial;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedRotation {
    angle: Angle,
    path: RotationPath,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedRotation {
    /// Rotate `child` to `angle`, turning numerically, over the 200 ms default duration
    /// along an ease-in-out curve.
    pub fn new(angle: Angle, child: impl IntoView) -> Self {
        Self {
            angle,
            path: RotationPath::default(),
            duration: DEFAULT_DURATION,
            curve: default_curve(),
            child: child.into_view().boxed(),
        }
    }

    /// Choose how the rotation reaches a new angle.
    #[must_use]
    pub fn path(mut self, path: RotationPath) -> Self {
        self.path = path;
        self
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve; accepts any [`Curve`], including the overshooting
    /// elastic and back curves.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedRotation")
            .field("angle", &self.angle)
            .field("path", &self.path)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedRotation`].
#[derive(Debug)]
pub struct AnimatedRotationState {
    angle: Angle,
    child: BoxedView,
}

impl StatefulView for AnimatedRotation {
    type State = AnimatedRotationState;

    fn create_state(&self) -> Self::State {
        AnimatedRotationState {
            angle: self.angle,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedRotation> for AnimatedRotationState {
    fn build(&self, _view: &AnimatedRotation, _ctx: &dyn BuildContext) -> impl IntoView {
        Transform::rotation(self.angle.radians()).child(self.child.clone())
    }

    fn did_update_view(&mut self, _old_view: &AnimatedRotation, new_view: &AnimatedRotation) {
        self.angle = new_view.angle;
        self.child = new_view.child.clone();
    }
}
