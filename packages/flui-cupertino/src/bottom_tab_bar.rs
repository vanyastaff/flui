//! [`CupertinoTabBar`] — the iOS-style bottom tab bar, and
//! [`CupertinoTabBarItem`], the per-tab icon/label pair it displays.
//!
//! ## What it does
//!
//! - A default `height` of `50.0`.
//! - The hairline top border (`0x4D000000`/`0x29000000` light/dark) — a real
//!   1.0px stroke, not a literal device-pixel `width: 0.0`, as
//!   [`crate::CupertinoNavigationBar`] documents.
//! - `inactive_color` defaults to `CupertinoColors::INACTIVE_GRAY`,
//!   `active_color` to the theme's `primary_color`, and each item's
//!   icon/label is recolored based on `current_index`.
//! - Self-padding against the bottom safe-area inset (`MediaQuery`'s
//!   `padding.bottom` folded into both the bar's own height and the label
//!   row's bottom padding), like the top-inset self-padding on
//!   [`crate::CupertinoNavigationBar`].
//! - `opaque(ctx)`: whether the resolved background is fully opaque —
//!   consumed by [`crate::CupertinoTabScaffold`]'s content-padding math (the
//!   opaque and translucent branches).
//! - Per-item `Semantics(selected: active)`, with each item keeping its own
//!   semantics node rather than merging into one.
//!
//! ## `CupertinoTabBarItem`, not a shared item type
//!
//! A cross-design-system bottom-navigation item type would be one ADR-0028
//! forbids depending on (`flui-material` is never a dependency of this
//! crate). This crate ships a narrower, Cupertino-owned type instead: `icon`
//! (required), `active_icon` (optional, defaults to `icon`), `label`
//! (optional `String`, rendered as plain [`Text`]). No
//! `tooltip`/`background_color`: neither the tab bar nor
//! `CupertinoTabScaffold` consumes them (the tab bar's own `background_color`
//! is a separate field).
//!
//! ## Deferred, named
//!
//! - **Blur** (`BackdropFilter` on a translucent background) — same gap
//!   `CupertinoNavigationBar` documents: no `BackdropFilter` primitive in
//!   `flui-widgets` yet. A caller-supplied translucent `background_color`
//!   does reach the opaque/translucent branch (`opaque(ctx)` is wired), it
//!   just paints with no blur behind it.
//! - **The localized `Semantics` hint** (tab index and count) — no
//!   `CupertinoLocalizations`-equivalent in this crate; `selected` is set,
//!   the hint is not.
//! - **A `copy_with` method** — `CupertinoTabBar: Clone` plus its own builder
//!   methods (`.current_index(...)`, `.on_tap(...)`) already give
//!   [`crate::CupertinoTabScaffold`] the same capability idiomatically.
//! - **Mouse-region and text-field tap-region wrappers** — no mouse-cursor or
//!   text-field tap-region substrate to wire either through.

use std::rc::Rc;

use flui_sdk::geometry::Size;
use flui_sdk::painting::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Column, CrossAxisAlignment, DecoratedBox, DefaultTextStyle, Expanded, GestureDetector,
    HitTestBehavior, IconTheme, IconThemeData, MainAxisAlignment, MediaQuery, Padding,
    PreferredSizeView, Row, Semantics, SizedBox, Text,
};

use crate::colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
use crate::theme::CupertinoTheme;

/// The standard iOS 10 tab bar height.
pub const TAB_BAR_HEIGHT: f64 = 50.0;

/// The stroke width the hairline border is painted at — see
/// [`crate::nav_bar::HAIRLINE_BORDER_WIDTH`]'s doc for why a literal
/// `width: 0.0` cannot be used.
pub const HAIRLINE_BORDER_WIDTH: f64 = crate::nav_bar::HAIRLINE_BORDER_WIDTH;

/// The default border color, `0x4D000000` light / `0x29000000` dark —
/// genuinely brightness-dependent, unlike
/// [`crate::CupertinoNavigationBar`]'s own hairline border color (a plain,
/// non-dynamic `Color`). Resolved fresh in `build` against the
/// ambient brightness, not baked into a `const` at construction time.
fn default_border_color() -> CupertinoDynamicColor {
    CupertinoDynamicColor::with_brightness(
        Color::from_argb(0x4D00_0000),
        Color::from_argb(0x2900_0000),
    )
}

/// One tab's icon/label pair — a Cupertino-owned type, see the module docs.
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
type TabTapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>, usize)>;

/// An iOS-style bottom tab bar — see the module docs for exactly what is and
/// is not supported.
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
    border: Option<Border<f64>>,
    /// Whether `border` still holds the un-overridden default. If so,
    /// `build` resolves [`default_border_color`]'s light/dark variant fresh
    /// against the ambient brightness every time, rather than using a color
    /// baked in once at construction — see that function's doc for why this
    /// component's default border (unlike `CupertinoNavigationBar`'s) is
    /// genuinely brightness-dependent.
    border_is_default: bool,
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
            border: None,
            border_is_default: true,
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
        on_tap: impl Fn(&mut flui_sdk::view::EventCx<'_>, usize) -> R + 'static,
    ) -> Self {
        self.on_tap = Some(Rc::new(move |cx, index| on_tap(cx, index).report()));
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

    /// Overrides the bar's height. Defaults to [`TAB_BAR_HEIGHT`].
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.height = height;
        self
    }

    /// Overrides the top border, or removes it with `None`. Defaults to
    /// the hairline border.
    #[must_use]
    pub fn border(mut self, border: Option<Border<f64>>) -> Self {
        self.border = border;
        self.border_is_default = false;
        self
    }

    /// The currently registered tap handler, if any. `pub(crate)` so
    /// [`crate::CupertinoTabScaffold`] can chain through the caller's
    /// original handler after overriding `on_tap` for its own
    /// index-tracking.
    pub(crate) fn on_tap_handler(&self) -> Option<TabTapCallback> {
        self.on_tap.clone()
    }

    /// Whether the resolved background is fully opaque. Consumed by
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
        let bottom_inset = MediaQuery::maybe_of(ctx).map_or(0.0, |data| data.padding.bottom);

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
                    vec![Expanded::new(flui_sdk::widgets::Center::new().child(icon)).boxed()];
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
                        Padding::new(flui_sdk::geometry::EdgeInsets::only_bottom(4.0)).child(
                            Column::new(column_children)
                                .main_axis_alignment(MainAxisAlignment::End),
                        ),
                    ),
                );

                let mut detector = GestureDetector::new().behavior(HitTestBehavior::Opaque);
                if let Some(on_tap) = self.on_tap.clone() {
                    detector = detector.on_tap(move |cx| on_tap(cx, index));
                }

                // `selected` is set; the localized `hint` is deferred (no
                // `CupertinoLocalizations`-equivalent tab label in this crate).
                Expanded::new(
                    Semantics::new()
                        .selected(is_active)
                        .child(detector.child(content)),
                )
                .boxed()
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

        let resolved_border = if self.border_is_default {
            Some(Border::new(
                Some(BorderSide::new(
                    CupertinoColor::Dynamic(default_border_color()).resolve(ctx),
                    HAIRLINE_BORDER_WIDTH,
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
            .child(SizedBox::height(self.height + bottom_inset).child(toolbar))
    }
}

impl PreferredSizeView for CupertinoTabBar {
    fn preferred_size(&self) -> Size {
        Size::new(f64::INFINITY, self.height)
    }
}
