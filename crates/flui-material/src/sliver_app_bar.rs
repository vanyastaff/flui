//! [`SliverAppBar`] — a Material app bar that lives in a
//! [`CustomScrollView`](flui_widgets::CustomScrollView) and collapses,
//! pins, or floats as the user scrolls.
//!
//! # What this composes, and what it owns
//!
//! The toolbar itself — leading / title / actions / bottom, theming, and the
//! top safe-area inset — is [`AppBar`], reused whole. The scrolling box —
//! extent interpolation, pinning, floating re-reveal — is
//! [`SliverPersistentHeader`] over the four persistent-header render
//! objects. What this widget owns is exactly what Flutter's
//! `_SliverAppBarDelegate` owns: the extent arithmetic joining the two, and
//! the layering of `flexible_space` behind the toolbar.
//!
//! Flutter parity: `material/app_bar.dart` `SliverAppBar` /
//! `_SliverAppBarDelegate` (extent formulas cross-checked at
//! `app_bar.dart:1339-1345` and `:2092-2094`).
//!
//! # Deferred, deliberately
//!
//! - **Snap** — starting a snap animation needs a scroll-end trigger from
//!   the enclosing scrollable, which does not exist yet; see the
//!   persistent-header widget's own note. A `snap` flag here today would do
//!   nothing.
//! - **Stretch and `FlexibleSpaceBar`** — over-scroll stretch is reachable
//!   through a custom [`SliverPersistentHeaderDelegate`]'s
//!   `stretch_configuration`; surfacing it here without a
//!   `FlexibleSpaceBar` to consume the stretched box would stretch a blank
//!   region.
//! - **Title fade / collapse interpolation** — the collapse fraction is
//!   available to a delegate as `shrink_offset`; the `FlexibleSpaceBar`
//!   treatment of it is its own widget.

use flui_types::Color;
use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};
use flui_widgets::layout::PreferredSizeView;
use flui_widgets::{MediaQuery, SizedBox, SliverPersistentHeader, SliverPersistentHeaderDelegate};

use crate::AppBar;

/// A Material app bar for a scroll view: collapses from
/// [`expanded_height`](Self::expanded_height) to its toolbar as the user
/// scrolls, optionally [`pinned`](Self::pinned) at the top edge and/or
/// [`floating`](Self::floating) back into view on any upward scroll.
///
/// The toolbar slots (`leading`, `title`, `actions`, `bottom`, colors,
/// elevation) forward to an inner [`AppBar`] and behave identically to the
/// standalone widget, including its "consumes the top inset itself"
/// contract.
///
/// # Examples
///
/// ```rust
/// use flui_material::SliverAppBar;
/// use flui_widgets::Text;
///
/// let _bar = SliverAppBar::new()
///     .title(Text::new("FLUI"))
///     .expanded_height(200.0)
///     .pinned(true);
/// ```
#[derive(Clone, StatelessView)]
pub struct SliverAppBar {
    app_bar: AppBar,
    /// Mirrors of the inner app bar's height inputs, kept here because the
    /// extent arithmetic needs them and [`AppBar`] does not expose getters.
    toolbar_height: f32,
    bottom_height: f32,
    has_bottom: bool,
    expanded_height: Option<f32>,
    collapsed_height: Option<f32>,
    pinned: bool,
    floating: bool,
}

impl SliverAppBar {
    /// A sliver app bar with an empty toolbar, no expansion, neither pinned
    /// nor floating.
    #[must_use]
    pub fn new() -> Self {
        Self {
            app_bar: AppBar::new(),
            toolbar_height: crate::app_bar::DEFAULT_TOOLBAR_HEIGHT,
            bottom_height: 0.0,
            has_bottom: false,
            expanded_height: None,
            collapsed_height: None,
            pinned: false,
            floating: false,
        }
    }

