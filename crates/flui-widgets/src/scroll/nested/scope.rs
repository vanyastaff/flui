//! Authored connection scope over the existing Listener coordinate origin.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::RenderId;
use flui_rendering::pipeline::WeakPipelineCell;
use flui_view::prelude::StatefulView;
use flui_view::{
    BoxedView, BuildContext, BuildContextExt, Child, InheritedView, IntoView, LifecycleContext,
    View, ViewExt, ViewState, impl_inherited_view,
};

use super::{
    connection::Connection,
    dispatcher::NestedScrollDispatcher,
    link::ConnectionLink,
    origin::ScrollOrigin,
    types::{NestedScrollConnection, OverscrollBehavior},
};
use crate::Listener;

type DispatcherSlot = Rc<RefCell<Option<Rc<NestedScrollDispatcher>>>>;

#[derive(Clone)]
enum Mount {
    Authored(Rc<dyn NestedScrollConnection>),
    Owner {
        connection: Rc<crate::scroll::scrollable::FlingEndpoint>,
        axis: flui_foundation::geometry::Axis,
        overscroll: OverscrollBehavior,
        dispatcher: DispatcherSlot,
    },
}

impl Mount {
    fn connection(&self) -> Connection {
        match self {
            Self::Authored(connection) => Connection::Authored(connection.clone()),
            Self::Owner {
                connection,
                axis,
                overscroll,
                ..
            } => Connection::Owner {
                connection: connection.clone(),
                axis: *axis,
                overscroll: *overscroll,
            },
        }
    }

    fn slot(&self) -> Option<&DispatcherSlot> {
        match self {
            Self::Authored(_) => None,
            Self::Owner { dispatcher, .. } => Some(dispatcher),
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.connection().same(&other.connection())
            && match (self.slot(), other.slot()) {
                (None, None) => true,
                (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                _ => false,
            }
    }
}

/// Adds synchronous pre/post scroll participation around descendant owners.
///
/// The connection may capture owner-local `Rc` state. Replacing its identity or
/// unmounting withdraws that exact link; an accepted sequence never discovers
/// a replacement through a label or its old place in the tree.
#[derive(Clone, StatefulView)]
pub struct NestedScroll {
    mount: Mount,
    child: Child,
}

impl NestedScroll {
    /// Author one connection; cloning the view still creates distinct mount links.
    #[must_use]
    pub fn new(connection: Rc<dyn NestedScrollConnection>) -> Self {
        Self {
            mount: Mount::Authored(connection),
            child: Child::empty(),
        }
    }

    pub(crate) fn owner(
        connection: Rc<crate::scroll::scrollable::FlingEndpoint>,
        axis: flui_foundation::geometry::Axis,
        overscroll: OverscrollBehavior,
        dispatcher: DispatcherSlot,
    ) -> Self {
        Self {
            mount: Mount::Owner {
                connection,
                axis,
                overscroll,
                dispatcher,
            },
            child: Child::empty(),
        }
    }

    /// The scrollable content this connection participates around.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl std::fmt::Debug for NestedScroll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NestedScroll").finish_non_exhaustive()
    }
}

/// Mounted connection identity and its owner-local sequence participation.
pub struct NestedScrollState {
    mount: Option<Mount>,
    node: Rc<Cell<Option<RenderId>>>,
    link: Option<Rc<ConnectionLink>>,
}

impl std::fmt::Debug for NestedScrollState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NestedScrollState")
            .field("mounted", &self.link.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for NestedScroll {
    type State = NestedScrollState;

    fn create_state(&self) -> Self::State {
        NestedScrollState {
            mount: Some(self.mount.clone()),
            node: Rc::new(Cell::new(None)),
            link: None,
        }
    }
}

impl NestedScrollState {
    fn install(
        &mut self,
        parent: Option<Weak<ConnectionLink>>,
        pipeline: Option<WeakPipelineCell>,
    ) {
        let mount = self
            .mount
            .as_ref()
            .expect("BUG: installed nested state owns its mount");
        let incoming = ConnectionLink::new(
            mount.connection().downgrade(),
            parent,
            self.node.clone(),
            pipeline,
        );
        let outgoing = self.link.replace(incoming.clone());
        let outgoing_dispatcher = mount
            .slot()
            .and_then(|slot| slot.replace(Some(Rc::new(NestedScrollDispatcher::new(incoming)))));
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if let Some(outgoing) = outgoing_dispatcher {
            recovery.run(|| outgoing.close());
            recovery.retire(outgoing);
        }
        if let Some(outgoing) = outgoing {
            recovery.run(|| outgoing.close());
            recovery.retire(outgoing);
        }
        recovery.finish();
    }
}

impl ViewState<NestedScroll> for NestedScrollState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.install(
            ConnectionScope::parent_of(ctx),
            ctx.pipeline_owner().map(|owner| owner.downgrade()),
        );
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let parent = ConnectionScope::parent_of(ctx);
        let pipeline = ctx.pipeline_owner().map(|owner| owner.downgrade());
        if let Some(current) = &self.link {
            let same_parent = match (&current.parent, &parent) {
                (None, None) => true,
                (Some(old), Some(new)) => Weak::ptr_eq(old, new),
                _ => false,
            };
            let same_pipeline = match (&current.pipeline, &pipeline) {
                (None, None) => true,
                (Some(old), Some(new)) => old
                    .upgrade()
                    .zip(new.upgrade())
                    .is_some_and(|(old, new)| old.ptr_eq(&new)),
                _ => false,
            };
            if same_parent && same_pipeline {
                return;
            }
        }
        self.install(parent, pipeline);
    }

