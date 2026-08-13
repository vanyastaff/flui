//! `FlexibleSpaceBar` integration coverage — mounts a real collapsing
//! `SliverAppBar` with a flexible space through the full pipeline
//! (`tests/common/mod.rs`, the `tests/sliver_app_bar.rs` pattern) and pins
//! the two observables the oracle's formulas produce: the background fades
//! as the bar collapses, and it parallaxes upward at a quarter of the
//! collapse distance.
//!
//! The formulas themselves (`t`, the fade window) are unit-tested
//! value-exact in `flexible_space_bar.rs`; what only a mount can prove is
//! the plumbing — the delegate republishes `FlexibleSpaceBarSettings` on
//! every seam rebuild, and the widget re-interpolates from it.

mod common;
use common::{lay_out, tight};
use flui_material::{FlexibleSpaceBar, SliverAppBar, Theme, ThemeData};
use flui_view::BoxedView;
use flui_view::IntoView;
use flui_view::view::ViewExt;
use flui_widgets::{
    ColoredBox, CustomScrollView, MediaQuery, MediaQueryData, SizedBox, SliverToBoxAdapter, Text,
};

fn trailing_content() -> BoxedView {
    SliverToBoxAdapter::new()
        .child(SizedBox::new(400.0, 800.0))
        .into_view()
        .boxed()
}

fn bar_at(offset: f32) -> Theme {
    let bar = SliverAppBar::new()
        .title(Text::new("FLUI"))
        .expanded_height(200.0)
        .pinned(true)
        .flexible_space(
            FlexibleSpaceBar::new().background(ColoredBox::new(flui_types::Color::rgb(10, 20, 30))),
        );
    Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            MediaQueryData::default(),
            CustomScrollView::new((bar, trailing_content())).offset(offset),
        ),
    )
}

/// Expanded (`t = 0`): the background is fully opaque — `RenderOpacity`'s
/// diagnostics hide the property at its 1.0 default, which is itself the
/// assertion — and sits at the bar's top (no parallax yet).
#[test]
fn expanded_background_is_opaque_and_unshifted() {
    let laid = lay_out(bar_at(0.0), tight(400.0, 600.0));

    let opacity = laid
        .find_by_render_type("RenderOpacity")
        .expect("the flexible space's fade wrapper is in the tree");
    assert_eq!(
        laid.render_property(opacity, "opacity").as_deref(),
        Some("1"),
        "expanded, the background must be fully opaque"
    );
    assert_eq!(
        laid.offset(opacity).dy.get(),
        0.0,
        "no parallax while fully expanded"
    );
}

/// Collapsed under a deep pinned scroll (`t = 1`): the background has faded
/// out entirely and drifted up by a quarter of the collapse distance —
/// `delta = 200 − 56 = 144`, so `top = −36`.
#[test]
fn collapsed_background_fades_out_and_parallaxes_up() {
    let mut laid = lay_out(bar_at(0.0), tight(400.0, 600.0));
    laid.pump_widget(bar_at(500.0));

    let opacity = laid
        .find_by_render_type("RenderOpacity")
        .expect("fade wrapper still in the tree");
    assert_eq!(
        laid.render_property(opacity, "opacity").as_deref(),
        Some("0"),
        "fully collapsed, the background must have faded to 0"
    );
    assert_eq!(
        laid.offset(opacity).dy.get(),
        -36.0,
        "CollapseMode::parallax drifts the background up by delta/4 \
         (144/4 = 36) at t = 1"
    );
}
