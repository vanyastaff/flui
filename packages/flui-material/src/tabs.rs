//! [`Tab`] and [`TabBar`] — the secondary M3 tab bar. See the module docs
//! below for exactly which contract this V1 ships.
//!
//! # `TabBar` ships the SECONDARY contract only
//!
//! Material 3 defines two tab bars: the primary (a 3dp,
//! label-width indicator meant for an `AppBar.bottom`) and the secondary (a
//! 2dp, full-tab-width indicator meant to separate content within a page
//! body). This crate ships **only** the
//! secondary style, as [`TabBar::secondary`] — there is no plain
//! `TabBar::new` that could read as "the primary bar, just less finished".
//! Shipping a primary bar honestly needs `TabBarIndicatorSize::Label`, which
//! needs the *width of each tab's own label content* after layout
//! (a `GlobalKey`-mediated post-layout measurement this crate has no
//! established pattern for yet). Rather than ship a primary bar that
//! silently degrades to `TabBarIndicatorSize::Tab` sizing, primary is
//! deferred wholesale until that measurement exists.
//!
//! # Fixed equal-share layout only (no `isScrollable`)
//!
//! A non-scrollable bar gives every tab an equal share via `Expanded` —
//! that is the only layout this crate implements.
//! `isScrollable`, `TabAlignment::{Start, StartOffset, Center}`, and
//! `scrollController` are named deferrals: the scrollable path additionally
//! needs post-layout tab-offset bookkeeping and a
//! horizontal `SingleChildScrollView`, neither of which any test in this
//! unit's acceptance list exercises.
//!
//! # Indicator: one rect painted from the controller's position
//!
//! Each tab cell reserves an `indicator_weight` band at its bottom edge, and
//! one `CustomPaint` over the whole bar paints the indicator into that band
//! from [`crate::TabController::position`]. The painter repaints on the
//! position's notifications without rebuilding the bar. Between whole tabs
//! the rect follows [`TabIndicatorAnimation`]: `Linear` interpolates both
//! edges, `Elastic` lets the leading edge lead and the trailing edge follow,
//! so the indicator stretches mid-way. Both are functions of the position
//! alone, so a swipe, a tap animation and a reversal all paint the same rect
//! for the same position. Under a right-to-left `Directionality` the tab row
//! runs from the right and the indicator mirrors with it.
//!
//! The divider (`showDivider: true` for a non-scrollable M3 bar) is a
//! `Positioned` full-width strip at the bar's bottom edge, stacked first, so
//! the tab row and then the indicator paint over it.
//!
//! # Named deferrals (not silently dropped)
//!
//! - **The primary bar** and its label-width indicator, whose M3 default is
//!   the elastic animation — see the "SECONDARY contract" section above. The
//!   secondary bar defaults to `Linear` and takes `Elastic` on request.
//! - **Label color interpolation** — label colors follow the committed
//!   `index`, not the fractional position.
//! - **`TabBarIndicatorSize::Label`**, custom `indicator: Decoration`,
//!   `indicatorPadding`, rounded-corner indicators — primary-bar-only or
//!   `Label`-sizing-only features; see the "SECONDARY contract"
//!   section above.
//! - **`WidgetStateColor` for `labelColor`** — a full tab bar lets
//!   `labelColor` itself be state-varying, ignoring `unselectedLabelColor`
//!   when it is. This crate resolves `labelColor`/`unselectedLabelColor` as
//!   two plain colors (exactly the shape of the M3 secondary default
//!   table, whose label colors are plain `Color`s).
//!   `overlayColor` — the hover/press/focus ramp — IS state-resolved (a
//!   genuine `WidgetStateProperty`), since the M3 default table needs it.
//! - **Icon recoloring** — a full tab bar wraps tab content in
//!   `IconTheme.merge` so a bare `Icon` child inherits the resolved
//!   label/icon color. This crate wraps only in
//!   [`flui_sdk::widgets::DefaultTextStyle`] (text recoloring); a caller-supplied
//!   icon keeps whatever color it was given. No test in this unit's
//!   acceptance list exercises icon color.
//! - **`labelPadding`/`TabBarThemeData` overrides for it** — fixed at
//!   [`kTabLabelPadding`](Self) (`16.0` horizontal), no widget or theme
//!   override surface yet.
//! - **`onHover`/`onFocusChange`/`mouseCursor`/`splashFactory`/
//!   `splashBorderRadius`/`dragStartBehavior`/`physics`/`textScaler`** — no
//!   consumer yet; `InkWell`'s own defaults apply.
//! - **The "Tab N of M" label** — a full tab bar adds it to each tab's
//!   semantics. Each tab here is one `SemanticsRole::Tab` node carrying its
//!   selected state and its content's text, inside a `SemanticsRole::TabList`
//!   node for the bar, as [`crate::NavigationBar`]'s destinations are; the
//!   position label needs a localizations table this crate does not have yet.
//! - **`enableFeedback`** — `InkWell.enableFeedback` (haptic/
//!   acoustic click feedback on tap, defaulting to `true`) has no analog
//!   here; [`crate::InkWell`] itself has no `enableFeedback` parameter yet
//!   (see that module's own docs), so `TabBar` has nothing to plumb it to.
//! - **`automaticIndicatorColorAdjustment`** — a full tab bar snaps
//!   `indicatorColor` to white when it would otherwise match the ambient
//!   `Material`'s own fill color (avoiding an invisible indicator). This
//!   crate's indicator band is a plain `Container` fill with no ambient-color
//!   lookup at all — an indicator configured (via `TabBarThemeData` or a
//!   theme with an unusual `ColorScheme.primary`) to match the bar's actual
//!   background paints invisibly, with no automatic correction.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use flui_sdk::animation::Vsync;
use flui_sdk::foundation::panic::PanicRecovery;
use flui_sdk::foundation::{Listenable, ListenerId};
use flui_sdk::geometry::{EdgeInsets, Rect, Size};
use flui_sdk::painting::{Canvas, Color, Paint, TextDirection, TextStyle};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, RebuildHandle};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Center, Column, CompanionRegistration, Container, CrossAxisAlignment, CustomPaint,
    CustomPainter, DefaultTextStyle, Directionality, Expanded, IgnorePointer, MergeSemantics,
    Padding, Positioned, PreferredSizeView, Row, Semantics, SemanticsAnchor, SemanticsRole,
    SizedBox, Stack, Text, WidgetState, WidgetStateConstraint, WidgetStateProperty,
};

