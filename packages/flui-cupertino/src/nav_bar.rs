//! The static iOS-style navigation bar.
//!
//! Implementation notes: `middle` is centered in one `Stack` layer across the
//! whole bar width, and `leading`/`trailing` are `Positioned` at the edges in
//! another. ADR-0028 prefers composition over a toolbar render object, so
//! nothing shifts or clips `middle` when the side slots grow wide enough to
//! collide with it.

use flui_sdk::geometry::Size;
use flui_sdk::painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_sdk::semantics::HeadingLevel;
use flui_sdk::view::BoxedView;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Center, DecoratedBox, DefaultTextStyle, Positioned, PreferredSizeView, SafeArea, Semantics,
    SizedBox, Stack,
};

use crate::CupertinoColor;
use crate::CupertinoTheme;

/// The persistent height of the bar (the minimum interactive dimension).
pub(crate) const NAV_BAR_PERSISTENT_HEIGHT: f64 = 44.0;

/// The horizontal inset `leading`/`trailing` sit at from the bar's edges.
const NAV_BAR_EDGE_PADDING: f64 = 16.0;

/// The default border color (`_kDefaultNavBarBorderColor`).
const DEFAULT_NAV_BAR_BORDER_COLOR: Color = Color::from_argb(0x4D00_0000);

/// The stroke width of the hairline borders of the navigation and tab bars:
/// one logical pixel, because a width of `0.0` paints nothing.
pub(crate) const HAIRLINE_BORDER_WIDTH: f64 = 1.0;

/// A bottom-only hairline of [`HAIRLINE_BORDER_WIDTH`].
fn default_border() -> Border {
    Border::new(
        None,
        None,
        Some(BorderSide::new(
            DEFAULT_NAV_BAR_BORDER_COLOR,
            HAIRLINE_BORDER_WIDTH,
            BorderStyle::Solid,
        )),
        None,
    )
}

/// A static iOS-style navigation bar: `leading`, `middle` and `trailing`
/// slots on a 44-point bar with a hairline bottom border.
///
/// - [`preferred_size`](PreferredSizeView::preferred_size) is `44.0` tall.
///   The bar's own height does not include the top safe-area inset;
///   [`CupertinoPageScaffold`](crate::CupertinoPageScaffold) adds that once,
///   and the bar pads its content against the inset itself with `SafeArea`.
/// - The background is opaque. An unset `background_color` resolves from
///   [`CupertinoThemeData::bar_background_color`](crate::CupertinoThemeData::bar_background_color).
/// - `middle` is centered across the bar's full width, whatever the widths
///   of `leading` and `trailing`.
/// - The default border is a bottom-only line of color `0x4D000000` and
///   width 1 logical pixel. iOS draws a device-pixel hairline, which some
///   toolkits spell as width `0.0`; `flui-painting` skips a side of width
///   `0.0` entirely, so the bar uses one logical pixel instead.
///
/// # Not implemented
///
/// This is the collapsed shape only. The bar has no large title or `bottom`
/// slot, no leading or middle implied from the previous route (a Router page
/// carries no title, so `leading` and `middle` are exactly what the caller
/// supplies), no scroll-under background fade, no blur (the bar paints no
/// `BackdropFilter`, so a translucent background shows the page unblurred),
/// no hero transition between bars, and no status-bar `brightness` or system
/// UI overlay style.
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
    border: Option<Border>,
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
            border: Some(default_border()),
        }
    }

    /// Sets the leading slot.
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(leading.into_view().boxed());
        self
    }

    /// Sets the middle (title) slot, true-centered across the bar's full
    /// width.
    #[must_use]
    pub fn middle(mut self, middle: impl IntoView) -> Self {
        self.middle = Some(middle.into_view().boxed());
        self
    }

    /// Sets the trailing slot.
    #[must_use]
    pub fn trailing(mut self, trailing: impl IntoView) -> Self {
        self.trailing = Some(trailing.into_view().boxed());
        self
    }

    /// Overrides the resolved background. Defaults to
    /// [`CupertinoThemeData::bar_background_color`](crate::CupertinoThemeData::bar_background_color).
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Overrides the bottom border, or removes it with `None`. Defaults to
    /// the one-pixel hairline border.
    #[must_use]
    pub fn border(mut self, border: Option<Border>) -> Self {
        self.border = border;
        self
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
            .field("has_border", &self.border.is_some())
            .finish_non_exhaustive()
    }
}

impl StatelessView for CupertinoNavigationBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = CupertinoTheme::of(ctx);
        let background = self
            .background_color
            .unwrap_or_else(|| theme.bar_background_color())
            .resolve(ctx);
        let top_inset = MediaQuery::maybe_of(ctx).map_or(0.0, |data| data.padding().top);

        let mut layers: Vec<BoxedView> = Vec::new();
        if let Some(middle) = &self.middle {
            let styled_middle = DefaultTextStyle::new(
                theme.text_theme().nav_title_text_style(),
                Semantics::new()
                    .heading_level(HeadingLevel::H1)
                    .child(middle.clone()),
            );
            layers.push(
                Positioned::new(Center::new().child(styled_middle))
                    .left(0.0)
                    .right(0.0)
                    .top(0.0)
                    .bottom(0.0)
                    .boxed(),
            );
        }
        if let Some(leading) = &self.leading {
            layers.push(
                Positioned::new(Center::new().child(leading.clone()))
                    .left(NAV_BAR_EDGE_PADDING)
                    .top(0.0)
                    .bottom(0.0)
                    .boxed(),
            );
        }
        if let Some(trailing) = &self.trailing {
            layers.push(
                Positioned::new(Center::new().child(trailing.clone()))
                    .right(NAV_BAR_EDGE_PADDING)
                    .top(0.0)
                    .bottom(0.0)
                    .boxed(),
            );
        }

        let toolbar = DefaultTextStyle::new(theme.text_theme().text_style(), Stack::new(layers));

        let decorated =
            DecoratedBox::new(BoxDecoration::with_color(background).set_border(self.border))
                .child(SafeArea::new(toolbar).bottom(false));

        SizedBox::height(NAV_BAR_PERSISTENT_HEIGHT + top_inset).child(decorated)
    }
}

impl PreferredSizeView for CupertinoNavigationBar {
    fn preferred_size(&self) -> Size {
        // The bar has no `bottom` or large-title height to add.
        Size::new(f64::INFINITY, NAV_BAR_PERSISTENT_HEIGHT)
    }
}
