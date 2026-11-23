//! The iOS-style bottom tab bar and its items.
//!
//! Implementation notes: each item is one interactive node carrying the
//! selection, the label's text and, while the bar has `on_tap`, the click.

use std::rc::Rc;

use flui_sdk::geometry::Size;
use flui_sdk::painting::styling::{Border, BorderSide, BorderStyle, Color};
use flui_sdk::rendering::HitTestBehavior;
use flui_sdk::rendering::objects::{CrossAxisAlignment, MainAxisAlignment};
use flui_sdk::semantics::SemanticsRole;
use flui_sdk::view::BoxedView;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::__private::node::{ControlTap, Description, InteractiveNode};
use flui_sdk::widgets::{
    Column, DefaultTextStyle, Expanded, IconTheme, IconThemeData, Padding, PreferredSizeView, Row,
    Semantics, SizedBox, Text,
};

use crate::bar_background::{BarBorder, bar_background, hairline_width};
use crate::colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::page_scaffold::ObstructingPreferredSizeView;
use crate::{CupertinoTheme, ResolvedCupertinoTheme};

/// The standard iOS 10 tab bar height (`_kTabBarHeight`).
pub(crate) const TAB_BAR_HEIGHT: f64 = 50.0;

/// The default border color, `0x4D000000` light / `0x29000000` dark: unlike the
/// navigation bar's border color it depends on brightness, so it is resolved in
/// `build` against the ambient brightness.
fn default_border_color() -> CupertinoDynamicColor {
    CupertinoDynamicColor::with_brightness(
        Color::from_argb(0x4D00_0000),
        Color::from_argb(0x2900_0000),
    )
}

/// One tab's icon and label for a [`CupertinoTabBar`].
///
/// `icon` is required, `active_icon` defaults to `icon`, and `label` is
/// optional text rendered below the icon. Unlike Flutter's
/// `BottomNavigationBarItem` it has no `tooltip` or `background_color`:
/// neither the tab bar nor `CupertinoTabScaffold` reads them, and ADR-0028
/// forbids sharing an item type with `flui-material`.
///
/// ```
/// use flui_cupertino::CupertinoTabBarItem;
/// use flui_sdk::widgets::{Icon, IconData};
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
    /// same icon as the inactive state.
    #[must_use]
    pub fn active_icon(mut self, active_icon: impl IntoView) -> Self {
        self.active_icon = Some(active_icon.into_view().boxed());
        self
    }

    /// Sets the label text, rendered below the icon.
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
type TabTapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventContext<'_>, usize)>;

/// An iOS-style bottom tab bar.
///
/// - The default height is `50.0` ([`height`](Self::height)).
/// - The default top border is a one-device-pixel line
///   (`1 / device_pixel_ratio` logical pixels under the ambient
///   `MediaQuery`, one logical pixel without one), `0x4D000000` in light and
///   `0x29000000` in dark, resolved against the ambient brightness on each
///   build.
/// - `inactive_color` defaults to `CupertinoColors::INACTIVE_GRAY` and
///   `active_color` to the theme's primary color; each item's icon and label
///   take the color for `current_index`.
/// - The bar pads itself against the bottom safe-area inset
///   (`MediaQuery` `padding.bottom` is added to its height and to the label
///   row's bottom padding).
/// - Behind a background that is not fully opaque (the theme's default is
///   translucent) the bar blurs what lies under it, a Gaussian blur of sigma
///   10 over the bar's box, and
///   [`should_fully_obstruct`](ObstructingPreferredSizeView::should_fully_obstruct)
///   answers `false`; [`CupertinoTabScaffold`](crate::CupertinoTabScaffold)
///   reads that answer to decide whether content extends under the bar.
/// - [`new`](Self::new) asserts, in debug builds only, that there are at
///   least two items, as Apple's Human Interface Guidelines require.
/// - There is no `copy_with`; the bar is `Clone` and its builder methods
///   serve the same purpose.
///
/// # Not implemented
///
/// Each item's `Semantics` sets `selected` but carries no
/// localized hint of tab index and count, since the package has no
/// localizations. The bar has no mouse-region or text-field tap-region
/// wrappers.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/bottom_tab_bar.dart`.
///
/// ```
/// use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
/// use flui_sdk::widgets::{Icon, IconData};
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
    icon_size: f64,
    height: f64,
    border: BarBorder,
}