    /// Sets the widget in the leading slot. See [`AppBar::leading`].
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.app_bar = self.app_bar.leading(leading);
        self
    }

    /// See [`AppBar::automatically_imply_leading`].
    #[must_use]
    pub fn automatically_imply_leading(mut self, imply: bool) -> Self {
        self.app_bar = self.app_bar.automatically_imply_leading(imply);
        self
    }

    /// Sets the title. See [`AppBar::title`].
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.app_bar = self.app_bar.title(title);
        self
    }

    /// Sets the trailing actions. See [`AppBar::actions`].
    #[must_use]
    pub fn actions(mut self, actions: Vec<BoxedView>) -> Self {
        self.app_bar = self.app_bar.actions(actions);
        self
    }

    /// Sets the toolbar height. See [`AppBar::toolbar_height`].
    #[must_use]
    pub fn toolbar_height(mut self, toolbar_height: f32) -> Self {
        self.toolbar_height = toolbar_height;
        self.app_bar = self.app_bar.toolbar_height(toolbar_height);
        self
    }

    /// Sets the background color. See [`AppBar::background_color`].
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.app_bar = self.app_bar.background_color(color);
        self
    }

    /// Sets the foreground color. See [`AppBar::foreground_color`].
    #[must_use]
    pub fn foreground_color(mut self, color: Color) -> Self {
        self.app_bar = self.app_bar.foreground_color(color);
        self
    }

    /// Sets the elevation. See [`AppBar::elevation`].
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.app_bar = self.app_bar.elevation(elevation);
        self
    }

    /// Sets the bottom widget (a tab bar, say). See [`AppBar::bottom`]; its
    /// preferred height joins the extent arithmetic exactly as Flutter's
    /// `_bottomHeight` does.
    #[must_use]
    pub fn bottom(mut self, bottom: impl PreferredSizeView) -> Self {
        self.bottom_height = bottom.preferred_size().height.get();
        self.has_bottom = true;
        self.app_bar = self.app_bar.bottom(bottom);
        self
    }

    /// The height this app bar expands to when fully scrolled to the top,
    /// EXCLUDING the top safe-area inset — the ambient inset is added on
    /// top when the extents are computed, so a caller must not fold it in
    /// (that would double-count it).
    ///
    /// `None` (the default) means the bar is its toolbar (plus `bottom`) and
    /// never expands.
    #[must_use]
    pub fn expanded_height(mut self, expanded_height: f32) -> Self {
        self.expanded_height = Some(expanded_height);
        self
    }

    /// Overrides the height the bar collapses to, EXCLUDING the top
    /// safe-area inset and `bottom` — both are added on top when the
    /// extents are computed. Defaults to the toolbar height.
    #[must_use]
    pub fn collapsed_height(mut self, collapsed_height: f32) -> Self {
        self.collapsed_height = Some(collapsed_height);
        self
    }

    /// A widget painted behind the toolbar — above the bar's own
    /// [`Material`](crate::Material) surface, below its content, filling
    /// the bar's current extent: the canvas a cover image or gradient
    /// collapses with. Forwards to [`AppBar::flexible_space`].
    #[must_use]
    pub fn flexible_space(mut self, flexible_space: impl IntoView) -> Self {
        self.app_bar = self.app_bar.flexible_space(flexible_space);
        self
    }

    /// Keep the collapsed bar visible at the top edge instead of letting it
    /// scroll away.
    #[must_use]
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Re-reveal the bar as soon as the user scrolls toward the start.
    #[must_use]
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }
}

impl Default for SliverAppBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SliverAppBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverAppBar")
            .field("toolbar_height", &self.toolbar_height)
            .field("expanded_height", &self.expanded_height)
            .field("pinned", &self.pinned)
            .field("floating", &self.floating)
            .finish_non_exhaustive()
    }
}

/// The extent pair for a sliver app bar, in Flutter's own formulas.
///
/// `app_bar.dart:2092-2094`: collapsed = (collapsedHeight ?? toolbarHeight)
/// plus bottomHeight plus topPadding — except pinned + floating + a bottom,
/// where the toolbar contribution drops to `collapsedHeight ?? 0.0` (the
/// toolbar floats away and only the bottom stays pinned).
/// `app_bar.dart:1339-1345`: min = collapsed; max = max(topPadding +
/// (expandedHeight ?? toolbar + bottom), min).
struct ExtentInputs {
    toolbar_height: f32,
    bottom_height: f32,
    has_bottom: bool,
    expanded_height: Option<f32>,
    collapsed_height: Option<f32>,
    pinned: bool,
    floating: bool,
    top_padding: f32,
}

fn extents(inputs: &ExtentInputs) -> (f32, f32) {
    let collapsed = if inputs.pinned && inputs.floating && inputs.has_bottom {
        inputs.collapsed_height.unwrap_or(0.0) + inputs.bottom_height + inputs.top_padding
    } else {
        inputs.collapsed_height.unwrap_or(inputs.toolbar_height)
            + inputs.bottom_height
            + inputs.top_padding
    };
    let expanded = inputs.top_padding
        + inputs
            .expanded_height
            .unwrap_or(inputs.toolbar_height + inputs.bottom_height);
    (collapsed, expanded.max(collapsed))
}

