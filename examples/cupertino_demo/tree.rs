//! The Cupertino sample-app tree — shared, via `#[path]`-inclusion, between
//! `examples/cupertino_demo/main.rs` (mounted on a live window through
//! `flui_app::run_app`) and the root-crate acceptance test
//! `tests/cupertino_demo.rs` (mounted headlessly through
//! `flui_binding::HeadlessBinding`). Both consumers exercise the exact same
//! tree, so the acceptance test proves the tree the example actually runs.
//!
//! Built entirely on `flui-cupertino`'s and `flui-widgets`' public APIs — no
//! raw render objects. This is the Catalog.1 Cupertino sample-app exit
//! criterion: "`CupertinoTabScaffold` + `CupertinoNavigationBar` + a
//! `CupertinoPageRoute` swipe-back renders and is interactive."
//!
//! # Composition
//!
//! [`CupertinoDemoApp`] wraps [`CupertinoDemoRoot`] in
//! `MediaQuery(default) -> CupertinoTheme(default)`, matching
//! `material_demo::tree::MaterialDemoApp`'s identical wrapping rationale:
//! `CupertinoPageScaffold`/`CupertinoNavigationBar` call `SafeArea`, which
//! calls `MediaQuery::of` unconditionally (panicking with no ancestor).
//!
//! [`CupertinoDemoRoot`] builds a two-tab [`CupertinoTabScaffold`]:
//! - **Home** tab: [`HomeTab`] owns its own `NavigatorHandle` (created once,
//!   in `create_state` — the same split `material_demo::tree::MaterialDemoRoot`
//!   uses for its single navigator, one level down here since each tab gets
//!   its own independent push/pop stack). Its home route is a
//!   `CupertinoPageScaffold` with a `CupertinoNavigationBar` (middle title)
//!   and a column of `CupertinoButton`s; the middle button pushes
//!   [`details_route`] — a `cupertino_page_route` whose page has its own
//!   nav bar (with a back-chevron `leading` button) and content.
//! - **Settings** tab: [`SettingsTab`] holds a `CupertinoButton`-driven
//!   counter in its persistent `State` — proof, together with
//!   `CupertinoTabScaffold`'s own `Offstage`-not-unmount mechanic, that
//!   switching to Home and back does not reset it.
//!
//! # Honest caveats (Catalog.1 exit criterion, Cupertino half)
//!
//! This app proves `CupertinoTabScaffold`/`CupertinoNavigationBar`/
//! `cupertino_page_route` mount, lay out, and respond to real gesture
//! dispatch — including an edge-swipe-back drag. It inherits every
//! deferral each component's own module docs already name (no large-title
//! nav bar, no nav-bar blur, no automatic-leading back button, no tab-bar
//! translucency); nothing here works around them.

use std::cell::Cell;
use std::rc::Rc;

use flui_cupertino::{
    CupertinoButton, CupertinoNavigationBar, CupertinoPageScaffold, CupertinoTabBar,
    CupertinoTabBarItem, CupertinoTabController, CupertinoTabScaffold, CupertinoTheme,
    CupertinoThemeData, cupertino_page_route,
};
use flui_view::RebuildHandle;
use flui_widgets::column;
use flui_widgets::prelude::*;

/// The Home tab's bar label.
pub const HOME_TAB_LABEL: &str = "Home";
/// The Settings tab's bar label.
pub const SETTINGS_TAB_LABEL: &str = "Settings";
/// The Home route's nav bar title.
pub const HOME_NAV_TITLE: &str = "Cupertino Demo";
/// The Settings tab's nav bar title — distinct from [`SETTINGS_TAB_LABEL`]
/// (which also renders, simultaneously, as the tab bar item's own label)
/// so a test can tell the two apart by rendered text alone.
pub const SETTINGS_NAV_TITLE: &str = "Settings Tab";
/// The pushed Details route's nav bar title.
pub const DETAILS_NAV_TITLE: &str = "Details";
/// The Details route's body text — distinct from every other rendered text
/// so tests can tell the routes apart by content alone.
pub const DETAILS_ROUTE_TEXT: &str = "Details route";
/// The Home page's push-navigation button label.
pub const PUSH_BUTTON_LABEL: &str = "Push Details";
/// The Details route's explicit back button label (in addition to the nav
/// bar's own leading chevron).
pub const BACK_BUTTON_LABEL: &str = "Back";
/// The Details route's nav-bar leading button label — distinct from
/// [`BACK_BUTTON_LABEL`] so a test can tell the two back controls apart by
/// rendered text alone.
pub const NAV_BACK_LABEL: &str = "‹";
/// The Settings tab's counter button label.
pub const INCREMENT_BUTTON_LABEL: &str = "Increment";

