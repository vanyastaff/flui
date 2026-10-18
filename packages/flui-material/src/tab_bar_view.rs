//! [`TabBarView`] — a [`crate::TabController`]-synced pager, the usual body
//! for a [`crate::TabBar`].
//!
//! # A `PageView` over the controller's pager
//!
//! The view composes a [`PageView`] whose [`PageController`] the tab
//! controller makes when the view attaches: while mounted, the pager *is* the
//! controller's position source (see [`crate::TabController`]'s module
//! docs). Dragging moves the pager, and so the indicator, directly; the
//! selection commits once the pager comes to rest. A tab change animates or
//! jumps the pager. The view itself never writes the selection, so there is
//! no second copy of it to keep in sync.
//!
//! Pages are `PageView`'s eager children: every page is built up front and
//! stays mounted, so a page's own state (a counter, a scroll position)
//! survives switching away and back. Off-screen pages are not laid out or
//! painted (`PageView`'s zero cache extent), but their tickers keep running.
//!
//! ## Controller resolution: explicit, else `DefaultTabController`
//!
//! Exactly [`crate::TabBar`]'s own contract: an explicit
//! [`controller`](TabBarView::controller) wins; otherwise the nearest
//! [`crate::DefaultTabController`] ancestor's controller is used. Exactly one
//! must be reachable, or `build` panics. The view re-resolves on every build
//! and re-attaches on a controller-identity change; `dispose` releases the
//! attachment, returning the controller's position to its owned source.
//!
//! ## One view per controller
//!
//! A controller's position has one source, so it drives at most one mounted
//! `TabBarView`. A second view resolving to a controller that is already
//! attached is refused: its `build` panics with a message naming the cause,
//! which the build-error boundary turns into an `ErrorView`, and the first
//! view keeps driving the controller. Silently letting the newer view take
//! over would leave the older one scrolling a pager nothing reads.
//!
//! ## Length mismatch
//!
//! [`TabBarView::new`]'s `children` count must equal the controller's
//! [`length`](crate::TabController::length). `build` checks it with a release
//! `assert!`, so a mismatch surfaces as a framework `ErrorView` (via the
//! build-error boundary) in every profile — same public-widget invariant
//! class as [`TabController`] index bounds (#1101).

use std::cell::RefCell;

use flui_sdk::animation::MotionPolicy;
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, PageController, PageView};

use crate::tab_controller::{DefaultTabController, PagerAttachment, PagerSuspender, TabController};

/// A [`TabController`]-synced pager: one page per tab, swiped or switched
/// by the controller (see the module docs).
///
/// A [`TabController`] is required either explicitly (via
/// [`controller`](Self::controller)) or via a [`DefaultTabController`]
/// ancestor — exactly one must be reachable, or `build` panics.
///
/// ```
/// use flui_material::{DefaultTabController, TabBarView};
/// use flui_sdk::widgets::{SizedBox, Text};
/// use flui_sdk::view::ViewExt;
///
/// let children = vec![Text::new("One").boxed(), SizedBox::shrink().boxed()];
/// let view = DefaultTabController::new(children.len(), TabBarView::new(children));
/// ```
#[derive(Clone, StatefulView)]
pub struct TabBarView {
    children: Vec<BoxedView>,
    controller: Option<TabController>,
}

impl TabBarView {
    /// A `TabBarView` over `children` — one child per tab, resolved against
    /// an explicit or ambient [`TabController`] (see [`controller`](Self::controller)).
    #[must_use]
    pub fn new(children: Vec<BoxedView>) -> Self {
        Self {
            children,
            controller: None,
        }
    }

    /// Supplies an explicit [`TabController`] instead of relying on a
    /// [`DefaultTabController`] ancestor.
    #[must_use]
    pub fn controller(mut self, controller: TabController) -> Self {
        self.controller = Some(controller);
        self
    }
}

impl std::fmt::Debug for TabBarView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBarView")
            .field("child_count", &self.children.len())
            .field("has_explicit_controller", &self.controller.is_some())
            .finish_non_exhaustive()
    }
}

/// Persistent state behind [`TabBarView`]: the resolved [`TabController`]
/// and the pager attached to it.
pub struct TabBarViewState {
    attached: RefCell<Option<(TabController, PagerAttachment)>>,
}

impl std::fmt::Debug for TabBarViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBarViewState")
            .field("attached", &self.attached.borrow())
            .finish()
    }
}

