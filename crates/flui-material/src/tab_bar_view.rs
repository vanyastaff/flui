//! [`TabBarView`] — a [`crate::TabController`]-synced page switcher, the
//! usual body for a [`crate::TabBar`].
//!
//! # Flutter parity
//!
//! `material/tabs.dart`'s `TabBarView` (oracle tag `3.44.0`) — for the
//! OBSERVABLE contract only: the active child tracks the controller's index,
//! an already-visited child's state survives switching away and back, and a
//! not-yet-visited child is never built. See "The switching mechanism: no
//! `PageView`" below for what does *not* carry over.
//!
//! ## The switching mechanism: no `PageView`
//!
//! The oracle's `_TabBarViewState` drives a real `PageView` (`PageController`,
//! `Scrollable`, `Viewport`): every child is a scrollable page, dragging
//! between tabs is native, and `TabController.animateTo` warps the page
//! controller to the target index over its animation duration. This
//! substrate has no page-scroll substrate at all yet (no `Scrollable` that
//! can host a viewport of arbitrary, individually-sized pages) — swiping is
//! a **named deferral**, not a silent drop.
//!
//! Instead, this ports the OTHER lazy-keep-alive switcher this workspace
//! already established: `flui_cupertino::CupertinoTabScaffold`'s private
//! `_TabSwitchingView` mechanic (`cupertino/tab_scaffold.dart`, oracle tag
//! `3.44.0`) — every child mounts a slot up front, but a child is only ever
//! *built* the first time its index becomes active (tracked per index, never
//! reset — "once visited, stays built"), and every non-active child is
//! [`Offstage`]-hidden + [`TickerMode`]-disabled rather than unmounted, so an
//! inactive child's own state (a counter, a scroll position, a nested
//! animation) survives a switch away and back, and its animations genuinely
//! stop advancing while hidden. This is the second occurrence of that exact
//! composition in this workspace (`CupertinoTabScaffold` is the first); per
//! this studio's rule of three, a second occurrence is composed again here,
//! not extracted into a shared abstraction yet.
//!
//! `CupertinoTabScaffold` additionally wraps each slot in `HeroMode` (gating
//! hero animations by tab) — that has no analog here: `TabBarView` has no
//! bottom-tab-bar-driven navigation stack of its own, so there is nothing
//! for `HeroMode` to gate.
//!
//! ## Controller resolution: explicit, else `DefaultTabController`
//!
//! Exactly [`crate::TabBar`]'s own contract: an explicit
//! [`controller`](TabBarView::controller) wins; otherwise the nearest
//! [`crate::DefaultTabController`] ancestor's controller is used. Exactly one
//! must be reachable, or `build` panics (Flutter parity: the oracle's
//! `_updateTabController`'s `FlutterError`/`assert`). The listener
//! subscription is registered the same way `TabBarState` registers its own
//! (re-resolved every `build`, re-homed on controller-identity change) and
//! removed in `dispose`: a controller that outlives this view must not keep
//! a dead `Rc` closure calling [`flui_view::RebuildHandle::schedule`] on an
//! unmounted element.
//!
//! ## Length mismatch
//!
//! [`TabBarView::new`]'s `children` count is expected to equal the
//! controller's [`length`](crate::TabController::length) — enforced with a
//! `debug_assert!`, matching `CupertinoTabScaffold`'s own out-of-range-index
//! precedent (`tab_scaffold.rs`'s `is_valid_tab_index` doc comment). In a
//! release build (where `debug_assert!` compiles out), an out-of-range
//! current index simply never matches any child's own index in `build`'s
//! `0..children.len()` iteration — every child renders `Offstage`-hidden and
//! none is marked active, with no panic and no `Vec` index out of bounds.
//! This is a documented fall-through, not a silent one: do not "fix" it by
//! clamping the index — the debug assertion exists so a real mismatch is
//! caught long before any release build ships.

use std::cell::RefCell;

use flui_foundation::ListenerId;
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};
use flui_widgets::{Offstage, SizedBox, Stack, StackFit, TickerMode};

use crate::tab_controller::{DefaultTabController, TabController};

/// A [`TabController`]-synced page switcher: one child per tab, the active
/// one shown, every visited one kept alive `Offstage` (see the module docs
/// for exactly what mechanism this uses and why).
///
/// A [`TabController`] is required either explicitly (via
/// [`controller`](Self::controller)) or via a [`DefaultTabController`]
/// ancestor — exactly one must be reachable, or `build` panics. See the
/// module docs' "Controller resolution" section.
///
/// ```
/// use flui_material::{DefaultTabController, TabBarView};
/// use flui_widgets::{SizedBox, Text};
/// use flui_view::ViewExt;
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

