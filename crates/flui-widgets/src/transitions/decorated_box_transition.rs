//! [`DecoratedBoxTransition`] — paints a [`BoxDecoration`] read off an
//! [`Animation`] around its child.

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::{DecorationPosition, RenderAnimatedDecoratedBox};
use flui_painting::styling::BoxDecoration;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

use super::proxy_slot::ProxySlot;

/// Paints the [`BoxDecoration`] an [`Animation`] currently holds behind (or,
/// with [`foreground`](Self::foreground), over) its child.
///
/// Backed by `RenderAnimatedDecoratedBox`, which listens to `decoration`
/// itself: a tick repaints this node without relaying out or rebuilding the
/// element tree. A `Tween<BoxDecoration<f64>>` interpolates color, border,
/// corner radius, shadows and gradient together; a field set on one end only
/// fades in or out.
///
/// ```rust,ignore
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
/// let tween = Tween::new(
///     BoxDecoration::with_color(Color::WHITE),
///     BoxDecoration::with_color(Color::BLACK)
///         .set_border_radius(Some(BorderRadius::circular(12.0))),
/// );
/// let decoration = Rc::new(tween.animate(Rc::new(controller.clone())));
/// let card = DecoratedBoxTransition::new(decoration, Text::new("hi"));
/// controller.forward(); // the card darkens and rounds its corners
/// ```
#[derive(Clone, StatefulView)]
pub struct DecoratedBoxTransition {
    decoration: Rc<dyn Animation<BoxDecoration<f64>>>,
    position: DecorationPosition,
    child: BoxedView,
}

impl DecoratedBoxTransition {
    /// A decoration transition driven by `decoration`, painted behind
    /// `child`.
    pub fn new(decoration: Rc<dyn Animation<BoxDecoration<f64>>>, child: impl IntoView) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
            child: child.into_view().boxed(),
        }
    }

    /// Paints the decoration over the child instead of behind it.
    #[must_use]
    pub fn foreground(mut self) -> Self {
        self.position = DecorationPosition::Foreground;
        self
    }
}

impl std::fmt::Debug for DecoratedBoxTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoratedBoxTransition")
            .field("decoration", &self.decoration.value())
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

/// State for [`DecoratedBoxTransition`]: the proxy the render object listens
/// to, kept across rebuilds so a new `decoration` retargets it in place.
#[derive(Debug)]
pub struct DecoratedBoxTransitionState {
    decoration: ProxySlot<BoxDecoration<f64>>,
}

impl ViewState<DecoratedBoxTransition> for DecoratedBoxTransitionState {
    fn build(&self, view: &DecoratedBoxTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedDecoratedBoxView {
            decoration: self.decoration.proxy(),
            position: view.position,
            child: view.child.clone(),
        }
    }

    fn did_update_view(
        &mut self,
        _old_view: &DecoratedBoxTransition,
        new_view: &DecoratedBoxTransition,
    ) {
        self.decoration.retarget(&new_view.decoration);
    }
}

impl StatefulView for DecoratedBoxTransition {
    type State = DecoratedBoxTransitionState;

    fn create_state(&self) -> Self::State {
        DecoratedBoxTransitionState {
            decoration: ProxySlot::new(&self.decoration),
        }
    }
}

#[derive(Clone)]
struct AnimatedDecoratedBoxView {
    decoration: ProxyAnimation<BoxDecoration<f64>>,
    position: DecorationPosition,
    child: BoxedView,
}

impl std::fmt::Debug for AnimatedDecoratedBoxView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnimatedDecoratedBoxView")
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedDecoratedBoxView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedDecoratedBox;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut node = RenderAnimatedDecoratedBox::new(self.decoration.clone());
        let _ = node.set_position(self.position);
        node
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        render_object.set_position(self.position)
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(AnimatedDecoratedBoxView);