impl StatefulView for TabBarView {
    type State = TabBarViewState;

    fn create_state(&self) -> Self::State {
        TabBarViewState {
            attached: RefCell::new(None),
        }
    }
}

impl TabBarViewState {
    /// Resolves `view`'s effective controller (explicit, else
    /// [`DefaultTabController::maybe_of`]).
    ///
    /// # Panics
    ///
    /// Panics if `view` has no explicit controller and there is no
    /// `DefaultTabController` ancestor.
    fn resolve(view: &TabBarView, ctx: &dyn BuildContext) -> TabController {
        view.controller
            .clone()
            .or_else(|| DefaultTabController::maybe_of(ctx))
            .expect(
                "TabBarView requires an explicit controller (TabBarView::controller) or a \
                 DefaultTabController ancestor",
            )
    }

    /// The pager attached to `resolved`, attaching anew when the controller
    /// identity changed. Called only for a view that passed its length check,
    /// so a failed build never leaves a claim on the controller.
    fn pager_for(&self, resolved: &TabController) -> (PageController, PagerSuspender) {
        let current = self
            .attached
            .borrow()
            .as_ref()
            .filter(|(controller, _)| controller == resolved)
            .map(|(_, attachment)| (attachment.pager().clone(), attachment.suspender()));
        if let Some(current) = current {
            return current;
        }

        // Release the old attachment before attaching to the new controller,
        // outside the borrow: releasing returns the old controller's position
        // to its owned source, which notifies its listeners.
        let displaced = self.attached.borrow_mut().take();
        drop(displaced);
        let attachment = resolved.attach_pager().expect(
            "TabBarView: this TabController already drives another mounted TabBarView — a \
             controller's position has one source, so give each TabBarView its own controller",
        );
        let current = (attachment.pager().clone(), attachment.suspender());
        *self.attached.borrow_mut() = Some((resolved.clone(), attachment));
        current
    }
}

/// A transparent wrapper around the first page that tells the controller when
/// the view is being removed. Unmounting runs deepest first, so a page hears
/// of it before the scrollable above it stops the pager's activity; that stop
/// is then not mistaken for the user letting go of a drag.
#[derive(Clone, StatefulView)]
struct LeaveGate {
    child: BoxedView,
    suspender: PagerSuspender,
}

impl std::fmt::Debug for LeaveGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaveGate").finish_non_exhaustive()
    }
}

struct LeaveGateState {
    suspender: PagerSuspender,
}

impl StatefulView for LeaveGate {
    type State = LeaveGateState;

    fn create_state(&self) -> Self::State {
        LeaveGateState {
            suspender: self.suspender.clone(),
        }
    }
}

impl ViewState<LeaveGate> for LeaveGateState {
    fn did_update_view(&mut self, _old: &LeaveGate, new: &LeaveGate) {
        self.suspender = new.suspender.clone();
    }

    fn deactivate(&mut self) {
        self.suspender.set(true);
    }

    fn activate(&mut self) {
        self.suspender.set(false);
    }

    fn dispose(&mut self) {
        self.suspender.set(true);
    }

    fn build(&self, view: &LeaveGate, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

impl ViewState<TabBarView> for TabBarViewState {
    /// Releases the pager attachment, so a controller that outlives this
    /// view returns to its owned position source.
    fn dispose(&mut self) {
        drop(self.attached.get_mut().take());
    }

    fn build(&self, view: &TabBarView, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = Self::resolve(view, ctx);
        let child_count = view.children.len();

        if child_count != controller.length() {
            // A view that cannot build must not keep its claim behind the
            // error it is about to become.
            let released = self.attached.borrow_mut().take();
            drop(released);
        }
        assert!(
            child_count == controller.length(),
            "TabBarView: {child_count} children does not match the TabController's length of \
             {} — the children list and the tab count must agree",
            controller.length()
        );

        // The claim comes first: a refused view must not change the policy
        // of the view that holds the controller.
        let (pager, suspender) = self.pager_for(&controller);
        controller.set_reduce_pager(MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce));
        let mut pages = view.children.clone();
        if let Some(first) = pages.first_mut() {
            *first = LeaveGate {
                child: first.clone(),
                suspender,
            }
            .into_view()
            .boxed();
        }
        PageView::new(pages).controller(pager)
    }
}