/// The [`SliverPersistentHeaderDelegate`] a [`SliverAppBar`] resolves to.
///
/// Extents are resolved numbers by the time this exists — the top inset is
/// read at widget build time (the delegate's extent getters are
/// context-free, so Flutter snapshots `topPadding` the same way).
struct SliverAppBarDelegate {
    app_bar: AppBar,
    min_extent: f32,
    max_extent: f32,
}

impl SliverPersistentHeaderDelegate for SliverAppBarDelegate {
    fn build(
        &self,
        _ctx: &dyn BuildContext,
        _shrink_offset: f32,
        _overlaps_content: bool,
    ) -> BoxedView {
        // Expand to the header's current box: the bar's Material covers the
        // whole expanded area (Flutter's SliverAppBar paints its background
        // across it), with the flexible space and toolbar layered INSIDE
        // that surface by the AppBar itself — the slot order that keeps the
        // opaque background behind the flexible content, never over it.
        SizedBox::expand()
            .child(self.app_bar.clone())
            .into_view()
            .boxed()
    }

    fn min_extent(&self) -> f32 {
        self.min_extent
    }

    fn max_extent(&self) -> f32 {
        self.max_extent
    }
}

impl StatelessView for SliverAppBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // Snapshot the inset at build time — the delegate's extent getters
        // are context-free, exactly why Flutter passes `topPadding` in.
        let top_padding = MediaQuery::maybe_of(ctx).map_or(0.0, |data| data.padding.top.get());
        let (min_extent, max_extent) = extents(&ExtentInputs {
            toolbar_height: self.toolbar_height,
            bottom_height: self.bottom_height,
            has_bottom: self.has_bottom,
            expanded_height: self.expanded_height,
            collapsed_height: self.collapsed_height,
            pinned: self.pinned,
            floating: self.floating,
            top_padding,
        });

        SliverPersistentHeader::new(SliverAppBarDelegate {
            app_bar: self.app_bar.clone(),
            min_extent,
            max_extent,
        })
        .pinned(self.pinned)
        .floating(self.floating)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flutter's default-shape oracle: no expansion, no bottom, no inset —
    /// the bar is its toolbar in both extents.
    #[test]
    fn extents_default_to_the_toolbar() {
        assert_eq!(
            extents(&ExtentInputs {
                toolbar_height: 56.0,
                bottom_height: 0.0,
                has_bottom: false,
                expanded_height: None,
                collapsed_height: None,
                pinned: false,
                floating: false,
                top_padding: 0.0
            }),
            (56.0, 56.0)
        );
    }

    /// `maxExtent = max(topPadding + expandedHeight, minExtent)` — an
    /// expanded height below the collapsed height must not invert the range.
    #[test]
    fn a_too_small_expanded_height_clamps_to_collapsed() {
        assert_eq!(
            extents(&ExtentInputs {
                toolbar_height: 56.0,
                bottom_height: 48.0,
                has_bottom: true,
                expanded_height: Some(80.0),
                collapsed_height: None,
                pinned: true,
                floating: false,
                top_padding: 0.0
            }),
            (104.0, 104.0)
        );
    }

    /// The pinned + floating + bottom special case: the toolbar floats away
    /// and only the bottom (plus any explicit collapsed height) stays —
    /// `app_bar.dart:2092-2093`.
    #[test]
    fn pinned_floating_with_bottom_drops_the_toolbar_from_collapsed() {
        assert_eq!(
            extents(&ExtentInputs {
                toolbar_height: 56.0,
                bottom_height: 48.0,
                has_bottom: true,
                expanded_height: Some(200.0),
                collapsed_height: None,
                pinned: true,
                floating: true,
                top_padding: 0.0
            }),
            (48.0, 200.0)
        );
        // ...and the ordinary pinned case keeps it.
        assert_eq!(
            extents(&ExtentInputs {
                toolbar_height: 56.0,
                bottom_height: 48.0,
                has_bottom: true,
                expanded_height: Some(200.0),
                collapsed_height: None,
                pinned: true,
                floating: false,
                top_padding: 0.0
            }),
            (104.0, 200.0)
        );
    }

    /// The top inset joins BOTH extents — a bar under a status bar is taller
    /// collapsed and taller expanded by the same amount.
    #[test]
    fn top_padding_raises_both_extents() {
        assert_eq!(
            extents(&ExtentInputs {
                toolbar_height: 56.0,
                bottom_height: 0.0,
                has_bottom: false,
                expanded_height: Some(200.0),
                collapsed_height: None,
                pinned: true,
                floating: false,
                top_padding: 24.0
            }),
            (80.0, 224.0)
        );
    }
}
