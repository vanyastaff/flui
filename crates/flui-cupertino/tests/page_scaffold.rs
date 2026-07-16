//! Integration tests for [`CupertinoPageScaffold`] — the background
//! default, the navigation-bar content-padding contract (bar height + top
//! inset), `resize_to_avoid_bottom_inset`, and that a navigation bar
//! actually mounts as an overlay.

mod common;

use common::{LaidOut, lay_out, tight};
use flui_cupertino::{CupertinoNavigationBar, CupertinoPageScaffold};
use flui_foundation::RenderId;
use flui_types::geometry::px;
use flui_widgets::prelude::EdgeInsets;
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

/// The unique `RenderConstrainedBox` sized exactly `width x height` — used
/// to locate the content marker unambiguously alongside the navigation
/// bar's own (differently-sized) `SizedBox`.
fn find_by_size(laid: &LaidOut, width: f32, height: f32) -> RenderId {
    laid.find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .find(|&id| {
            let size = laid.size(id);
            (size.width.get() - width).abs() < 0.01 && (size.height.get() - height).abs() < 0.01
        })
        .unwrap_or_else(|| panic!("no RenderConstrainedBox sized {width}x{height}"))
}

/// `_kDefaultTheme`'s `scaffoldBackgroundColor` fallback,
/// `CupertinoColors.systemBackground`'s light variant: opaque white.
#[test]
fn background_defaults_to_the_themes_system_background_color() {
    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0)),
        ),
        tight(400.0, 600.0),
    );
    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the scaffold always paints its background via DecoratedBox");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        decoration.contains("r: 255, g: 255, b: 255, a: 255"),
        "must resolve systemBackground's light (opaque white) variant by default: {decoration}"
    );
}

/// With a navigation bar present, content is pushed down by exactly
/// `preferred_size().height + MediaQuery.padding.top` —
/// `page_scaffold.dart`'s `topPadding` (oracle tag `3.44.0`).
///
/// Red-check: drop `+ media.padding.top` from `top_padding`'s computation in
/// `page_scaffold.rs` — this test's offset assertion fails (would read
/// `44.0` instead of `64.0`).
#[test]
fn content_is_padded_below_the_nav_bar_plus_the_top_inset() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(px(20.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        MediaQuery::new(
            media,
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0))
                .navigation_bar(CupertinoNavigationBar::new()),
        ),
        tight(400.0, 600.0),
    );

    let content = find_by_size(&laid, 60.0, 30.0);
    let offset = laid.absolute_offset(content);
    assert!(
        (offset.dy.get() - 64.0).abs() < 0.01,
        "44.0 nav bar height + 20.0 top inset must push content to y=64.0: {offset:?}"
    );
}

/// With no navigation bar, content sits flush at the top — no padding is
/// added on its behalf.
#[test]
fn content_is_unpadded_with_no_navigation_bar() {
    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0)),
        ),
        tight(400.0, 600.0),
    );

    let content = find_by_size(&laid, 60.0, 30.0);
    let offset = laid.absolute_offset(content);
    assert!(
        offset.dy.get().abs() < 0.01,
        "no navigation bar means no top padding: {offset:?}"
    );
}

/// `resize_to_avoid_bottom_inset` (default `true`) reserves the bottom view
/// inset (e.g. an on-screen keyboard) as bottom padding around content;
/// `false` applies none, letting content extend under the inset. Read off
/// the mounted `RenderPadding`'s own `padding` diagnostic — a `Padding`'s
/// bottom value moves no offset (it is a *bottom*-only inset), so this
/// checks the applied inset directly rather than a position.
///
/// Red-check: hardcode `resize_to_avoid_bottom_inset` to always `true`
/// inside `build` — this test's `disabled` branch assertion fails (the
/// padding would still carry `bottom: 300px` either way).
#[test]
fn resize_to_avoid_bottom_inset_toggles_the_bottom_padding() {
    let media = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };

    let enabled = lay_out(
        MediaQuery::new(
            media.clone(),
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0)),
        ),
        tight(400.0, 600.0),
    );
    let padding_box = enabled
        .find_by_render_type("RenderPadding")
        .expect("resize_to_avoid_bottom_inset always wraps content in a Padding");
    let padding = enabled
        .render_property(padding_box, "padding")
        .expect("RenderPadding always reports its padding");
    assert!(
        padding.contains("bottom: 300px"),
        "resize_to_avoid_bottom_inset defaults true: the 300px keyboard inset must become \
         bottom padding: {padding}"
    );

    let disabled = lay_out(
        MediaQuery::new(
            media,
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0))
                .resize_to_avoid_bottom_inset(false),
        ),
        tight(400.0, 600.0),
    );
    assert!(
        disabled.find_by_render_type("RenderPadding").is_none(),
        "resize_to_avoid_bottom_inset(false) with no navigation bar must not wrap content \
         in a Padding at all"
    );
}

/// A set navigation bar actually mounts as a `Positioned` overlay, not just
/// contributing to the content's padding math.
#[test]
fn the_navigation_bar_mounts_as_an_overlay() {
    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoPageScaffold::new(SizedBox::new(60.0, 30.0))
                .navigation_bar(CupertinoNavigationBar::new()),
        ),
        tight(400.0, 600.0),
    );

    // The nav bar's own SafeArea-wrapped SafeArea/Stack composition mounts a
    // second DecoratedBox (its own background/border), on top of the
    // scaffold's own background DecoratedBox.
    assert_eq!(
        laid.find_all_by_render_type("RenderDecoratedBox").len(),
        2,
        "the scaffold's own background plus the nav bar's own background/border"
    );
}
