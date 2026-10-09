//! `CupertinoActivityIndicator` on virtual time: each tick's alpha against
//! Flutter's formula `A[(i − ⌊8·t / 1 s⌋) mod 8]`,
//! `A = [47, 47, 47, 47, 72, 97, 122, 147]` (`activity_indicator.dart`),
//! evaluated here independently of the widget.

use std::time::Duration;

use flui_cupertino::CupertinoActivityIndicator;
use flui_sdk::animation::Vsync;
use flui_sdk::painting::DrawOp;
use flui_sdk::widgets::animated::VsyncScope;
use flui_testing::a11y::Role;

use crate::common::{LaidOut, lay_out_animated, loose};

const ALPHAS: [u8; 8] = [47, 47, 47, 47, 72, 97, 122, 147];

/// Flutter's alpha for tick `index` at `millis` into a cycle.
fn flutter_alpha(index: i64, millis: i64) -> u8 {
    let active = (8 * millis).div_euclid(1000);
    ALPHAS[usize::try_from((index - active).rem_euclid(8)).expect("0..8")]
}

/// The alpha of every tick painted in the last frame, in paint order.
fn tick_alphas(laid: &LaidOut) -> Vec<u8> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::RRect { paint, .. } => Some(paint.color.a),
            _ => None,
        })
        .collect()
}

fn mount() -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), CupertinoActivityIndicator::new()),
        loose(100.0),
        vsync.clone(),
    );
    let next = Vsync::new();
    laid.pump_widget(VsyncScope::new(
        next.clone(),
        CupertinoActivityIndicator::new(),
    ));
    assert!(
        vsync.is_empty(),
        "the retained indicator leaves its old registry"
    );
    assert_eq!(next.len(), 1);
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        CupertinoActivityIndicator::new(),
    ));
    assert!(next.is_empty());
    // The first tick anchors the repeating run at virtual time zero.
    laid.pump_for(Duration::ZERO);
    (laid, vsync)
}

pub fn ticks_step_once_per_eighth_of_a_second() {
    let (mut laid, vsync) = mount();
    assert_eq!(vsync.len(), 1);
    let mut now = 0;
    for millis in [0, 124, 125, 999, 1000, 1130] {
        laid.pump_for(Duration::from_millis(millis - now));
        now = millis;
        let expected: Vec<u8> = (0..8)
            .map(|index| flutter_alpha(index, i64::try_from(millis % 1000).expect("small")))
            .collect();
        assert_eq!(tick_alphas(&laid), expected, "at {millis} ms");
    }
    let next = Vsync::new();
    let before = tick_alphas(&laid);
    laid.pump_widget(VsyncScope::new(
        next.clone(),
        CupertinoActivityIndicator::new(),
    ));
    assert!(vsync.is_empty());
    assert_eq!(tick_alphas(&laid), before);
    next.tick_all(
        &flui_sdk::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.0)),
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(tick_alphas(&laid), before);
    next.tick_all(
        &flui_sdk::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.25)),
    );
    laid.pump_for(Duration::ZERO);
    let expected: Vec<_> = (0..8).map(|index| flutter_alpha(index, 380)).collect();
    assert_eq!(tick_alphas(&laid), expected);
}

pub fn announced_as_a_loading_spinner() {
    let (mut laid, _vsync) = mount();
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let node = tree
        .find_by_label("Loading")
        .expect("the indicator's label");
    assert_eq!(node.role(), Role::ProgressIndicator);
}

pub fn unmount_releases_the_controller() {
    let (mut laid, vsync) = mount();
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        flui_sdk::widgets::SizedBox::new(1.0, 1.0),
    ));
    assert_eq!(vsync.len(), 0);
}
