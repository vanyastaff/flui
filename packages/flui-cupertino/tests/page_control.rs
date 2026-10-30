//! `CupertinoPageControl`: the iOS dots follow a pager and turn it.

use std::time::Duration;

use flui_cupertino::CupertinoPageControl;
use flui_sdk::animation::Vsync;
use flui_sdk::painting::{Color, DrawOp};
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Alternative, Center, ColoredBox, Column, Expanded, GestureAlternatives, GestureCommand,
    MenuSpec, PageController, PageIntent, PageView,
};

use crate::common::{LaidOut, lay_out_animated, tight};

const SIDE: f64 = 300.0;

/// Each dot drawn: its centre's x, its diameter and its colour.
fn dots(laid: &LaidOut) -> Vec<(f64, f64, Color)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Circle {
                center,
                radius,
                paint,
            } => Some((center.x, radius * 2.0, paint.color)),
            _ => None,
        })
        .collect()
}

fn pages(count: u8) -> Vec<BoxedView> {
    (0..count)
        .map(|index| ColoredBox::new(Color::rgb(40 * index, 20, 30)).boxed())
        .collect()
}

#[test]
fn the_control_draws_ios_dots_and_turns_its_pager() {
    let controller = PageController::new();
    let vsync = Vsync::new();
    let tree = VsyncScope::new(
        vsync.clone(),
        Column::new(vec![
            Expanded::new(
                PageView::new(pages(4), pager_alternatives()).controller(controller.clone()),
            )
            .boxed(),
            Center::new()
                .child(CupertinoPageControl::controlled(&controller, 4))
                .boxed(),
        ]),
    );
    let mut laid = lay_out_animated(tree, tight(SIDE, SIDE), vsync);
    laid.tick();

    let drawn = dots(&laid);
    assert_eq!(drawn.len(), 4);
    assert_eq!(drawn[1].0 - drawn[0].0, 16.0, "7 pt dots 9 pt apart");
    assert_eq!(drawn[0].1, 7.0);
    assert_eq!(drawn[0].2, Color::WHITE, "the current page is white");
    assert_eq!(drawn[1].2, Color::WHITE.with_opacity(0.3));

    // The four dots span 4 · 7 + 3 · 9 = 55 pt, centred, in the bottom 16 pt.
    let right = f64::midpoint(SIDE, 55.0) - 4.0;
    laid.dispatch_pointer_down(right, SIDE - 8.0);
    laid.dispatch_pointer_up(right, SIDE - 8.0);
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.page(),
        Some(1.0),
        "a tap at the end turns forward"
    );
    assert_eq!(dots(&laid)[1].2, Color::WHITE);
}

#[test]
fn colours_can_be_set() {
    let red = Color::rgb(255, 0, 0);
    let laid = lay_out_animated(
        VsyncScope::new(
            Vsync::new(),
            Center::new().child(
                CupertinoPageControl::observed(flui_sdk::animation::ConstantAnimation::new(0.0), 3)
                    .current_color(red)
                    .other_color(Color::BLACK),
            ),
        ),
        tight(SIDE, SIDE),
        Vsync::new(),
    );
    let colours: Vec<Color> = dots(&laid).into_iter().map(|dot| dot.2).collect();
    assert_eq!(colours, [red, Color::BLACK, Color::BLACK]);
}

/// A pager's required alternatives to its swipe: previous and next page in a menu.
fn pager_alternatives() -> GestureAlternatives {
    GestureAlternatives::try_new(Alternative::ContextMenu(
        MenuSpec::new(
            GestureCommand::new("Previous page", PageIntent::Previous),
            |_cx, _request| {},
        )
        .item(GestureCommand::new("Next page", PageIntent::Next)),
    ))
    .expect("two labelled commands")
}
