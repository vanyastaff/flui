//! [`CupertinoTabBar`] — the iOS-style bottom tab bar, and
//! [`CupertinoTabBarItem`], the per-tab icon/label pair it displays.
//!
//! Flutter parity: `cupertino/bottom_tab_bar.dart`'s `CupertinoTabBar` and
//! `BottomNavigationBarItem` (the latter is `material`'s shared item type in
//! the oracle; [`CupertinoTabBarItem`] is this crate's own, narrower type —
//! see below). Oracle tag `3.44.0`.
//!
//! ## What this ports
//!
//! - `_kTabBarHeight` (`50.0`) as the default `height`.
//! - The hairline top border, `_kDefaultTabBarBorderColor`
//!   (`0x4D000000`/`0x29000000` light/dark) — same "real 1.0px stroke, not a
//!   literal device-pixel `width: 0.0`" divergence [`crate::CupertinoNavigationBar`]
//!   documents.
//! - `inactiveColor`'s default (`CupertinoColors.inactiveGray`),
//!   `activeColor`'s default (the theme's `primaryColor`), and the
//!   icon/label recoloring per item based on `current_index`.
//! - Self-padding against the bottom safe-area inset (`MediaQuery`'s
//!   `padding.bottom` folded into both the bar's own height and the label
//!   row's bottom padding) — mirroring `_kNavBarPersistentHeight`'s
//!   top-inset self-padding on [`crate::CupertinoNavigationBar`].
//! - `opaque(ctx)`: whether the resolved background is fully opaque —
//!   consumed by [`crate::CupertinoTabScaffold`]'s content-padding math (the
//!   opaque/translucent branch, both ported).
//! - Per-item `Semantics(selected: active)`, and
//!   `Semantics(explicitChildNodes: true)` around the item row so each item
//!   keeps its own semantics node rather than merging into one.
//!
//! ## `CupertinoTabBarItem`, not `BottomNavigationBarItem`
//!
//! The oracle's item type is `material`'s `BottomNavigationBarItem`
//! (`icon`, `activeIcon`, `label`, `tooltip`, `backgroundColor`, …) — a
//! shared cross-design-system type ADR-0028 forbids depending on
//! (`flui-material` is never a dependency of this crate). This ships a
//! narrower, Cupertino-owned type instead: `icon` (required), `active_icon`
//! (optional, defaults to `icon`), `label` (optional `String`, rendered as
//! plain [`Text`]). No `tooltip`/`backgroundColor` — neither the tab bar nor
//! `CupertinoTabScaffold` consumes them in the oracle either
//! (`backgroundColor` on the *item* is Material-only; the tab bar's own
//! `backgroundColor` is a separate, already-ported field).
//!
//! ## Deferred, named
//!
//! - **Blur** (`BackdropFilter` on a translucent background) — same gap
//!   `CupertinoNavigationBar` documents: no `BackdropFilter` primitive in
//!   `flui-widgets` yet. A caller-supplied translucent `background_color`
//!   does reach the opaque/translucent branch (`opaque(ctx)` is wired), it
//!   just paints with no blur behind it.
//! - **The localized `Semantics.hint`**
//!   (`localizations.tabSemanticsLabel(tabIndex:, tabCount:)`) — no
//!   `CupertinoLocalizations`-equivalent in this crate; `selected` is ported,
//!   the hint is not.
//! - **`copyWith`** — Flutter's manual clone-with-overrides method.
//!   `CupertinoTabBar: Clone` plus its own builder methods (`.current_index(...)`,
//!   `.on_tap(...)`) already give [`crate::CupertinoTabScaffold`] the same
//!   capability idiomatically; no separate method is needed.
//! - **`MouseRegion`/`TextFieldTapRegion`** — no mouse-cursor or text-field
//!   tap-region substrate to wire either through.

use std::rc::Rc;

use flui_types::Size;
use flui_types::geometry::px;
use flui_types::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{
    Column, CrossAxisAlignment, DecoratedBox, DefaultTextStyle, Expanded, GestureDetector,
    HitTestBehavior, IconTheme, IconThemeData, MainAxisAlignment, MediaQuery, Padding,
    PreferredSizeView, Row, Semantics, SizedBox, Text,
};

