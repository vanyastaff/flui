//! [`ScrollObserverScope`] — where an ancestor watches the scroll view
//! nearest below it.
//!
//! Flutter tells an ancestor about a scroll view by bubbling
//! `ScrollNotification`s and `ScrollMetricsNotification`s through the
//! element tree. Here the ancestor installs a scope instead, and the nearest
//! [`Scrollable`](super::Scrollable) under it attaches its position and the
//! direction it scrolls in. The ancestor then reads the position directly
//! and subscribes to it, so a scroll frame costs one listener call and
//! nothing bubbles.
//!
//! Only the scroll view at depth zero attaches: the one no other scroll
//! view sits between it and the scope. Each side compares the nearest
//! [`ScrollPositionScope`] it sees, so a list nested inside the observed one
//! sees the outer list's position, not the one the scope saw, and stays out.
//!
//! [`Scrollbar`](super::Scrollbar) is the first observer. A public
//! notification layer (`ScrollNotification`, `NotificationListener`) is to
//! build on this scope rather than beside it (`docs/gaps.md`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use flui_foundation::{ChangeNotifier, Subscription};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::view::ScrollPosition;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

use super::{ScrollController, ScrollPositionScope};

/// What a scroll view attached to an observer: the controller that moves it
/// and the direction its offset grows in.
#[derive(Clone, Debug)]
pub(crate) struct ObservedScroll {
    pub(crate) controller: ScrollController,
    pub(crate) axis_direction: AxisDirection,
}

impl ObservedScroll {
    fn same_as(&self, other: &Self) -> bool {
        self.axis_direction == other.axis_direction
            && self
                .controller
                .position()
                .ptr_eq(&other.controller.position())
    }
}

/// The observer's side: the scroll views attached to it, the most recent
/// first in line, and who hears that the observed one changed.
#[derive(Default)]
pub(crate) struct ScrollObserver {
    /// Each attachment is owned by its scroll view's [`ObserverSeat`]; a
    /// dropped seat leaves a dead entry the next change prunes.
    attached: RefCell<Vec<Weak<RefCell<ObservedScroll>>>>,
    changed: ChangeNotifier,
}

impl std::fmt::Debug for ScrollObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollObserver")
            .field("observed", &self.observed())
            .finish_non_exhaustive()
    }
}

impl ScrollObserver {
    /// The scroll view this observer follows: the latest attached one still
    /// mounted.
    pub(crate) fn observed(&self) -> Option<ObservedScroll> {
        self.attached
            .borrow()
            .iter()
            .rev()
            .find_map(Weak::upgrade)
            .map(|scroll| scroll.borrow().clone())
    }

    /// Hear when the observed scroll view changes: attached, detached or
    /// re-attached with another position or direction.
    pub(crate) fn listen(&self, listener: Rc<dyn Fn()>) -> Subscription {
        self.changed.listen(listener)
    }

    fn attach(self: &Rc<Self>, scroll: ObservedScroll) -> ObserverSeat {
        let entry = Rc::new(RefCell::new(scroll));
        {
            let mut attached = self.attached.borrow_mut();
            attached.retain(|weak| weak.strong_count() > 0);
            attached.push(Rc::downgrade(&entry));
        }
        // Listeners run with nothing borrowed.
        self.changed.notify_listeners();
        ObserverSeat {
            observer: Rc::downgrade(self),
            entry: Some(entry),
        }
    }

    fn detached(&self) {
        self.attached
            .borrow_mut()
            .retain(|weak| weak.strong_count() > 0);
        self.changed.notify_listeners();
    }
}

/// A scroll view's attachment to an observer; dropping it detaches.
pub(crate) struct ObserverSeat {
    observer: Weak<ScrollObserver>,
    /// Always `Some` until the seat drops.
    entry: Option<Rc<RefCell<ObservedScroll>>>,
}

impl std::fmt::Debug for ObserverSeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObserverSeat")
            .field(
                "scroll",
                &self.entry.as_ref().map(|entry| entry.borrow().clone()),
            )
            .finish_non_exhaustive()
    }
}

impl ObserverSeat {
    fn is_for(&self, observer: &Rc<ScrollObserver>) -> bool {
        self.observer.as_ptr() == Rc::as_ptr(observer)
    }

