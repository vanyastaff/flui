//! [`RouterHandle`] and [`RouterError`].

use std::fmt;
use std::rc::Rc;

use flui_foundation::notifier::{Listenable, ListenerObserver};
use flui_foundation::{ListenerCallback, MutatedDuringBuild, Subscription};

use super::routable::Routable;
use super::router::{Editing, RouterEntry, RouterRetiredValues, RouterShared, clone_routes, paths};
use crate::navigator::{RouteId, RouteResult};
use flui_view::navigation::{RouteParseError, RoutePath};

/// An owned, cloneable capability to drive the nearest
/// [`Router<R>`](super::Router), acquired with
/// [`Router::of`](super::Router::of).
///
/// Owner-affine (`!Send`), like [`PageHandle`](crate::PageHandle),
/// and inert once the router unmounts: every edit then answers
/// [`RouterError::Disposed`]. Each edit takes effect on the router's navigator
/// at once; the pages it adds build on the next frame.
///
/// It is [`Listenable`]: its listeners are told when the [`stack`](Self::stack)
/// changes, by an edit through any handle or a pop the navigator makes itself
/// (a Back key, a back gesture, a barrier). They are told on a later turn of
/// the UI runtime's tasks, before any transition ends and never from inside
/// the navigation, and once for all the edits since they were last told; a
/// popup is not a change, and neither is a stack that stands. A listener may
/// navigate: that is told in the next delivery. A committed edit is told even
/// when user code it runs after the commit panics. The router drops its
/// listeners when it unmounts, so one may hold a handle to its router.
pub struct RouterHandle<R: Routable> {
    shared: Rc<RouterShared<R>>,
}

impl<R: Routable> Clone for RouterHandle<R> {
    fn clone(&self) -> Self {
        Self {
            shared: Rc::clone(&self.shared),
        }
    }
}

impl<R: Routable> fmt::Debug for RouterHandle<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let routes = self.shared.routes();
        f.debug_struct("RouterHandle")
            .field("stack", &paths(routes.0.iter().map(|route| &**route)))
            .field("mounted", &self.shared.mounted.get())
            .finish()
    }
}

impl<R: Routable> RouterHandle<R> {
    pub(super) fn new(shared: Rc<RouterShared<R>>) -> Self {
        Self { shared }
    }

    fn mounted(&self) -> Result<(), RouterError> {
        if self.shared.mounted.get() {
            Ok(())
        } else {
            Err(RouterError::Disposed)
        }
    }

