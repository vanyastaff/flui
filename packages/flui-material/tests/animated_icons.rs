//! The Material animated icons: every pair's data passes its check, draws inside its box at
//! each end and half way, and the pairs that match the text direction mirror under
//! right-to-left text.

use std::time::Duration;

use flui_material::AnimatedIcons;
use flui_painting::paint::PathCommand;
use flui_sdk::animation::{ConstantAnimation, Vsync};
use flui_sdk::geometry::Point;
use flui_sdk::painting::{DrawOp, TextDirection};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{AnimatedIcon, AnimatedIconData, Directionality};

use crate::common::{lay_out_animated, loose};

const SIDE: f64 = 48.0;

const ICONS: [(&str, AnimatedIconData, bool); 14] = [
    ("add_event", AnimatedIcons::ADD_EVENT, false),
    ("arrow_menu", AnimatedIcons::ARROW_MENU, true),
    ("close_menu", AnimatedIcons::CLOSE_MENU, false),
    ("ellipsis_search", AnimatedIcons::ELLIPSIS_SEARCH, false),
    ("event_add", AnimatedIcons::EVENT_ADD, false),
    ("home_menu", AnimatedIcons::HOME_MENU, false),
    ("list_view", AnimatedIcons::LIST_VIEW, true),
    ("menu_arrow", AnimatedIcons::MENU_ARROW, true),
    ("menu_close", AnimatedIcons::MENU_CLOSE, false),
    ("menu_home", AnimatedIcons::MENU_HOME, false),
    ("pause_play", AnimatedIcons::PAUSE_PLAY, false),
    ("play_pause", AnimatedIcons::PLAY_PAUSE, false),
    ("search_ellipsis", AnimatedIcons::SEARCH_ELLIPSIS, false),
    ("view_list", AnimatedIcons::VIEW_LIST, true),
];

/// Every point of every path drawn by `data` at `progress`, `SIDE` pixels wide.
fn points(data: AnimatedIconData, progress: f64, direction: TextDirection) -> Vec<Point<f64>> {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Directionality::new(
                direction,
                AnimatedIcon::new(data, ConstantAnimation::new(progress)).size(SIDE),
            ),
        ),
        loose(200.0),
        vsync,
    );
    laid.pump_for(Duration::ZERO);
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Path { path, .. } => Some(path),
            _ => None,
        })
        .flat_map(|path| {
            path.commands()
                .flat_map(|command| match command {
                    PathCommand::MoveTo(p) | PathCommand::LineTo(p) => vec![p],
                    PathCommand::CubicTo(a, b, p) => vec![a, b, p],
                    PathCommand::QuadraticTo(a, p) => vec![a, p],
                    PathCommand::Close => Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Each pair's data is well formed, draws inside its box, and mirrors when it says so.
pub(crate) fn every_icon_draws_inside_its_box() {
    let mut mirrored = Vec::new();
    for (name, data, mirrors) in ICONS {
        assert_eq!(data.check(), Ok(()), "{name}");
        assert!(
            data.frame_count() >= 2,
            "{name}: {} frames",
            data.frame_count()
        );
        for progress in [0.0, 0.5, 1.0] {
            let drawn = points(data.clone(), progress, TextDirection::Ltr);
            assert!(!drawn.is_empty(), "{name} at {progress}: nothing drawn");
            for point in &drawn {
                assert!(
                    (-1.0..=SIDE + 1.0).contains(&point.x)
                        && (-1.0..=SIDE + 1.0).contains(&point.y),
                    "{name} at {progress}: {point:?} outside the box"
                );
            }
        }
        assert_eq!(data.match_text_direction(), mirrors, "{name}");
        let ltr = points(data.clone(), 0.0, TextDirection::Ltr);
        let rtl = points(data.clone(), 0.0, TextDirection::Rtl);
        let flipped: Vec<_> = ltr.iter().map(|p| Point::new(SIDE - p.x, p.y)).collect();
        let close = |a: &[Point<f64>], b: &[Point<f64>]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9)
        };
        if close(&rtl, &flipped) && !close(&rtl, &ltr) {
            mirrored.push(name);
        } else {
            assert!(close(&rtl, &ltr), "{name}: neither as drawn nor mirrored");
        }
    }
    assert_eq!(
        mirrored,
        ["arrow_menu", "list_view", "menu_arrow", "view_list"]
    );
}