/// `Icons`-style codepoints for the two tab items. Renders as tofu (no
/// bundled icon font in this substrate) — the same accepted gap
/// `examples/material_demo/tree.rs::settings_icon_data` already documents.
fn home_icon_data() -> IconData {
    IconData::new(0xF3A1)
}

fn settings_icon_data() -> IconData {
    IconData::new(0xF411)
}

/// The Details route, pushed by the Home tab's middle button.
///
/// `leading` is an explicit back-chevron `CupertinoButton` — `V1`'s
/// `CupertinoNavigationBar` has no `automaticallyImplyLeading` heuristic
/// (see that module's own deferred list), so a pushed page wanting a back
/// control must supply one itself.
fn details_route() -> PageRoute<()> {
    cupertino_page_route::<(), _>(|ctx, _primary, _secondary| {
        let navigator = NavigatorHandle::maybe_of(ctx)
            .expect("BUG: details_route only builds inside HomeTab's own Navigator");
        let navigator_for_leading = navigator.clone();
        let navigator_for_button = navigator;

        CupertinoPageScaffold::new(Center::new().child(Column::new(column![
            Text::new(DETAILS_ROUTE_TEXT),
            CupertinoButton::new(Text::new(BACK_BUTTON_LABEL)).on_pressed(move || {
                navigator_for_button.pop();
            }),
        ])))
        .navigation_bar(
            CupertinoNavigationBar::new()
                .middle(Text::new(DETAILS_NAV_TITLE))
                .leading(
                    CupertinoButton::new(Text::new(NAV_BACK_LABEL)).on_pressed(move || {
                        navigator_for_leading.pop();
                    }),
                ),
        )
        .into_view()
        .boxed()
    })
    .named("details")
}

/// The Home tab's persistent `Navigator` shell over its own home route.
///
/// Split into its own `StatefulView` (rather than building the `Navigator`
/// straight in `CupertinoDemoRoot`) so the tab keeps one push/pop stack
/// across `CupertinoTabScaffold`'s own rebuilds — the same reason
/// `material_demo::tree::MaterialDemoRoot` creates its `NavigatorHandle`
/// once, in `create_state`, rather than on every `build`.
#[derive(Clone, StatefulView)]
struct HomeTab;

struct HomeTabState {
    navigator: NavigatorHandle,
}

impl StatefulView for HomeTab {
    type State = HomeTabState;

    fn create_state(&self) -> Self::State {
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(SimpleRoute::<()>::new(home_page).named("/"));
        HomeTabState { navigator }
    }
}

impl ViewState<HomeTab> for HomeTabState {
    fn build(&self, _view: &HomeTab, _ctx: &dyn BuildContext) -> impl IntoView {
        Navigator::new(self.navigator.clone())
    }
}

/// The Home tab's root page content.
fn home_page(ctx: &dyn BuildContext) -> BoxedView {
    let navigator = NavigatorHandle::maybe_of(ctx)
        .expect("BUG: home_page only builds inside HomeTab's own Navigator");

    CupertinoPageScaffold::new(Center::new().child(Column::new(column![
        CupertinoButton::new(Text::new("Item 1")),
        CupertinoButton::filled(Text::new(PUSH_BUTTON_LABEL)).on_pressed(move || {
            navigator.push(details_route());
        }),
        CupertinoButton::new(Text::new("Item 2")),
    ])))
    .navigation_bar(CupertinoNavigationBar::new().middle(Text::new(HOME_NAV_TITLE)))
    .into_view()
    .boxed()
}

