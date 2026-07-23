//! [`CupertinoTabScaffold`] — a tabbed iOS application's root layout: a
//! [`crate::CupertinoTabBar`] at the bottom, per-tab content built lazily
//! and kept alive [`Offstage`] once visited — and [`CupertinoTabController`],
//! the shared selection state driving it.
//!
//! Flutter parity: `cupertino/tab_scaffold.dart`'s `CupertinoTabScaffold`,
//! `CupertinoTabController`, and `_TabSwitchingView` (oracle tag `3.44.0`).
//! See "Deferred, named" below for exactly what this V1 does not carry over.
//!
//! ## What this ports
//!
//! - The `_TabSwitchingView` mechanic: every tab mounts a slot up front, but
//!   a tab's content is only ever *built* the first time it becomes active
//!   (`should_build_tab`, tracked per index and never reset — "once
//!   visited, stays built"), and every non-active tab is
//!   [`HeroMode`]-disabled + [`Offstage`]-hidden + [`TickerMode`]-disabled
//!   rather than unmounted — so an inactive tab's own state (a counter, a
//!   scroll position, a nested `Navigator` stack) survives a switch away and
//!   back, and its animations genuinely stop advancing while hidden
//!   ("Off stage tabs' animations are stopped", `tab_scaffold.dart`'s own
//!   doc comment on `_TabSwitchingView`) — nested in oracle order,
//!   `HeroMode(enabled: active, child: Offstage(offstage: !active, child:
//!   TickerMode(enabled: active, child: …)))`.
//! - The content-padding contract: content is pushed up by exactly
//!   [`preferred_size`](PreferredSizeView::preferred_size)'s height
//!   *plus* `MediaQuery.padding.bottom`, unless the on-screen keyboard
//!   inset is already taller than the tab bar (`tab_scaffold.dart`'s exact
//!   two-step `contentPadding` computation, oracle tag `3.44.0` — a real,
//!   not simplified, edge case).
//! - `resize_to_avoid_bottom_inset` (default `true`), same contract as
//!   [`crate::CupertinoPageScaffold`]'s.
//! - The scaffold background, resolved from
//!   [`crate::CupertinoThemeData::scaffold_background_color`] the same way.
//!
//! ## `CupertinoTabController` is required, not auto-created
//!
//! The oracle creates and owns an internal `RestorableCupertinoTabController`
//! when the caller supplies none, so a `CupertinoTabScaffold` with no
//! `controller` argument still works. That auto-creation exists mostly to
//! give `RestorationMixin` something to restore — a feature this crate does
//! not port (no restoration substrate in FLUI at all yet). Without it, an
//! internally-created controller would just be state a caller can never
//! reach to drive tab switches programmatically, which is worse than
//! requiring one explicitly. This port always takes an explicit
//! [`CupertinoTabController`]; there is no auto-create fallback.
//!
//! ## Deferred, named
//!
//! - **State restoration** (`RestorationMixin`, `restorationId`,
//!   `RestorableCupertinoTabController`) — no restoration substrate in FLUI.
//! - **Per-tab `FocusScope`** (`_TabSwitchingViewState`'s
//!   `tabFocusNodes`/`_focusActiveTab`) — no per-tab focus-scope wiring;
//!   [`Offstage`] alone governs visibility.
//! - **Text-scaling suppression on the tab bar**
//!   (`MediaQuery.withNoTextScaling`) — `MediaQueryData` has no
//!   no-scaling variant to apply yet.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_types::geometry::{EdgeInsets, px};
use flui_types::styling::BoxDecoration;
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_view::{AnimatedView, impl_animated_view};
use flui_widgets::{
    DecoratedBox, HeroMode, MediaQuery, Offstage, Padding, Positioned, PreferredSizeView, SizedBox,
    Stack, StackFit, TickerMode,
};

use crate::bottom_tab_bar::CupertinoTabBar;
use crate::colors::CupertinoColor;
use crate::theme::CupertinoTheme;