/// Persistent state behind [`TabBarView`]: the currently-subscribed
/// [`TabController`] and its listener registration (re-resolved every
/// `build`, exactly [`crate::tabs::TabBarState`]'s own contract), plus which
/// child indices have ever been built.
pub struct TabBarViewState {
    controller: RefCell<Option<TabController>>,
    listener_id: RefCell<Option<ListenerId>>,
    rebuild: Option<RebuildHandle>,
    /// Flutter parity: `_TabSwitchingViewState.shouldBuildTab` — grown to
    /// match `view.children.len()` on every build, never reset for an index
    /// that already built once. See the module docs' "switching mechanism"
    /// section.
    should_build: RefCell<Vec<bool>>,
}

impl std::fmt::Debug for TabBarViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBarViewState")
            .field("controller", &self.controller.borrow())
            .finish_non_exhaustive()
    }
}

impl StatefulView for TabBarView {
    type State = TabBarViewState;

    fn create_state(&self) -> Self::State {
        TabBarViewState {
            controller: RefCell::new(None),
            listener_id: RefCell::new(None),
            rebuild: None,
            should_build: RefCell::new(Vec::new()),
        }
    }
}

impl TabBarViewState {
    /// Resolves `view`'s effective controller (explicit, else
    /// [`DefaultTabController::maybe_of`]) and, if it differs by identity
    /// from the currently-subscribed one, swaps the listener registration
    /// onto it. Identical contract to `crate::tabs::TabBarState::resolve_controller`
    /// — see that method's doc comment for why this runs from `build` rather
    /// than a lifecycle hook, and why re-resolving unconditionally on every
    /// `build` is cheap and always correct.
    ///
    /// # Panics
    ///
    /// Panics if `view` has no explicit controller and there is no
    /// `DefaultTabController` ancestor. Flutter parity: `_updateTabController`'s
    /// `FlutterError`.
    fn resolve_controller(&self, view: &TabBarView, ctx: &dyn BuildContext) -> TabController {
        let resolved = view
            .controller
            .clone()
            .or_else(|| DefaultTabController::maybe_of(ctx))
            .expect(
                "TabBarView requires an explicit controller (TabBarView::controller) or a \
                 DefaultTabController ancestor",
            );

        let changed = self
            .controller
            .borrow()
            .as_ref()
            .is_none_or(|current| *current != resolved);

        if changed {
            let previous_controller = self.controller.borrow_mut().take();
            let previous_listener = self.listener_id.borrow_mut().take();
            if let (Some(previous_controller), Some(id)) = (previous_controller, previous_listener)
            {
                previous_controller.remove_listener(id);
            }

            let rebuild = self
                .rebuild
                .clone()
                .expect("init_state runs before the first build");
            let id = resolved.add_listener(move || {
                rebuild.schedule(flui_view::RebuildReason::AnimationTick);
            });
            *self.listener_id.borrow_mut() = Some(id);
            *self.controller.borrow_mut() = Some(resolved.clone());
        }

        resolved
    }
}

impl ViewState<TabBarView> for TabBarViewState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    /// Unregisters this view's listener from whatever controller it's
    /// currently subscribed to — without this, a controller that outlives
    /// this view keeps firing a dead `Rc` closure that calls
    /// `rebuild.schedule(reason)` on a `RebuildHandle` whose element no longer
    /// exists (see `crate::tabs::TabBarState::dispose`'s doc comment for the
    /// identical leak on `TabBar`'s side).
    fn dispose(&mut self) {
        let controller = self.controller.get_mut().take();
        let listener_id = self.listener_id.get_mut().take();
        if let (Some(controller), Some(id)) = (controller, listener_id) {
            controller.remove_listener(id);
        }
    }

    fn build(&self, view: &TabBarView, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.resolve_controller(view, ctx);
        let child_count = view.children.len();

        debug_assert!(
            child_count == controller.length(),
            "TabBarView: {child_count} children does not match the TabController's length of \
             {} — the children list and the tab count must agree",
            controller.length()
        );

        let current_index = controller.index();

        let mut should_build = self.should_build.borrow_mut();
        should_build.resize(child_count, false);
        if let Some(flag) = should_build.get_mut(current_index) {
            *flag = true;
        }

        let layers: Vec<BoxedView> = view
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let active = index == current_index;
                let content = if should_build[index] {
                    child.clone()
                } else {
                    SizedBox::shrink().boxed()
                };
                Offstage::new()
                    .offstage(!active)
                    .child(TickerMode::new(content).enabled(active))
                    .boxed()
            })
            .collect();

        Stack::new(layers).fit(StackFit::Expand)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_no_explicit_controller() {
        let view = TabBarView::new(vec![flui_widgets::SizedBox::shrink().boxed()]);
        assert!(view.controller.is_none());
    }

    #[test]
    fn controller_sets_the_explicit_controller() {
        let controller = TabController::new(1, 0);
        let view = TabBarView::new(vec![flui_widgets::SizedBox::shrink().boxed()])
            .controller(controller.clone());
        assert_eq!(view.controller, Some(controller));
    }

    #[test]
    fn debug_format_does_not_panic() {
        let view = TabBarView::new(vec![flui_widgets::SizedBox::shrink().boxed()]);
        let rendered = format!("{view:?}");
        assert!(rendered.contains("TabBarView"));
    }
}