use crate::ink_well::InkWell;
use crate::ripple::StateLayerAlpha;
use crate::tab_controller::{ClockLease, DefaultTabController, TabController, TabPosition};
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// A `Tab` with no icon's height.
pub const TAB_HEIGHT: f64 = 46.0;

/// A `Tab` with both an icon and text/child's height.
pub const TEXT_AND_ICON_TAB_HEIGHT: f64 = 72.0;

/// The horizontal padding every tab label gets, both sides
/// (`EdgeInsets.symmetric(horizontal: 16.0)`) — see the module docs for why this crate has no override surface
/// for it yet.
pub const TAB_LABEL_HORIZONTAL_PADDING: f64 = 16.0;

/// One [`TabBar`] tab's label content: some combination of `text`/`child`
/// and `icon`.
///
/// ```
/// use flui_material::Tab;
///
/// let _text_tab = Tab::new().text("Home");
/// let _custom_height_tab = Tab::new().text("Settings").height(56.0);
/// ```
#[derive(Clone, Debug, Default, StatelessView)]
pub struct Tab {
    text: Option<String>,
    child: Option<BoxedView>,
    icon: Option<BoxedView>,
    height: Option<f64>,
}

impl Tab {
    /// An empty tab — set `text`, `child`, and/or `icon` before using it (at
    /// least one is required; a tab with none debug-asserts in `build`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tab's text label. Mutually exclusive with
    /// [`child`](Self::child) — the last one set wins (a builder has no
    /// single constructor call to assert against).
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self.child = None;
        self
    }

    /// Sets an arbitrary label widget in place of [`text`](Self::text).
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view().boxed());
        self.text = None;
        self
    }

    /// Adds an icon above the text/child label (or, alone, makes this an
    /// icon-only tab).
    #[must_use]
    pub fn icon(mut self, icon: impl IntoView) -> Self {
        self.icon = Some(icon.into_view().boxed());
        self
    }

    /// Overrides the computed height (`46.0`, or `72.0` when both an icon
    /// and text/child are present).
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    fn has_icon(&self) -> bool {
        self.icon.is_some()
    }

    fn has_text_or_child(&self) -> bool {
        self.text.is_some() || self.child.is_some()
    }
}

