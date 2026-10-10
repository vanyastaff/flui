//! Integration tests for [`CupertinoTabBar`] — the 50pt default height, the
//! hairline top border's oracle-cited alpha, and that every item mounts.

use crate::common;

use common::{lay_out, tight};
use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
use flui_sdk::painting::DrawOp;
use flui_sdk::widgets::{Icon, IconData, MediaQuery, MediaQueryData};

fn two_items() -> Vec<CupertinoTabBarItem> {
    vec![
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
    ]
}

/// Every item's icon and label reach the submitted paint commands, independently
/// of the render objects used to compose them. This is not a rasterization test.
pub fn every_item_mounts_its_icon_and_label() {
    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), CupertinoTabBar::new(two_items())),
        tight(400.0, 50.0),
    );

    assert!(laid.did_paint_last_frame(), "the tab bar must paint");
    let mut painted = Vec::new();
    for command in laid.draw_ops() {
        if let DrawOp::Paragraph { paragraph, .. } = command.op {
            assert!(paragraph.line_count() > 0, "item text must be laid out");
            painted.push(paragraph.text().to_owned());
        }
    }
    painted.sort();
    let mut expected = ["\u{f3a1}", "\u{f3a2}", "Home", "Settings"];
    expected.sort_unstable();
    assert_eq!(painted, expected, "each icon and label must paint once");
}
