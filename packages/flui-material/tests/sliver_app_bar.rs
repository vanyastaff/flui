//! `SliverAppBar` widget-level integration coverage — mounts a real
//! collapsing app bar through the full render pipeline (`tests/common/
//! mod.rs`, matching `tests/app_bar.rs`'s established pattern).
//!
//! The extent arithmetic is unit-tested against Flutter's formulas in
//! `sliver_app_bar.rs` itself; what only a mount can prove is the
//! composition: the delegate builds the real `AppBar` through the
//! build-during-layout seam, the header's box tracks the computed extents
//! as the scroll offset changes, and `pinned` actually holds the collapsed
//! bar on screen.

use crate::common;
use common::{lay_out, tight};
use flui_material::{SliverAppBar, Theme, ThemeData};
use flui_sdk::view::BoxedView;
use flui_sdk::view::IntoView;
use flui_sdk::view::view::ViewExt;
use flui_sdk::widgets::{
    CustomScrollView, MediaQuery, MediaQueryData, SizedBox, SliverToBoxAdapter, Text,
};

fn trailing_content() -> BoxedView {
    SliverToBoxAdapter::new()
        .child(SizedBox::new(400.0, 800.0))
        .into_view()
        .boxed()
}

/// The `AppBar` inside the delegate resolves `Theme::of` and (via
/// `SafeArea`) `MediaQuery::of`, both of which panic with no ancestor —
/// same provisioning as `tests/app_bar.rs`, with a zero-inset media query
/// so the extent numbers stay the bare formulas.
fn scroll_view_at(offset: f64, bar: SliverAppBar) -> Theme {
    Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            MediaQueryData::default(),
            CustomScrollView::new((bar, trailing_content())).offset(offset),
        ),
    )
}

/// The header's current main-axis box, read from the delegate-built child
/// (the child is laid out to the header's layout extent every pass).
fn header_child_height(laid: &common::LaidOut, render_type: &str) -> f64 {
    let header = laid
        .try_find_by_render_type(render_type)
        .unwrap_or_else(|| panic!("a {render_type} must be in the tree"));
    laid.size(laid.only_child(header)).height
}

/// Scrolled deep, a pinned bar collapses to — and holds — its collapsed
/// extent (the toolbar height, with no bottom and no inset).
pub fn a_pinned_bar_holds_its_collapsed_height_at_deep_scroll() {
    let bar = SliverAppBar::new()
        .title(Text::new("FLUI"))
        .expanded_height(200.0)
        .pinned(true);

    let mut laid = lay_out(scroll_view_at(0.0, bar.clone()), tight(400.0, 600.0));
    laid.pump_widget(scroll_view_at(500.0, bar));

    assert_eq!(
        header_child_height(&laid, "RenderSliverPinnedPersistentHeader"),
        56.0,
        "a pinned bar's box collapses to the toolbar height and stays"
    );
}
