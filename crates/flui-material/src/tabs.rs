//! [`Tab`] and [`TabBar`] — the secondary M3 tab bar. See the module docs
//! below for exactly which oracle contract this V1 ships.
//!
//! # Flutter parity
//!
//! `material/tabs.dart` (oracle tag `3.44.0`).
//!
//! # `TabBar` ships the SECONDARY contract only
//!
//! The oracle's `TabBar` has two constructors sharing one `_TabBarState`:
//! the default (primary — a 3dp, `TabBarIndicatorSize.label`-by-default
//! indicator meant for an `AppBar.bottom`) and `TabBar.secondary` (a 2dp,
//! `TabBarIndicatorSize.tab` indicator meant to separate content within a
//! page body, `_TabsSecondaryDefaultsM3`). This crate ships **only** the
//! secondary style, as [`TabBar::secondary`] — there is no plain
//! `TabBar::new` that could read as "the primary bar, just less finished".
//! Shipping a primary bar honestly needs `TabBarIndicatorSize::Label`, which
//! needs the *width of each tab's own label content* after layout
//! (`_IndicatorPainter.indicatorRect`'s `tabKeys[tabIndex].currentContext!.size!.width`
//! — a `GlobalKey`-mediated post-layout measurement this crate has no
//! established pattern for yet). Rather than ship a primary bar that
//! silently degrades to `TabBarIndicatorSize::Tab` sizing, primary is
//! deferred wholesale until that measurement exists.
//!
//! # Fixed equal-share layout only (no `isScrollable`)
//!
//! The oracle's non-scrollable path already gives every tab an equal share
//! via `Expanded` (`_TabBarState.build`'s `effectiveTabAlignment ==
//! TabAlignment.fill` branch) — that is the only layout this crate ports.
//! `isScrollable`, `TabAlignment::{Start, StartOffset, Center}`, and
//! `scrollController` are named deferrals: the scrollable path additionally
//! needs `_saveTabOffsets`'s post-layout tab-offset bookkeeping and a
//! horizontal `SingleChildScrollView`, neither of which any test in this
//! unit's acceptance list exercises.
//!
//! # Indicator: per-cell reserved band, not a `CustomPainter`
//!
//! The oracle's `_IndicatorPainter` computes one animated `Rect` per frame
//! from `Animation<double>` + `TabController.index`/`previousIndex` and
//! paints it (plus the divider) directly on a `Canvas`. This crate has no
//! `AnimationController` wired to [`crate::TabController`] (see that type's
//! module docs), so there is no per-frame interpolated value to paint in the
//! first place — every index change is instantaneous. Given that, painting
//! the 2dp indicator as a literal `Canvas` rect is unnecessary machinery:
//! each tab cell reserves a fixed-height band at its own bottom edge
//! (`indicator_weight` tall) and fills it with the resolved indicator color
//! when selected, [`Color::TRANSPARENT`] otherwise — the same "always
//! reserved, painted transparent when unselected" shape
//! [`crate::NavigationBar`]'s destination indicator already uses. Because
//! every tab cell is an equal `Expanded` share of the bar's width (see
//! above), this composition renders **exactly** the rect the oracle's
//! `_IndicatorPainter.indicatorRect` computes for `TabBarIndicatorSize::Tab`
//! with `indicatorPadding: EdgeInsets.zero` — independently pinned by this
//! module's test-only `indicator_rect` re-derivation of that geometry (see
//! `tests`), not a function the paint path itself calls.
//!
//! The divider (`showDivider: true` for a non-scrollable M3 bar) is a
//! `Positioned` full-width strip at the bar's bottom edge, stacked *behind*
//! the tab row — so an unselected tab's transparent band still lets the
//! divider line show through beneath it, and a selected tab's opaque
//! indicator band paints over it, matching the oracle's paint order
//! (divider drawn first, indicator second, in the same `Canvas` pass).
//!
//! # Named deferrals (not silently dropped)
//!
//! - **Indicator animation** (linear sweep / elastic stretch between tabs,
//!   `_applyLinearEffect`/`_applyElasticEffect`) — needs the
//!   `AnimationController` `TabController` doesn't have yet.
//! - **`TabBarIndicatorSize::Label`**, custom `indicator: Decoration`,
//!   `indicatorPadding`, rounded-corner indicators — primary-bar-only or
//!   `Label`-sizing-only oracle features; see the "SECONDARY contract"
//!   section above.
//! - **`WidgetStateColor` for `labelColor`** — the oracle lets `labelColor`
//!   itself be state-varying, ignoring `unselectedLabelColor` when it is.
//!   This crate resolves `labelColor`/`unselectedLabelColor` as two plain
//!   colors (exactly the M3 default table's own shape:
//!   `_TabsSecondaryDefaultsM3.labelColor`/`unselectedLabelColor` are both
//!   plain `Color`s, not `WidgetStateColor`s).
//!   `overlayColor` — the hover/press/focus ramp — IS state-resolved (a
//!   genuine `WidgetStateProperty`), since the M3 default table needs it.
//! - **Icon recoloring** — the oracle wraps tab content in
//!   `IconTheme.merge` so a bare `Icon` child inherits the resolved
//!   label/icon color. This crate wraps only in
//!   [`flui_widgets::DefaultTextStyle`] (text recoloring); a caller-supplied
//!   icon keeps whatever color it was given. No test in this unit's
//!   acceptance list exercises icon color.
//! - **`labelPadding`/`TabBarThemeData` overrides for it** — fixed at
//!   [`kTabLabelPadding`](Self) (`16.0` horizontal), no widget or theme
//!   override surface yet.
//! - **`onHover`/`onFocusChange`/`mouseCursor`/`splashFactory`/
//!   `splashBorderRadius`/`dragStartBehavior`/`physics`/`textScaler`** — no
//!   consumer yet; `InkWell`'s own defaults apply.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::ListenerId;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_types::{EdgeInsets, Size, geometry::px};
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};
use flui_widgets::{
    Center, Column, Container, CrossAxisAlignment, DefaultTextStyle, Expanded, Padding, Positioned,
    PreferredSizeView, Row, SizedBox, Stack, Text, WidgetState, WidgetStateConstraint,
    WidgetStateProperty,
};

