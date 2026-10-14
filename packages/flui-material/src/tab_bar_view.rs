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
//! ## Length mismatch
//!
//! [`TabBarView::new`]'s `children` count must equal the controller's
//! [`length`](crate::TabController::length). `build` checks it with a release
//! `assert!`, so a mismatch surfaces as a framework `ErrorView` (via the
//! build-error boundary) in every profile — same public-widget invariant
//! class as [`TabController`] index bounds (#1101).

use std::cell::RefCell;

use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{PageController, PageView};

use crate::tab_controller::{DefaultTabController, PagerAttachment, TabController};

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
    /// [`DefaultTabController::maybe_of`]) and returns it with the pager
    /// attached to it, attaching anew when the controller identity changed.
    ///
    /// # Panics
    ///
    /// Panics if `view` has no explicit controller and there is no
    /// `DefaultTabController` ancestor.
    fn resolve(
        &self,
        view: &TabBarView,
        ctx: &dyn BuildContext,
    ) -> (TabController, PageController) {
        let resolved = view
            .controller
            .clone()
            .or_else(|| DefaultTabController::maybe_of(ctx))
            .expect(
                "TabBarView requires an explicit controller (TabBarView::controller) or a \
                 DefaultTabController ancestor",
            );

        let current = self
            .attached
            .borrow()
            .as_ref()
            .filter(|(controller, _)| *controller == resolved)
            .map(|(_, attachment)| attachment.pager().clone());
        if let Some(pager) = current {
            return (resolved, pager);
        }

        // Release the old attachment before attaching to the new controller,
        // outside the borrow: releasing returns the old controller's position
        // to its owned source, which notifies its listeners.
        let displaced = self.attached.borrow_mut().take();
        drop(displaced);
        let attachment = resolved.attach_pager();
        let pager = attachment.pager().clone();
        *self.attached.borrow_mut() = Some((resolved.clone(), attachment));
        (resolved, pager)
    }
}

impl ViewState<TabBarView> for TabBarViewState {
    /// Releases the pager attachment, so a controller that outlives this
    /// view returns to its owned position source.
    fn dispose(&mut self) {
        drop(self.attached.get_mut().take());
    }

    fn build(&self, view: &TabBarView, ctx: &dyn BuildContext) -> impl IntoView {
        let (controller, pager) = self.resolve(view, ctx);
        let child_count = view.children.len();

        assert!(
            child_count == controller.length(),
            "TabBarView: {child_count} children does not match the TabController's length of \
             {} — the children list and the tab count must agree",
            controller.length()
        );

        PageView::new(view.children.clone()).controller(pager)
    }
}
