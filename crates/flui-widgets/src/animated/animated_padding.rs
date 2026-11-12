//! `AnimatedPadding` retains component velocities when its target changes.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec};
use flui_foundation::geometry::EdgeInsets;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{Child, IntoView, ViewState};

use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};
use crate::{AnimatedBuilder, Padding};

/// Animates its child's padding with independent component velocities.
/// The initial build rests at the configured insets; later targets flow from
/// the last layout sample. An ambient `VsyncScope` drives the motion.
#[derive(Clone, StatefulView)]
pub struct AnimatedPadding {
    padding: EdgeInsets,
    motion: MotionSpec,
    child: Child,
}

impl AnimatedPadding {
    /// Animate these insets with a 200 ms ease-in-out transition.
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            motion: MotionSpec::default(),
            child: Child::empty(),
        }
    }

    /// Sets the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl MotionBuilder for AnimatedPadding {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedPadding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedPadding")
            .field("padding", &self.padding)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the motion and the stable stream observed by the layout builder.
#[derive(Debug)]
pub struct AnimatedPaddingState {
    animation: ImplicitMotion<EdgeInsets>,
    stream: Rc<AnimatedValueView<EdgeInsets>>,
    child: Child,
}

impl StatefulView for AnimatedPadding {
    type State = AnimatedPaddingState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        let animation = ImplicitMotion::new(
            "AnimatedPadding",
            self.padding,
            EdgeInsets::ZERO,
            &self.motion,
        );
        let stream = Rc::new(animation.stream());
        AnimatedPaddingState {
            animation,
            stream,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedPadding> for AnimatedPaddingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, _view: &AnimatedPadding, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let child = self.child.clone();
        AnimatedBuilder::new(self.stream.clone(), move || {
            // Layout receives non-negative padding even while motion overshoots.
            Padding::new(stream.value().clamp_non_negative()).child_slot(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedPadding, new_view: &AnimatedPadding) {
        self.child = new_view.child.clone();
        self.animation.retarget(new_view.padding, &new_view.motion);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
