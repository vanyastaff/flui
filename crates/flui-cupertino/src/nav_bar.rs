//! [`CupertinoNavigationBar`] — a static iOS-style navigation bar: a
//! centered `middle` title with `leading`/`trailing` slots.
//!
//! Flutter parity: `cupertino/nav_bar.dart`'s `CupertinoNavigationBar`
//! (oracle tag `3.44.0`), narrowed to the **collapsed, static** shape —
//! `CupertinoNavigationBar`'s default constructor, not `.large()`, and not
//! `CupertinoSliverNavigationBar` (an entirely different, scroll-driven
//! widget). See "Deferred, named" below for exactly what that excludes.
//!
//! ## What this ports
//!
//! - `_kNavBarPersistentHeight` (`= kMinInteractiveDimensionCupertino =
//!   44.0`) as [`preferred_size`](PreferredSizeView::preferred_size)'s
//!   height — verified at the oracle source, not the plan's guess: it is
//!   *not* itself topped up by `MediaQuery.padding.top` (that addition
//!   happens once, in `CupertinoPageScaffold`, the same "advertise the bar
//!   height only" contract `flui_material::AppBar` already established in
//!   this workspace).
//! - The hairline bottom border, `_kDefaultNavBarBorder` (color
//!   `0x4D000000`) — see the "hairline" divergence note below.
//! - The opaque background: `backgroundColor` resolved from
//!   [`crate::CupertinoThemeData::bar_background_color`] when unset.
//! - Self-padding against the top safe-area inset via [`SafeArea`],
//!   matching the oracle's `_PersistentNavigationBar` (and
//!   `flui_material::AppBar`'s own "the bar pads itself" contract).
//! - `middle` true-centered across the bar's full width (a [`Stack`] +
//!   full-bleed [`Center`], not `Row`'s remaining-space centering) — see
//!   "Layout: no `NavigationToolbar`" below.
//!
//! ## Deferred, named
//!
//! - **`automaticallyImplyLeading`/`automaticallyImplyMiddle`/
//!   `previousPageTitle`** — the back-chevron-from-the-previous-route's-title
//!   heuristics. No consumer route carries a `title` to imply from yet in
//!   this crate (`cupertino_page_route`'s own `title` field is itself
//!   deferred — see `route.rs`'s module docs). `leading`/`middle` are always
//!   exactly what the caller supplies.
//! - **`.large()` / `largeTitle` / `bottom`** — the expanded large-title
//!   layout and the bottom-accessory slot. This type ships the collapsed
//!   shape only.
//! - **`automaticBackgroundVisibility` / scroll-under fade** — no
//!   `ScrollNotificationObserver` substrate to drive
//!   `_scrollAnimationValue` from. The background is always fully opaque at
//!   its resolved color; `_kTransparentNavBarBorder`'s scroll-under lerp
//!   never applies.
//! - **Blur** (`enableBackgroundFilterBlur`, `BackdropFilter`). Flutter
//!   itself only blurs when the resolved background's alpha is not `0xFF`
//!   (`_wrapWithBackground`'s `enabled: backgroundColor.alpha != 0xFF && ...`)
//!   — moot today since this V1 has no translucent-background path to
//!   trigger it, and `flui-widgets` has no `BackdropFilter` primitive yet
//!   regardless.
//! - **Hero transition between nav bars** (`transitionBetweenRoutes`,
//!   `heroTag`, `_TransitionableNavigationBar`, `_NavigationBarTransition`)
//!   — the push/pop hand-off where two nav bars visually merge. Out of this
//!   component's scope; each nav bar is independently mounted per route.
//! - **`brightness`** (system status-bar style override) — no platform
//!   status-bar styling seam in FLUI.
//! - **System UI overlay style** (`AnnotatedRegion<SystemUiOverlayStyle>`)
//!   — platform-only, no FLUI equivalent.
//!
//! ## The hairline border is *not* a literal `width: 0.0`
//!
//! The oracle's `_kDefaultNavBarBorder` is `BorderSide(color: ..., width:
//! 0.0)`, with the comment "0.0 means one physical pixel" — Flutter's
//! device-pixel-hairline convention. `flui-painting`'s box-border painter
//! has no such convention: `paint_border` skips any side with
//! `width.get() <= 0.0` outright (`decoration.rs`). A literal `width: 0.0`
//! port would render **no border at all**, silently failing the "hairline
//! bottom border" contract. This uses [`HAIRLINE_BORDER_WIDTH`] (one logical
//! pixel) instead — a real, honestly-approximated stroke, not Flutter's
//! true device-pixel width.
//!
//! ## Layout: no `NavigationToolbar`
//!
//! The oracle composes `leading`/`middle`/`trailing` with
//! `NavigationToolbar`, whose render object centers `middle` in the full
//! toolbar width and *shifts* it only if `leading`/`trailing` would
//! otherwise overlap it. ADR-0028 prefers composition over a new render
//! object unless one is proven necessary, and ordinary
//! leading/middle/trailing widths never approach that overlap case in
//! practice — so this ports the visual outcome (a true-centered title) with
//! a [`Stack`]: `middle` is [`Center`]-ed across the **entire** bar width in
//! one layer, `leading`/`trailing` are [`Positioned`] at the edges in
//! another. **Named divergence**: unlike `NavigationToolbar`, nothing here
//! shifts or clips `middle` if `leading`/`trailing` grow wide enough to
//! visually collide with it.