use crate::colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::theme::CupertinoTheme;

/// `_kTabBarHeight` (`bottom_tab_bar.dart`, oracle tag `3.44.0`) — standard
/// iOS 10 tab bar height.
pub const TAB_BAR_HEIGHT: f32 = 50.0;

/// The stroke width this port paints the hairline border at — see
/// [`crate::nav_bar::HAIRLINE_BORDER_WIDTH`]'s doc for why a literal
/// `width: 0.0` cannot be ported verbatim.
pub const HAIRLINE_BORDER_WIDTH: f32 = crate::nav_bar::HAIRLINE_BORDER_WIDTH;

/// `_kDefaultTabBarBorderColor` (`bottom_tab_bar.dart`, oracle tag `3.44.0`):
/// `CupertinoDynamicColor.withBrightness(color: 0x4D000000, darkColor:
/// 0x29000000)` — genuinely brightness-dependent, unlike
/// [`crate::CupertinoNavigationBar`]'s own hairline border color (a plain,
/// non-dynamic `Color` in the oracle). Resolved fresh in `build` against the
/// ambient brightness, not baked into a `const` at construction time.
fn default_border_color() -> CupertinoDynamicColor {
    CupertinoDynamicColor::with_brightness(
        Color::from_argb(0x4D00_0000),
        Color::from_argb(0x2900_0000),
    )
}

/// One tab's icon/label pair. Flutter parity: `BottomNavigationBarItem` —
/// narrowed to a Cupertino-owned type, see the module docs.
///
/// ```
/// use flui_cupertino::CupertinoTabBarItem;
/// use flui_widgets::{Icon, IconData};
///
/// let _item = CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home");
/// ```
#[derive(Clone)]
pub struct CupertinoTabBarItem {
    icon: BoxedView,
    active_icon: Option<BoxedView>,
    label: Option<String>,
}

impl CupertinoTabBarItem {
    /// An item showing `icon` (in both active and inactive states) with no
    /// label.
    #[must_use]
    pub fn new(icon: impl IntoView) -> Self {
        Self {
            icon: icon.into_view().boxed(),
            active_icon: None,
            label: None,
        }
    }

    /// Overrides the icon shown when this tab is active. Defaults to the
    /// same icon as the inactive state. Flutter parity:
    /// `BottomNavigationBarItem.activeIcon`.
    #[must_use]
    pub fn active_icon(mut self, active_icon: impl IntoView) -> Self {
        self.active_icon = Some(active_icon.into_view().boxed());
        self
    }

    /// Sets the label text, rendered below the icon. Flutter parity:
    /// `BottomNavigationBarItem.label`.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl std::fmt::Debug for CupertinoTabBarItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTabBarItem")
            .field("has_active_icon", &self.active_icon.is_some())
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// A user tap handler over a tab's index. `Rc`-based (owner-local, per
/// ADR-0027) — matches `GestureDetector::on_tap`'s own callback shape.
type TabTapCallback = Rc<dyn Fn(usize)>;

/// An iOS-style bottom tab bar. Flutter parity: `CupertinoTabBar`
/// (`bottom_tab_bar.dart`, oracle tag `3.44.0`) — see the module docs for
/// exactly what is and is not ported.
///
/// ```
/// use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
/// use flui_widgets::{Icon, IconData};
///
/// let _bar = CupertinoTabBar::new(vec![
///     CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
///     CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
/// ]);
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoTabBar {
    items: Vec<CupertinoTabBarItem>,
    on_tap: Option<TabTapCallback>,
    current_index: usize,
    background_color: Option<CupertinoColor>,
    active_color: Option<CupertinoColor>,
    inactive_color: CupertinoColor,
    icon_size: f32,
    height: f32,
    border: Option<Border<flui_types::geometry::Pixels>>,
    /// Whether `border` still holds the un-overridden default. If so,
    /// `build` resolves [`default_border_color`]'s light/dark variant fresh
    /// against the ambient brightness every time, rather than using a color
    /// baked in once at construction — see that function's doc for why this
    /// component's default border (unlike `CupertinoNavigationBar`'s) is
    /// genuinely brightness-dependent in the oracle.
    border_is_default: bool,
}

impl CupertinoTabBar {
    /// A tab bar showing `items`, `current_index: 0`, the default hairline
    /// top border, and the theme's `bar_background_color`.
    ///
    /// `items` must carry at least 2 entries — Apple's Human Interface
    /// Guidelines require it, and the oracle's own constructor asserts the
    /// same (debug-only, matching Dart's `assert`).
    #[must_use]
    pub fn new(items: Vec<CupertinoTabBarItem>) -> Self {
        debug_assert!(
            items.len() >= 2,
            "CupertinoTabBar needs at least 2 items to conform to Apple's HIG"
        );
        Self {
            items,
            on_tap: None,
            current_index: 0,
            background_color: None,
            active_color: None,
            inactive_color: CupertinoColor::Dynamic(CupertinoColors::INACTIVE_GRAY),
            icon_size: 30.0,
            height: TAB_BAR_HEIGHT,
            border: None,
            border_is_default: true,
        }
    }