    /// Whether an edit may run now: the router is mounted and its UI runtime
    /// is not building (ADR-0330 §3). A refusal from `build` is reported on
    /// the `flui.signals` target. The edit counts as running while the
    /// returned guard lives.
    pub(super) fn editable(&self) -> Result<Editing<'_>, RouterError> {
        self.mounted()?;
        let building = self.shared.build_phase.is_building();
        MutatedDuringBuild::check(building, "Router").map_err(|_| RouterError::DuringBuild)?;
        Ok(Editing::begin(&self.shared.editing))
    }

    /// The id of the top page.
    fn top_page(&self) -> RouteId {
        self.shared
            .stack
            .borrow()
            .last()
            .map(|entry| entry.id)
            .expect("BUG: a mounted Router's stack always holds a page")
    }

    /// Pop the popups above the top page: they belong to it (ADR-0330 §2).
    fn dismiss_popups(&self) {
        let top = self.top_page();
        self.shared.navigator.pop_until(|id| id == top);
    }

    /// Push `route` as a new page, with its entrance transition. A value equal
    /// to the current top is pushed again.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted, and
    /// [`RouterError::DuringBuild`] while its UI runtime builds.
    pub fn push(&self, route: R) -> Result<(), RouterError> {
        self.push_for_result::<()>(route).map(drop)
    }

    /// Push `route` as a new page, as [`push`](Self::push) does, and return
    /// the result the page leaves with.
    ///
    /// The result resolves once the page leaves the stack: to `Ok(Some(value))`
    /// for a [`pop_with`](Self::pop_with) of a `T`, to `Ok(None)` when it
    /// leaves without a value (a plain pop, a [`replace`](Self::replace), a
    /// [`go`](Self::go) that drops it, or the router unmounting), and to
    /// [`RouteResultError::TypeMismatch`](crate::RouteResultError::TypeMismatch)
    /// when it is popped with a value of another type. It stays on the owner
    /// thread: `T` needs no `Send`, and neither the result nor the value
    /// crosses a thread.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted, and
    /// [`RouterError::DuringBuild`] while its UI runtime builds.
    pub fn push_for_result<T: 'static>(&self, route: R) -> Result<RouteResult<T>, RouterError> {
        let _editing = self.editable()?;
        let _told = self.shared.announce_on_exit();
        let page = self.shared.page_route::<T>(&route);
        let ((), result) = self.shared.navigator.push_page(page, |id| {
            self.shared
                .stack
                .borrow_mut()
                .push(RouterEntry::new(id, route));
        });
        Ok(result)
    }

    /// Replace the top page with `route`, running the entrance transition. Popups
    /// above the top page are popped first.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted, and
    /// [`RouterError::DuringBuild`] while its UI runtime builds.
    pub fn replace(&self, route: R) -> Result<(), RouterError> {
        let _editing = self.editable()?;
        let _told = self.shared.announce_on_exit();
        self.dismiss_popups();
        let target = self.top_page();
        let page = self.shared.page_route::<()>(&route);
        let retired = self
            .shared
            .navigator
            .push_replacement_page(target, page, |id| {
                let mut stack = self.shared.stack.borrow_mut();
                let retired = stack
                    .iter()
                    .position(|entry| entry.id == target)
                    .map(|index| stack.remove(index));
                stack.push(RouterEntry::new(id, route));
                RouterRetiredValues(retired.into_iter().collect())
            });
        drop(retired);
        Ok(())
    }

    /// Pop the top navigator route: a popup above the top page if there is
    /// one, else the top page.
    ///
    /// Answers `Ok(false)` and changes nothing when the only thing left is one
    /// page: a Router always has a location, so it never empties the stack.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted, and
    /// [`RouterError::DuringBuild`] while its UI runtime builds.
    pub fn pop(&self) -> Result<bool, RouterError> {
        let _editing = self.editable()?;
        let popup_on_top = self.shared.navigator.current() != Some(self.top_page());
        if !popup_on_top && self.shared.stack.borrow().len() <= 1 {
            return Ok(false);
        }
        Ok(self.shared.navigator.pop())
    }

    /// Pop the top navigator route, as [`pop`](Self::pop) does, delivering
    /// `value` as its result.
    ///
    /// A page pushed with [`push_for_result::<T>`](Self::push_for_result)
    /// resolves to `Ok(Some(value))`; a page whose result is of another type
    /// still pops, and its result resolves to
    /// [`RouteResultError::TypeMismatch`](crate::RouteResultError::TypeMismatch).
    /// Answers `Ok(false)` and changes nothing when the only thing left is one
    /// page; `value` is then dropped.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted, and
    /// [`RouterError::DuringBuild`] while its UI runtime builds.
    pub fn pop_with<T: 'static>(&self, value: T) -> Result<bool, RouterError> {
        let _editing = self.editable()?;
        let popup_on_top = self.shared.navigator.current() != Some(self.top_page());
        if !popup_on_top && self.shared.stack.borrow().len() <= 1 {
            return Ok(false);
        }
        Ok(self.shared.navigator.pop_with(value))
    }

    /// Navigate to `location`: derive its stack with
    /// [`Routable::back_stack`] and reconcile the current stack to it.
    ///
    /// The pages both stacks share from the bottom stay mounted with their
    /// state. When the new stack is a prefix of the current one, the pages
    /// above it pop with their exit transitions. Otherwise the pages above the
    /// shared part are removed, the new pages beneath the new top are added
    /// without a transition, and only the new top runs its entrance. A page below the point where the stacks diverge is
    /// rebuilt from scratch, so its state is not kept.
    ///
    /// # Errors
    ///
    /// [`RouterError::Parse`] when `location` does not parse or matches no
    /// route — the stack is then unchanged — [`RouterError::Disposed`] once
    /// the router has unmounted, and [`RouterError::DuringBuild`] while its UI
    /// runtime builds.
    pub fn go(&self, location: &str) -> Result<(), RouterError> {
        let editing = self.editable()?;
        let path = RoutePath::parse(location)?;
        self.go_within(editing, &path)
    }

    /// [`go`](Self::go) to a location already parsed: how an inbound URL
    /// reaches the router.
    pub(super) fn go_path(&self, path: &RoutePath) -> Result<(), RouterError> {
        let editing = self.editable()?;
        self.go_within(editing, path)
    }

    fn go_within(&self, _editing: Editing<'_>, path: &RoutePath) -> Result<(), RouterError> {
        let _told = self.shared.announce_on_exit();
        // Parsed values can own arbitrary destructors, including values in the
        // retained prefix. Protect them before popup dismissal delivers effects.
        let mut target = RouterRetiredValues(R::back_stack(path)?);
        // The routes' `==` runs on a snapshot, outside the stack's borrow, and
        // may navigate: only the matching pages still in place are kept.
        let common_prefix = || {
            let pages = self.shared.pages();
            let equal = pages
                .0
                .iter()
                .zip(&target.0)
                .take_while(|((_, route), target)| **route == **target)
                .count();
            let stack = self.shared.stack.borrow();
            let shared_len = stack
                .iter()
                .zip(&pages.0[..equal])
                .take_while(|(entry, (id, _))| entry.id == *id)
                .count();
            (shared_len, stack.len())
        };
        let mut prefix = common_prefix();
        if prefix.0 != target.0.len() {
            self.dismiss_popups();
            self.mounted()?;
            // Popup observers may have navigated again. No cached index or
            // kept-page identity survives that user-effect boundary.
            prefix = common_prefix();
        }
        let (shared_len, current_len) = prefix;
        if shared_len == target.0.len() {
            if shared_len < current_len {
                let keep = self.shared.stack.borrow()[shared_len - 1].id;
                self.shared.navigator.pop_until(|id| id == keep);
            }
            return Ok(());
        }

        let keep = shared_len
            .checked_sub(1)
            .map(|index| self.shared.stack.borrow()[index].id);
        // Move the admitted values into the stack; do not leave their original
        // copies in an unguarded temporary across observer delivery.
        let mut added = RouterRetiredValues(target.0.split_off(shared_len));
        let top = added
            .0
            .last()
            .expect("BUG: the new stack is longer than the shared part");
        let below_pages = added.0[..added.0.len() - 1]
            .iter()
            .map(|route| self.shared.page_route::<()>(route))
            .collect();
        let top_page = self.shared.page_route::<()>(top);

        let retired = self
            .shared
            .navigator
            .replace_tail(keep, below_pages, top_page, |ids| {
                let mut stack = self.shared.stack.borrow_mut();
                let retired = stack.split_off(shared_len);
                stack.extend(
                    ids.into_iter()
                        .zip(std::mem::take(&mut added.0))
                        .map(|(id, route)| RouterEntry::new(id, route)),
                );
                RouterRetiredValues(retired)
            });
        drop(retired);
        Ok(())
    }

    /// The current location: the top route's path.
    #[must_use]
    pub fn location(&self) -> RoutePath {
        self.shared
            .location()
            .expect("BUG: a Router's stack always holds a page once it is built")
    }

    /// The top route. Its `Clone` runs after the router's stack is released,
    /// so it may drive this router.
    #[must_use]
    pub fn current(&self) -> R {
        let routes = self.shared.routes();
        let top = routes
            .0
            .last()
            .expect("BUG: a Router's stack always holds a page once it is built");
        R::clone(top)
    }

    /// The route stack, bottom to top, as every committed edit left it: what
    /// an application saves to reopen with
    /// [`Router::from_stack`](super::Router::from_stack).
    ///
    /// Popups are not on it: they belong to the page beneath them. The routes
    /// are cloned after the router's stack is released, so a route's `Clone`
    /// may drive this router; the result is the stack as it was when read.
    #[must_use]
    pub fn stack(&self) -> Vec<R> {
        let routes = self.shared.routes();
        clone_routes(routes.0.iter().map(|route| &**route))
    }

    /// Whether a [`pop`](Self::pop) would remove a page: more than one page is
    /// on the stack.
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.shared.stack.borrow().len() > 1
    }
}