    /// Point the seat at `scroll`; the observer hears it only when the
    /// position or the direction changed.
    fn update(&self, scroll: ObservedScroll) {
        let entry = self
            .entry
            .as_ref()
            .expect("BUG: a seat holds its entry until it drops");
        if entry.borrow().same_as(&scroll) {
            return;
        }
        let replaced = std::mem::replace(&mut *entry.borrow_mut(), scroll);
        // The replaced controller drops with nothing borrowed.
        drop(replaced);
        if let Some(observer) = self.observer.upgrade() {
            observer.changed.notify_listeners();
        }
    }
}

impl Drop for ObserverSeat {
    fn drop(&mut self) {
        // The entry dies first, so the observer prunes it.
        drop(self.entry.take());
        if let Some(observer) = self.observer.upgrade() {
            observer.detached();
        }
    }
}
/// A scroll view's side of observation: the seat it holds, kept across
/// rebuilds while the same observer stays above it.
#[derive(Debug, Default)]
pub(crate) struct ObservedBy {
    seat: Option<ObserverSeat>,
    /// Whether the scope of that observer paints this scroll view's bar.
    under_scrollbar: bool,
}

impl ObservedBy {
    /// Attach `scroll` to the observer above `ctx`, if this scroll view is
    /// the one at depth zero under it; leave any other observer.
    pub(crate) fn sync(&mut self, ctx: &dyn BuildContext, scroll: ObservedScroll) {
        match ScrollObserverScope::depth_zero_observer(ctx) {
            Some((observer, paints_scrollbar)) => {
                self.under_scrollbar = paints_scrollbar;
                match &self.seat {
                    Some(seat) if seat.is_for(&observer) => seat.update(scroll),
                    _ => {
                        let outgoing = self.seat.replace(observer.attach(scroll));
                        drop(outgoing);
                    }
                }
            }
            None => self.clear(),
        }
    }

    /// Whether the observer this scroll view is attached to paints the bar
    /// of this very scroll view, so its behavior adds none.
    pub(crate) fn under_scrollbar(&self) -> bool {
        self.seat.is_some() && self.under_scrollbar
    }

    /// Detach from the observer, if any.
    pub(crate) fn clear(&mut self) {
        self.under_scrollbar = false;
        drop(self.seat.take());
    }
}

/// Provides a [`ScrollObserver`] to the scroll view at depth zero below.
#[derive(Clone)]
pub(crate) struct ScrollObserverScope {
    observer: Rc<ScrollObserver>,
    /// The position of the scroll view the scope itself sits in, if any.
    enclosing: Option<ScrollPosition>,
    /// Whether the observer paints the bar of the scroll view it observes:
    /// a bar that follows a controller of its own does not.
    paints_scrollbar: bool,
    child: BoxedView,
}

impl ScrollObserverScope {
    /// Install `observer` over `child`; `ctx` is where the scope is built,
    /// read for the scroll view it sits in. `paints_scrollbar` when the
    /// observer is a bar showing what it observes.
    pub(crate) fn new(
        ctx: &dyn BuildContext,
        observer: Rc<ScrollObserver>,
        paints_scrollbar: bool,
        child: impl IntoView,
    ) -> Self {
        Self {
            observer,
            enclosing: ScrollPositionScope::position_of(ctx),
            paints_scrollbar,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// The observer a scroll view built at `ctx` attaches to, and whether it
    /// paints that scroll view's bar: the nearest scope's, when no other
    /// scroll view sits between the two.
    fn depth_zero_observer(ctx: &dyn BuildContext) -> Option<(Rc<ScrollObserver>, bool)> {
        let (observer, enclosing, paints_scrollbar) = ctx.depend_on::<Self, _>(|scope| {
            (
                Rc::clone(&scope.observer),
                scope.enclosing.clone(),
                scope.paints_scrollbar,
            )
        })?;
        let nearest = ScrollPositionScope::position_of(ctx);
        let depth_zero = match (&enclosing, &nearest) {
            (None, None) => true,
            (Some(enclosing), Some(nearest)) => enclosing.ptr_eq(nearest),
            _ => false,
        };
        depth_zero.then_some((observer, paints_scrollbar))
    }
}

impl std::fmt::Debug for ScrollObserverScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollObserverScope")
            .field("observer", &self.observer)
            .finish_non_exhaustive()
    }
}

impl InheritedView for ScrollObserverScope {
    type Data = Rc<ScrollObserver>;

    fn data(&self) -> &Self::Data {
        &self.observer
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.observer, &old.observer)
            || self.paints_scrollbar != old.paints_scrollbar
            || match (&self.enclosing, &old.enclosing) {
                (None, None) => false,
                (Some(new), Some(old)) => !new.ptr_eq(old),
                _ => true,
            }
    }
}

impl_inherited_view!(ScrollObserverScope);
