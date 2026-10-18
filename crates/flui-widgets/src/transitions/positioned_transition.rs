//! [`PositionedTransition`] — moves a [`Stack`](crate::Stack) child's
//! rectangle from an [`Animation<RelativeRect>`].

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::geometry::RelativeRect;
use flui_objects::RenderAnimatedInset;
use flui_painting::paint::Clip;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

use super::proxy_slot::ProxySlot;
use crate::Positioned;

/// Places a child of a [`Stack`](crate::Stack) at a [`RelativeRect`] (insets
/// from the stack's edges) read off an [`Animation`], as a [`Positioned`]
/// child with all four edges set.
///
/// Must be a direct child of a `Stack`: it fills the stack and insets the
/// child inside a `RenderAnimatedInset`, which listens to `rect` itself, so a
/// tick relays out this node without rebuilding the element tree. The child
/// is laid out tight to the rectangle; insets may be negative, placing it
/// partly outside the stack, where it is clipped as
/// [`clip_behavior`](Self::clip_behavior) says. A non-finite inset, or one
/// whose total with the opposite inset overflows, is ignored and the last
/// admitted rectangle stays.
///
/// ```rust,ignore
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
/// let tween = Tween::new(
///     RelativeRect::from_ltrb(0.0, 0.0, 0.0, 0.0),
///     RelativeRect::from_ltrb(100.0, 200.0, 100.0, 200.0),
/// );
/// let rect = Rc::new(tween.animate(Rc::new(controller.clone())));
/// let stack = Stack::new((PositionedTransition::new(rect, card()).boxed(),));
/// controller.forward(); // the card shrinks toward the middle
/// ```
#[derive(Clone, StatefulView)]
pub struct PositionedTransition {
    rect: Rc<dyn Animation<RelativeRect>>,
    clip_behavior: Clip,
    child: BoxedView,
}

impl PositionedTransition {
    /// A positioned transition driven by `rect`, placing `child`.
    pub fn new(rect: Rc<dyn Animation<RelativeRect>>, child: impl IntoView) -> Self {
        Self {
            rect,
            clip_behavior: Clip::HardEdge,
            child: child.into_view().boxed(),
        }
    }

    /// Sets how the child is clipped where its rectangle leaves the stack
    /// (default [`Clip::HardEdge`], a stack's own default). The stack sees
    /// this transition fill it exactly, so the stack's clip never applies;
    /// pass [`Clip::None`] under a stack that does not clip.
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }
}

impl std::fmt::Debug for PositionedTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PositionedTransition")
            .field("rect", &self.rect.value())
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

/// State for [`PositionedTransition`]: the proxy the render object listens
/// to, kept across rebuilds so a new `rect` retargets it in place.
#[derive(Debug)]
pub struct PositionedTransitionState {
    rect: ProxySlot<RelativeRect>,
}

impl ViewState<PositionedTransition> for PositionedTransitionState {
    fn build(&self, view: &PositionedTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        Positioned::fill(AnimatedInsetView {
            rect: self.rect.proxy(),
            clip_behavior: view.clip_behavior,
            child: view.child.clone(),
        })
    }

    fn did_update_view(
        &mut self,
        _old_view: &PositionedTransition,
        new_view: &PositionedTransition,
    ) {
        self.rect.retarget(&new_view.rect);
    }
}

impl StatefulView for PositionedTransition {
    type State = PositionedTransitionState;

    fn create_state(&self) -> Self::State {
        PositionedTransitionState {
            rect: ProxySlot::new(&self.rect),
        }
    }
}

#[derive(Clone)]
struct AnimatedInsetView {
    rect: ProxyAnimation<RelativeRect>,
    clip_behavior: Clip,
    child: BoxedView,
}

impl std::fmt::Debug for AnimatedInsetView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnimatedInsetView")
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedInsetView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedInset;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut node = RenderAnimatedInset::new(self.rect.clone());
        let _ = node.set_clip_behavior(self.clip_behavior);
        node
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        render_object.set_clip_behavior(self.clip_behavior)
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(AnimatedInsetView);
