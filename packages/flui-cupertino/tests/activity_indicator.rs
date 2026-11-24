//! `CupertinoActivityIndicator` on virtual time: each tick's alpha against
//! Flutter's formula `A[(i − ⌊8·t / 1 s⌋) mod 8]`,
//! `A = [47, 47, 47, 47, 72, 97, 122, 147]` (`activity_indicator.dart`),
//! evaluated here independently of the widget.

use flui_animation::__runtime::VsyncDriver;
use std::time::Duration;

use flui_cupertino::CupertinoActivityIndicator;
use flui_sdk::animation::Vsync;
use flui_sdk::painting::DrawOp;
use flui_sdk::view::environment::VsyncScope;
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
            DrawOp::RRect { paint, .. } => Some(paint.color.to_rgba8()[3]),
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
    let driver = VsyncDriver::new();
    let before = tick_alphas(&laid);
    laid.pump_widget(VsyncScope::new(
        driver.vsync().clone(),
        CupertinoActivityIndicator::new(),
    ));
    assert!(vsync.is_empty());
    assert_eq!(tick_alphas(&laid), before);
    driver.tick_all(
        &flui_sdk::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.0)),
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(tick_alphas(&laid), before);
    driver.tick_all(
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

pub fn reduced_motion_keeps_visible_loading_without_continuation() {
    use flui_sdk::animation::MotionOverride;
    use flui_sdk::widgets::SizedBox;
    fn live() {
        policy_case(false);
    }
    fn initial() {
        policy_case(true);
    }
    fn policy_case(initially_reduced: bool) {
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(vsync.clone(), SizedBox::shrink()),
            loose(100.0),
            vsync.clone(),
        );
        laid.set_motion_override(if initially_reduced {
            MotionOverride::Reduce
        } else {
            MotionOverride::Full
        });
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            CupertinoActivityIndicator::new(),
        ));
        laid.pump_for(Duration::ZERO);
        if !initially_reduced {
            laid.pump_for(Duration::from_millis(250));
            laid.set_motion_override(MotionOverride::Reduce);
            laid.pump_for(Duration::ZERO);
        }
        let stopped = tick_alphas(&laid);
        assert_eq!(stopped.len(), 8, "static loading retains all eight ticks");
        assert!(stopped.iter().any(|alpha| *alpha > 0));
        laid.pump_for(Duration::from_millis(375));
        assert_eq!(tick_alphas(&laid), stopped, "Reduce stops visible motion");
        assert!(
            !vsync.has_running(),
            "a parked indicator needs no continuation"
        );
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("Loading")
            .expect("loading label retained");
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.value(), None);
        laid.set_motion_override(MotionOverride::Full);
        laid.pump_for(Duration::ZERO);
        let resumed = tick_alphas(&laid);
        laid.pump_for(Duration::from_millis(125));
        assert_ne!(tick_alphas(&laid), resumed, "Full resumes visible motion");
        laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::shrink()));
        assert!(vsync.is_empty());
    }
    crate::common::run_cases(&[("live preference", live), ("initial preference", initial)]);
}

pub fn inactive_loading_stays_visible_and_resumes() {
    use flui_sdk::widgets::{SizedBox, TickerMode};
    fn disabled() {
        inactive_case(false);
    }
    fn detached() {
        inactive_case(true);
    }
    fn inactive_case(detached: bool) {
        let vsync = Vsync::new();
        let root = if detached {
            VsyncScope::detached(TickerMode::new(CupertinoActivityIndicator::new()))
        } else {
            VsyncScope::new(
                vsync.clone(),
                TickerMode::new(CupertinoActivityIndicator::new()).enabled(false),
            )
        };
        let mut laid = lay_out_animated(root, loose(100.0), vsync.clone());
        laid.pump_for(Duration::from_millis(500));
        assert_eq!(tick_alphas(&laid), ALPHAS);
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("Loading")
            .expect("loading remains visible");
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.value(), None);
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            TickerMode::new(CupertinoActivityIndicator::new()),
        ));
        laid.pump_for(Duration::ZERO);
        let resumed = tick_alphas(&laid);
        laid.pump_for(Duration::from_millis(125));
        assert_ne!(tick_alphas(&laid), resumed);
        laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::shrink()));
        assert!(vsync.is_empty());
    }
    crate::common::run_cases(&[("disabled", disabled), ("detached", detached)]);
}

/// Mounts `indicator` on a fresh registry, anchored at virtual time zero.
fn mount_one(indicator: CupertinoActivityIndicator) -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), indicator),
        loose(100.0),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    (laid, vsync)
}

