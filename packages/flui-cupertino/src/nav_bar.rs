//! The static iOS-style navigation bar.

use flui_sdk::foundation::TextDirection;
use flui_sdk::geometry::{EdgeInsets, Size};
use flui_sdk::painting::Alignment;
use flui_sdk::painting::styling::{Border, BorderSide, BorderStyle, Color};
use flui_sdk::semantics::HeadingLevel;
use flui_sdk::view::BoxedView;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Align, DefaultTextStyle, Directionality, Padding, PreferredSizeView, SafeArea, Semantics,
    SizedBox,
};

use crate::bar_background::{BarBorder, bar_background, hairline_width};
use crate::navigation_toolbar::NavigationToolbar;
use crate::page_scaffold::ObstructingPreferredSizeView;
use crate::{CupertinoColor, CupertinoTheme, ResolvedCupertinoTheme};

/// The persistent height of the bar (the minimum interactive dimension).
pub(crate) const NAV_BAR_PERSISTENT_HEIGHT: f64 = 44.0;

/// The inset `leading` keeps from the start edge and `trailing` from the end
/// edge (`_kNavBarEdgePadding`).
const NAV_BAR_EDGE_PADDING: f64 = 16.0;

/// The gap `middle` keeps from `leading` and `trailing`: the `middleSpacing`
/// the persistent bar passes to its toolbar
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart:1849`).
const NAV_BAR_MIDDLE_SPACING: f64 = 6.0;

/// The default border color (`_kDefaultNavBarBorderColor`), the same in
/// light and dark.
const DEFAULT_NAV_BAR_BORDER_COLOR: Color = Color::from_argb(0x4D00_0000);

/// A static iOS-style navigation bar: `leading`, `middle` and `trailing`
/// slots on a 44-point bar with a hairline bottom border.
///
/// - [`preferred_size`](PreferredSizeView::preferred_size) is `44.0` tall.
///   The bar's own height does not include the top safe-area inset;
///   [`CupertinoPageScaffold`](crate::CupertinoPageScaffold) adds that once,
///   and the bar pads its content against the inset itself with `SafeArea`.
/// - An unset `background_color` resolves from
///   [`CupertinoThemeData::bar_background_color`](crate::CupertinoThemeData::bar_background_color),
///   which is translucent by default. Behind a background that is not fully
///   opaque the bar blurs what lies under it (a Gaussian blur of sigma 10
///   over the bar's box), and
///   [`should_fully_obstruct`](ObstructingPreferredSizeView::should_fully_obstruct)
///   answers `false`, so a page scaffold lets its content run under the bar.
/// - `leading` sits 16 points from the start edge and `trailing` 16 points
///   from the end edge, both vertically centred. `middle` is centred across
///   the bar's full width while it keeps 6 points from both; a `middle` too
///   wide for that moves toward the side with room, and one wider than the
///   space between `leading` and `trailing` is limited to that space. Start
///   and end follow the ambient `Directionality`: `leading` is on the right
///   in a right-to-left context.
/// - The default border is a bottom-only line of color `0x4D000000`, one
///   device pixel wide: `1 / device_pixel_ratio` logical pixels under the
///   ambient `MediaQuery`, one logical pixel without one.
///
/// # Not implemented
///
/// This is the collapsed shape only. The bar has no large title or `bottom`
/// slot, no leading or middle implied from the previous route (a Router page
/// carries no title, so `leading` and `middle` are exactly what the caller
/// supplies), no scroll-under background fade, no hero transition between
/// bars, no text-scale clamp on its slots, and no status-bar `brightness` or
/// system UI overlay style.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/nav_bar.dart`.
///
/// ```
/// use flui_cupertino::CupertinoNavigationBar;
/// use flui_sdk::widgets::Text;
///
/// let _bar = CupertinoNavigationBar::new().middle(Text::new("Settings"));
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoNavigationBar {
    leading: Option<BoxedView>,
    middle: Option<BoxedView>,
    trailing: Option<BoxedView>,
    background_color: Option<CupertinoColor>,
    border: BarBorder,
}

impl CupertinoNavigationBar {
    /// A bar with no leading/middle/trailing, the theme's
    /// `bar_background_color`, and the default hairline bottom border.
    #[must_use]
    pub fn new() -> Self {
        Self {
            leading: None,
            middle: None,
            trailing: None,
            background_color: None,
            border: BarBorder::Hairline,
        }
    }

