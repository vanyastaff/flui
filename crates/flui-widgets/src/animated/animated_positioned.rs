//! [`AnimatedPositioned`] — a [`Positioned`] whose edges and size animate
//! when they change.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec, MotionUpdate, Vsync, VsyncUpdate};
use flui_foundation::ChangeNotifier;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::MotionBuilder;
use crate::animated::property_motion::PropertyMotion;
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Positioned};

/// The six placement properties, in the order they are staged.
const LABELS: [&str; 6] = [
    "AnimatedPositioned.left",
    "AnimatedPositioned.top",
    "AnimatedPositioned.right",
    "AnimatedPositioned.bottom",
    "AnimatedPositioned.width",
    "AnimatedPositioned.height",
];

/// A [`Stack`](crate::Stack) child placed like [`Positioned`], whose edges and
/// size animate to each new value.
///
/// Each property set on both the old and the new build moves from the value
/// shown, keeping its own velocity. A property that appears or disappears
/// snaps, as there is nothing to interpolate from or to. A placement with a
/// non-finite value is refused as a whole: a later one keeps the current
/// motion, and a first one leaves the child unplaced.
///
/// ```
/// use flui_widgets::{AnimatedPositioned, MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let badge = AnimatedPositioned::new(SizedBox::new(16.0, 16.0))
///     .top(8.0)
///     .right(8.0)
///     .duration(Duration::from_millis(250));
/// # let _ = badge;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedPositioned {
    /// `left`, `top`, `right`, `bottom`, `width`, `height`.
    placement: [Option<f64>; 6],
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedPositioned {
    /// A child with no placement yet: chain the edge and size builders.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            placement: [None; 6],
            motion: MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }

    /// Distance from the stack's left edge.
    #[must_use]
    pub fn left(mut self, left: f64) -> Self {
        self.placement[0] = Some(left);
        self
    }

    /// Distance from the stack's top edge.
    #[must_use]
    pub fn top(mut self, top: f64) -> Self {
        self.placement[1] = Some(top);
        self
    }

    /// Distance from the stack's right edge.
    #[must_use]
    pub fn right(mut self, right: f64) -> Self {
        self.placement[2] = Some(right);
        self
    }

    /// Distance from the stack's bottom edge.
    #[must_use]
    pub fn bottom(mut self, bottom: f64) -> Self {
        self.placement[3] = Some(bottom);
        self
    }

    /// The child's width.
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.placement[4] = Some(width);
        self
    }

    /// The child's height.
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.placement[5] = Some(height);
        self
    }
}

impl MotionBuilder for AnimatedPositioned {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedPositioned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedPositioned")
            .field("placement", &self.placement)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns one motion per placement property and the channel the builder
/// observes.
#[derive(Debug)]
pub struct AnimatedPositionedState {
    properties: [PropertyMotion<f64>; 6],
    notifications: Rc<ChangeNotifier>,
    vsync: Option<Vsync>,
    child: BoxedView,
}

impl StatefulView for AnimatedPositioned {
    type State = AnimatedPositionedState;

    fn create_state(&self) -> Self::State {
        let notifications = Rc::new(ChangeNotifier::new());
        // A placement with any non-finite value is refused as a whole, as an
        // update would be: the child starts unplaced (aligned by the stack).
        let admitted = self
            .placement
            .iter()
            .flatten()
            .all(|value| value.is_finite());
        if !admitted {
            tracing::warn!("initial placement refused; the child starts unplaced");
        }
        let properties = std::array::from_fn(|index| {
            PropertyMotion::new(
                LABELS[index],
                self.placement[index].filter(|_| admitted),
                self.motion.clone(),
                &notifications,
                None,
            )
            .expect("BUG: an initial placement property was made finite")
        });
        AnimatedPositionedState {
            properties,
            notifications,
            vsync: None,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedPositioned> for AnimatedPositionedState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        if let Err(error) = VsyncUpdate::run(|update| {
            for property in &mut self.properties {
                property.stage_binding(update, self.vsync.as_ref());
            }
        }) {
            tracing::error!(%error, "placement animation has no clock");
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.init_state(ctx);
    }

    fn build(&self, _view: &AnimatedPositioned, _ctx: &dyn BuildContext) -> impl IntoView {
        let streams: [Option<AnimatedValueView<f64>>; 6] =
            std::array::from_fn(|index| self.properties[index].animation());
        let child = self.child.clone();
        AnimatedBuilder::new(self.notifications.clone(), move || {
            let value = |index: usize| streams[index].as_ref().map(Animation::value);
            let mut positioned = Positioned::new(child.clone());
            if let Some(left) = value(0) {
                positioned = positioned.left(left);
            }
            if let Some(top) = value(1) {
                positioned = positioned.top(top);
            }
            if let Some(right) = value(2) {
                positioned = positioned.right(right);
            }
            if let Some(bottom) = value(3) {
                positioned = positioned.bottom(bottom);
            }
            // Layout receives a non-negative size even while motion overshoots.
            if let Some(width) = value(4) {
                positioned = positioned.width(width.max(0.0));
            }
            if let Some(height) = value(5) {
                positioned = positioned.height(height.max(0.0));
            }
            positioned
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedPositioned, new_view: &AnimatedPositioned) {
        self.child = new_view.child.clone();
        if new_view
            .placement
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            tracing::warn!("placement targets refused; retaining the published motion");
            return;
        }
        let result = MotionUpdate::run(|update| {
            for (property, target) in self.properties.iter_mut().zip(new_view.placement) {
                property.stage(
                    update,
                    target,
                    new_view.motion.clone(),
                    &self.notifications,
                    self.vsync.as_ref(),
                )?;
            }
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!(%error, "placement motion refused; retaining its admitted properties");
        }
    }

    fn dispose(&mut self) {
        MotionUpdate::run(|update| {
            for property in &mut self.properties {
                property.stage_disposal(update);
            }
            Ok(())
        })
        .expect("BUG: retiring motion owners has no preparation refusal");
    }
}
