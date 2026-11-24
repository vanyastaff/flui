//! Integration tests for [`CupertinoTabBar`] — the 50pt default height, the
//! hairline top border's alpha, and that every item mounts.

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

/// The default top border is one device pixel tall at the bar's top edge,
/// `0x4D000000` in light and `0x29000000` in dark
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/bottom_tab_bar.dart:75`);
/// `border(None)` removes it and a custom border replaces it.
pub fn the_default_border_is_a_hairline_and_border_replaces_it() {
    use flui_sdk::painting::styling::{Border, BorderSide, BorderStyle, Color};
    use flui_sdk::platform::Brightness;

    let light = Color::from_argb(0x4D00_0000);
    let dark = Color::from_argb(0x2900_0000);
    let custom = Color::from_argb(0xFFFF_0000);
    let custom_border = Border::new(
        Some(BorderSide::new(custom, 2.0, BorderStyle::Solid)),
        None,
        None,
        None,
    );
    let cases = [
        ("light default", Brightness::Light, None, Some((light, 0.5))),
        ("dark default", Brightness::Dark, None, Some((dark, 0.5))),
        ("removed", Brightness::Light, Some(None), None),
        (
            "custom",
            Brightness::Light,
            Some(Some(custom_border)),
            Some((custom, 2.0)),
        ),
    ];
    for (name, brightness, border, expected) in cases {
        let media = MediaQueryData {
            device_pixel_ratio: 2.0,
            platform_brightness: brightness,
            ..MediaQueryData::default()
        };
        let mut bar = CupertinoTabBar::new(two_items());
        if let Some(border) = border {
            bar = bar.border(border);
        }
        let laid = lay_out(MediaQuery::new(media, bar), tight(400.0, 50.0));
        let sides: Vec<_> = laid
            .draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Rect { rect, paint } if [light, dark, custom].contains(&paint.color) => {
                    Some((paint.color, rect))
                }
                _ => None,
            })
            .collect();
        match expected {
            None => assert!(sides.is_empty(), "{name}: no border: {sides:?}"),
            Some((color, height)) => {
                let [(painted, rect)] = sides.as_slice() else {
                    panic!("{name}: one border side: {sides:?}");
                };
                assert_eq!(*painted, color, "{name}: the border's colour");
                assert!(
                    (rect.height() - height).abs() < 1e-9 && rect.min.y.abs() < 1e-9,
                    "{name}: a {height}-point side along the top edge: {rect:?}"
                );
            }
        }
    }
}