    /// Sets the leading slot, at the start edge.
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(leading.into_view().boxed());
        self
    }

    /// Sets the middle (title) slot, centred across the bar's full width
    /// while it fits clear of `leading` and `trailing`.
    #[must_use]
    pub fn middle(mut self, middle: impl IntoView) -> Self {
        self.middle = Some(middle.into_view().boxed());
        self
    }

    /// Sets the trailing slot, at the end edge.
    #[must_use]
    pub fn trailing(mut self, trailing: impl IntoView) -> Self {
        self.trailing = Some(trailing.into_view().boxed());
        self
    }

    /// Overrides the resolved background. Defaults to
    /// [`CupertinoThemeData::bar_background_color`](crate::CupertinoThemeData::bar_background_color).
    /// A fully opaque color turns the blur off and makes the bar fully
    /// obstruct the content behind it.
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Overrides the bottom border, or removes it with `None`. Defaults to
    /// the one-device-pixel hairline border.
    #[must_use]
    pub fn border(mut self, border: Option<Border>) -> Self {
        self.border = BarBorder::authored(border);
        self
    }

    /// The authored background resolved against `ctx`, or the theme's.
    fn background(&self, theme: &ResolvedCupertinoTheme, ctx: &dyn BuildContext) -> Color {
        self.background_color
            .map_or(theme.bar_background_color, |color| color.resolve(ctx))
    }
}

impl Default for CupertinoNavigationBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CupertinoNavigationBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoNavigationBar")
            .field("has_leading", &self.leading.is_some())
            .field("has_middle", &self.middle.is_some())
            .field("has_trailing", &self.trailing.is_some())
            .field("border", &self.border)
            .finish_non_exhaustive()
    }
}

impl StatelessView for CupertinoNavigationBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = CupertinoTheme::of(ctx);
        let background = self.background(&theme, ctx);
        let top_inset = MediaQuery::maybe_of(ctx).map_or(0.0, |data| data.padding().top);
        let direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        let (start, end) = match direction {
            TextDirection::Ltr => (
                EdgeInsets::only_left(NAV_BAR_EDGE_PADDING),
                EdgeInsets::only_right(NAV_BAR_EDGE_PADDING),
            ),
            TextDirection::Rtl => (
                EdgeInsets::only_right(NAV_BAR_EDGE_PADDING),
                EdgeInsets::only_left(NAV_BAR_EDGE_PADDING),
            ),
        };

        // `leading` gets the toolbar's full height, so it centres its content
        // itself and keeps the width of that content.
        let leading = self.leading.as_ref().map(|leading| {
            Align::new(Alignment::CENTER)
                .width_factor(1.0)
                .child(Padding::new(start).child(leading.clone()))
                .boxed()
        });
        let middle = self.middle.as_ref().map(|middle| {
            DefaultTextStyle::new(
                theme.text_theme.nav_title_text_style.clone(),
                Semantics::new()
                    .heading_level(HeadingLevel::H1)
                    .child(middle.clone()),
            )
            .boxed()
        });
        let trailing = self
            .trailing
            .as_ref()
            .map(|trailing| Padding::new(end).child(trailing.clone()).boxed());

        let toolbar = DefaultTextStyle::new(
            theme.text_theme.text_style.clone(),
            NavigationToolbar {
                leading,
                middle,
                trailing,
                middle_spacing: NAV_BAR_MIDDLE_SPACING,
            },
        );

        let border = self.border.resolve(|| {
            Border::new(
                None,
                None,
                Some(BorderSide::new(
                    DEFAULT_NAV_BAR_BORDER_COLOR,
                    hairline_width(ctx),
                    BorderStyle::Solid,
                )),
                None,
            )
        });

        SizedBox::height(NAV_BAR_PERSISTENT_HEIGHT + top_inset).child(bar_background(
            background,
            border,
            SafeArea::new(toolbar).bottom(false),
        ))
    }
}

impl PreferredSizeView for CupertinoNavigationBar {
    fn preferred_size(&self) -> Size {
        // The bar has no `bottom` or large-title height to add.
        Size::new(f64::INFINITY, NAV_BAR_PERSISTENT_HEIGHT)
    }
}

impl ObstructingPreferredSizeView for CupertinoNavigationBar {
    /// `true` when the bar's background, resolved under `ctx`, is fully
    /// opaque.
    fn should_fully_obstruct(&self, ctx: &dyn BuildContext) -> bool {
        self.background(&CupertinoTheme::of(ctx), ctx).is_opaque()
    }
}