/// This tab's content height: `height` override first, else `72.0` when
/// both an icon and text/child are present, else `46.0`. One function here
/// serves both [`Tab::preferred_size`] and [`TabBar`]'s own height math.
fn tab_content_height(tab: &Tab) -> f64 {
    if let Some(height) = tab.height {
        return height;
    }
    if tab.has_icon() && tab.has_text_or_child() {
        TEXT_AND_ICON_TAB_HEIGHT
    } else {
        TAB_HEIGHT
    }
}

/// `_TabsPrimaryDefaultsM3.iconMargin` (`Tab.iconMargin`'s M3 default,
/// `EdgeInsets.only(bottom: 2.0)`, per `Tab.iconMargin`'s own doc comment)
/// — no per-tab override surface yet.
fn icon_margin() -> Padding {
    Padding::only(0.0, 0.0, 0.0, 2.0)
}

impl StatelessView for Tab {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        debug_assert!(
            self.has_text_or_child() || self.has_icon(),
            "Tab requires at least one of text, child, or icon"
        );

        let label: BoxedView = if !self.has_icon() {
            label_content(self)
        } else if !self.has_text_or_child() {
            self.icon
                .clone()
                .expect("has_icon() checked above guarantees this")
        } else {
            Column::new(vec![
                icon_margin()
                    .child(
                        self.icon
                            .clone()
                            .expect("has_icon() checked above guarantees this"),
                    )
                    .boxed(),
                label_content(self).boxed(),
            ])
            .boxed()
        };

        SizedBox::height(tab_content_height(self)).child(Center::new().child(label))
    }
}

/// The text/child label alone (no icon) — `child` if set, else a plain
/// `Text(text)`. Only called where `has_text_or_child()` is already known
/// true.
fn label_content(tab: &Tab) -> BoxedView {
    if let Some(child) = &tab.child {
        child.clone()
    } else {
        Text::new(
            tab.text
                .clone()
                .expect("caller guarantees text or child is set"),
        )
        .boxed()
    }
}

impl PreferredSizeView for Tab {
    fn preferred_size(&self) -> Size {
        Size::new(0.0, tab_content_height(self))
    }
}

/// The M3 secondary tab bar — see the module docs for exactly what this
/// ships (secondary only, fixed equal-share layout, an indicator that
/// follows [`TabController::position`]).
///
/// A [`TabController`] is required either explicitly (via
/// [`controller`](Self::controller)) or via a
/// [`DefaultTabController`] ancestor — exactly
/// one must be reachable, or `build` panics.
///
/// ```
/// use flui_material::{DefaultTabController, Tab, TabBar};
///
/// let tabs = vec![Tab::new().text("One"), Tab::new().text("Two")];
/// let bar = DefaultTabController::new(tabs.len(), TabBar::secondary(tabs));
/// ```
#[derive(Clone, StatefulView)]
pub struct TabBar {
    tabs: Vec<Tab>,
    controller: Option<TabController>,
    indicator_weight: f64,
    indicator_animation: TabIndicatorAnimation,
    on_tap: Option<crate::event_callback::ValueCallback<usize>>,
}

impl TabBar {
    /// The M3 secondary tab bar over `tabs`. See the module/type docs for
    /// why this is the only constructor this crate ships.
    #[must_use]
    pub fn secondary(tabs: Vec<Tab>) -> Self {
        Self {
            tabs,
            controller: None,
            indicator_weight: 2.0,
            indicator_animation: TabIndicatorAnimation::Linear,
            on_tap: None,
        }
    }

