//! [`AnimatedBuilder`] — rebuilds a subtree each time a [`Listenable`] changes.

use std::{rc::Rc, sync::Arc};

use flui_foundation::Listenable;
use flui_view::prelude::BuildContext;
use flui_view::{
    AnimatedView, BoxedView, IntoView, StatefulView, ViewExt, ViewState, impl_animated_view,
};

/// The boxed rebuild closure: invoked on every tick to produce the current
/// subtree. `Rc` keeps the owning [`AnimatedBuilder`] cheap to clone when the
/// view is re-cloned on rebuild; the closure itself is UI-owner-local.
type BuilderFn = Rc<dyn Fn() -> BoxedView>;

/// Rebuilds whatever its `builder` returns whenever `listenable` notifies.
///
/// Flutter parity: `widgets/transitions.dart` `AnimatedBuilder` — the general
/// reactive primitive the explicit transitions (`FadeTransition`, …) are
/// special cases of. Hand it an animation (or any [`Listenable`]) and a closure
/// that reads the animation's current value; each notification schedules a
/// rebuild that re-invokes the closure.
///
/// The closure captures the value source by clone (an `Animation`, a `Tween`),
/// so it re-reads the live value on every call. It is the spine of FLUI's
/// implicitly-animated widgets: each holds a persistent controller and returns
/// an `AnimatedBuilder` over it, so only this inner builder rebuilds per frame —
/// the implicit widget itself rebuilds solely on a configuration change.
#[derive(Clone)]
pub struct AnimatedBuilder {
    listenable: Arc<dyn Listenable>,
    builder: BuilderFn,
}

impl AnimatedBuilder {
    /// Rebuild `builder()` whenever `listenable` notifies.
    pub fn new<V, F>(listenable: Arc<dyn Listenable>, builder: F) -> Self
    where
        V: IntoView,
        F: Fn() -> V + 'static,
    {
        Self {
            listenable,
            builder: Rc::new(move || builder().into_view().boxed()),
        }
    }
}

impl std::fmt::Debug for AnimatedBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedBuilder").finish_non_exhaustive()
    }
}

/// State for [`AnimatedBuilder`]. Stateless beyond the listenable subscription
/// [`AnimatedView`] manages — the value lives on the captured animation.
#[derive(Debug)]
pub struct AnimatedBuilderState;

impl ViewState<AnimatedBuilder> for AnimatedBuilderState {
    fn build(&self, view: &AnimatedBuilder, _ctx: &dyn BuildContext) -> impl IntoView {
        (view.builder)()
    }
}

impl StatefulView for AnimatedBuilder {
    type State = AnimatedBuilderState;

    fn create_state(&self) -> Self::State {
        AnimatedBuilderState
    }
}

impl AnimatedView for AnimatedBuilder {
    fn listenable(&self) -> Arc<dyn Listenable> {
        self.listenable.clone()
    }
}

impl_animated_view!(AnimatedBuilder);