/// Coordinates tab selection between a [`CupertinoTabBar`] and a
/// [`CupertinoTabScaffold`]. Flutter parity: `CupertinoTabController`
/// (`tab_scaffold.dart`, oracle tag `3.44.0`) — a `ChangeNotifier` wrapping
/// an `int`, ported here as a genuinely `Arc`-shared handle (every
/// `.clone()` observes and mutates the *same* index — unlike
/// `flui_foundation::ValueNotifier<T>`, whose `Clone` deep-copies the
/// value; this controller is handed to both the scaffold and the tab bar's
/// `on_tap` closure and both must see one shared index).
///
/// ```
/// use flui_cupertino::CupertinoTabController;
///
/// let controller = CupertinoTabController::new(0);
/// assert_eq!(controller.index(), 0);
/// controller.set_index(1);
/// assert_eq!(controller.index(), 1);
/// ```
#[derive(Clone)]
pub struct CupertinoTabController {
    index: Arc<AtomicUsize>,
    notifier: ChangeNotifier,
}

impl CupertinoTabController {
    /// A controller starting at `initial_index`.
    #[must_use]
    pub fn new(initial_index: usize) -> Self {
        Self {
            index: Arc::new(AtomicUsize::new(initial_index)),
            notifier: ChangeNotifier::new(),
        }
    }

    /// The index of the currently selected tab.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index.load(Ordering::Acquire)
    }

    /// Selects `index`, notifying listeners if it actually changed. Flutter
    /// parity: `CupertinoTabController.index`'s setter.
    pub fn set_index(&self, index: usize) {
        let previous = self.index.swap(index, Ordering::AcqRel);
        if previous != index {
            self.notifier.notify_listeners();
        }
    }
}

impl std::fmt::Debug for CupertinoTabController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTabController")
            .field("index", &self.index())
            .finish_non_exhaustive()
    }
}

impl Listenable for CupertinoTabController {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

/// A per-tab content builder: `(ctx, tab_index) -> content`. `Rc`-based
/// (owner-local, per ADR-0027).
type TabBuilder = Rc<dyn Fn(&dyn BuildContext, usize) -> BoxedView>;

/// A tabbed iOS application's root layout: [`CupertinoTabBar`] at the
/// bottom, `tab_builder`'s output for the active tab above it. Flutter
/// parity: `CupertinoTabScaffold` (`tab_scaffold.dart`, oracle tag
/// `3.44.0`) — see the module docs for exactly what is and is not ported.
///
/// ```
/// use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem, CupertinoTabController, CupertinoTabScaffold};
/// use flui_view::prelude::*;
/// use flui_widgets::{Icon, IconData, SizedBox};
///
/// let controller = CupertinoTabController::new(0);
/// let tab_bar = CupertinoTabBar::new(vec![
///     CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
///     CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
/// ]);
/// let _scaffold = CupertinoTabScaffold::new(tab_bar, controller, |_ctx, _index| {
///     SizedBox::shrink().into_view().boxed()
/// });
/// ```
#[derive(Clone)]
pub struct CupertinoTabScaffold {
    tab_bar: CupertinoTabBar,
    controller: CupertinoTabController,
    tab_builder: TabBuilder,
    background_color: Option<CupertinoColor>,
    resize_to_avoid_bottom_inset: bool,
}

impl CupertinoTabScaffold {
    /// A scaffold showing `tab_bar` at the bottom and `tab_builder`'s output
    /// for `controller`'s current index above it, with the theme's
    /// `scaffold_background_color` and `resize_to_avoid_bottom_inset: true`.
    #[must_use]
    pub fn new(
        tab_bar: CupertinoTabBar,
        controller: CupertinoTabController,
        tab_builder: impl Fn(&dyn BuildContext, usize) -> BoxedView + 'static,
    ) -> Self {
        Self {
            tab_bar,
            controller,
            tab_builder: Rc::new(tab_builder),
            background_color: None,
            resize_to_avoid_bottom_inset: true,
        }
    }

    /// Overrides the resolved background. Defaults to
    /// [`crate::CupertinoThemeData::scaffold_background_color`]. Flutter
    /// parity: `CupertinoTabScaffold.backgroundColor`.
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Whether content should size itself to avoid the window's bottom
    /// inset. Defaults to `true`. Flutter parity:
    /// `CupertinoTabScaffold.resizeToAvoidBottomInset`.
    #[must_use]
    pub fn resize_to_avoid_bottom_inset(mut self, resize: bool) -> Self {
        self.resize_to_avoid_bottom_inset = resize;
        self
    }
}

impl std::fmt::Debug for CupertinoTabScaffold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTabScaffold")
            .field("tab_count", &self.tab_bar.items().len())
            .field("current_index", &self.controller.index())
            .finish_non_exhaustive()
    }
}