    /// The configured items, in display order.
    #[must_use]
    pub fn items(&self) -> &[CupertinoTabBarItem] {
        &self.items
    }

    /// Sets the tap handler, called with the tapped item's index. Flutter
    /// parity: `CupertinoTabBar.onTap`.
    #[must_use]
    pub fn on_tap(mut self, on_tap: impl Fn(usize) + 'static) -> Self {
        self.on_tap = Some(Rc::new(on_tap));
        self
    }

    /// Sets which item is drawn active. Flutter parity:
    /// `CupertinoTabBar.currentIndex`.
    #[must_use]
    pub fn current_index(mut self, current_index: usize) -> Self {
        self.current_index = current_index;
        self
    }

    /// The index this bar is currently drawing as active.
    #[must_use]
    pub fn current_index_value(&self) -> usize {
        self.current_index
    }

    /// Overrides the resolved background. Defaults to
    /// [`crate::CupertinoThemeData::bar_background_color`]. Flutter parity:
    /// `CupertinoTabBar.backgroundColor`.
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Overrides the active item's icon/label color. Defaults to
    /// [`crate::CupertinoThemeData::primary_color`]. Flutter parity:
    /// `CupertinoTabBar.activeColor`.
    #[must_use]
    pub fn active_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.active_color = Some(color.into());
        self
    }

    /// Overrides the inactive items' icon/label color. Defaults to
    /// [`CupertinoColors::INACTIVE_GRAY`]. Flutter parity:
    /// `CupertinoTabBar.inactiveColor`.
    #[must_use]
    pub fn inactive_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.inactive_color = color.into();
        self
    }

    /// Overrides the icon size. Defaults to `30.0`. Flutter parity:
    /// `CupertinoTabBar.iconSize`.
    #[must_use]
    pub fn icon_size(mut self, icon_size: f32) -> Self {
        self.icon_size = icon_size;
        self
    }

    /// Overrides the bar's height. Defaults to [`TAB_BAR_HEIGHT`]. Flutter
    /// parity: `CupertinoTabBar.height`.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Overrides the top border, or removes it with `None`. Defaults to
    /// the hairline border. Flutter parity: `CupertinoTabBar.border`.
    #[must_use]
    pub fn border(mut self, border: Option<Border<flui_types::geometry::Pixels>>) -> Self {
        self.border = border;
        self.border_is_default = false;
        self
    }

    /// The currently registered tap handler, if any. `pub(crate)` so
    /// [`crate::CupertinoTabScaffold`] can chain through the caller's
    /// original handler after overriding `on_tap` for its own
    /// index-tracking — Flutter parity: `widget.tabBar.onTap?.call(newIndex)`
    /// in `_CupertinoTabScaffoldState.build`.
    pub(crate) fn on_tap_handler(&self) -> Option<TabTapCallback> {
        self.on_tap.clone()
    }

    /// Whether the resolved background is fully opaque — Flutter parity:
    /// `CupertinoTabBar.opaque`. Consumed by
    /// [`crate::CupertinoTabScaffold`]'s content-padding math.
    #[must_use]
    pub fn opaque(&self, ctx: &dyn BuildContext) -> bool {
        let theme = CupertinoTheme::of(ctx);
        let background = self
            .background_color
            .unwrap_or_else(|| theme.bar_background_color())
            .resolve(ctx);
        background.a == 0xFF
    }
}

