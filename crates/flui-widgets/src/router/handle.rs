//! [`RouterHandle`] and [`RouterError`].

use std::fmt;
use std::rc::Rc;

use super::path::{RouteParseError, RoutePath};
use super::routable::Routable;
use super::router::{RouterEntry, RouterShared};
use crate::navigator::RouteId;

/// An owned, cloneable capability to drive the nearest
/// [`Router<R>`](super::Router), acquired with
/// [`Router::handle`](super::Router::handle).
///
/// Owner-affine (`!Send`), like [`NavigatorHandle`](crate::NavigatorHandle),
/// and inert once the router unmounts: every edit then answers
/// [`RouterError::Disposed`]. Each edit takes effect on the router's navigator
/// at once; the pages it adds build on the next frame.
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
        f.debug_struct("RouterHandle")
            .field("location", &self.shared.location().map(|p| p.to_string()))
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

    /// The id of the top page.
    fn top_page(&self) -> RouteId {
        self.shared
            .stack
            .borrow()
            .last()
            .map(|entry| entry.id)
            .expect("BUG: a mounted Router's stack always holds a page")
    }

    /// Pop the popups above the top page: they belong to it (ADR-0093 §2).
    fn dismiss_popups(&self) {
        let top = self.top_page();
        self.shared.navigator.pop_until(|id| id == top);
    }

    /// Push `route` as a new page, with its entrance transition. A value equal
    /// to the current top is pushed again, as Flutter allows.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted.
    pub fn push(&self, route: R) -> Result<(), RouterError> {
        self.mounted()?;
        let page = self.shared.page_route(&route);
        let id = self.shared.navigator.push_page(page);
        self.shared
            .stack
            .borrow_mut()
            .push(RouterEntry { id, route });
        Ok(())
    }

    /// Replace the top page with `route` — Flutter's `pushReplacement`. Popups
    /// above the top page are popped first.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted.
    pub fn replace(&self, route: R) -> Result<(), RouterError> {
        self.mounted()?;
        self.dismiss_popups();
        let target = self.top_page();
        let page = self.shared.page_route(&route);
        let id = self.shared.navigator.push_replacement_page(target, page);
        let mut stack = self.shared.stack.borrow_mut();
        stack.retain(|entry| entry.id != target);
        stack.push(RouterEntry { id, route });
        Ok(())
    }

    /// Pop the top navigator route: a popup above the top page if there is
    /// one, else the top page.
    ///
    /// Answers `Ok(false)` and changes nothing when the only thing left is one
    /// page: a Router always has a location. Flutter's `Navigator.pop` would
    /// empty the stack here.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted.
    pub fn pop(&self) -> Result<bool, RouterError> {
        self.mounted()?;
        let popup_on_top = self.shared.navigator.current() != Some(self.top_page());
        if !popup_on_top && self.shared.stack.borrow().len() <= 1 {
            return Ok(false);
        }
        Ok(self.shared.navigator.pop())
    }

    /// Navigate to `location`: derive its stack with
    /// [`Routable::back_stack`] and reconcile the current stack to it.
    ///
    /// The pages both stacks share from the bottom stay mounted with their
    /// state. When the new stack is a prefix of the current one, the pages
    /// above it pop with their exit transitions. Otherwise the pages above the
    /// shared part are removed, the new pages beneath the new top are added
    /// without a transition, and only the new top runs its entrance — Flutter's
    /// page-list diff. A page below the point where the stacks diverge is
    /// rebuilt from scratch, so its state is not kept.
    ///
    /// # Errors
    ///
    /// [`RouterError::Parse`] when `location` does not parse or matches no
    /// route — the stack is then unchanged — and [`RouterError::Disposed`]
    /// once the router has unmounted.
    pub fn go(&self, location: &str) -> Result<(), RouterError> {
        self.mounted()?;
        let target = R::back_stack(&RoutePath::parse(location)?)?;
        let (shared_len, current_len) = {
            let stack = self.shared.stack.borrow();
            let shared_len = stack
                .iter()
                .zip(&target)
                .take_while(|(entry, route)| entry.route == **route)
                .count();
            (shared_len, stack.len())
        };
        if shared_len == target.len() {
            if shared_len < current_len {
                let keep = self.shared.stack.borrow()[shared_len - 1].id;
                self.shared.navigator.pop_until(|id| id == keep);
            }
            return Ok(());
        }

        self.dismiss_popups();
        let keep = shared_len
            .checked_sub(1)
            .map(|index| self.shared.stack.borrow()[index].id);
        let mut added = target[shared_len..].to_vec();
        let top = added
            .pop()
            .expect("BUG: the new stack is longer than the shared part");
        let below_pages = added
            .iter()
            .map(|route| self.shared.page_route(route))
            .collect();
        let top_page = self.shared.page_route(&top);
        added.push(top);

        self.shared.reconciling.set(true);
        let ids = self
            .shared
            .navigator
            .replace_tail(keep, below_pages, top_page);
        self.shared.reconciling.set(false);

        let mut stack = self.shared.stack.borrow_mut();
        stack.truncate(shared_len);
        stack.extend(
            ids.into_iter()
                .zip(added)
                .map(|(id, route)| RouterEntry { id, route }),
        );
        Ok(())
    }

    /// The current location: the top route's path.
    #[must_use]
    pub fn location(&self) -> RoutePath {
        self.current().to_path()
    }

    /// The top route.
    #[must_use]
    pub fn current(&self) -> R {
        self.shared
            .stack
            .borrow()
            .last()
            .map(|entry| entry.route.clone())
            .expect("BUG: a Router's stack always holds a page once it is built")
    }

    /// Whether a [`pop`](Self::pop) would remove a page: more than one page is
    /// on the stack.
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.shared.stack.borrow().len() > 1
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
    /// A route that is not an `R` was pushed on a Router's navigator.
    #[error(
        "only `{route_type}` values may enter a Router's stack; a route with no path was pushed"
    )]
    NotAddressable {
        /// `type_name` of `R`.
        route_type: &'static str,
    },
    /// The Router this handle names has unmounted.
    #[error("the Router this handle names has been disposed")]
    Disposed,
    /// The location did not produce a route.
    #[error(transparent)]
    Parse(#[from] RouteParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigator::Unaddressable;

    #[test]
    fn not_addressable_text_is_the_navigator_refusal_text() {
        let route_type = "app::AppRoute";
        assert_eq!(
            RouterError::NotAddressable { route_type }.to_string(),
            Unaddressable { route_type }.to_string()
        );
    }
}