impl CupertinoTabBar {
    /// A tab bar showing `items`, `current_index: 0`, the default hairline
    /// top border, and the theme's `bar_background_color`.
    ///
    /// `items` must carry at least 2 entries — Apple's Human Interface
    /// Guidelines require it, and the constructor asserts the
    /// same (debug builds only).
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
            border: BarBorder::Hairline,
        }
    }

    /// The configured items, in display order.
    #[must_use]
    pub fn items(&self) -> &[CupertinoTabBarItem] {
        &self.items
    }

    /// Sets the tap handler, called with the tapped item's index.
    #[must_use]
    pub fn on_tap<R: flui_sdk::view::EventOutcome>(
        mut self,
        on_tap: impl Fn(&mut flui_sdk::view::EventContext<'_>, usize) -> R + 'static,
    ) -> Self {
        self.on_tap = Some(Rc::new(move |ctx, index| on_tap(ctx, index).report()));
        self
    }

    /// Sets which item is drawn active.
    ///
    /// # Panics
    ///
    /// Panics if `current_index` is outside this bar's items, in every build profile.
    #[must_use]
    pub fn current_index(mut self, current_index: usize) -> Self {
        assert!(
            current_index < self.items.len(),
            "CupertinoTabBar's current index {current_index} is out of bounds for {} items",
            self.items.len()
        );
        self.current_index = current_index;
        self
    }

    /// The index this bar is currently drawing as active.
    #[must_use]
    pub fn current_index_value(&self) -> usize {
        self.current_index
    }

    /// Overrides the resolved background. Defaults to
    /// [`crate::CupertinoThemeData::bar_background_color`].
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Overrides the active item's icon/label color. Defaults to
    /// [`crate::CupertinoThemeData::primary_color`].
    #[must_use]
    pub fn active_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.active_color = Some(color.into());
        self
    }

    /// Overrides the inactive items' icon/label color. Defaults to
    /// [`CupertinoColors::INACTIVE_GRAY`].
    #[must_use]
    pub fn inactive_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.inactive_color = color.into();
        self
    }

    /// Overrides the icon size. Defaults to `30.0`.
    #[must_use]
    pub fn icon_size(mut self, icon_size: f64) -> Self {
        self.icon_size = icon_size;
        self
    }

    /// Overrides the bar's height. Defaults to `50.0`.
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.height = height;
        self
    }

    /// Overrides the top border, or removes it with `None`. Defaults to
    /// the hairline border.
    #[must_use]
    pub fn border(mut self, border: Option<Border>) -> Self {
        self.border = BarBorder::authored(border);
        self
    }

    /// The currently registered tap handler, if any. `pub(crate)` so
    /// [`crate::CupertinoTabScaffold`] can chain through the caller's
    /// original handler after overriding `on_tap` for its own
    /// index-tracking.
    pub(crate) fn on_tap_handler(&self) -> Option<TabTapCallback> {
        self.on_tap.clone()
    }

    /// The authored background resolved against `ctx`, or the theme's.
    fn background(&self, theme: &ResolvedCupertinoTheme, ctx: &dyn BuildContext) -> Color {
        self.background_color
            .map_or(theme.bar_background_color, |color| color.resolve(ctx))
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
        let background = self.background(&theme, ctx);
        let inactive = self.inactive_color.resolve(ctx);
        let active = self
            .active_color
            .map_or(theme.primary_color, |color| color.resolve(ctx));
        let bottom_inset = MediaQuery::maybe_of(ctx).map_or(0.0, |data| data.padding().bottom);

        let tab_label_style = &theme.text_theme.tab_label_text_style;

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
                    vec![Expanded::new(flui_sdk::widgets::Center::new().child(icon)).boxed()];
                if let Some(label) = &item.label {
                    column_children.push(Text::new(label.clone()).boxed());
                }

                let content = IconTheme::new(
                    IconThemeData::default().color(color).size(self.icon_size),
                    DefaultTextStyle::new(
                        tab_label_style.clone().with_color(color),
                        Padding::new(flui_sdk::geometry::EdgeInsets::only_bottom(4.0)).child(
                            Column::new(column_children)
                                .main_axis_alignment(MainAxisAlignment::End),
                        ),
                    ),
                );

                // Flutter's `Semantics(selected:)` and opaque `GestureDetector`
                // around the item are one interactive node here (ADR-0322
                // §1): its description carries the selection, and the label's
                // text joins the item's node as it joined that `Semantics`.
                // Its tap, while the bar has `on_tap`, offers the click. An
                // item without a tap is not disabled, only inert, so the node
                // stays enabled.
                let mut node = InteractiveNode::new(content)
                    .behavior(HitTestBehavior::Opaque)
                    .semantics(Description::new(SemanticsRole::None).selected(is_active));
                if let Some(on_tap) = self.on_tap.clone() {
                    node = node.tap(ControlTap::new().on_tap(move |ctx| on_tap(ctx, index)));
                }

                // No localized `hint` (tab index and count): the package has no
                // localizations.
                Expanded::new(node).boxed()
            })
            .collect();

        // Bottom padding around the item row; each item owns its own semantics
        // node rather than merging into one.
        let toolbar = Padding::new(flui_sdk::geometry::EdgeInsets::only_bottom(bottom_inset))
            .child(
                Semantics::new()
                    .explicit_child_nodes(true)
                    .child(Row::new(item_views).cross_axis_alignment(CrossAxisAlignment::End)),
            );

        let border = self.border.resolve(|| {
            Border::new(
                Some(BorderSide::new(
                    CupertinoColor::Dynamic(default_border_color()).resolve(ctx),
                    hairline_width(ctx),
                    BorderStyle::Solid,
                )),
                None,
                None,
                None,
            )
        });

        bar_background(
            background,
            border,
            SizedBox::height(self.height + bottom_inset).child(toolbar),
        )
    }
}

impl PreferredSizeView for CupertinoTabBar {
    fn preferred_size(&self) -> Size {
        Size::new(f64::INFINITY, self.height)
    }
}

impl ObstructingPreferredSizeView for CupertinoTabBar {
    /// `true` when the bar's background, resolved under `ctx`, is fully
    /// opaque.
    fn should_fully_obstruct(&self, ctx: &dyn BuildContext) -> bool {
        self.background(&CupertinoTheme::of(ctx), ctx).is_opaque()
    }
}