impl std::fmt::Debug for CupertinoTabBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoTabBar")
            .field("item_count", &self.items.len())
            .field("current_index", &self.current_index)
            .finish_non_exhaustive()
    }
}

impl StatelessView for CupertinoTabBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = CupertinoTheme::of(ctx);
        let background = self
            .background_color
            .unwrap_or_else(|| theme.bar_background_color())
            .resolve(ctx);
        let inactive = self.inactive_color.resolve(ctx);
        let active = self
            .active_color
            .unwrap_or_else(|| theme.primary_color())
            .resolve(ctx);
        let bottom_inset = MediaQuery::maybe_of(ctx).map_or(px(0.0), |data| data.padding.bottom);

        let tab_label_style = theme.text_theme().tab_label_text_style();

        let item_views: Vec<BoxedView> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let is_active = index == self.current_index;
                let icon = if is_active {
                    item.active_icon
                        .clone()
                        .unwrap_or_else(|| item.icon.clone())
                } else {
                    item.icon.clone()
                };
                let color = if is_active { active } else { inactive };

                let mut column_children: Vec<BoxedView> =
                    vec![Expanded::new(flui_widgets::Center::new().child(icon)).boxed()];
                if let Some(label) = &item.label {
                    column_children.push(Text::new(label.clone()).boxed());
                }

                let content = IconTheme::new(
                    IconThemeData {
                        color: Some(color),
                        size: Some(self.icon_size),
                        ..IconThemeData::default()
                    },
                    DefaultTextStyle::new(
                        tab_label_style.clone().with_color(color),
                        Padding::new(flui_types::geometry::EdgeInsets::only_bottom(px(4.0))).child(
                            Column::new(column_children)
                                .main_axis_alignment(MainAxisAlignment::End),
                        ),
                    ),
                );

                let mut detector = GestureDetector::new().behavior(HitTestBehavior::Opaque);
                if let Some(on_tap) = self.on_tap.clone() {
                    detector = detector.on_tap(move || on_tap(index));
                }

                // `Semantics(selected: active, hint: localizations.tabSemanticsLabel(...), …)`
                // (`bottom_tab_bar.dart`, oracle tag `3.44.0`) — `selected`
                // ported; the localized `hint` is deferred (no
                // `CupertinoLocalizations`-equivalent `tabSemanticsLabel` in
                // this crate).
                Expanded::new(
                    Semantics::new()
                        .selected(is_active)
                        .child(detector.child(content)),
                )
                .boxed()
            })
            .collect();

        // `Padding(bottom: bottomPadding, child: Semantics(explicitChildNodes:
        // true, child: Row(...)))` (`bottom_tab_bar.dart`, oracle tag
        // `3.44.0`) — each item owns its own semantics node rather than
        // merging into one.
        let toolbar = Padding::new(flui_types::geometry::EdgeInsets::only_bottom(bottom_inset))
            .child(
                Semantics::new()
                    .explicit_child_nodes(true)
                    .child(Row::new(item_views).cross_axis_alignment(CrossAxisAlignment::End)),
            );

        let resolved_border = if self.border_is_default {
            Some(Border::new(
                Some(BorderSide::new(
                    CupertinoColor::Dynamic(default_border_color()).resolve(ctx),
                    px(HAIRLINE_BORDER_WIDTH),
                    BorderStyle::Solid,
                )),
                None,
                None,
                None,
            ))
        } else {
            self.border
        };

        DecoratedBox::new(BoxDecoration::with_color(background).set_border(resolved_border))
            .child(SizedBox::height(self.height + bottom_inset.get()).child(toolbar))
    }
}

impl PreferredSizeView for CupertinoTabBar {
    fn preferred_size(&self) -> Size {
        Size::new(px(f32::INFINITY), px(self.height))
    }
}