use crate::ink_well::InkWell;
use crate::tab_controller::{DefaultTabController, TabController};
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// A `Tab` with no icon's height. Flutter parity: `_kTabHeight`
/// (`tabs.dart`, oracle tag `3.44.0`).
pub const TAB_HEIGHT: f32 = 46.0;

/// A `Tab` with both an icon and text/child's height. Flutter parity:
/// `_kTextAndIconTabHeight`.
pub const TEXT_AND_ICON_TAB_HEIGHT: f32 = 72.0;

/// The horizontal padding every tab label gets, both sides. Flutter parity:
/// `kTabLabelPadding` (`constants.dart`, `EdgeInsets.symmetric(horizontal:
/// 16.0)`) — see the module docs for why this crate has no override surface
/// for it yet.
pub const TAB_LABEL_HORIZONTAL_PADDING: f32 = 16.0;

/// One [`TabBar`] tab's label content: some combination of `text`/`child`
/// and `icon`. Flutter parity: `Tab` (`tabs.dart`, oracle tag `3.44.0`).
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
    height: Option<f32>,
}

impl Tab {
    /// An empty tab — set `text`, `child`, and/or `icon` before using it (at
    /// least one is required; a tab with none debug-asserts in `build`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tab's text label. Mutually exclusive with
    /// [`child`](Self::child) — the last one set wins (mirrors the oracle's
    /// constructor-time assert as a "last write wins" builder instead, since
    /// a builder has no single constructor call to assert against).
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
    /// and text/child are present). Flutter parity: `Tab.height`.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
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
/// both an icon and text/child are present, else `46.0`. Flutter parity:
/// `Tab.preferredSize`/`Tab.build`'s `calculatedHeight` (`tabs.dart`, oracle
/// tag `3.44.0`) — the two oracle computations agree, so one function here
/// serves both [`Tab::preferred_size`] and [`TabBar`]'s own height math.
fn tab_content_height(tab: &Tab) -> f32 {
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
        Size::new(px(0.0), px(tab_content_height(self)))
    }
}