    /// How the indicator moves between tabs. Defaults to
    /// [`TabIndicatorAnimation::Linear`], the M3 default for a full-tab-width
    /// indicator.
    #[must_use]
    pub fn indicator_animation(mut self, animation: TabIndicatorAnimation) -> Self {
        self.indicator_animation = animation;
        self
    }

    /// Supplies an explicit [`TabController`] instead of relying on a
    /// [`DefaultTabController`] ancestor.
    #[must_use]
    pub fn controller(mut self, controller: TabController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// A callback fired with the tapped tab's index, in addition to (not
    /// instead of) the default `controller.animate_to(index)` dispatch.
    #[must_use]
    pub fn on_tap<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>, usize) -> R + 'static,
    ) -> Self {
        self.on_tap = Some(crate::event_callback::value_callback(callback));
        self
    }
}

impl std::fmt::Debug for TabBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBar")
            .field("tab_count", &self.tabs.len())
            .field("has_explicit_controller", &self.controller.is_some())
            .finish_non_exhaustive()
    }
}

impl PreferredSizeView for TabBar {
    fn preferred_size(&self) -> Size {
        Size::new(f64::INFINITY, bar_height(&self.tabs, self.indicator_weight))
    }
}

/// The bar's total height: the tallest tab's content height (`46.0` if
/// `tabs` is empty) plus `indicator_weight`. This also covers the zero-tab
/// special case (`TAB_HEIGHT + indicator_weight`). A nonempty bar uses only
/// its tabs' requested heights, including overrides below the default.
fn bar_height(tabs: &[Tab], indicator_weight: f64) -> f64 {
    let max_content_height = tabs
        .iter()
        .map(tab_content_height)
        .reduce(f64::max)
        .unwrap_or(TAB_HEIGHT);
    max_content_height + indicator_weight
}

/// Whether any tab in `tabs` has both an icon and text/child (i.e. its
/// content height is `TEXT_AND_ICON_TAB_HEIGHT`).
fn tab_has_text_and_icon(tabs: &[Tab]) -> bool {
    tabs.iter()
        .any(|tab| tab_content_height(tab) == TEXT_AND_ICON_TAB_HEIGHT)
}

/// [`TabBar`]'s theme-resolved colors/styles — see [`resolve_style`]'s doc
/// comment for the widget → theme → default cascade (this V1 has no
/// per-widget override for these, only theme → default; see the module
/// docs).
struct ResolvedTabBarStyle {
    indicator_color: Color,
    label_color: Color,
    unselected_label_color: Color,
    label_style: TextStyle,
    unselected_label_style: TextStyle,
    divider_color: Color,
    divider_height: f64,
    overlay_color: WidgetStateProperty<Option<Color>>,
}

/// Resolves the M3 secondary tab bar defaults through the theme → default
/// cascade: `TabBarThemeData` field if set, else the literal M3 secondary
/// default.
///
/// | Field | M3 secondary default |
/// |---|---|
/// | `indicator_color` | `ColorScheme.primary` |
/// | `label_color` | `ColorScheme.onSurface` |
/// | `unselected_label_color` | `ColorScheme.onSurfaceVariant` |
/// | `label_style` / `unselected_label_style` | `TextTheme.titleSmall` |
/// | `divider_color` | `ColorScheme.outlineVariant` |
/// | `divider_height` | `1.0` |
/// | `overlay_color` | pressed→`onSurface@0.1`, hovered→`onSurface@0.08`, focused→`onSurface@0.1`, else none |
fn resolve_style(theme: &ThemeData) -> ResolvedTabBarStyle {
    let tab_bar_theme = theme.tab_bar_theme.as_ref();
    let colors = &theme.color_scheme;
    let title_small = theme.text_theme.title_small.clone().unwrap_or_default();

    ResolvedTabBarStyle {
        indicator_color: tab_bar_theme
            .and_then(|t| t.indicator_color)
            .unwrap_or(colors.primary),
        label_color: tab_bar_theme
            .and_then(|t| t.label_color)
            .unwrap_or(colors.on_surface),
        unselected_label_color: tab_bar_theme
            .and_then(|t| t.unselected_label_color)
            .unwrap_or(colors.on_surface_variant),
        label_style: tab_bar_theme
            .and_then(|t| t.label_style.clone())
            .unwrap_or_else(|| title_small.clone()),
        unselected_label_style: tab_bar_theme
            .and_then(|t| t.unselected_label_style.clone())
            .unwrap_or(title_small),
        divider_color: tab_bar_theme
            .and_then(|t| t.divider_color)
            .unwrap_or(colors.outline_variant),
        divider_height: tab_bar_theme.and_then(|t| t.divider_height).unwrap_or(1.0),
        overlay_color: tab_bar_theme
            .and_then(|t| t.overlay_color.clone())
            .unwrap_or_else(|| default_overlay_color(colors.on_surface)),
    }
}

