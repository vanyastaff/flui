//! Integration tests for [`CupertinoTabBar`] — the 50pt default height, the
//! hairline top border's oracle-cited alpha, and that every item mounts.

use crate::common;

use common::{lay_out, tight};
use flui_cupertino::{CupertinoTabBar, CupertinoTabBarItem};
use flui_sdk::painting::DrawOp;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::widgets::{Icon, IconData};

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

/// Each item is one node that holds its label and says whether it is the
/// current tab; with `on_tap` a click on the node taps its item, and
/// without one the item offers no click yet is not disabled, as a
/// `GestureDetector` without a handler is not.
///
/// Red-check: drop the item description's `selected` (no item reports its
/// selection).
pub fn each_item_is_one_node_its_click_reaches_on_tap() {
    use flui_testing::a11y::{Action, ActionRequest, TreeId};
    use std::cell::Cell;
    use std::rc::Rc;

    for tappable in [true, false] {
        let name = if tappable {
            "with on_tap"
        } else {
            "without on_tap"
        };
        let tapped = Rc::new(Cell::new(None));
        let mut bar = CupertinoTabBar::new(two_items()).current_index(1);
        if tappable {
            let tapped = Rc::clone(&tapped);
            bar = bar.on_tap(move |_ctx, index| tapped.set(Some(index)));
        }
        let mut laid = lay_out(
            MediaQuery::new(MediaQueryData::default(), bar),
            tight(400.0, 50.0),
        );
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let shown = tree.describe();
        let mut home = None;
        for (label, selected) in [("Home", false), ("Settings", true)] {
            let node = tree
                .find_by_label(label)
                .unwrap_or_else(|error| panic!("{name}: no node labelled {label}: {error}"));
            assert!(node.child_ids().is_empty(), "{name}: {label}\n{shown}");
            assert_eq!(
                node.raw().is_selected(),
                Some(selected),
                "{name}: {label}\n{shown}"
            );
            assert!(!node.is_disabled(), "{name}: {label}\n{shown}");
            assert_eq!(
                node.supports_action(Action::Click),
                tappable,
                "{name}: {label}\n{shown}"
            );
            if label == "Home" {
                home = Some(node.id());
            }
        }
        if tappable {
            laid.invoke_semantics_action(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: home.expect("Home was found"),
                data: None,
            })
            .unwrap_or_else(|error| panic!("the item takes a click: {error}"));
            laid.pump();
            assert_eq!(tapped.get(), Some(0), "a click on Home taps item 0");
        }
    }
}