/// `radius` sizes the indicator as a square of twice it, and an invalid
/// radius keeps the default 10; `color` tints every tick with the colour it
/// resolves to, keeping each tick's own alpha.
pub fn radius_and_color_size_and_tint_the_ticks() {
    use flui_cupertino::CupertinoColors;
    let (laid, _vsync) = mount_one(
        CupertinoActivityIndicator::new()
            .radius(14.0)
            .color(CupertinoColors::SYSTEM_RED),
    );
    let size = laid.size(laid.root());
    assert_eq!((size.width, size.height), (28.0, 28.0));
    let [red, green, blue, _] = CupertinoColors::SYSTEM_RED.color.to_rgba8();
    let colors: Vec<[u8; 4]> = laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::RRect { paint, .. } => Some(paint.color.to_rgba8()),
            _ => None,
        })
        .collect();
    let expected: Vec<[u8; 4]> = ALPHAS
        .iter()
        .map(|alpha| [red, green, blue, *alpha])
        .collect();
    assert_eq!(colors, expected, "light red, each tick at its own alpha");

    for radius in [0.0, -3.0, f64::NAN, f64::INFINITY, f64::MAX] {
        let (laid, _vsync) = mount_one(CupertinoActivityIndicator::new().radius(radius));
        let size = laid.size(laid.root());
        assert_eq!(
            (size.width, size.height),
            (20.0, 20.0),
            "radius {radius} keeps the default"
        );
        assert_eq!(tick_alphas(&laid).len(), 8, "radius {radius} still paints");
    }
}

/// `animating(false)` paints the ticks at rest and requests no frames; a
/// rebuild that turns it on starts the cycle, and one that turns it off again
/// stops it with no continuation frame.
pub fn animating_starts_and_stops_a_mounted_indicator() {
    let vsync = Vsync::new();
    let scoped = |animating: bool| {
        VsyncScope::new(
            vsync.clone(),
            CupertinoActivityIndicator::new().animating(animating),
        )
    };
    let mut laid = lay_out_animated(scoped(false), loose(100.0), vsync.clone());
    laid.pump_for(Duration::ZERO);
    assert_eq!(tick_alphas(&laid), ALPHAS, "still ticks at rest");
    assert!(!vsync.has_running(), "a still indicator requests no frames");
    laid.pump_for(Duration::from_millis(250));
    assert_eq!(tick_alphas(&laid), ALPHAS);

    laid.pump_widget(scoped(true));
    laid.pump_for(Duration::ZERO);
    assert!(vsync.has_running(), "turning it on starts the cycle");
    let started = tick_alphas(&laid);
    laid.pump_for(Duration::from_millis(125));
    assert_ne!(tick_alphas(&laid), started, "the ticks move");

    laid.pump_widget(scoped(false));
    laid.pump_for(Duration::ZERO);
    assert!(
        !vsync.has_running(),
        "turning it off leaves no continuation"
    );
    assert_eq!(tick_alphas(&laid), ALPHAS, "back at rest");
    laid.pump_for(Duration::from_millis(375));
    assert_eq!(tick_alphas(&laid), ALPHAS);
}

/// `partially_revealed(p)` shows `ceil(8 · p)` ticks from 12 o'clock at
/// alpha 147, Flutter's `i < 8 · progress` (`activity_indicator.dart:179`);
/// at `1.0` the eight ticks rest. It never animates, ignores `animating`,
/// and publishes its progress as a percentage. A progress outside `0..=1` is
/// clamped and NaN reveals nothing.
pub fn partially_revealed_shows_still_ticks_for_its_progress() {
    let rows: [(f64, Vec<u8>, &str); 7] = [
        (0.5, vec![147; 4], "50%"),
        (0.1, vec![147; 1], "10%"),
        (0.0, vec![], "0%"),
        (1.0, ALPHAS.to_vec(), "100%"),
        (1.5, ALPHAS.to_vec(), "100%"),
        (-0.5, vec![], "0%"),
        (f64::NAN, vec![], "0%"),
    ];
    for (progress, expected, announced) in rows {
        let (mut laid, vsync) = mount_one(
            CupertinoActivityIndicator::partially_revealed(progress)
                .animating(true)
                .radius(12.0),
        );
        assert_eq!(tick_alphas(&laid), expected, "progress {progress}");
        let size = laid.size(laid.root());
        assert_eq!((size.width, size.height), (24.0, 24.0));
        assert!(
            !vsync.has_running(),
            "progress {progress} requests no frames"
        );
        laid.pump_for(Duration::from_millis(250));
        assert_eq!(tick_alphas(&laid), expected, "progress {progress} stays");
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("Loading")
            .expect("the indicator's label");
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.value(), Some(announced), "progress {progress}");
    }
}
