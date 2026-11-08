//! The private inherited boundary and presentation URL owner of a [`Router`].

use std::rc::{Rc, Weak};

use flui_view::element::ElementKind;
use flui_view::navigation::{
    NavigationIntent, NavigationOp, NavigationRefused, UrlOwner, UrlOwnerRegistration,
};
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, RoutePath, impl_inherited_view};

use super::handle::{RouterError, RouterHandle};
use super::routable::Routable;
use super::router::RouterShared;

/// Places a private marker between nested routers and builds the Navigator.
pub(super) struct RouterScope<R: Routable> {
    shared: Weak<RouterShared<R>>,
    child: BoxedView,
}

impl<R: Routable> RouterScope<R> {
    pub(super) fn new(shared: &Rc<RouterShared<R>>, child: impl IntoView) -> Self {
        Self {
            shared: Rc::downgrade(shared),
            child: child.into_view().boxed(),
        }
    }
}

impl<R: Routable> Clone for RouterScope<R> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            child: self.child.clone(),
        }
    }
}

impl<R: Routable> View for RouterScope<R> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl<R: Routable> StatefulView for RouterScope<R> {
    type State = RouterScopeState<R>;

    fn create_state(&self) -> Self::State {
        RouterScopeState {
            shared: self.shared.clone(),
            registration: None,
        }
    }
}

/// Owns exactly this scope's registration generation. Reparenting withdraws
/// the old generation before considering the new ancestor and slot.
pub(super) struct RouterScopeState<R: Routable> {
    shared: Weak<RouterShared<R>>,
    registration: Option<UrlOwnerRegistration>,
}

impl<R: Routable> RouterScopeState<R> {
    fn sync_registration(&mut self, cx: &dyn LifecycleContext) {
        let nested = cx.depend_on::<RouterScopeMarker, _>(|_| ()).is_some();
        let slot = if nested { None } else { cx.url_owner_slot() };
        let previous = self.registration.take();
        drop(previous);
        let (Some(slot), Some(shared)) = (slot, self.shared.upgrade()) else {
            return;
        };
        let owner: Rc<dyn UrlOwner> = Rc::new(RouterUrlOwner::<R> {
            shared: Rc::downgrade(&shared),
        });
        match slot.try_register(owner) {
            Ok(registration) => self.registration = Some(registration),
            Err(error) => {
                tracing::warn!(%error, "the outermost Router could not claim the URL-owner slot");
            }
        }
    }
}

impl<R: Routable> ViewState<RouterScope<R>> for RouterScopeState<R> {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.sync_registration(cx);
    }

    fn did_change_dependencies(&mut self, cx: &dyn LifecycleContext) {
        self.sync_registration(cx);
    }

    fn dispose(&mut self) {
        self.registration.take();
    }

    fn build(&self, view: &RouterScope<R>, _cx: &dyn BuildContext) -> impl IntoView {
        RouterScopeMarker {
            marker: (),
            child: view.child.clone(),
        }
    }
}

/// Private evidence that an enclosing router already owns the presentation URL.
#[derive(Clone)]
struct RouterScopeMarker {
    marker: (),
    child: BoxedView,
}

impl InheritedView for RouterScopeMarker {
    type Data = ();

    fn data(&self) -> &Self::Data {
        &self.marker
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        false
    }
}

impl_inherited_view!(RouterScopeMarker);

/// The presentation-facing, route-type-erased capability of one outer Router.
struct RouterUrlOwner<R: Routable> {
    shared: Weak<RouterShared<R>>,
}

impl<R: Routable> RouterUrlOwner<R> {
    fn unknown(shared: &RouterShared<R>, path: RoutePath) -> NavigationRefused {
        shared.notify_unknown(&path);
        NavigationRefused::NoMatch(path)
    }

    fn handle_result(
        shared: &RouterShared<R>,
        path: RoutePath,
        result: Result<(), RouterError>,
    ) -> Result<(), NavigationRefused> {
        match result {
            Ok(()) => Ok(()),
            Err(RouterError::Parse(_) | RouterError::NoRouter { .. }) => {
                Err(Self::unknown(shared, path))
            }
            Err(RouterError::Disposed | RouterError::EmptyStack) => {
                Err(NavigationRefused::NoUrlOwner)
            }
        }
    }
}

impl<R: Routable> UrlOwner for RouterUrlOwner<R> {
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused> {
        let Some(shared) = self.shared.upgrade() else {
            return Err(NavigationRefused::NoUrlOwner);
        };
        let _busy = UrlNavigationBusy::enter(&shared)?;
        let handle = RouterHandle::new(Rc::clone(&shared));
        match intent.op {
            NavigationOp::Push(path) => match R::from_path(&path) {
                Ok(route) => Self::handle_result(&shared, path, handle.push(route)),
                Err(_) => Err(Self::unknown(&shared, path)),
            },
            NavigationOp::Replace(path) => match R::from_path(&path) {
                Ok(route) => Self::handle_result(&shared, path, handle.replace(route)),
                Err(_) => Err(Self::unknown(&shared, path)),
            },
            NavigationOp::Go(path) => {
                let result = handle.go(path.as_str());
                Self::handle_result(&shared, path, result)
            }
            NavigationOp::Pop => match handle.pop() {
                Ok(_) => Ok(()),
                Err(_) => Err(NavigationRefused::NoUrlOwner),
            },
            _ => Err(NavigationRefused::NoUrlOwner),
        }
    }
}

/// Only URL navigation takes this guard. Ordinary handle callbacks may still
/// re-enter and navigate, while a nested runtime intent observes `Busy`.
struct UrlNavigationBusy<'a, R: Routable>(&'a RouterShared<R>);

impl<'a, R: Routable> UrlNavigationBusy<'a, R> {
    fn enter(shared: &'a Rc<RouterShared<R>>) -> Result<Self, NavigationRefused> {
        if !shared.mounted.get() {
            return Err(NavigationRefused::NoUrlOwner);
        }
        if shared.url_navigating.get() {
            return Err(NavigationRefused::Busy);
        }
        shared.url_navigating.set(true);
        Ok(Self(shared))
    }
}

impl<R: Routable> Drop for UrlNavigationBusy<'_, R> {
    fn drop(&mut self) {
        self.0.url_navigating.set(false);
    }
}
