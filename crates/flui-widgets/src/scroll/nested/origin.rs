//! Coordinate identity of the Listener that localizes a scroll owner's input.

use std::{cell::Cell, rc::Rc};

use flui_foundation::RenderId;
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, IntoView, LifecycleContext, ViewState};

/// This must be the Listener's immediate child: strict ancestor lookup then
/// names that Listener's render node, rather than a descendant or another box.
#[derive(Clone, StatefulView)]
pub(super) struct ScrollOrigin {
    pub(super) node: Rc<Cell<Option<RenderId>>>,
    pub(super) child: BoxedView,
}

pub(super) struct ScrollOriginState {
    node: Rc<Cell<Option<RenderId>>>,
}

impl StatefulView for ScrollOrigin {
    type State = ScrollOriginState;

    fn create_state(&self) -> Self::State {
        ScrollOriginState {
            node: self.node.clone(),
        }
    }
}

impl ViewState<ScrollOrigin> for ScrollOriginState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.node.set(ctx.find_render_object());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.node.set(ctx.find_render_object());
    }

    fn build(&self, view: &ScrollOrigin, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }

    fn did_update_view(&mut self, _old_view: &ScrollOrigin, view: &ScrollOrigin) {
        if !Rc::ptr_eq(&self.node, &view.node) {
            let current = self.node.replace(None);
            view.node.set(current);
            self.node = view.node.clone();
        }
    }

    fn dispose(&mut self) {
        self.node.set(None);
    }
}