/// The M3 secondary tab bar — see the module docs for exactly what this
/// ships (secondary only, fixed equal-share layout, no indicator
/// animation).
///
/// Flutter parity: `TabBar.secondary` (`tabs.dart`, oracle tag `3.44.0`).
///
/// A [`TabController`] is required either explicitly (via
/// [`controller`](Self::controller)) or via a
/// [`DefaultTabController`] ancestor — exactly
/// one must be reachable, or `build` panics (Flutter parity: the oracle's
/// `_updateTabController` `FlutterError`/`assert`).
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
    indicator_weight: f32,
    on_tap: Option<Rc<dyn Fn(usize)>>,
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
            on_tap: None,
        }
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
    /// Flutter parity: `TabBar.onTap`.
    #[must_use]
    pub fn on_tap(mut self, callback: impl Fn(usize) + 'static) -> Self {
        self.on_tap = Some(Rc::new(callback));
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
        Size::new(
            px(f32::INFINITY),
            px(bar_height(&self.tabs, self.indicator_weight)),
        )
    }
}

/// The bar's total height: the tallest tab's content height (`46.0` if
/// `tabs` is empty — Flutter parity: `TabBar.preferredSize`'s `maxHeight`
/// seed) plus `indicator_weight`. Also, unmodified, the oracle's zero-tab
/// special case (`_kTabHeight + indicatorWeight`, `_TabBarState.build`'s
/// early return) — no separate branch is needed here because folding over
/// an empty `tabs` slice already returns the `TAB_HEIGHT` seed.
fn bar_height(tabs: &[Tab], indicator_weight: f32) -> f32 {
    let max_content_height = tabs
        .iter()
        .map(tab_content_height)
        .fold(TAB_HEIGHT, f32::max);
    max_content_height + indicator_weight
}

/// Whether any tab in `tabs` has both an icon and text/child (i.e. its
/// content height is `TEXT_AND_ICON_TAB_HEIGHT`). Flutter parity:
/// `TabBar.tabHasTextAndIcon`.
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
    divider_height: f32,
    overlay_color: WidgetStateProperty<Option<Color>>,
}

/// Resolves the M3 secondary defaults (`_TabsSecondaryDefaultsM3`,
/// `tabs.dart`, oracle tag `3.44.0`) through the theme → default cascade:
/// `TabBarThemeData` field if set, else the literal M3 secondary default.
///
/// | Field | M3 secondary default | Oracle |
/// |---|---|---|
/// | `indicator_color` | `ColorScheme.primary` | `_TabsSecondaryDefaultsM3.indicatorColor` |
/// | `label_color` | `ColorScheme.onSurface` | `_TabsSecondaryDefaultsM3.labelColor` |
/// | `unselected_label_color` | `ColorScheme.onSurfaceVariant` | `_TabsSecondaryDefaultsM3.unselectedLabelColor` |
/// | `label_style` / `unselected_label_style` | `TextTheme.titleSmall` | `_TabsSecondaryDefaultsM3.labelStyle`/`unselectedLabelStyle` |
/// | `divider_color` | `ColorScheme.outlineVariant` | `_TabsSecondaryDefaultsM3.dividerColor` |
/// | `divider_height` | `1.0` | `_TabsSecondaryDefaultsM3.dividerHeight` |
/// | `overlay_color` | pressed→`onSurface@0.1`, hovered→`onSurface@0.08`, focused→`onSurface@0.1`, else none | `_TabsSecondaryDefaultsM3.overlayColor` |
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