impl_animated_view!(CupertinoTabScaffold);

impl AnimatedView for CupertinoTabScaffold {
    /// Rebuilds whenever the controller's index changes — Flutter parity:
    /// `_CupertinoTabScaffoldState._onCurrentIndexChange`'s `setState`.
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::new(self.controller.clone()) as Arc<dyn Listenable>
    }
}

impl StatefulView for CupertinoTabScaffold {
    type State = CupertinoTabScaffoldState;

    fn create_state(&self) -> Self::State {
        CupertinoTabScaffoldState {
            should_build_tab: RefCell::new(Vec::new()),
        }
    }
}

/// Persistent state for [`CupertinoTabScaffold`]: which tabs have ever been
/// built. Flutter parity: `_TabSwitchingViewState.shouldBuildTab` — grown
/// or truncated to match the tab count on every build (`didUpdateWidget`'s
/// partial-invalidation contract), never reset for an index that already
/// built once.
pub struct CupertinoTabScaffoldState {
    should_build_tab: RefCell<Vec<bool>>,
}

impl std::fmt::Debug for CupertinoTabScaffoldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTabScaffoldState")
            .finish_non_exhaustive()
    }
}

impl ViewState<CupertinoTabScaffold> for CupertinoTabScaffoldState {
    fn build(&self, view: &CupertinoTabScaffold, ctx: &dyn BuildContext) -> impl IntoView {
        let current_index = view.controller.index();
        let tab_count = view.tab_bar.items().len();

        // Flutter parity: the constructor's own `assert(controller == null ||
        // controller.index < tabBar.items.length, ...)` plus
        // `_onCurrentIndexChange`'s identical `assert` on every subsequent
        // `controller.index` change (`tab_scaffold.dart`, oracle tag
        // `3.44.0`) — both debug-only, like `debug_assert!`. Unlike a bare
        // port of just those asserts, the oracle *also* crashes
        // unconditionally in **release** builds: `_TabSwitchingViewState
        // ._focusActiveTab` indexes `tabFocusNodes[widget.currentTabIndex]`,
        // and Dart's `List` bounds-checks on every build profile, so an
        // out-of-range index throws a `RangeError` there regardless of
        // `assert` stripping. This port has no `tabFocusNodes` array (see the
        // module doc's "Deferred, named" — no per-tab `FocusScope` wiring),
        // so it has no equivalent unconditional check to inherit for free.
        // Named divergence, not a silent one: release builds (where
        // `debug_assert!` compiles out) fall through to every tab
        // `Offstage`-hidden and `tab_builder` never invoked for
        // `current_index` — the oracle instead crashes hard in every build
        // mode. Do not "fix" this by silently clamping `current_index`; the
        // oracle doesn't clamp either, it crashes.
        //
        // A panic here is caught by this crate's own build-error boundary
        // (`build_or_recover`, `flui_view::element::behavior_commons`) and
        // substitutes an `ErrorView` for this whole subtree rather than
        // unwinding to the caller — mirroring Flutter's own
        // `ComponentElement.performRebuild` try/catch → `ErrorWidget.builder`
        // recovery for a `build()`-phase exception. So this crash is loud
        // (a rendered error) rather than silent, but it is not a raw unwind
        // out of `build`; see `tests/tab_scaffold.rs`'s
        // `out_of_range_controller_index_builds_an_error_instead_of_silently_hiding_every_tab`.
        debug_assert!(
            is_valid_tab_index(current_index, tab_count),
            "CupertinoTabScaffold's current index {current_index} is out of bounds for \
             the tab bar with {tab_count} tabs"
        );

        let tab_layers: Vec<BoxedView> = {
            let mut should_build = self.should_build_tab.borrow_mut();
            should_build.resize(tab_count, false);
            if let Some(flag) = should_build.get_mut(current_index) {
                *flag = true;
            }

            (0..tab_count)
                .map(|index| {
                    let active = index == current_index;
                    let content = if should_build[index] {
                        (view.tab_builder)(ctx, index)
                    } else {
                        SizedBox::shrink().boxed()
                    };
                    HeroMode::new(
                        Offstage::new()
                            .offstage(!active)
                            .child(TickerMode::new(content).enabled(active)),
                    )
                    .enabled(active)
                    .boxed()
                })
                .collect()
        };

        let media = MediaQuery::maybe_of(ctx).unwrap_or_default();
        let tab_bar_height = px(view.tab_bar.preferred_size().height.get());

        let mut reduced = media.clone();
        let mut content_padding_bottom = px(0.0);

        if view.resize_to_avoid_bottom_inset {
            reduced.view_insets.bottom = px(0.0);
            content_padding_bottom = media.view_insets.bottom;
        }

        // Only pad content with the tab bar's height if it isn't already
        // entirely obstructed by the keyboard (or another view inset) —
        // `tab_scaffold.dart`'s exact two-step contract (oracle tag
        // `3.44.0`), not a simplification: don't double-pad.
        if !view.resize_to_avoid_bottom_inset || tab_bar_height > media.view_insets.bottom {
            let bottom_padding = tab_bar_height + media.padding.bottom;
            if view.tab_bar.opaque(ctx) {
                // Opaque: directly stop content higher, and the bar's own
                // height is fully consumed out of the republished padding.
                content_padding_bottom = bottom_padding;
                reduced.padding.bottom = px(0.0);
            } else {
                // Translucent: content may draw behind the bar; hint the
                // obstructed area via padding instead of shifting content.
                reduced.padding.bottom = bottom_padding;
            }
        }

        let content = MediaQuery::new(
            reduced,
            Padding::new(EdgeInsets::new(
                px(0.0),
                px(0.0),
                content_padding_bottom,
                px(0.0),
            ))
            .child(Stack::new(tab_layers).fit(StackFit::Expand)),
        );

        let background = view
            .background_color
            .unwrap_or_else(|| CupertinoTheme::of(ctx).scaffold_background_color())
            .resolve(ctx);

        // The tab bar's own `currentIndex`/`onTap` are overridden here —
        // `_CupertinoTabScaffoldState.build`'s `widget.tabBar.copyWith(...)`,
        // ported as `Clone` + builder methods rather than a hand-written
        // `copyWith` — see `bottom_tab_bar.rs`'s module docs. The original
        // handler is captured first so the override can still chain into
        // it, exactly as the oracle's `widget.tabBar.onTap?.call(newIndex)`.
        let original_on_tap = view.tab_bar.on_tap_handler();
        let controller = view.controller.clone();
        let bar = view
            .tab_bar
            .clone()
            .current_index(current_index)
            .on_tap(move |index| {
                controller.set_index(index);
                if let Some(original) = &original_on_tap {
                    original(index);
                }
            });

        DecoratedBox::new(BoxDecoration::with_color(background)).child(Stack::new(vec![
            content.boxed(),
            Positioned::new(bar)
                .left(0.0)
                .right(0.0)
                .bottom(0.0)
                .boxed(),
        ]))
    }
}

/// Whether `current_index` is a mountable tab index for `tab_count` tabs.
/// Extracted from `build`'s `debug_assert!` so the exact guard condition is
/// unit-testable without mounting a render tree — see `build`'s doc comment
/// for the full oracle-mechanism citation.
fn is_valid_tab_index(current_index: usize, tab_count: usize) -> bool {
    current_index < tab_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_tab_index_accepts_every_in_range_index() {
        assert!(is_valid_tab_index(0, 2));
        assert!(is_valid_tab_index(1, 2));
    }

    /// Red-check: change `is_valid_tab_index` to `current_index <= tab_count`
    /// (an off-by-one) — this assertion starts passing when it shouldn't.
    #[test]
    fn is_valid_tab_index_rejects_the_first_out_of_range_index() {
        assert!(
            !is_valid_tab_index(2, 2),
            "index == tab_count is out of range"
        );
    }

    #[test]
    fn is_valid_tab_index_rejects_a_far_out_of_range_index() {
        assert!(!is_valid_tab_index(5, 2));
    }
}
