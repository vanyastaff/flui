//! [`CupertinoPageScaffold`] — a single iOS page's layout: a navigation bar
//! slot on top, content below it.
//!
//! Flutter parity: `cupertino/page_scaffold.dart`'s `CupertinoPageScaffold`
//! (oracle tag `3.44.0`). See "Deferred, named" below for what this V1 does
//! not carry over.
//!
//! ## What this ports
//!
//! - The `Stack` composition itself: content first, the navigation bar
//!   `Positioned` on top — `_CupertinoPageScaffoldState.build`'s own shape,
//!   not a `CustomMultiChildLayoutDelegate` (the oracle doesn't use one
//!   either; `flui_material::Scaffold`'s is a different, busier widget with
//!   a floating-action-button slot this one has no equivalent of).
//! - The content padding contract: when a navigation bar is present, the
//!   content is pushed down by exactly
//!   [`preferred_size`](PreferredSizeView::preferred_size)'s height *plus*
//!   the ambient `MediaQuery.padding.top` — mirroring
//!   `flui_material::Scaffold`'s own "the app bar slot's cap is
//!   `preferred_height + padding.top`, but `content_top` reads the app bar's
//!   *measured* height only" contract, which this scaffold reproduces with
//!   `Padding` instead of a layout delegate because there is no second slot
//!   (a floating action button, a bottom nav bar) competing for the same
//!   measured geometry.
//! - `resize_to_avoid_bottom_inset` (default `true`): consumes
//!   `MediaQuery.view_insets.bottom` into the content's own bottom padding
//!   and zeroes it out of the `MediaQuery` republished to descendants — the
//!   same "reduce, don't just pass through" contract `Scaffold`'s module
//!   docs describe for its own body slot.
//! - The scaffold background: `DecoratedBox` filled with `background_color`,
//!   falling back to [`crate::CupertinoThemeData::scaffold_background_color`].
//!
//! ## Deferred, named
//!
//! - **`CupertinoPageScaffoldBackgroundColor`** (the `InheritedWidget`
//!   publishing this scaffold's resolved background so a descendant
//!   `CupertinoNavigationBar` can lerp toward it under
//!   `automaticBackgroundVisibility`). Moot today:
//!   [`crate::CupertinoNavigationBar`] does not implement
//!   `automaticBackgroundVisibility` yet either (see that module's own
//!   deferred list) — nothing would consume the publish.
//! - **The nav bar never fully-obstructs-vs-translucent branches.** The
//!   oracle picks between two `MediaQuery` transforms depending on
//!   `navigationBar!.shouldFullyObstruct(context)` (opaque: fully consume
//!   `padding.top`; translucent: keep it, so content sliding underneath
//!   still avoids the notch). `flui_widgets::PreferredSizeView` carries no
//!   `should_fully_obstruct` — this V1 always takes the opaque branch,
//!   matching `flui_material::AppBar`/`Scaffold`'s own "no translucent
//!   content-behind-the-bar" contract in this same workspace.
//! - **Status-bar tap-to-scroll-to-top** (`_HitTestableAtOrigin`,
//!   `PrimaryScrollController.animateTo`). No `PrimaryScrollController`/
//!   `ScrollNotificationObserver` substrate in FLUI to wire this through.
//! - **Text-scaling suppression on the navigation bar**
//!   (`MediaQuery.withNoTextScaling`). `MediaQueryData` has no
//!   `TextScaler`/no-scaling variant to apply yet — `text_scale_factor`
//!   passes through unchanged.

use flui_types::geometry::{EdgeInsets, px};
use flui_types::styling::BoxDecoration;
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{DecoratedBox, MediaQuery, Padding, Positioned, PreferredSizeView, Stack};

use crate::colors::CupertinoColor;
use crate::theme::CupertinoTheme;

/// A single iOS page's layout: an optional [`crate::CupertinoNavigationBar`]
/// (or any other [`PreferredSizeView`]) on top, `child` below it. Flutter
/// parity: `CupertinoPageScaffold` (`page_scaffold.dart`, oracle tag
/// `3.44.0`) — see the module docs for exactly what is and is not ported.
///
/// ```
/// use flui_cupertino::{CupertinoNavigationBar, CupertinoPageScaffold};
/// use flui_widgets::{SizedBox, Text};
///
/// let _page = CupertinoPageScaffold::new(SizedBox::shrink())
///     .navigation_bar(CupertinoNavigationBar::new().middle(Text::new("Settings")));
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoPageScaffold {
    navigation_bar: Option<BoxedView>,
    navigation_bar_preferred_height: f32,
    background_color: Option<CupertinoColor>,
    resize_to_avoid_bottom_inset: bool,
    child: BoxedView,
}

