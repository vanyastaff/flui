//! [`AlignTransition`] — moves its child's alignment from an
//! [`Animation<Alignment>`].

use std::rc::Rc;

use flui_animation::Animation;
use flui_objects::AlignMotion;
use flui_painting::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::align_view::AnimatedAlignView;
use super::proxy_slot::ProxySlot;

/// Aligns its child within itself at an [`Alignment`] read off an
/// [`Animation`], sizing itself as an `Align` does.
///
/// Backed by `RenderAnimatedAlign`, which listens to `alignment` itself: a
/// tick relays out this node without rebuilding the element tree. A
/// non-finite alignment is ignored and the last finite one stays.
///
/// ```rust,ignore
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
/// let tween = Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT);
/// let alignment = Rc::new(tween.animate(Rc::new(controller.clone())));
/// let moving = AlignTransition::new(alignment, Text::new("hi"));
/// controller.forward(); // the child crosses the box diagonally
/// ```
#[derive(Clone, StatefulView)]
pub struct AlignTransition {
    alignment: Rc<dyn Animation<Alignment>>,
    width_factor: Option<f64>,
    height_factor: Option<f64>,
    child: BoxedView,
}

impl AlignTransition {
    /// An alignment transition driven by `alignment`, placing `child`.
    pub fn new(alignment: Rc<dyn Animation<Alignment>>, child: impl IntoView) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            child: child.into_view().boxed(),
        }
    }

    /// Sizes the box to the child's width times `factor` instead of filling
    /// the width constraint.
    #[must_use]
    pub fn width_factor(mut self, factor: f64) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Sizes the box to the child's height times `factor` instead of filling
    /// the height constraint.
    #[must_use]
    pub fn height_factor(mut self, factor: f64) -> Self {
        self.height_factor = Some(factor);
        self
    }
}

impl std::fmt::Debug for AlignTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlignTransition")
            .field("alignment", &self.alignment.value())
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
            .finish_non_exhaustive()
    }
}

/// State for [`AlignTransition`]: the proxy the render object listens to,
/// kept across rebuilds so a new `alignment` retargets it in place.
#[derive(Debug)]
pub struct AlignTransitionState {
    alignment: ProxySlot<Alignment>,
}

impl ViewState<AlignTransition> for AlignTransitionState {
    fn build(&self, view: &AlignTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedAlignView {
            motion: AlignMotion::Alignment(self.alignment.proxy()),
            alignment: Alignment::CENTER,
            width_factor: view.width_factor,
            height_factor: view.height_factor,
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AlignTransition, new_view: &AlignTransition) {
        self.alignment.retarget(&new_view.alignment);
    }
}

impl StatefulView for AlignTransition {
    type State = AlignTransitionState;

    fn create_state(&self) -> Self::State {
        AlignTransitionState {
            alignment: ProxySlot::new(&self.alignment),
        }
    }
}