    fn build(&self, view: &NestedScroll, _ctx: &dyn BuildContext) -> impl IntoView {
        let link = self
            .link
            .as_ref()
            .expect("BUG: nested connection was not installed during lifecycle")
            .clone();
        Listener::new().child(ScrollOrigin {
            node: self.node.clone(),
            child: ConnectionScope {
                link,
                child: view
                    .child
                    .clone()
                    .into_inner()
                    .unwrap_or_else(|| crate::SizedBox::shrink().boxed()),
            }
            .boxed(),
        })
    }

    fn did_update_view(&mut self, _old_view: &NestedScroll, view: &NestedScroll) {
        if self
            .mount
            .as_ref()
            .is_some_and(|mount| mount.same(&view.mount))
        {
            return;
        }
        let outgoing = self.mount.replace(view.mount.clone());
        let outgoing_dispatcher = outgoing
            .as_ref()
            .and_then(Mount::slot)
            .and_then(|slot| slot.borrow_mut().take());
        let (parent, pipeline) = self.link.as_ref().map_or((None, None), |link| {
            (link.parent.clone(), link.pipeline.clone())
        });
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        recovery.run(|| self.install(parent, pipeline));
        if let Some(dispatcher) = outgoing_dispatcher {
            recovery.run(|| dispatcher.close());
            recovery.retire(dispatcher);
        }
        recovery.retire(outgoing);
        recovery.finish();
    }

    fn dispose(&mut self) {
        let mount = self.mount.take();
        let outgoing = mount
            .as_ref()
            .and_then(Mount::slot)
            .and_then(|slot| slot.borrow_mut().take());
        let link = self.link.take();
        self.node.set(None);
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if let Some(outgoing) = outgoing {
            recovery.run(|| outgoing.close());
            recovery.retire(outgoing);
        }
        if let Some(link) = link {
            recovery.run(|| link.close());
            recovery.retire(link);
        }
        recovery.retire(mount);
        recovery.finish();
    }
}

#[derive(Clone)]
pub(super) struct ConnectionScope {
    pub(super) link: Rc<ConnectionLink>,
    pub(super) child: BoxedView,
}

impl ConnectionScope {
    pub(super) fn parent_of(ctx: &dyn BuildContext) -> Option<Weak<ConnectionLink>> {
        ctx.depend_on::<Self, _>(|scope| Rc::downgrade(&scope.link))
    }
}

impl InheritedView for ConnectionScope {
    type Data = Rc<ConnectionLink>;
    fn data(&self) -> &Self::Data {
        &self.link
    }
    fn child(&self) -> &dyn View {
        &self.child
    }
    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.link, &old.link)
    }
}

impl_inherited_view!(ConnectionScope);