impl CupertinoPageScaffold {
    /// A scaffold showing `child`, with no navigation bar, the theme's
    /// `scaffold_background_color`, and `resize_to_avoid_bottom_inset: true`.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            navigation_bar: None,
            navigation_bar_preferred_height: 0.0,
            background_color: None,
            resize_to_avoid_bottom_inset: true,
            child: child.into_view().boxed(),
        }
    }

    /// Sets the navigation bar slot, drawn at the top of the screen and
    /// shifting `child` down by its
    /// [`preferred_size`](PreferredSizeView::preferred_size) height (plus the
    /// ambient top inset). Flutter parity:
    /// `CupertinoPageScaffold.navigationBar`.
    #[must_use]
    pub fn navigation_bar(mut self, navigation_bar: impl PreferredSizeView) -> Self {
        self.navigation_bar_preferred_height = navigation_bar.preferred_size().height.get();
        self.navigation_bar = Some(navigation_bar.boxed());
        self
    }

    /// Overrides the resolved background. Defaults to
    /// [`crate::CupertinoThemeData::scaffold_background_color`]. Flutter
    /// parity: `CupertinoPageScaffold.backgroundColor`.
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Whether `child` should size itself to avoid the window's bottom
    /// inset (e.g. an on-screen keyboard). Defaults to `true`. Flutter
    /// parity: `CupertinoPageScaffold.resizeToAvoidBottomInset`.
    #[must_use]
    pub fn resize_to_avoid_bottom_inset(mut self, resize: bool) -> Self {
        self.resize_to_avoid_bottom_inset = resize;
        self
    }
}

impl std::fmt::Debug for CupertinoPageScaffold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoPageScaffold")
            .field("has_navigation_bar", &self.navigation_bar.is_some())
            .field(
                "resize_to_avoid_bottom_inset",
                &self.resize_to_avoid_bottom_inset,
            )
            .finish_non_exhaustive()
    }
}

impl StatelessView for CupertinoPageScaffold {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let background = self
            .background_color
            .unwrap_or_else(|| CupertinoTheme::of(ctx).scaffold_background_color())
            .resolve(ctx);
        let media = MediaQuery::maybe_of(ctx).unwrap_or_default();

        let padded_content = if self.navigation_bar.is_some() {
            // `topPadding = navigationBar!.preferredSize.height +
            // existingMediaQuery.padding.top` (`page_scaffold.dart`, oracle
            // tag `3.44.0`) — always the "fully obstructing" branch, see the
            // module docs' deferred list.
            let top_padding = px(self.navigation_bar_preferred_height) + media.padding.top;
            let bottom_padding = if self.resize_to_avoid_bottom_inset {
                media.view_insets.bottom
            } else {
                px(0.0)
            };
            let mut reduced = media.clone();
            reduced.padding.top = px(0.0);
            if self.resize_to_avoid_bottom_inset {
                reduced.view_insets.bottom = px(0.0);
            }
            MediaQuery::new(
                reduced,
                Padding::new(EdgeInsets::new(
                    top_padding,
                    px(0.0),
                    bottom_padding,
                    px(0.0),
                ))
                .child(self.child.clone()),
            )
            .boxed()
        } else if self.resize_to_avoid_bottom_inset {
            let mut reduced = media.clone();
            reduced.view_insets.bottom = px(0.0);
            MediaQuery::new(
                reduced,
                Padding::new(EdgeInsets::only_bottom(media.view_insets.bottom))
                    .child(self.child.clone()),
            )
            .boxed()
        } else {
            self.child.clone()
        };

        let mut layers: Vec<BoxedView> = vec![padded_content];
        if let Some(navigation_bar) = &self.navigation_bar {
            layers.push(
                Positioned::new(navigation_bar.clone())
                    .top(0.0)
                    .left(0.0)
                    .right(0.0)
                    .boxed(),
            );
        }

        DecoratedBox::new(BoxDecoration::with_color(background)).child(Stack::new(layers))
    }
}
