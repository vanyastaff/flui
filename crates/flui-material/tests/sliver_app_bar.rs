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

mod common;
use common::{lay_out, tight};
use flui_material::{SliverAppBar, Theme, ThemeData};
use flui_view::BoxedView;
use flui_view::IntoView;
use flui_view::view::ViewExt;
use flui_widgets::{
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
fn scroll_view_at(offset: f32, bar: SliverAppBar) -> Theme {
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
fn header_child_height(laid: &common::LaidOut, render_type: &str) -> f32 {
    let header = laid
        .find_by_render_type(render_type)
        .unwrap_or_else(|| panic!("a {render_type} must be in the tree"));
    laid.size(laid.only_child(header)).height.get()
}

/// Fully scrolled to the top, an expanded bar's box is its max extent — and
/// the real `AppBar` toolbar was built through the seam, in the same frame.
#[test]
fn an_expanded_bar_opens_at_its_expanded_height() {
    let bar = SliverAppBar::new()
        .title(Text::new("FLUI"))
        .expanded_height(200.0)
        .pinned(true);

    let mut laid = lay_out(scroll_view_at(0.0, bar), tight(400.0, 600.0));

    assert_eq!(
        header_child_height(&laid, "RenderSliverPinnedPersistentHeader"),
        200.0,
        "at scroll offset 0 the bar fills its expanded height"
    );
    assert_eq!(
        laid.count_elements_by_view_type::<flui_material::AppBar>(),
        1,
        "the delegate must have built the real AppBar through the seam"
    );
}

/// Scrolled deep, a pinned bar collapses to — and holds — its collapsed
/// extent (the toolbar height, with no bottom and no inset).
#[test]
fn a_pinned_bar_holds_its_collapsed_height_at_deep_scroll() {
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

/// The un-pinned variant maps to the scrolling render object and scrolls
/// away entirely — the flag choice reaches the render layer.
#[test]
fn an_unpinned_bar_uses_the_scrolling_variant() {
    let bar = SliverAppBar::new().title(Text::new("FLUI"));

    let laid = lay_out(scroll_view_at(0.0, bar), tight(400.0, 600.0));

    assert!(
        laid.find_by_render_type("RenderSliverScrollingPersistentHeader")
            .is_some(),
        "pinned(false)/floating(false) must select the scrolling variant"
    );
    assert!(
        laid.find_by_render_type("RenderSliverPinnedPersistentHeader")
            .is_none(),
        "no pinned render object may exist for an unpinned bar"
    );
}

/// `flexible_space` fills the bar's expanded box AND paints inside the
/// bar's own Material surface — behind the toolbar, above the background.
/// The wrong-but-plausible composition (stacking the flexible content as a
/// SIBLING under the whole AppBar) hides it beneath the opaque surface and
/// leaves the Material sized to the toolbar strip instead of the box.
#[test]
fn flexible_space_fills_the_expanded_box_inside_the_material_surface() {
    let bar = SliverAppBar::new()
        .title(Text::new("FLUI"))
        .expanded_height(180.0)
        .pinned(true)
        .flexible_space(flui_widgets::ColoredBox::new(flui_types::Color::rgb(
            10, 20, 30,
        )));

    let laid = lay_out(scroll_view_at(0.0, bar), tight(400.0, 600.0));

    let fill = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the flexible space's render object is in the tree");
    assert_eq!(
        laid.size(fill).height.get(),
        180.0,
        "the flexible space must fill the bar's current extent"
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("the bar's Material surface is in the tree");
    assert_eq!(
        laid.size(material).height.get(),
        180.0,
        "the Material surface must cover the whole expanded box, not just          the toolbar strip"
    );

    // Structural pin of the paint order: the flexible space is a DESCENDANT
    // of the Material, so the opaque background is behind it by
    // construction. A sibling composition (the bug) fails this.
    fn subtree_contains(
        laid: &common::LaidOut,
        root: flui_foundation::RenderId,
        needle: flui_foundation::RenderId,
    ) -> bool {
        if root == needle {
            return true;
        }
        laid.children(root)
            .into_iter()
            .any(|child| subtree_contains(laid, child, needle))
    }
    assert!(
        subtree_contains(&laid, material, fill),
        "the flexible space must paint INSIDE the bar's Material surface"
    );
}