/// `_TabsSecondaryDefaultsM3.overlayColor`'s resolver — pressed/hovered/
/// focused ramp over `on_surface`, identical whether or not the tab is
/// selected (the oracle's own `selected`-branch and non-`selected`-branch
/// happen to produce the same three values — see `tabs.dart`'s
/// `_TabsSecondaryDefaultsM3.overlayColor` getter, oracle tag `3.44.0`).
fn default_overlay_color(on_surface: Color) -> WidgetStateProperty<Option<Color>> {
    WidgetStateProperty::from_map([
        (
            WidgetStateConstraint::Is(WidgetState::Pressed),
            Some(on_surface.with_opacity(0.1)),
        ),
        (
            WidgetStateConstraint::Is(WidgetState::Hovered),
            Some(on_surface.with_opacity(0.08)),
        ),
        (
            WidgetStateConstraint::Is(WidgetState::Focused),
            Some(on_surface.with_opacity(0.1)),
        ),
    ])
}

/// This tab's label padding: [`TAB_LABEL_HORIZONTAL_PADDING`] both sides,
/// plus `±13.0` vertical when `tab`'s own content height is `TAB_HEIGHT`
/// (`46.0`) but the bar as a whole has a text-and-icon tab (`72.0`) — the
/// mechanism that centers a plain tab's content inside a taller mixed bar.
/// Flutter parity: `_TabBarState.build`'s `verticalAdjustment` (`(
/// _kTextAndIconTabHeight - _kTabHeight) / 2.0`, i.e. `13.0`), added to
/// `kTabLabelPadding` when `tab.preferredSize.height == _kTabHeight &&
/// widget.tabHasTextAndIcon` (`tabs.dart`, oracle tag `3.44.0`) — ported
/// honestly as padding, not as a `Center`-widget trick, because that is
/// exactly the mechanism the oracle itself uses.
fn label_padding(tab: &Tab, bar_has_mixed_tabs: bool) -> EdgeInsets {
    let vertical = if bar_has_mixed_tabs && tab_content_height(tab) == TAB_HEIGHT {
        (TEXT_AND_ICON_TAB_HEIGHT - TAB_HEIGHT) / 2.0
    } else {
        0.0
    };
    EdgeInsets::symmetric(px(vertical), px(TAB_LABEL_HORIZONTAL_PADDING))
}

/// Persistent state behind [`TabBar`]: the currently-subscribed
/// [`TabController`] and its listener registration, re-resolved every
/// `build` — see `resolve_controller`'s doc comment (private, `impl
/// TabBarState`) for exactly when the subscription is re-homed.
pub struct TabBarState {
    controller: RefCell<Option<TabController>>,
    listener_id: RefCell<Option<ListenerId>>,
    rebuild: Option<RebuildHandle>,
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
    /// `DefaultTabController` ancestor. Flutter parity: `_updateTabController`'s
    /// `FlutterError`.
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
                rebuild.schedule();
            });
            *self.listener_id.borrow_mut() = Some(id);
            *self.controller.borrow_mut() = Some(resolved.clone());
        }

        resolved
    }
}

impl ViewState<TabBar> for TabBarState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &TabBar, ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.resolve_controller(view, ctx);
        let theme = Theme::of(ctx);
        let resolved = resolve_style(&theme);
        let height = bar_height(&view.tabs, view.indicator_weight);

        if view.tabs.is_empty() {
            // Flutter parity: `_TabBarState.build`'s zero-tabs early return
            // (`LimitedBox(maxWidth: 0.0, child: SizedBox(width:
            // double.infinity, height: _kTabHeight + indicatorWeight))`).
            // The `LimitedBox(maxWidth: 0.0)` half only matters when the
            // incoming width constraint is unbounded — a plain
            // width-unconstrained `SizedBox::height` behaves identically in
            // every bounded parent this bar is normally mounted under; named
            // simplification for that one unbounded-width edge case.
            //
            // A controller is still resolved above even for zero tabs —
            // matching the oracle exactly: `_updateTabController`'s
            // controller resolution runs in `didChangeDependencies`,
            // unconditionally, before `build` ever checks
            // `_controller!.length == 0`. A zero-tab `TabBar` with no
            // controller and no `DefaultTabController` ancestor still
            // panics, same as a non-empty one.
            return SizedBox::height(height).boxed();
        }

        let mixed = tab_has_text_and_icon(&view.tabs);
        let current_index = controller.index();

        let cells: Vec<BoxedView> = view
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                build_tab_cell(
                    index,
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

        let content: BoxedView =
            if resolved.divider_height > 0.0 && resolved.divider_color != Color::TRANSPARENT {
                let divider = Positioned::new(
                    Container::new()
                        .color(resolved.divider_color)
                        .height(resolved.divider_height),
                )
                .left(0.0)
                .right(0.0)
                .bottom(0.0);
                Stack::new(vec![divider.boxed(), row.boxed()]).boxed()
            } else {
                row.boxed()
            };

        SizedBox::height(height).child(content).boxed()
    }
}

