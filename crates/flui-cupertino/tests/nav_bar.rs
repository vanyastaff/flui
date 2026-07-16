//! Integration tests for [`CupertinoNavigationBar`] — the 44pt persistent
//! height contract, the hairline border's oracle-cited alpha, background
//! resolution against the theme, the top safe-area inset, and that
//! leading/middle/trailing all actually reach the mounted render tree.
//!
//! Every mount wraps the bar in a [`MediaQuery`] ancestor: `SafeArea`
//! (`nav_bar.rs`'s own self-padding, matching `flui_material::AppBar`'s
//! identical contract) reads `MediaQuery::of` unconditionally and panics
//! with no ancestor — see `flui-material/tests/app_bar.rs` for the same
//! precedent.

mod common;

use common::{lay_out, loose, tight};
use flui_cupertino::CupertinoNavigationBar;
use flui_types::Size;
use flui_types::geometry::px;
use flui_widgets::prelude::EdgeInsets;
use flui_widgets::{MediaQuery, MediaQueryData, PreferredSizeView, SizedBox, Text};

fn media_with_top_padding(top: f32) -> MediaQueryData {
    MediaQueryData {
        padding: EdgeInsets::new(px(top), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    }
}

/// `CupertinoNavigationBar.preferredSize` (`nav_bar.dart`, oracle tag
/// `3.44.0`): `Size.fromHeight(_kNavBarPersistentHeight)`, i.e. `44.0` — with
/// no `bottom`/`largeTitle` contribution (both deferred, see `nav_bar.rs`'s
/// module docs) and, critically, **no** `MediaQuery.padding.top` folded in
/// (that addition happens once, in `CupertinoPageScaffold`).
#[test]
fn preferred_size_is_the_44pt_persistent_height_with_no_top_inset_folded_in() {
    let preferred = CupertinoNavigationBar::new().preferred_size();
    assert_eq!(preferred, Size::new(px(f32::INFINITY), px(44.0)));
}

/// `_kDefaultNavBarBorderColor` (`nav_bar.dart`, oracle tag `3.44.0`) is
/// `Color(0x4D000000)` — alpha `0x4D` = `77` decimal. The default bar paints
/// it; `.border(None)` removes it entirely.
///
/// Red-check: hardcode `default_border()`'s alpha to any other byte — this
/// test's first assertion fails on the wrong number, not just "some border".
#[test]
fn default_hairline_border_carries_the_oracles_exact_alpha_and_border_none_removes_it() {
    let with_border = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    let decorated = with_border
        .find_by_render_type("RenderDecoratedBox")
        .expect("the bar always paints a DecoratedBox for its background/border");
    let decoration = with_border
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        decoration.contains("a: 77"),
        "the default border's color must carry the oracle's exact 0x4D (77) alpha: {decoration}"
    );

    let without_border = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new().border(None),
        ),
        tight(400.0, 44.0),
    );
    let decorated = without_border
        .find_by_render_type("RenderDecoratedBox")
        .expect("still paints a background even with no border");
    let decoration = without_border
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        !decoration.contains("a: 77"),
        "border(None) must remove the hairline entirely: {decoration}"
    );
}

/// `_kDefaultTheme.barBackgroundColor`'s light variant (`theme.rs`, ported
/// from `theme.dart`'s `_kDefaultTheme`): `Color.rgba(0xF9, 0xF9, 0xF9,
/// 0xF0)` = `(249, 249, 249, 240)`. No `CupertinoTheme` ancestor and a
/// default (light) `MediaQuery` resolves to this light default.
#[test]
fn background_defaults_to_the_themes_light_bar_background_color() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the bar paints its background via DecoratedBox");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        decoration.contains("r: 249, g: 249, b: 249, a: 240"),
        "must resolve the theme's light barBackgroundColor by default: {decoration}"
    );
}

/// The bar's total mounted height is `_kNavBarPersistentHeight +
/// MediaQuery.paddingOf(context).top` — `_PersistentNavigationBar`'s own
/// `SizedBox(height: _kNavBarPersistentHeight + MediaQuery.paddingOf(context).top)`
/// (`nav_bar.dart`, oracle tag `3.44.0`). Constrained loosely so the bar's
/// own preferred height — not an outer tight constraint — determines the
/// measured size.
///
/// Red-check: drop `+ top_inset.get()` from the `SizedBox::height` call in
/// `nav_bar.rs` — this test's height assertion fails (would read `44.0`
/// instead of `64.0`).
#[test]
fn total_mounted_height_adds_the_top_media_query_inset() {
    let laid = lay_out(
        MediaQuery::new(media_with_top_padding(20.0), CupertinoNavigationBar::new()),
        loose(400.0),
    );

    let bar_box = laid
        .find_by_render_type("RenderConstrainedBox")
        .expect("the bar's own outer SizedBox mounts as a RenderConstrainedBox");
    let size = laid.size(bar_box);
    assert!(
        (size.height.get() - 64.0).abs() < 0.01,
        "44.0 persistent height + 20.0 top inset must equal 64.0: {size:?}"
    );
}

/// `leading`/`middle`/`trailing` all reach the mounted render tree, not just
/// the constructor's stored fields — proven by a delta against a bar with
/// none of the three set (whose own outer `SizedBox` already contributes one
/// `RenderConstrainedBox`, so an absolute count would be misleading).
#[test]
fn leading_middle_and_trailing_all_mount() {
    let empty = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    let empty_constrained_box_count = empty.find_all_by_render_type("RenderConstrainedBox").len();

    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoNavigationBar::new()
                .leading(SizedBox::new(20.0, 20.0))
                .middle(Text::new("Settings"))
                .trailing(SizedBox::new(20.0, 20.0)),
        ),
        tight(400.0, 44.0),
    );

    assert!(
        laid.find_by_render_type("RenderParagraph").is_some(),
        "the middle Text must mount as a RenderParagraph"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        empty_constrained_box_count + 2,
        "both the leading and trailing SizedBox must mount, on top of the bar's own"
    );
}

/// With no `leading`/`middle`/`trailing` set, the bar still mounts (an empty
/// `Stack`) without panicking — the degenerate case a layout composed of
/// conditionally-pushed `Positioned` layers must tolerate.
#[test]
fn an_empty_bar_mounts_without_panicking() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoNavigationBar::new()),
        tight(400.0, 44.0),
    );
    assert_eq!(laid.size(laid.root()).height.get(), 44.0);
}
