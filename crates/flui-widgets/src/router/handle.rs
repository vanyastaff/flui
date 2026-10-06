//! [`RouterHandle`] and [`RouterError`].

use std::fmt;
use std::rc::Rc;

use super::path::{RouteParseError, RoutePath};
use super::routable::Routable;
use super::router::{RouterEntry, RouterRetiredValues, RouterShared};
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
    /// to the current top is pushed again.
    ///
    /// # Errors
    ///
    /// [`RouterError::Disposed`] once the router has unmounted.
    pub fn push(&self, route: R) -> Result<(), RouterError> {
        self.mounted()?;
        let page = self.shared.page_route(&route);
        self.shared.navigator.push_page(page, |id| {
            self.shared
                .stack
                .borrow_mut()
                .push(RouterEntry { id, route });
        });
        Ok(())
    }

    /// Replace the top page with `route`, running the entrance transition. Popups
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
        let retired = self
            .shared
            .navigator
            .push_replacement_page(target, page, |id| {
                let mut stack = self.shared.stack.borrow_mut();
                let retired = stack
                    .iter()
                    .position(|entry| entry.id == target)
                    .map(|index| stack.remove(index));
                stack.push(RouterEntry { id, route });
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
    /// without a transition, and only the new top runs its entrance. A page below the point where the stacks diverge is
    /// rebuilt from scratch, so its state is not kept.
    ///
    /// # Errors
    ///
    /// [`RouterError::Parse`] when `location` does not parse or matches no
    /// route — the stack is then unchanged — and [`RouterError::Disposed`]
    /// once the router has unmounted.
    pub fn go(&self, location: &str) -> Result<(), RouterError> {
        self.mounted()?;
        // Parsed values can own arbitrary destructors, including values in the
        // retained prefix. Protect them before popup dismissal delivers effects.
        let mut target = RouterRetiredValues(R::back_stack(&RoutePath::parse(location)?)?);
        let common_prefix = || {
            let stack = self.shared.stack.borrow();
            let shared_len = stack
                .iter()
                .zip(&target.0)
                .take_while(|(entry, route)| entry.route == **route)
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
            .map(|route| self.shared.page_route(route))
            .collect();
        let top_page = self.shared.page_route(top);

        let retired = self
            .shared
            .navigator
            .replace_tail(keep, below_pages, top_page, |ids| {
                let mut stack = self.shared.stack.borrow_mut();
                let retired = stack.split_off(shared_len);
                stack.extend(
                    ids.into_iter()
                        .zip(std::mem::take(&mut added.0))
                        .map(|(id, route)| RouterEntry { id, route }),
                );
                RouterRetiredValues(retired)
            });
        drop(retired);
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

    /// The route stack, bottom to top, as every committed edit left it: what
    /// an application saves to reopen with
    /// [`Router::from_stack`](super::Router::from_stack).
    ///
    /// Not yet the whole stack: it holds the top route alone.
    #[must_use]
    pub fn stack(&self) -> Vec<R> {
        vec![self.current()]
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
    /// The Router this handle names has unmounted.
    #[error("the Router this handle names has been disposed")]
    Disposed,
    /// The location did not produce a route.
    #[error(transparent)]
    Parse(#[from] RouteParseError),
    /// A router was asked to open on an empty stack; it needs a page.
    #[error("a Router cannot open on an empty stack")]
    EmptyStack,
}
