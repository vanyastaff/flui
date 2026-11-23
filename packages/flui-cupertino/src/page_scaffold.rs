//! The single-page layout: a navigation bar slot above content.

use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::styling::BoxDecoration;
use flui_sdk::view::BoxedView;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{DecoratedBox, Padding, Positioned, PreferredSizeView, Stack};

use crate::CupertinoColor;
use crate::CupertinoTheme;

/// A single iOS page's layout: an optional
/// [`CupertinoNavigationBar`](crate::CupertinoNavigationBar) (or any other
/// [`PreferredSizeView`]) on top and `child` below it.
///
/// - The content is a `Stack` layer under the navigation bar, which is
///   `Positioned` on top; there is no layout delegate and no
///   floating-action-button slot.
/// - With a navigation bar, the content is pushed down by the bar's
///   [`preferred_size`](PreferredSizeView::preferred_size) height plus the
///   ambient `MediaQuery` `padding.top`. The scaffold removes that top
///   padding from the `MediaQuery` it publishes to the content.
/// - [`resize_to_avoid_bottom_inset`](Self::resize_to_avoid_bottom_inset)
///   (default `true`) moves `MediaQuery` `view_insets.bottom` into the
///   content's bottom padding and removes it from the `MediaQuery` published
///   to descendants, so it is not applied twice.
/// - The background is a `DecoratedBox` filled with `background_color`, or
///   [`CupertinoThemeData::scaffold_background_color`](crate::CupertinoThemeData::scaffold_background_color).
///
/// # Not implemented
///
/// - The scaffold always treats the navigation bar as opaque: it consumes
///   all of `padding.top`, because `PreferredSizeView` carries no
///   `should_fully_obstruct`. Content never slides under a translucent bar.
/// - It does not publish its resolved background for a navigation bar to
///   lerp toward; the bar has no automatic background visibility.
/// - Tapping the status bar does not scroll to the top.
/// - It does not suppress text scaling on the navigation bar, because
///   `MediaQueryData` has no no-scaling variant.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/page_scaffold.dart`.
///
/// ```
/// use flui_cupertino::{CupertinoNavigationBar, CupertinoPageScaffold};
/// use flui_sdk::widgets::{SizedBox, Text};
///
/// let _page = CupertinoPageScaffold::new(SizedBox::shrink())
///     .navigation_bar(CupertinoNavigationBar::new().middle(Text::new("Settings")));
/// ```
#[derive(Clone, StatelessView)]
pub struct CupertinoPageScaffold {
    navigation_bar: Option<BoxedView>,
    navigation_bar_preferred_height: f64,
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
    /// ambient top inset).
    #[must_use]
    pub fn navigation_bar(mut self, navigation_bar: impl PreferredSizeView) -> Self {
        self.navigation_bar_preferred_height = navigation_bar.preferred_size().height;
        self.navigation_bar = Some(navigation_bar.boxed());
        self
    }

    /// Overrides the resolved background. Defaults to
    /// [`crate::CupertinoThemeData::scaffold_background_color`].
    #[must_use]
    pub fn background_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.background_color = Some(color.into());
        self
    }

    /// Whether `child` should size itself to avoid the window's bottom
    /// inset (e.g. an on-screen keyboard). Defaults to `true`.
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
            // The bar's preferred height plus the ambient top inset: always
            // the "fully obstructing" branch.
            let top_padding = self.navigation_bar_preferred_height + media.padding().top;
            let bottom_padding = if self.resize_to_avoid_bottom_inset {
                media.view_insets.bottom
            } else {
                0.0
            };
            let mut reduced = media;
            reduced.remove_top_padding();
            if self.resize_to_avoid_bottom_inset {
                reduced.remove_bottom_view_inset();
            }
            MediaQuery::new(
                reduced,
                Padding::new(EdgeInsets::new(top_padding, 0.0, bottom_padding, 0.0))
                    .child(self.child.clone()),
            )
            .boxed()
        } else if self.resize_to_avoid_bottom_inset {
            let mut reduced = media.clone();
            reduced.remove_bottom_view_inset();
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