/// Builds one tab's `Expanded` cell: label content (recolored/padded per
/// selection), the reserved indicator band, wrapped in an [`InkWell`] that
/// dispatches taps to `controller`/`on_tap`. Flutter parity: the relevant
/// slice of `_TabBarState.build` (label wrapping, `_TabStyle`, `InkWell`,
/// `Expanded`) — see the module docs for what is and is not ported.
#[allow(clippy::too_many_arguments, reason = "internal helper, not public API")]
fn build_tab_cell(
    index: usize,
    tab: &Tab,
    selected: bool,
    bar_has_mixed_tabs: bool,
    resolved: &ResolvedTabBarStyle,
    indicator_weight: f32,
    controller: &TabController,
    on_tap: Option<&Rc<dyn Fn(usize)>>,
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

    let band_color = if selected {
        resolved.indicator_color
    } else {
        Color::TRANSPARENT
    };
    let band = Container::new().height(indicator_weight).color(band_color);

    let cell = Column::new(vec![Expanded::new(styled).boxed(), band.boxed()])
        .cross_axis_alignment(CrossAxisAlignment::Stretch);

    let tap_controller = controller.clone();
    let tap_callback = on_tap.cloned();
    let ink_well = InkWell::new(cell)
        .overlay_color(resolved.overlay_color.clone())
        .on_tap(move || {
            tap_controller.animate_to(index);
            if let Some(callback) = &tap_callback {
                callback(index);
            }
        });

    Expanded::new(ink_well).boxed()
}

#[cfg(test)]
mod tests {
    use flui_types::Rect;

    use super::*;
    use crate::theme_data::TabBarThemeData;

    /// The 2dp indicator's rect for `index` in a `tab_count`-tab bar —
    /// re-derives the geometry `build_tab_cell`'s per-cell band composition
    /// implicitly renders (see the module docs' "Indicator: per-cell
    /// reserved band" section), so that geometry is pinned by a test
    /// independent of the widget tree. Flutter parity:
    /// `_IndicatorPainter.indicatorRect` specialized to
    /// `TabBarIndicatorSize::Tab` with `indicatorPadding: EdgeInsets.zero`
    /// (this crate's only supported combination) and this crate's fixed
    /// equal-share tab widths.
    fn indicator_rect(
        bar_width: f32,
        bar_height: f32,
        tab_count: usize,
        indicator_weight: f32,
        index: usize,
    ) -> Rect {
        assert!(tab_count > 0, "indicator_rect requires at least one tab");
        assert!(index < tab_count, "index out of range for tab_count");
        let tab_width = bar_width / tab_count as f32;
        Rect::from_ltwh(
            px(tab_width * index as f32),
            px(bar_height - indicator_weight),
            px(tab_width),
            px(indicator_weight),
        )
    }

    #[test]
    fn tab_content_height_defaults_to_tab_height_with_no_icon() {
        let tab = Tab::new().text("Home");
        assert_eq!(tab_content_height(&tab), TAB_HEIGHT);
    }

    #[test]
    fn tab_content_height_is_text_and_icon_height_with_both() {
        let tab = Tab::new()
            .text("Home")
            .icon(flui_widgets::SizedBox::shrink());
        assert_eq!(tab_content_height(&tab), TEXT_AND_ICON_TAB_HEIGHT);
    }

    #[test]
    fn tab_content_height_is_tab_height_for_icon_only() {
        let tab = Tab::new().icon(flui_widgets::SizedBox::shrink());
        assert_eq!(tab_content_height(&tab), TAB_HEIGHT);
    }

