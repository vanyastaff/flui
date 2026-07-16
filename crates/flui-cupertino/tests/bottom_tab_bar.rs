//! Integration tests for [`CupertinoTabBar`] — the 50pt default height, the
//! hairline top border's oracle-cited alpha, and that every item mounts.

mod common;

use common::{lay_out, tight};
use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem, CupertinoTheme, CupertinoThemeData};
use flui_types::Size;
use flui_types::geometry::px;
use flui_types::platform::Brightness;
use flui_widgets::{Icon, IconData, MediaQuery, MediaQueryData, PreferredSizeView};

fn two_items() -> Vec<CupertinoTabBarItem> {
    vec![
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
    ]
}

/// `_kTabBarHeight` (`bottom_tab_bar.dart`, oracle tag `3.44.0`): `50.0`.
#[test]
fn preferred_size_is_the_50pt_default_height() {
    let preferred = CupertinoTabBar::new(two_items()).preferred_size();
    assert_eq!(preferred, Size::new(px(f32::INFINITY), px(50.0)));
}

/// `_kDefaultTabBarBorderColor`'s light variant (`bottom_tab_bar.dart`,
/// oracle tag `3.44.0`) is `Color(0x4D000000)` — alpha `77` decimal.
///
/// Red-check: hardcode `default_border()`'s alpha to any other byte — this
/// test's assertion fails on the wrong number, not just "some border".
#[test]
fn default_hairline_border_carries_the_oracles_exact_alpha() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoTabBar::new(two_items())),
        tight(400.0, 50.0),
    );
    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the tab bar always paints a DecoratedBox for its background/border");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        decoration.contains("a: 77"),
        "the default border's color must carry the oracle's exact 0x4D (77) alpha: {decoration}"
    );
}

/// `_kDefaultTabBarBorderColor`'s **dark** variant (`bottom_tab_bar.dart`,
/// oracle tag `3.44.0`) is `Color(0x29000000)` — alpha `41` decimal.
/// `CupertinoNavigationBar`'s own hairline border has no dark variant (a
/// plain, non-dynamic oracle `Color`) — this component's default border is
/// genuinely brightness-dependent, and this is its dark-side coverage: the
/// light case above proved `0x4D`/`77`, this proves the *other* half of
/// `CupertinoDynamicColor::with_brightness` actually gets read, not just
/// stored.
///
/// Red-check: swap `default_border_color()`'s dark argument for the light
/// one (`Color::from_argb(0x4D00_0000)` in both positions) — this test's
/// assertion fails (would read `a: 77`, not `a: 41`).
#[test]
fn default_hairline_border_carries_the_oracles_exact_dark_alpha() {
    let laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            CupertinoTheme::new(
                CupertinoThemeData::default().with_brightness(Brightness::Dark),
                CupertinoTabBar::new(two_items()),
            ),
        ),
        tight(400.0, 50.0),
    );
    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("the tab bar always paints a DecoratedBox for its background/border");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox always reports its decoration");
    assert!(
        decoration.contains("a: 41"),
        "under dark brightness, the default border's color must carry the oracle's exact \
         0x29 (41) alpha: {decoration}"
    );
}

/// Every item's icon and label reach the mounted render tree. Both `Icon`
/// (a glyph from an icon font) and `Text` mount as `RenderParagraph` — two
/// items × (one icon + one label) = 4.
#[test]
fn every_item_mounts_its_icon_and_label() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoTabBar::new(two_items())),
        tight(400.0, 50.0),
    );

    assert_eq!(
        laid.find_all_by_render_type("RenderParagraph").len(),
        4,
        "both items' icon glyphs and labels must mount as RenderParagraph"
    );
}

/// A bare bar with no `on_tap` handler still mounts and can be tapped
/// without panicking (`onTap == null` skips the handler entirely, matching
/// `CupertinoNavigationBar`/`CupertinoButton`'s established "no handler, no
/// crash" contract).
#[test]
fn a_bar_with_no_on_tap_handler_tolerates_a_tap() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoTabBar::new(two_items())),
        tight(400.0, 50.0),
    );
    laid.dispatch_pointer_down(50.0, 25.0);
    laid.dispatch_pointer_up(50.0, 25.0);
}