/// The secondary overlay color's resolver — pressed/hovered ramp over
/// `on_surface`, identical whether or not the tab is selected (the M3 spec's
/// selected and unselected branches happen to produce the same values). A
/// focused tab shows the focus ring, not a layer (ADR-0079).
fn default_overlay_color(on_surface: Color) -> WidgetStateProperty<Option<Color>> {
    WidgetStateProperty::from_map([
        (
            WidgetStateConstraint::Is(WidgetState::Pressed),
            Some(on_surface.with_opacity(StateLayerAlpha::M3.pressed)),
        ),
        (
            WidgetStateConstraint::Is(WidgetState::Hovered),
            Some(on_surface.with_opacity(StateLayerAlpha::M3.hovered)),
        ),
    ])
}

/// This tab's label padding: [`TAB_LABEL_HORIZONTAL_PADDING`] both sides,
/// plus `±13.0` vertical when `tab`'s own content height is `TAB_HEIGHT`
/// (`46.0`) but the bar as a whole has a text-and-icon tab (`72.0`) — the
/// mechanism that centers a plain tab's content inside a taller mixed bar.
/// The vertical adjustment is `(TEXT_AND_ICON_TAB_HEIGHT - TAB_HEIGHT) / 2.0`,
/// i.e. `13.0`, added to the label padding when the tab's height is
/// `TAB_HEIGHT` and the bar has a text-and-icon tab — done as padding, not
/// as a `Center`-widget trick.
fn label_padding(tab: &Tab, bar_has_mixed_tabs: bool) -> EdgeInsets {
    let vertical = if bar_has_mixed_tabs && tab_content_height(tab) == TAB_HEIGHT {
        (TEXT_AND_ICON_TAB_HEIGHT - TAB_HEIGHT) / 2.0
    } else {
        0.0
    };
    EdgeInsets::symmetric(vertical, TAB_LABEL_HORIZONTAL_PADDING)
}

/// Persistent state behind [`TabBar`]: the currently-subscribed
/// [`TabController`] and its listener registration, re-resolved every
/// `build` — see `resolve_controller`'s doc comment (private, `impl
/// TabBarState`) for exactly when the subscription is re-homed.
pub struct TabBarState {
    controller: RefCell<Option<TabController>>,
    listener_id: RefCell<Option<ListenerId>>,
    rebuild: Option<RebuildHandle>,
    /// The ambient clock the controller's position runs on.
    vsync: Option<Vsync>,
    /// This bar's registration of that clock with the subscribed controller.
    clock: RefCell<Option<ClockLease>>,
    /// The anchor each tab's node declares, made with the subscribed
    /// controller, and the bar's registration of them as the companion of the
    /// controller's step commands.
    companion: RefCell<Option<TabCompanion>>,
}

/// A bar's tabs as the single-pointer path of a [`crate::TabBarView`]'s
/// commands: registered while the bar is mounted.
struct TabCompanion {
    anchors: Rc<[SemanticsAnchor]>,
    _registration: CompanionRegistration,
}

impl std::fmt::Debug for TabBarState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBarState")
            .field("controller", &self.controller.borrow())
            .finish_non_exhaustive()
    }
}