use flui_types::Size;
use flui_types::geometry::px;
use flui_types::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{
    Center, DecoratedBox, DefaultTextStyle, MediaQuery, Positioned, PreferredSizeView, SafeArea,
    Semantics, SizedBox, Stack,
};

use crate::colors::CupertinoColor;
use crate::theme::CupertinoTheme;

/// `_kNavBarPersistentHeight` (`nav_bar.dart`, oracle tag `3.44.0`) —
/// `kMinInteractiveDimensionCupertino`, `44.0`.
pub const NAV_BAR_PERSISTENT_HEIGHT: f32 = 44.0;

/// `_kNavBarEdgePadding` (`nav_bar.dart`, oracle tag `3.44.0`) — the
/// horizontal inset `leading`/`trailing` sit at from the bar's edges (the
/// oracle's "if leading is an automatically-inserted back button, padding is
/// 0" branch is unreachable here — see the module docs' deferred
/// `automaticallyImplyLeading`).
const NAV_BAR_EDGE_PADDING: f32 = 16.0;

/// `_kDefaultNavBarBorderColor` (`nav_bar.dart`, oracle tag `3.44.0`).
const DEFAULT_NAV_BAR_BORDER_COLOR: Color = Color::from_argb(0x4D00_0000);

/// The stroke width this port paints the hairline border at — see the
/// module docs' "hairline" divergence note. One logical pixel, not the
/// oracle's true device-pixel width.
pub const HAIRLINE_BORDER_WIDTH: f32 = 1.0;

/// `_kDefaultNavBarBorder` (`nav_bar.dart`, oracle tag `3.44.0`): a
/// bottom-only hairline, approximated per [`HAIRLINE_BORDER_WIDTH`]'s doc.
fn default_border() -> Border<flui_types::geometry::Pixels> {
    Border::new(
        None,
        None,
        Some(BorderSide::new(
            DEFAULT_NAV_BAR_BORDER_COLOR,
            px(HAIRLINE_BORDER_WIDTH),
            BorderStyle::Solid,
        )),
        None,
    )
}

/// A static iOS-style navigation bar: `leading` / `middle` / `trailing`
/// slots on a 44pt-tall bar with a hairline bottom border, self-padded
/// against the top safe-area inset. Flutter parity: `CupertinoNavigationBar`
/// (`nav_bar.dart`, oracle tag `3.44.0`, collapsed-static shape only) — see
/// the module docs for exactly what is and is not ported.
///
/// ```
/// use flui_cupertino::CupertinoNavigationBar;
/// use flui_widgets::Text;
///
/// let _bar = CupertinoNavigationBar::new().middle(Text::new("Settings"));
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoNavigationBar {
    leading: Option<BoxedView>,
    middle: Option<BoxedView>,
    trailing: Option<BoxedView>,
    background_color: Option<CupertinoColor>,
    border: Option<Border<flui_types::geometry::Pixels>>,
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

    /// Sets the leading slot — Flutter parity: `CupertinoNavigationBar.leading`.
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(leading.into_view().boxed());
        self
    }

    /// Sets the middle (title) slot, true-centered across the bar's full
    /// width — see the module docs' layout note. Flutter parity:
    /// `CupertinoNavigationBar.middle`.
    #[must_use]
    pub fn middle(mut self, middle: impl IntoView) -> Self {
        self.middle = Some(middle.into_view().boxed());
        self
    }

    /// Sets the trailing slot — Flutter parity: `CupertinoNavigationBar.trailing`.
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
    /// the hairline border — see the module docs' divergence note. Flutter
    /// parity: `CupertinoNavigationBar.border`.
    #[must_use]
    pub fn border(mut self, border: Option<Border<flui_types::geometry::Pixels>>) -> Self {
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
        let top_inset = MediaQuery::maybe_of(ctx).map_or(px(0.0), |data| data.padding.top);

        let mut layers: Vec<BoxedView> = Vec::new();
        if let Some(middle) = &self.middle {
            let styled_middle = DefaultTextStyle::new(
                theme.text_theme().nav_title_text_style(),
                Semantics::new().header(true).child(middle.clone()),
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
                .child(SafeArea::new().bottom(false).child(toolbar));

        SizedBox::height(NAV_BAR_PERSISTENT_HEIGHT + top_inset.get()).child(decorated)
    }
}

impl PreferredSizeView for CupertinoNavigationBar {
    fn preferred_size(&self) -> Size {
        // `CupertinoNavigationBar.preferredSize` (`nav_bar.dart`, oracle tag
        // `3.44.0`), minus the `bottom`/`largeTitle` height contributions
        // (both deferred — see the module docs).
        Size::new(px(f32::INFINITY), px(NAV_BAR_PERSISTENT_HEIGHT))
    }
}
