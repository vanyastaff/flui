//! A traversal policy boundary carried by a plain focus-tree node.

use super::Focus;
use crate::localization::Directionality;
use flui_interaction::{
    FocusNode, FocusNodeRegistration, FocusTraversalPolicy, ReadingOrderPolicy,
    TraversalEdgeBehavior,
};
use flui_painting::typography::TextDirection;
use flui_view::prelude::*;
use std::rc::Rc;

/// Orders descendants as one traversal block without creating a focus scope.
///
/// The default reading-order policy uses inherited text direction. By default
/// an edge continues in the containing group or scope; other edge behaviors
/// loop inside the group, stop, or release presentation focus.
#[derive(Clone, Debug, StatefulView)]
pub struct FocusTraversalGroup {
    child: BoxedView,
    policy: Rc<dyn FocusTraversalPolicy>,
    edge: TraversalEdgeBehavior,
}

impl FocusTraversalGroup {
    /// Group the child's focus nodes into one policy-ordered traversal block.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            policy: Rc::new(ReadingOrderPolicy),
            edge: TraversalEdgeBehavior::ParentScope,
        }
    }
    /// Use a custom ordering policy for this group.
    #[must_use]
    pub fn policy(mut self, policy: Rc<dyn FocusTraversalPolicy>) -> Self {
        self.policy = policy;
        self
    }
    /// Select what traversal does at this group's edge.
    #[must_use]
    pub fn edge_behavior(mut self, edge: TraversalEdgeBehavior) -> Self {
        self.edge = edge;
        self
    }
}

/// Lifecycle ownership of the group's generation-checked policy registration.
#[derive(Debug)]
pub struct FocusTraversalGroupState {
    node: Rc<FocusNode>,
    registration: Option<FocusNodeRegistration>,
    policy: Rc<dyn FocusTraversalPolicy>,
    edge: TraversalEdgeBehavior,
    direction: TextDirection,
}

impl StatefulView for FocusTraversalGroup {
    type State = FocusTraversalGroupState;
    fn create_state(&self) -> Self::State {
        let node = FocusNode::with_debug_label("FocusTraversalGroup");
        node.set_can_request_focus(false);
        node.set_skip_traversal(true);
        FocusTraversalGroupState {
            node,
            registration: None,
            policy: Rc::clone(&self.policy),
            edge: self.edge,
            direction: TextDirection::Ltr,
        }
    }
}

impl FocusTraversalGroupState {
    fn register(&mut self) {
        self.registration = Some(self.node.register_traversal_group(
            Rc::clone(&self.policy),
            self.direction,
            self.edge,
        ));
    }
}

impl ViewState<FocusTraversalGroup> for FocusTraversalGroupState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.direction = Directionality::maybe_of(ctx).unwrap_or_default();
        self.register();
    }
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.direction = Directionality::maybe_of(ctx).unwrap_or_default();
        self.register();
    }
    fn did_update_view(&mut self, _old: &FocusTraversalGroup, new: &FocusTraversalGroup) {
        self.edge = new.edge;
        let previous = std::mem::replace(&mut self.policy, Rc::clone(&new.policy));
        self.register();
        drop(previous);
    }
    fn dispose(&mut self) {
        self.registration.take();
    }
    fn build(&self, view: &FocusTraversalGroup, _ctx: &dyn BuildContext) -> impl IntoView {
        Focus::with_external_node(Rc::clone(&self.node), view.child.clone())
            .include_semantics(false)
    }
}