impl StatefulView for TabBar {
    type State = TabBarState;

    fn create_state(&self) -> Self::State {
        TabBarState {
            controller: RefCell::new(None),
            listener_id: RefCell::new(None),
            rebuild: None,
            vsync: None,
            clock: RefCell::new(None),
            companion: RefCell::new(None),
        }
    }
}

impl TabBarState {
    /// Resolves `view`'s effective controller (explicit, else
    /// [`DefaultTabController::maybe_of`]) and, if it differs by identity
    /// from the currently-subscribed one, swaps the listener registration
    /// onto it. Called from `build` (see that trait's doc on why `&self`
    /// mutation goes through `RefCell` here, matching
    /// [`crate::InkWellState`]'s own pattern) — `ViewState::did_change_dependencies`
    /// has no `view` parameter, so it cannot see `view.controller` to decide
    /// the fallback, and re-resolving unconditionally on every `build` is
    /// cheap (one identity comparison) and always correct regardless of
    /// which lifecycle hook triggered the rebuild.
    ///
    /// # Panics
    ///
    /// Panics if `view` has no explicit controller and there is no
    /// `DefaultTabController` ancestor.
    fn resolve_controller(&self, view: &TabBar, ctx: &dyn BuildContext) -> TabController {
        let resolved = view
            .controller
            .clone()
            .or_else(|| DefaultTabController::maybe_of(ctx))
            .expect(
                "TabBar requires an explicit controller (TabBar::controller) or a \
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
                rebuild.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            });
            *self.listener_id.borrow_mut() = Some(id);
            let lease = resolved.lease_clock(self.vsync.clone());
            let _previous_lease = self.clock.borrow_mut().replace(lease);
            let _prev = self.controller.borrow_mut().replace(resolved.clone());

            // The previous registration leaves the previous controller's link
            // once the new one is in place; each tab keeps its anchor across
            // rebuilds, so the published tree does not churn.
            let anchors: Rc<[SemanticsAnchor]> = (0..resolved.length())
                .map(|_| SemanticsAnchor::new())
                .collect();
            let companion = TabCompanion {
                _registration: resolved.register_tab_bar(Rc::clone(&anchors)),
                anchors,
            };
            let _previous_companion = self.companion.borrow_mut().replace(companion);
        }

        resolved
    }
}

