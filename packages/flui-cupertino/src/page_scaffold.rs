//! The single-page layout: a navigation bar slot above content, and the
//! obstruction answer a bar gives the scaffold.

use std::any::Any;

use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::styling::BoxDecoration;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, View};
use flui_sdk::widgets::{DecoratedBox, Padding, Positioned, PreferredSizeView, Stack};

use crate::CupertinoColor;
use crate::CupertinoTheme;

/// A bar with a preferred size that says whether it hides what lies behind
/// it: [`CupertinoPageScaffold`] moves content out from under a bar that
/// fully obstructs it, and lets content run under one that does not.
///
/// [`CupertinoNavigationBar`](crate::CupertinoNavigationBar) and
/// [`CupertinoTabBar`](crate::CupertinoTabBar) implement it: each fully
/// obstructs when its background, resolved for the context, is fully opaque.
///
/// Flutter reference: `ObstructingPreferredSizeWidget`
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/page_scaffold.dart:278`).
pub trait ObstructingPreferredSizeView: PreferredSizeView {
    /// Whether this view, built under `ctx`, fully hides what lies behind its
    /// [`preferred_size`](PreferredSizeView::preferred_size). `false` means
    /// content behind it shows through, as under a translucent bar.
    fn should_fully_obstruct(&self, ctx: &dyn BuildContext) -> bool;
}

/// The navigation bar a scaffold holds: the bar's view, its preferred
/// height, and its obstruction answer reached through that same view.
///
/// The bar is erased into a `BoxedView` for the tree, so the answer is a
/// function monomorphised for the bar's type that finds the bar again in that
/// view: one copy of the bar, read for the context of each build.
#[derive(Clone)]
struct ScaffoldNavigationBar {
    view: BoxedView,
    preferred_height: f64,
    should_fully_obstruct: fn(&dyn View, &dyn BuildContext) -> bool,
}

impl ScaffoldNavigationBar {
    fn new<B: ObstructingPreferredSizeView>(bar: B) -> Self {
        Self {
            preferred_height: bar.preferred_size().height,
            view: bar.boxed(),
            should_fully_obstruct: should_fully_obstruct::<B>,
        }
    }

    fn should_fully_obstruct(&self, ctx: &dyn BuildContext) -> bool {
        (self.should_fully_obstruct)(self.view.as_view(), ctx)
    }
}

/// `B`'s answer for the bar inside `view`.
fn should_fully_obstruct<B: ObstructingPreferredSizeView>(
    view: &dyn View,
    ctx: &dyn BuildContext,
) -> bool {
    let view: &dyn Any = view;
    view.downcast_ref::<B>()
        .expect("BUG: a ScaffoldNavigationBar's view holds the bar its function was made for")
        .should_fully_obstruct(ctx)
}

/// A single iOS page's layout: an optional
/// [`CupertinoNavigationBar`](crate::CupertinoNavigationBar) (or any other
/// [`ObstructingPreferredSizeView`]) on top and `child` below it.
///
/// - The content is a `Stack` layer under the navigation bar, which is
///   `Positioned` on top; there is no layout delegate and no
///   floating-action-button slot.
/// - The obstructed height is the bar's
///   [`preferred_size`](PreferredSizeView::preferred_size) height plus the
///   ambient `MediaQuery` `padding.top`. The scaffold asks the bar
///   [`should_fully_obstruct`](ObstructingPreferredSizeView::should_fully_obstruct)
///   on each build:
///   - a bar that fully obstructs pushes the content down by that height, and
///     the `MediaQuery` the content reads has no top padding left;
///   - a bar that does not (the default, translucent navigation bar) leaves
///     the content at the top, under the bar, and the `MediaQuery` the content
///     reads has `padding.top` equal to that height, so content can keep its
///     own material clear of the bar.
/// - [`resize_to_avoid_bottom_inset`](Self::resize_to_avoid_bottom_inset)
///   (default `true`) moves `MediaQuery` `view_insets.bottom` into the
///   content's bottom padding and removes it from the `MediaQuery` published
///   to descendants, so it is not applied twice.
/// - The background is a `DecoratedBox` filled with `background_color`, or
///   [`CupertinoThemeData::scaffold_background_color`](crate::CupertinoThemeData::scaffold_background_color).
///
/// # Not implemented
///
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
    navigation_bar: Option<ScaffoldNavigationBar>,
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
            background_color: None,
            resize_to_avoid_bottom_inset: true,
            child: child.into_view().boxed(),
        }
    }

    /// Sets the navigation bar slot, drawn at the top of the screen. Whether
    /// `child` is shifted below it or runs under it is the bar's
    /// [`should_fully_obstruct`](ObstructingPreferredSizeView::should_fully_obstruct)
    /// answer.
    #[must_use]
    pub fn navigation_bar(mut self, navigation_bar: impl ObstructingPreferredSizeView) -> Self {
        self.navigation_bar = Some(ScaffoldNavigationBar::new(navigation_bar));
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
        let background = self.background_color.map_or_else(
            || CupertinoTheme::of(ctx).scaffold_background_color,
            |color| color.resolve(ctx),
        );
        let media = MediaQuery::maybe_of(ctx).unwrap_or_default();

        let padded_content = if let Some(navigation_bar) = &self.navigation_bar {
            let top_padding = navigation_bar.preferred_height + media.padding().top;
            let bottom_padding = if self.resize_to_avoid_bottom_inset {
                media.view_insets.bottom
            } else {
                0.0
            };
            let mut reduced = media;
            if self.resize_to_avoid_bottom_inset {
                reduced.remove_bottom_view_inset();
            }
            let content_top = if navigation_bar.should_fully_obstruct(ctx) {
                // The bar hides what is behind it: the content starts below
                // the bar, which consumes the top padding.
                reduced.remove_top_padding();
                top_padding
            } else {
                // The content runs under the bar and reads the obstructed
                // height as its top padding: `view_padding.top` is raised
                // above the covered inset so `padding().top` is exactly it.
                reduced.view_padding.top = reduced.view_insets.top + top_padding;
                0.0
            };
            MediaQuery::new(
                reduced,
                Padding::new(EdgeInsets::new(content_top, 0.0, bottom_padding, 0.0))
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
                Positioned::new(navigation_bar.view.clone())
                    .top(0.0)
                    .left(0.0)
                    .right(0.0)
                    .boxed(),
            );
        }

        DecoratedBox::new(BoxDecoration::with_color(background)).child(Stack::new(layers))
    }
}