    #[test]
    fn tab_content_height_override_wins_over_computed_height() {
        let tab = Tab::new()
            .text("Home")
            .icon(flui_widgets::SizedBox::shrink())
            .height(20.0);
        assert_eq!(tab_content_height(&tab), 20.0);
    }

    #[test]
    fn tab_preferred_size_matches_content_height() {
        let tab = Tab::new().text("Home");
        assert_eq!(tab.preferred_size().height, px(TAB_HEIGHT));
    }

    #[test]
    fn bar_height_is_48_for_the_default_tab_height() {
        let tabs = vec![Tab::new().text("A"), Tab::new().text("B")];
        assert_eq!(bar_height(&tabs, 2.0), 48.0);
    }

    /// Red-check: mutating the `fold` seed from `TAB_HEIGHT` to `0.0` would
    /// still pass every non-empty case (a real tab is always >= 46) but
    /// break the empty-bar case below.
    #[test]
    fn bar_height_for_zero_tabs_is_48() {
        assert_eq!(bar_height(&[], 2.0), TAB_HEIGHT + 2.0);
    }

    #[test]
    fn bar_height_is_74_when_a_tab_has_text_and_icon() {
        let tabs = vec![
            Tab::new().text("A"),
            Tab::new().text("B").icon(flui_widgets::SizedBox::shrink()),
        ];
        assert_eq!(bar_height(&tabs, 2.0), TEXT_AND_ICON_TAB_HEIGHT + 2.0);
    }

    #[test]
    fn tab_bar_preferred_size_matches_bar_height() {
        let tabs = vec![Tab::new().text("A"), Tab::new().text("B")];
        let bar = TabBar::secondary(tabs);
        assert_eq!(bar.preferred_size().height, px(48.0));
    }

    #[test]
    fn tab_bar_preferred_size_for_zero_tabs_is_48() {
        let bar = TabBar::secondary(vec![]);
        assert_eq!(bar.preferred_size().height, px(48.0));
    }

    #[test]
    fn tab_has_text_and_icon_is_false_with_no_mixed_tabs() {
        let tabs = vec![Tab::new().text("A"), Tab::new().text("B")];
        assert!(!tab_has_text_and_icon(&tabs));
    }

    #[test]
    fn tab_has_text_and_icon_is_true_with_one_mixed_tab() {
        let tabs = vec![
            Tab::new().text("A"),
            Tab::new().text("B").icon(flui_widgets::SizedBox::shrink()),
        ];
        assert!(tab_has_text_and_icon(&tabs));
    }

    #[test]
    fn label_padding_has_no_vertical_adjustment_in_a_uniform_bar() {
        let tab = Tab::new().text("A");
        let padding = label_padding(&tab, false);
        assert_eq!(padding, EdgeInsets::symmetric(px(0.0), px(16.0)));
    }

    #[test]
    fn label_padding_adds_13dp_vertical_for_a_plain_tab_in_a_mixed_bar() {
        let tab = Tab::new().text("A");
        let padding = label_padding(&tab, true);
        assert_eq!(padding, EdgeInsets::symmetric(px(13.0), px(16.0)));
    }

    /// Red-check: if the mixed-bar check ignored `Tab::height` overrides and
    /// compared only computed height, a height-overridden tab that happens
    /// to equal `TAB_HEIGHT` numerically would still get the adjustment even
    /// when it's a deliberate override rather than the plain 46px default —
    /// this asserts the *override* path is not skipped for that comparison.
    #[test]
    fn label_padding_uses_the_overridden_height_not_only_the_computed_one() {
        let overridden = Tab::new().text("A").height(TEXT_AND_ICON_TAB_HEIGHT);
        assert_eq!(
            label_padding(&overridden, true),
            EdgeInsets::symmetric(px(0.0), px(16.0))
        );
    }

    #[test]
    fn indicator_rect_spans_the_first_of_two_equal_tabs() {
        let rect = indicator_rect(200.0, 48.0, 2, 2.0, 0);
        assert_eq!(rect, Rect::from_ltwh(px(0.0), px(46.0), px(100.0), px(2.0)));
    }