impl ViewState<TabBar> for TabBarState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        self.vsync = VsyncScope::maybe_of(ctx);
    }

    /// Follows a replaced ambient clock, rebinding the subscribed
    /// controller's position to it.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        if let Some(clock) = self.clock.get_mut() {
            clock.set(self.vsync.clone());
        }
    }

    /// Unregisters this bar's listener from whatever controller it's
    /// currently subscribed to. Without this, a controller that outlives the bar (an explicit
    /// `TabBar::controller` shared with a sibling, or any
    /// `DefaultTabController` ancestor that itself outlives one particular
    /// `TabBar` child) keeps firing an `Rc` closure that calls
    /// `rebuild.schedule(reason)` on a `RebuildHandle` whose element no longer
    /// exists — a leaked listener per unmount, the same class of lifecycle
    /// bug `MaterialTextFieldState::dispose` guards against for its exact
    /// `FocusNode` listener.
    ///
    /// `&mut self` here (unlike `resolve_controller`, called from `build`'s
    /// `&self`) means the `RefCell`s can be read via `get_mut` — a plain
    /// field access with no runtime borrow check — rather than
    /// `borrow_mut`.
    fn dispose(&mut self) {
        drop(self.companion.get_mut().take());
        drop(self.clock.get_mut().take());
        let controller = self.controller.get_mut().take();
        let listener_id = self.listener_id.get_mut().take();
        if let (Some(controller), Some(id)) = (controller, listener_id) {
            controller.remove_listener(id);
        }
    }

    fn build(&self, view: &TabBar, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.resolve_controller(view, ctx);
        let theme = Theme::of(ctx);
        let resolved = resolve_style(&theme);
        let height = bar_height(&view.tabs, view.indicator_weight);

        if view.tabs.is_empty() {
            // Zero-tabs early return: a full-width `SizedBox` of height
            // `TAB_HEIGHT + indicator_weight`. A width-unconstrained
            // `SizedBox::height` behaves identically in every bounded parent
            // this bar is normally mounted under; a named simplification
            // for the one unbounded-width edge case.
            //
            // A controller is still resolved above even for zero tabs:
            // controller resolution runs unconditionally, before `build`
            // ever checks `controller.length() == 0`. A zero-tab `TabBar` with no
            // controller and no `DefaultTabController` ancestor still
            // panics, same as a non-empty one.
            assert!(
                controller.length() == 0,
                "TabBar: empty tabs list does not match the TabController's length of {} — \
                 the tabs list and the tab count must agree",
                controller.length()
            );
            return SizedBox::height(height).boxed();
        }

        assert!(
            view.tabs.len() == controller.length(),
            "TabBar: {} tabs does not match the TabController's length of {} — \
             the tabs list and the tab count must agree",
            view.tabs.len(),
            controller.length()
        );

        let mixed = tab_has_text_and_icon(&view.tabs);
        let current_index = controller.index();
        let anchors = self
            .companion
            .borrow()
            .as_ref()
            .map(|companion| Rc::clone(&companion.anchors))
            .expect("BUG: resolving the controller made the tab anchors");

        let cells: Vec<BoxedView> = view
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                build_tab_cell(
                    index,
                    &anchors[index],
                    tab,
                    index == current_index,
                    mixed,
                    &resolved,
                    view.indicator_weight,
                    &controller,
                    view.on_tap.as_ref(),
                )
            })
            .collect();

        let row = Row::new(cells).cross_axis_alignment(CrossAxisAlignment::Stretch);

        let painter = TabIndicatorPainter {
            position: controller.position(),
            length: view.tabs.len(),
            animation: view.indicator_animation,
            color: resolved.indicator_color,
            weight: view.indicator_weight,
            rtl: Directionality::maybe_of(ctx) == Some(TextDirection::Rtl),
        };
        // The indicator only paints: the tabs' ink wells beneath it own every
        // pointer in the band.
        let indicator =
            Positioned::new(IgnorePointer::new().child(CustomPaint::new().painter(painter)))
                .left(0.0)
                .right(0.0)
                .bottom(0.0)
                .height(view.indicator_weight);

        let mut layers = Vec::with_capacity(3);
        if resolved.divider_height > 0.0 && resolved.divider_color != Color::TRANSPARENT {
            let divider = Positioned::new(
                Container::new()
                    .color(resolved.divider_color)
                    .height(resolved.divider_height),
            )
            .left(0.0)
            .right(0.0)
            .bottom(0.0);
            layers.push(divider.boxed());
        }
        layers.push(row.boxed());
        layers.push(indicator.boxed());

        let bar = SizedBox::height(height).child(Stack::new(layers));
        Semantics::new()
            .role(SemanticsRole::TabList)
            .container(true)
            .explicit_child_nodes(true)
            .child(bar)
            .boxed()
    }
}

/// How a tab indicator moves between two tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabIndicatorAnimation {
    /// Both edges move together: the indicator slides, interpolating its
    /// position and width between the two tabs.
    #[default]
    Linear,
    /// The leading edge leads and the trailing edge follows, so the
    /// indicator stretches across the gap mid-way and contracts onto the
    /// new tab.
    Elastic,
}

/// The indicator's `(left, right)` edges as fractions of the bar width, for
/// `length` equal tabs laid out left to right, at `position` (clamped to the
/// tabs). Each edge eases between the two whole tabs around `position`; the
/// right edge, which leads toward higher indices, takes `sin(πt/2)` under
/// `Elastic` and the left edge `1 − cos(πt/2)`, so moving either way the
/// leading edge is ahead of the trailing one.
fn indicator_span(position: f64, length: usize, animation: TabIndicatorAnimation) -> (f64, f64) {
    if length == 0 {
        return (0.0, 0.0);
    }
    let count = length as f64;
    let position = if position.is_finite() {
        position.clamp(0.0, count - 1.0)
    } else {
        0.0
    };
    let from = position.floor();
    let t = position - from;
    let (lead, trail) = match animation {
        TabIndicatorAnimation::Linear => (t, t),
        TabIndicatorAnimation::Elastic => {
            let quarter = t * std::f64::consts::FRAC_PI_2;
            (quarter.sin(), 1.0 - quarter.cos())
        }
    };
    // Equal shares: tab `i` spans `[i, i + 1] / count`, and the next tab is
    // one share further on.
    let left = (from + trail) / count;
    let right = (from + 1.0 + lead) / count;
    (left, right)
}

