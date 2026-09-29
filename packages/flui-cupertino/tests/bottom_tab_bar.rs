//! Integration tests for [`CupertinoTabBar`] — the 50pt default height, the
//! hairline top border's oracle-cited alpha, and that every item mounts.

use crate::common;

use common::{lay_out, tight};
use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
use flui_sdk::widgets::{Icon, IconData, MediaQuery, MediaQueryData};

fn two_items() -> Vec<CupertinoTabBarItem> {
    vec![
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
    ]
}

/// Every item's icon and label reach the mounted render tree. Both `Icon`
/// (a glyph from an icon font) and `Text` mount as `RenderParagraph` — two
/// items × (one icon + one label) = 4.
pub fn every_item_mounts_its_icon_and_label() {
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