    #[test]
    fn indicator_rect_spans_the_second_of_two_equal_tabs() {
        let rect = indicator_rect(200.0, 48.0, 2, 2.0, 1);
        assert_eq!(
            rect,
            Rect::from_ltwh(px(100.0), px(46.0), px(100.0), px(2.0))
        );
    }

    /// Red-check: if `indicator_rect` divided by `index` instead of
    /// `tab_count`, three equal tabs would not tile the bar width evenly.
    #[test]
    fn indicator_rect_tiles_three_equal_tabs_across_the_full_width() {
        let first = indicator_rect(300.0, 48.0, 3, 2.0, 0);
        let second = indicator_rect(300.0, 48.0, 3, 2.0, 1);
        let third = indicator_rect(300.0, 48.0, 3, 2.0, 2);
        assert_eq!(first.width(), px(100.0));
        assert_eq!(second.width(), px(100.0));
        assert_eq!(third.width(), px(100.0));
        assert_eq!(first.left(), px(0.0));
        assert_eq!(second.left(), px(100.0));
        assert_eq!(third.left(), px(200.0));
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_secondary_token_table() {
        let theme = ThemeData::light();
        let resolved = resolve_style(&theme);

        assert_eq!(resolved.indicator_color, theme.color_scheme.primary);
        assert_eq!(resolved.label_color, theme.color_scheme.on_surface);
        assert_eq!(
            resolved.unselected_label_color,
            theme.color_scheme.on_surface_variant
        );
        assert_eq!(resolved.divider_color, theme.color_scheme.outline_variant);
        assert_eq!(resolved.divider_height, 1.0);
        assert_eq!(
            resolved.label_style,
            theme.text_theme.title_small.clone().unwrap_or_default()
        );
        assert_eq!(resolved.label_style, resolved.unselected_label_style);
    }

    #[test]
    fn resolve_style_overlay_color_matches_the_secondary_defaults_table() {
        let theme = ThemeData::light();
        let resolved = resolve_style(&theme);
        let on_surface = theme.color_scheme.on_surface;

        let pressed = resolved
            .overlay_color
            .resolve(&flui_widgets::WidgetStates::from(WidgetState::Pressed));
        let hovered = resolved
            .overlay_color
            .resolve(&flui_widgets::WidgetStates::from(WidgetState::Hovered));
        let focused = resolved
            .overlay_color
            .resolve(&flui_widgets::WidgetStates::from(WidgetState::Focused));
        let none = resolved
            .overlay_color
            .resolve(&flui_widgets::WidgetStates::NONE);

        assert_eq!(pressed, Some(on_surface.with_opacity(0.1)));
        assert_eq!(hovered, Some(on_surface.with_opacity(0.08)));
        assert_eq!(focused, Some(on_surface.with_opacity(0.1)));
        assert_eq!(none, None);
    }

    #[test]
    fn resolve_style_theme_override_beats_the_default() {
        let mut theme = ThemeData::light();
        let themed_indicator = Color::rgb(9, 9, 9);
        theme.tab_bar_theme = Some(TabBarThemeData {
            indicator_color: Some(themed_indicator),
            ..Default::default()
        });

        let resolved = resolve_style(&theme);

        assert_eq!(resolved.indicator_color, themed_indicator);
        // Fields left unset on the theme slot still fall through to their
        // own M3 default independently.
        assert_eq!(resolved.label_color, theme.color_scheme.on_surface);
    }

    #[test]
    fn tab_bar_secondary_starts_with_no_explicit_controller() {
        let bar = TabBar::secondary(vec![Tab::new().text("A")]);
        assert!(bar.controller.is_none());
    }

    #[test]
    fn tab_bar_controller_sets_the_explicit_controller() {
        let controller = TabController::new(1, 0);
        let bar = TabBar::secondary(vec![Tab::new().text("A")]).controller(controller.clone());
        assert_eq!(bar.controller, Some(controller));
    }

    #[test]
    fn debug_format_does_not_panic() {
        let bar = TabBar::secondary(vec![Tab::new().text("A")]);
        let rendered = format!("{bar:?}");
        assert!(rendered.contains("TabBar"));
    }
}