/// Paints the tab indicator into the bar's bottom band from the controller's
/// position.
#[derive(Debug)]
struct TabIndicatorPainter {
    position: TabPosition,
    length: usize,
    animation: TabIndicatorAnimation,
    color: Color,
    weight: f64,
    rtl: bool,
}

impl CustomPainter for TabIndicatorPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        if !size.width.is_finite() || !size.height.is_finite() || size.width <= 0.0 {
            return;
        }
        let (left, right) = indicator_span(self.position.value(), self.length, self.animation);
        let (left, right) = if self.rtl {
            (1.0 - right, 1.0 - left)
        } else {
            (left, right)
        };
        if right <= left {
            return;
        }
        canvas.draw_rect(
            Rect::from_ltrb(
                left * size.width,
                size.height - self.weight,
                right * size.width,
                size.height,
            ),
            &Paint::fill(self.color),
        );
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        let Some(old) = old.as_any().downcast_ref::<Self>() else {
            return true;
        };
        !old.position.same_controller(&self.position)
            || old.length != self.length
            || old.animation != self.animation
            || old.color != self.color
            || old.weight != self.weight
            || old.rtl != self.rtl
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(self.position.listenable())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builds one tab's `Expanded` cell: label content (recolored/padded per
/// selection) over the reserved indicator band, wrapped in an [`InkWell`]
/// that dispatches taps to `controller`/`on_tap`.
#[expect(clippy::too_many_arguments, reason = "internal helper, not public API")]
fn build_tab_cell(
    index: usize,
    anchor: &SemanticsAnchor,
    tab: &Tab,
    selected: bool,
    bar_has_mixed_tabs: bool,
    resolved: &ResolvedTabBarStyle,
    indicator_weight: f64,
    controller: &TabController,
    on_tap: Option<&crate::event_callback::ValueCallback<usize>>,
) -> BoxedView {
    let (label_color, label_style) = if selected {
        (resolved.label_color, resolved.label_style.clone())
    } else {
        (
            resolved.unselected_label_color,
            resolved.unselected_label_style.clone(),
        )
    };

    let padded = Padding::new(label_padding(tab, bar_has_mixed_tabs))
        .child(Center::new().child(tab.clone()));
    let styled = DefaultTextStyle::new(label_style.with_color(label_color), padded);

    let band = SizedBox::height(indicator_weight);

    let cell = Column::new(vec![Expanded::new(styled).boxed(), band.boxed()])
        .cross_axis_alignment(CrossAxisAlignment::Stretch);

    let tap_controller = controller.clone();
    let tap_callback = on_tap.cloned();
    let ink_well = InkWell::new(cell)
        .overlay_color(resolved.overlay_color.clone())
        .on_tap(move |cx| {
            // A panicking position observer must not skip the caller's
            // callback: both run, then the first failure resumes.
            let mut recovery = PanicRecovery::new();
            recovery.run(|| tap_controller.animate_to(index));
            if let Some(callback) = &tap_callback {
                recovery.run(|| callback(cx, index));
            }
            recovery.finish();
        });

    // One node per tab, as a navigation destination is: the well's tap and
    // the label text fold into it, and the anchor names it for the
    // alternatives a [`crate::TabBarView`] relies on this bar for.
    let tab_node = Semantics::new()
        .role(SemanticsRole::Tab)
        .selected(selected)
        .anchor(anchor.clone())
        .child(ink_well);
    Expanded::new(MergeSemantics::new().child(tab_node)).boxed()
}