/// The Settings tab: a counter driven by a `CupertinoButton`, whose value
/// must survive switching to Home and back — `CupertinoTabScaffold`'s
/// `Offstage`-not-unmount contract, proven end to end.
#[derive(Clone, StatefulView)]
struct SettingsTab {
    /// Shared with [`CupertinoDemoRoot`] so the acceptance test can read the
    /// count directly, the same `Rc`-exposed-to-the-test pattern
    /// `material_demo::tree::MaterialDemoRoot::selected` uses.
    count: Rc<Cell<u32>>,
}

struct SettingsTabState {
    count: Rc<Cell<u32>>,
    /// `None` only before `init_state` has run; every `build` call happens
    /// after it (`ViewState` lifecycle order), so it is always `Some` there.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for SettingsTab {
    type State = SettingsTabState;

    fn create_state(&self) -> Self::State {
        SettingsTabState {
            count: Rc::clone(&self.count),
            rebuild: None,
        }
    }
}

impl ViewState<SettingsTab> for SettingsTabState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // Lifecycle-only acquisition (ADR-0018, port-check trigger #22) —
        // matches `material_demo::tree::MaterialDemoHomeState::init_state`.
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &SettingsTab, _ctx: &dyn BuildContext) -> impl IntoView {
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build (ViewState lifecycle order)");
        let count_for_tap = Rc::clone(&self.count);

        CupertinoPageScaffold::new(Center::new().child(Column::new(column![
            Text::new(format!("Count: {}", self.count.get())),
            CupertinoButton::new(Text::new(INCREMENT_BUTTON_LABEL)).on_pressed(move || {
                count_for_tap.set(count_for_tap.get() + 1);
                rebuild.schedule();
            }),
        ])))
        .navigation_bar(CupertinoNavigationBar::new().middle(Text::new(SETTINGS_NAV_TITLE)))
    }
}

/// The Cupertino demo root: a two-tab [`CupertinoTabScaffold`].
///
/// `controller`/`settings_count` are `Rc`/handle-shared so a caller (the
/// acceptance test) can keep a clone from before mounting — the same
/// pattern `material_demo::tree::MaterialDemoRoot` uses for its own
/// externally-inspectable state.
#[derive(Clone, StatelessView)]
pub struct CupertinoDemoRoot {
    /// The shared tab-selection controller.
    pub controller: CupertinoTabController,
    /// The Settings tab's counter — `0` until the acceptance test (or a
    /// real user) taps [`INCREMENT_BUTTON_LABEL`].
    pub settings_count: Rc<Cell<u32>>,
}

impl CupertinoDemoRoot {
    /// A fresh demo tree, Home tab active, counter at `0`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            controller: CupertinoTabController::new(0),
            settings_count: Rc::new(Cell::new(0)),
        }
    }
}

impl Default for CupertinoDemoRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl StatelessView for CupertinoDemoRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let tab_bar = CupertinoTabBar::new(vec![
            CupertinoTabBarItem::new(Icon::new(home_icon_data())).label(HOME_TAB_LABEL),
            CupertinoTabBarItem::new(Icon::new(settings_icon_data())).label(SETTINGS_TAB_LABEL),
        ]);

        let settings_count = Rc::clone(&self.settings_count);
        CupertinoTabScaffold::new(tab_bar, self.controller.clone(), move |_ctx, index| {
            if index == 0 {
                HomeTab.into_view().boxed()
            } else {
                SettingsTab {
                    count: Rc::clone(&settings_count),
                }
                .into_view()
                .boxed()
            }
        })
    }
}

/// Build a fresh demo tree, ready to mount.
#[must_use]
pub fn demo_root() -> CupertinoDemoRoot {
    CupertinoDemoRoot::new()
}

/// Thin `StatelessView` entry point for [`flui_app::run_app`](https://docs.rs/flui-app),
/// which requires a stateless root. Wraps the tree in
/// `MediaQuery(default) -> CupertinoTheme(default)` once, at the very root —
/// see the module docs' composition section for why both ancestors are
/// required.
#[derive(Clone, StatelessView)]
pub struct CupertinoDemoApp;

impl StatelessView for CupertinoDemoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoTheme::new(CupertinoThemeData::default(), demo_root()),
        )
    }
}