impl<R: Routable> Listenable for RouterHandle<R> {
    /// Once the router has unmounted the listener is dropped, not kept: it
    /// would never be told, and one holding this handle would keep the
    /// router's state alive.
    fn listen(&self, listener: ListenerCallback) -> flui_foundation::Subscription {
        let notifier = &self.shared.announcer.notifier;
        let subscription = notifier.listen(listener);
        if !self.shared.mounted.get() {
            // Unmounted: the listener would never be told — drop it and hand
            // the caller the inert form.
            drop(subscription);
            return Subscription::inert();
        }
        subscription
    }
    fn listen_with_recovery(&self, relay: ListenerObserver) -> flui_foundation::Subscription {
        let notifier = &self.shared.announcer.notifier;
        let subscription = notifier.listen_with_recovery(relay);
        if !self.shared.mounted.get() {
            // Unmounted: the relay would never be told — drop it and hand
            // the caller the inert form.
            drop(subscription);
            return Subscription::inert();
        }
        subscription
    }
}

/// Why a [`RouterHandle`] could not be acquired or did not act.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouterError {
    /// No `Router<R>` is above the element that asked.
    #[error("no Router<{route_type}> above this element")]
    NoRouter {
        /// `type_name` of `R`.
        route_type: &'static str,
    },
    /// The Router this handle names has unmounted.
    #[error("the Router this handle names has been disposed")]
    Disposed,
    /// The location did not produce a route.
    #[error(transparent)]
    Parse(#[from] RouteParseError),
    /// A router was asked to open on an empty stack; it needs a page.
    #[error("a Router cannot open on an empty stack")]
    EmptyStack,
    /// The edit was made while the router's UI runtime was building, and
    /// refused: `build` describes the tree and drives nothing (ADR-0330 §3).
    /// Navigate from an event callback, a lifecycle hook or a post-frame
    /// callback.
    #[error("a Router was edited during build")]
    DuringBuild,
}
